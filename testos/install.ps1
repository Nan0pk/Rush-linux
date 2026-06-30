# testos/install.ps1 - download the latest prebuilt testOS image and write it to a USB stick.
#
# Usage (from an elevated PowerShell):
#   .\install.ps1 -Device \\.\PhysicalDrive1
#
# Or download-and-run (one-liner, using a scriptblock to pass parameters):
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.ps1))) -Device \\.\PhysicalDrive1
#
# What it does:
#   1. Finds the latest testOS release on GitHub.
#   2. Downloads testos-<version>.raw, the testos-ingest binary, the bench-list.toml,
#      and SHA256SUMS to a temp directory.
#   3. Verifies the checksums.
#   4. Refuses to write to a drive that's in use or that looks like the system disk.
#   5. Asks you to confirm the device path twice.
#   6. Writes the image to the USB using dd-equivalent direct disk writes
#      (no Rufus, no Etcher, no WSL - pure PowerShell).
#   7. Prints next steps for collecting results.
#
# Requirements:
#   - Windows 10 / 11 (PowerShell 5.1+ or PowerShell 7+)
#   - Administrator privileges (to write to a raw disk)
#   - The USB stick inserted and visible in Disk Management
#
# How to find your USB's physical drive number:
#   Get-Disk | Format-Table Number, FriendlyName, Size, PartitionStyle
# Then use -Device \\.\PhysicalDrive<N>
#
# Why no curl: Windows PowerShell aliases curl to Invoke-WebRequest which has
# different flags. This script uses Invoke-WebRequest natively so there's no
# alias collision.

[CmdletBinding()]
param(
    [Parameter(Mandatory=$false)]
    [string]$Device,

    # Path to a local .raw or .raw.zst file. Skips the GitHub download.
    [Parameter(Mandatory=$false)]
    [string]$ImageFile,

    # Skip SHA256 verification (only relevant with -ImageFile).
    [switch]$SkipVerification,

    # Delete the local cache dir before running (clean slate).
    [switch]$CleanCache,

    [switch]$DryRun,
    [switch]$ListOnly,
    [switch]$Force,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

# --- Helpers ------------------------------------------------------
function Write-Info  { param([string]$msg) Write-Host ">> $msg" -ForegroundColor White }
function Write-OK    { param([string]$msg) Write-Host "[OK] $msg" -ForegroundColor Green }
function Write-Warn  { param([string]$msg) Write-Host "[!]  $msg" -ForegroundColor Yellow }
function Write-Err   { param([string]$msg) Write-Host "[X]  $msg" -ForegroundColor Red; exit 1 }

if ($Help) {
    @'
testOS installer for Windows - download and write the latest testOS image to USB.

Usage:
  .\install.ps1 -Device \\.\PhysicalDrive<N>        Download + write to the specified disk
  .\install.ps1 -ListOnly                           Show latest release assets without writing
  .\install.ps1 -DryRun                             Download and verify, don't write
  .\install.ps1 -ImageFile C:\path\to\file.raw.zst  Use a local image, skip download
  .\install.ps1 -CleanCache                         Delete the local cache and re-download
  .\install.ps1 -Help                               This message

How to find your USB's physical drive number:
  Get-Disk | Format-Table Number, FriendlyName, Size, PartitionStyle, BusType

Then pass -Device \\.\PhysicalDrive<N> (e.g. \\.\PhysicalDrive1).

Common issue - "cannot be loaded because running scripts is disabled":
  Windows blocks downloaded scripts by default. Run with execution policy
  bypassed for this process only:
    powershell -ExecutionPolicy Bypass -File .\install.ps1 -Device \\.\PhysicalDrive<N>

  If that still fails, unblock the downloaded file first:
    Unblock-File .\install.ps1
    powershell -ExecutionPolicy Bypass -File .\install.ps1 -Device \\.\PhysicalDrive<N>

Requirements:
  - Windows 10/11 with PowerShell 5.1+ (built-in) or PowerShell 7+
  - Administrator privileges (Run as Administrator)

Options:
  -Device <path>       Raw disk path, e.g. \\.\PhysicalDrive1
  -ImageFile <path>    Path to a local .raw or .raw.zst. Skips the GitHub download.
                       SHA256 is still verified against the release SHA256SUMS unless
                       you also pass -SkipVerification.
  -SkipVerification    Skip SHA256 check (use with -ImageFile when offline).
  -CleanCache          Delete %LOCALAPPDATA%\testos-installer\cache\ before running.
  -DryRun              Download/verify/decompress but don't write to the device.
  -ListOnly            Just show what's in the latest release.
  -Force               Skip removable-media and size-sanity safety checks.
  -Help                This message.

Cache directory: %LOCALAPPDATA%\testos-installer\cache\
  The installer caches the downloaded .raw.zst and decompressed .raw here.
  Second run of the same version reuses the cache - no 582 MB re-download.
'@ | Write-Host
    exit 0
}

# --- Admin check --------------------------------------------------
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin -and -not $ListOnly -and -not $DryRun) {
    Write-Warn "Not running as Administrator. Disk writes will fail."
    Write-Warn "Re-run from an elevated PowerShell: right-click PowerShell -> 'Run as Administrator'."
    Write-Warn "Continuing anyway in 3 seconds... (Ctrl-C to abort)"
    Start-Sleep -Seconds 3
}

# --- Proactive decompressor check ---------------------------------
# Check NOW (before the download) that we have something to decompress with.
# zstd.exe (winget install Meta.Zstandard) is preferred. tar.exe ships with
# Windows 10 1803+ and is the built-in fallback.
if (-not $ListOnly) {
    $zstdAvail = Get-Command zstd    -ErrorAction SilentlyContinue
    $tarAvail  = Get-Command tar.exe -ErrorAction SilentlyContinue
    if (-not $zstdAvail -and -not $tarAvail) {
        Write-Warn "Neither zstd.exe nor tar.exe was found on PATH."
        Write-Warn "tar.exe ships with Windows 10 1803+ at C:\Windows\System32\tar.exe."
        Write-Warn "If you are on an older Windows, install zstd first:"
        Write-Warn "  winget install Meta.Zstandard"
        Write-Warn "Then re-run this script."
        exit 1
    }
}

# --- Find latest release ------------------------------------------
$Repo = "Nan0pk/Rush-linux"
# Use /releases (not /releases/latest, which skips prereleases) and fetch
# 10 so we can skip draft releases. GitHub returns drafts first in the
# /releases listing; if a draft exists, per_page=1 would return it instead
# of the latest published release. We filter drafts below.
$ApiUrl = "https://api.github.com/repos/$Repo/releases?per_page=10"

Write-Info "Finding the latest testOS release..."
try {
    $Releases = Invoke-RestMethod -Uri $ApiUrl -Headers @{ "User-Agent" = "testos-installer" } -ErrorAction Stop
} catch {
    Write-Err "Could not fetch release info from $ApiUrl. Either there are no releases yet, or you're rate-limited. Try again in a few minutes, or build from source: see the README's 'Build from source' section."
}

# /releases returns an array; take the first non-draft release. GitHub
# returns drafts first in the listing, so we must filter them out.
$Release = $Releases | Where-Object { -not $_.draft } | Select-Object -First 1
if (-not $Release) {
    Write-Err "No non-draft releases found at $ApiUrl. The release workflow may not have run yet - see the README's 'Build from source' section."
}

$Version = $Release.tag_name
if (-not $Version) { Write-Err "Could not parse release tag. The release may be malformed." }
Write-Info "Latest release: $Version"

if ($ListOnly) {
    Write-Host ""
    Write-Host "Assets in this release:"
    $Release.assets | ForEach-Object { Write-Host ("  " + $_.browser_download_url) }
    Write-Host ""
    exit 0
}

# --- Check that a release image exists ----------------------------
$Assets = $Release.assets
$ImageAsset = $Assets | Where-Object { $_.name -match '^testos-.*\.raw(\.zst)?$' } | Select-Object -First 1
if (-not $ImageAsset) {
    Write-Warn "The latest release ($Version) does not contain a testos-*.raw(.zst) image."
    Write-Warn "This usually means the release workflow is still running, or the project"
    Write-Warn "hasn't published a testOS image yet."
    Write-Host ""
    Write-Host "To build from source instead, see:"
    Write-Host "  https://github.com/$Repo#build-from-source"
    exit 1
}

# --- Set up working directory and cache dir ----------------------
$WorkDir = Join-Path $env:TEMP ("testos-install-" + [System.Guid]::NewGuid().ToString("N").Substring(0,8))
New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null

# Cache dir: %LOCALAPPDATA%\testos-installer\cache\ (fall back to %TEMP% if LOCALAPPDATA is unset)
$LocalAppData = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:TEMP }
$CacheDir = Join-Path $LocalAppData "testos-installer\cache"

if ($CleanCache) {
    Write-Info "Cleaning cache at $CacheDir ..."
    if (Test-Path $CacheDir) {
        Remove-Item -Path $CacheDir -Recurse -Force -ErrorAction SilentlyContinue
        Write-OK "Cache cleared."
    } else {
        Write-Info "Cache dir does not exist - nothing to clear."
    }
}

New-Item -ItemType Directory -Path $CacheDir -Force | Out-Null

try {
    # --- Helper functions -----------------------------------------
    function Download-File {
        param([string]$Url, [string]$DestPath)
        Write-Info "Downloading $(Split-Path $DestPath -Leaf)..."
        try {
            Invoke-WebRequest -Uri $Url -OutFile $DestPath -UseBasicParsing -ErrorAction Stop
        } catch {
            Write-Err "Download failed: $Url  --  $($_.Exception.Message)"
        }
    }

    # Decompress a .zst file to $DestPath. Returns nothing; exits on failure.
    function Expand-Zst {
        param([string]$ZstPath, [string]$DestPath)
        $name = Split-Path $ZstPath -Leaf
        Write-Info "Decompressing $name ..."
        $zstdExe = Get-Command zstd -ErrorAction SilentlyContinue
        if ($zstdExe) {
            # PS 5.1 with $ErrorActionPreference="Stop" still throws NativeCommandError
            # for any stderr output from native exes, even with 2>$null.
            # Temporarily relax EAP, run zstd, capture exit code, then restore.
            $savedEAP = $ErrorActionPreference
            $ErrorActionPreference = "Continue"
            & zstd -d -f $ZstPath -o $DestPath 2>$null
            $zstdExit = $LASTEXITCODE
            $ErrorActionPreference = $savedEAP
            if ($zstdExit -ne 0) { Write-Err "zstd decompression failed (exit $zstdExit)." }
        } else {
            # bsdtar (Windows 10 1803+ tar.exe) can stream-decompress a .zst.
            # Use cmd.exe so the redirect works correctly.
            $tmpError = Join-Path $env:TEMP "testos-zstd-err.txt"
            & cmd.exe /c "tar.exe --use-compress-program=zstd -xf `"$ZstPath`" --to-stdout > `"$DestPath`" 2>`"$tmpError`""
            if ($LASTEXITCODE -ne 0) {
                $errDetail = if (Test-Path $tmpError) { Get-Content $tmpError -Raw } else { "(no stderr)" }
                Write-Err ("Could not decompress $name (tar.exe exit $LASTEXITCODE). " + $errDetail + " Install zstd: winget install Meta.Zstandard")
            }
        }
        if (-not (Test-Path $DestPath) -or (Get-Item $DestPath).Length -eq 0) {
            Write-Err "Decompression of $name produced an empty file. The .zst may be corrupt."
        }
    }

    # Verify one file against SHA256SUMS content. Returns $true if match, $false if not found.
    # Exits with an error if found but hash mismatches.
    function Test-Sha256 {
        param([string]$FilePath, [string]$SumsContent)
        $FileName = Split-Path $FilePath -Leaf
        foreach ($Line in ($SumsContent -split "`n")) {
            if ($Line -match '^\s*([0-9a-fA-F]{64})\s+\*?' + [regex]::Escape($FileName) + '\s*$') {
                $ExpectedHash = $Matches[1].ToLower()
                Write-Info "Verifying SHA256 for $FileName ..."
                $ActualHash = (Get-FileHash -Path $FilePath -Algorithm SHA256).Hash.ToLower()
                if ($ActualHash -eq $ExpectedHash) {
                    Write-OK "SHA256 OK for $FileName"
                    return $true
                } else {
                    Write-Warn "SHA256 MISMATCH for $FileName"
                    Write-Warn "  Expected: $ExpectedHash"
                    Write-Warn "  Actual:   $ActualHash"
                    Write-Err "Checksum verification failed. The file may be corrupted or stale."
                }
            }
        }
        return $false  # filename not in SHA256SUMS
    }

    # ---------------------------------------------------------------
    # Locate or build the .raw image. Three paths:
    #   A) -ImageFile supplied by user (local file, skip download)
    #   B) No -ImageFile: check cache, download if miss
    # ---------------------------------------------------------------

    # Always fetch SHA256SUMS fresh (it's ~500 bytes) so we can verify
    # both cached and fresh downloads. Skip only if -SkipVerification.
    $SumsAsset   = $Assets | Where-Object { $_.name -eq "SHA256SUMS" } | Select-Object -First 1
    $SumsFile    = Join-Path $WorkDir "SHA256SUMS"
    $SumsContent = $null

    if ($SumsAsset -and -not $SkipVerification) {
        Download-File $SumsAsset.browser_download_url $SumsFile
        $SumsContent = Get-Content $SumsFile -Raw
    }

    # Paths we'll resolve to:
    $ZstCachePath = Join-Path $CacheDir $ImageAsset.name                                              # e.g. cache	estos-0.7.0-beta.2.raw.zst
    $RawName      = [System.IO.Path]::GetFileNameWithoutExtension($ImageAsset.name)                   # testos-0.7.0-beta.2.raw  (strips .zst)
    $RawCachePath = Join-Path $CacheDir $RawName                                                      # cache	estos-0.7.0-beta.2.raw
    $IsZst        = $ImageAsset.name -match '\.zst$'
    $ResolvedRaw  = $null   # set to the path of the usable .raw before writing

    if ($ImageFile) {
        # ---- PATH A: user supplied a local file ------------------
        if (-not (Test-Path $ImageFile)) {
            Write-Err "The file specified with -ImageFile does not exist: $ImageFile"
        }
        $localName = Split-Path $ImageFile -Leaf
        Write-Info "Using local file: $ImageFile"

        if ($ImageFile -match '\.zst$') {
            # Decompress to cache dir
            if ($SkipVerification) {
                Write-Warn "Skipping SHA256 verification (-SkipVerification)."
            } elseif ($SumsContent) {
                $ok = Test-Sha256 $ImageFile $SumsContent
                if (-not $ok) { Write-Warn "$localName not found in SHA256SUMS - cannot verify. Pass -SkipVerification to proceed anyway."; exit 1 }
            }
            Expand-Zst $ImageFile $RawCachePath
            $ResolvedRaw = $RawCachePath
        } else {
            # Treat as a plain .raw
            if (-not $SkipVerification -and $SumsContent) {
                $ok = Test-Sha256 $ImageFile $SumsContent
                if (-not $ok) { Write-Warn "$localName not found in SHA256SUMS - cannot verify. Pass -SkipVerification to proceed anyway."; exit 1 }
            } elseif ($SkipVerification) {
                Write-Warn "Skipping SHA256 verification (-SkipVerification)."
            }
            $ResolvedRaw = $ImageFile
        }
    } else {
        # ---- PATH B: download (with cache) -----------------------
        Write-Info "Checking cache at $CacheDir ..."

        # Sub-path B1: cached .raw exists and .zst hash matches -> skip both download AND decompress
        if ($IsZst -and (Test-Path $RawCachePath) -and (Test-Path $ZstCachePath) -and -not $SkipVerification -and $SumsContent) {
            $zstHashOk = $false
            foreach ($Line in ($SumsContent -split "`n")) {
                if ($Line -match '^\s*([0-9a-fA-F]{64})\s+\*?' + [regex]::Escape($ImageAsset.name) + '\s*$') {
                    $ExpectedHash = $Matches[1].ToLower()
                    $ActualHash   = (Get-FileHash -Path $ZstCachePath -Algorithm SHA256).Hash.ToLower()
                    if ($ActualHash -eq $ExpectedHash) { $zstHashOk = $true }
                    break
                }
            }
            if ($zstHashOk) {
                Write-OK "Cache hit (raw+zst): using $RawCachePath (skipping download and decompression)."
                $ResolvedRaw = $RawCachePath
            } else {
                Write-Warn "Cached .zst hash mismatch - stale cache. Deleting and re-downloading."
                Remove-Item $ZstCachePath -Force -ErrorAction SilentlyContinue
                Remove-Item $RawCachePath -Force -ErrorAction SilentlyContinue
            }
        }

        # Sub-path B2: cached .zst exists, hash matches -> skip download, decompress to .raw
        if ($null -eq $ResolvedRaw -and $IsZst -and (Test-Path $ZstCachePath) -and -not $SkipVerification -and $SumsContent) {
            $zstHashOk = $false
            foreach ($Line in ($SumsContent -split "`n")) {
                if ($Line -match '^\s*([0-9a-fA-F]{64})\s+\*?' + [regex]::Escape($ImageAsset.name) + '\s*$') {
                    $ExpectedHash = $Matches[1].ToLower()
                    $ActualHash   = (Get-FileHash -Path $ZstCachePath -Algorithm SHA256).Hash.ToLower()
                    if ($ActualHash -eq $ExpectedHash) { $zstHashOk = $true }
                    break
                }
            }
            if ($zstHashOk) {
                Write-OK "Cache hit (.zst): using cached $ZstCachePath"
                Expand-Zst $ZstCachePath $RawCachePath
                $ResolvedRaw = $RawCachePath
            } else {
                Write-Warn "Cached .zst hash mismatch - deleting stale file."
                Remove-Item $ZstCachePath -Force -ErrorAction SilentlyContinue
            }
        }

        # Sub-path B3: cache miss - download from GitHub
        if ($null -eq $ResolvedRaw) {
            Write-Info "Cache miss - downloading from GitHub..."
            $DownloadDest = Join-Path $WorkDir $ImageAsset.name
            Download-File $ImageAsset.browser_download_url $DownloadDest

            # Verify before caching
            if (-not $SkipVerification -and $SumsContent) {
                $ok = Test-Sha256 $DownloadDest $SumsContent
                if (-not $ok) { Write-Warn "Image filename not found in SHA256SUMS - skipping verification."; $ok = $true }
            }

            # Copy into cache for next time
            Write-Info "Caching downloaded image to $ZstCachePath ..."
            Copy-Item $DownloadDest $ZstCachePath -Force

            if ($IsZst) {
                Expand-Zst $ZstCachePath $RawCachePath
                $ResolvedRaw = $RawCachePath
            } else {
                $ResolvedRaw = $ZstCachePath
            }
        }
    }

    # Download side-car files (ingest binary + bench-list) to WorkDir for DryRun listing
    $IngestAsset = $Assets | Where-Object { $_.name -match '^testos-ingest-.*-linux-x86_64$' } | Select-Object -First 1
    if ($IngestAsset) {
        Download-File $IngestAsset.browser_download_url (Join-Path $WorkDir $IngestAsset.name)
    }
    $BenchListAsset = $Assets | Where-Object { $_.name -eq "bench-list.toml" } | Select-Object -First 1
    if ($BenchListAsset) {
        Download-File $BenchListAsset.browser_download_url (Join-Path $WorkDir "bench-list.toml")
    }

    $ImageSizeBytes = (Get-Item $ResolvedRaw).Length
    $ImageSizeMB    = [math]::Round($ImageSizeBytes / 1MB)

    # --- Dry-run stops here ---------------------------------------
    if ($DryRun) {
        Write-OK "Dry run complete."
        Write-Host ""
        Write-Host "Image: $ResolvedRaw ($ImageSizeMB MB)"
        Write-Host "Cache: $CacheDir"
        Write-Host ""
        Write-Host "Re-run without -DryRun and with a USB device to write:"
        Write-Host "  .\install.ps1 -Device \\.\PhysicalDrive<N>"
        exit 0
    }

    # Alias so the rest of the write section finds $ImageFile
    $ImageFile = $ResolvedRaw

    # --- Device selection and safety checks ----------------------
    if (-not $Device) {
        Write-Host ""
        Write-Host "Available disks:"
        Get-Disk | Format-Table Number, FriendlyName, @{Name="SizeGB";Expression={[math]::Round($_.Size/1GB,1)}}, PartitionStyle, BusType
        Write-Host ""
        Write-Host "Find your USB stick in the table above (look for BusType=USB and the right size),"
        Write-Host "then re-run with -Device \\.\PhysicalDrive<N>."
        Write-Host ""
        Write-Host "Example:"
        Write-Host "  .\install.ps1 -Device \\.\PhysicalDrive1"
        exit 1
    }

    # Validate the device path format.
    if ($Device -notmatch '^\\\\\.\\PhysicalDrive\d+$') {
        Write-Err "Device path must be in the form \\.\PhysicalDrive<N> (e.g. \\.\PhysicalDrive1). Got: $Device"
    }

    # Extract the disk number for safety checks.
    $DiskNum = [int]($Device -replace '^\\\\\.\\PhysicalDrive','')

    # Look up the disk's identity for safety checks and confirmation.
    try {
        $DiskInfo = Get-Disk -Number $DiskNum -ErrorAction Stop
    } catch {
        Write-Err "Disk $DiskNum not found. Check 'Get-Disk' and pass a valid -Device \\.\PhysicalDrive<N>."
    }

    # --- Safety check 1: refuse the Windows system disk ---------
    try {
        $SystemDisk = Get-Partition | Where-Object { $_.DriveLetter -eq $env:SystemDrive[0] } | Select-Object -ExpandProperty DiskNumber -First 1
        if ($null -ne $SystemDisk -and $DiskNum -eq $SystemDisk) {
            Write-Err "Device $Device is the Windows system disk (Disk $SystemDisk, $($DiskInfo.FriendlyName)). Refusing to overwrite. If you really meant to write to your boot disk, you're holding the script wrong - use a USB stick."
        }
    } catch {
        Write-Warn "Could not determine the system disk for safety check. Proceed with extreme caution."
    }

    # --- Safety check 2: refuse non-USB bus types unless -Force -
    # A USB stick shows up as BusType=USB. Internal SATA/NVMe disks show
    # up as BusType=SATA/NVMe/RAID. Refusing non-USB bus types catches
    # the most common accident: targeting an internal data disk.
    $BusType = $DiskInfo.BusType
    if ($BusType -ne 'USB' -and -not $Force) {
        Write-Warn "Disk $DiskNum ($($DiskInfo.FriendlyName)) is on bus type '$BusType', not 'USB'."
        Write-Warn "This looks like an internal disk, not a USB stick."
        Write-Warn "Writing to it would destroy any data on it."
        Write-Err "Refusing to write to a non-USB disk. If you really mean to do this (e.g. writing to an internal test disk), re-run with -Force. Otherwise, find your USB with 'Get-Disk' and try again."
    }

    # --- Safety check 3: refuse mounted volumes unless -Force ---
    $MountedParts = Get-Partition -DiskNumber $DiskNum -ErrorAction SilentlyContinue | Where-Object { $_.DriveLetter }
    if ($MountedParts -and -not $Force) {
        Write-Warn "Disk $DiskNum ($($DiskInfo.FriendlyName)) has mounted volumes:"
        $MountedParts | ForEach-Object { Write-Warn "  $($_.DriveLetter):" }
        Write-Warn "Writing will destroy all data on these volumes."
        Write-Err "Aborting. Re-run with -Force to proceed anyway, or unmount the volumes first."
    }

    # --- Safety check 4: size sanity ----------------------------
    # If the target disk is more than 4x the image size, warn. People
    # sometimes image a 500MB USB onto a 2TB HDD by mistake.
    $DiskSizeBytes = $DiskInfo.Size
    $DiskSizeGB = [math]::Round($DiskSizeBytes / 1GB, 1)
    if ($DiskSizeBytes -gt ($ImageSizeBytes * 4)) {
        Write-Warn "Target disk is $DiskSizeGB GB but the image is only $ImageSizeMB MB."
        Write-Warn "This is unusual - you may be targeting the wrong disk (e.g. an internal HDD instead of a USB stick)."
        if (-not $Force) {
            Write-Err "Refusing to write to a disk that's much larger than the image. If this is intentional (e.g. a large USB stick), re-run with -Force."
        }
    }
    # Also warn if the target is smaller than the image (would fail mid-write).
    if ($DiskSizeBytes -lt $ImageSizeBytes) {
        Write-Err "Target disk ($DiskSizeGB GB) is smaller than the image ($ImageSizeMB MB). The write would fail mid-way and leave the disk in a broken state."
    }

    # --- Confirm: show the disk's identity and ask 'yes' --------
    Write-Host ""
    Write-Host "About to write $ImageSizeMB MB to:" -ForegroundColor White
    Write-Host "  Device:       $Device" -ForegroundColor White
    Write-Host "  FriendlyName: $($DiskInfo.FriendlyName)" -ForegroundColor White
    Write-Host "  BusType:      $BusType" -ForegroundColor White
    Write-Host "  Size:         $DiskSizeGB GB" -ForegroundColor White
    Write-Host "  PartitionStyle: $($DiskInfo.PartitionStyle)" -ForegroundColor White
    Write-Host ""
    Write-Host "ALL DATA ON THIS DISK WILL BE LOST." -ForegroundColor Red
    Write-Host ""

    if (-not $Force) {
        $Confirm = Read-Host "Is this your USB stick? Type 'yes' to confirm (anything else aborts)"
        if ($Confirm -ne 'yes') {
            Write-Err "Confirmation was not 'yes'. Aborting."
        }
    }

    # --- Write the image ------------------------------------------
    Write-Info "Opening $Device for raw write..."

    # Open the physical drive for raw write access.
    # We use CreateFile from kernel32 (via P/Invoke) because PowerShell
    # doesn't have a native "open raw disk for writing" cmdlet.
    Add-Type -Namespace Win32 -Name Native -MemberDefinition @"
        [DllImport("kernel32.dll", SetLastError=true, CharSet=System.Runtime.InteropServices.CharSet.Auto)]
        public static extern System.IntPtr CreateFile(
            string lpFileName, uint dwDesiredAccess, uint dwShareMode,
            System.IntPtr lpSecurityAttributes, uint dwCreationDisposition,
            uint dwFlagsAndAttributes, System.IntPtr hTemplateFile);
        [DllImport("kernel32.dll", SetLastError=true)]
        public static extern bool WriteFile(
            System.IntPtr hFile, byte[] lpBuffer, uint nNumberOfBytesToWrite,
            out uint lpNumberOfBytesWritten, System.IntPtr lpOverlapped);
        [DllImport("kernel32.dll", SetLastError=true)]
        public static extern bool FlushFileBuffers(System.IntPtr hFile);
        [DllImport("kernel32.dll", SetLastError=true)]
        public static extern bool CloseHandle(System.IntPtr hObject);
        [DllImport("kernel32.dll")]
        public static extern uint GetLastError();
"@

    $GENERIC_WRITE = 0x40000000
    $GENERIC_READ  = 0x80000000
    $FILE_SHARE_NONE = 0
    $OPEN_EXISTING = 3

    $Handle = [Win32.Native]::CreateFile($Device, $GENERIC_WRITE -bor $GENERIC_READ, $FILE_SHARE_NONE, [IntPtr]::Zero, $OPEN_EXISTING, 0, [IntPtr]::Zero)
    if ($Handle -eq [IntPtr]-1) {
        $ErrCode = [System.Runtime.InteropServices.Marshal]::GetLastWin32Error()
        Write-Err "Failed to open $Device for writing (Win32 error $ErrCode). The disk may be in use - close any Disk Management windows, or run 'diskpart' then 'select disk $DiskNum' / 'clean' to clear it."
    }

    try {
        $Stream = [System.IO.File]::OpenRead($ImageFile)
        $BufferSize = 4MB
        $Buffer = New-Object byte[] $BufferSize
        $TotalBytes = 0
        $TotalSize = $Stream.Length
        $StartTime = Get-Date

        while ($true) {
            $Read = $Stream.Read($Buffer, 0, $BufferSize)
            if ($Read -eq 0) { break }
            if ($Read -lt $BufferSize) {
                $SmallBuffer = New-Object byte[] $Read
                [Array]::Copy($Buffer, $SmallBuffer, $Read)
                $Written = 0
                $Success = [Win32.Native]::WriteFile($Handle, $SmallBuffer, $Read, [ref]$Written, [IntPtr]::Zero)
            } else {
                $Written = 0
                $Success = [Win32.Native]::WriteFile($Handle, $Buffer, $BufferSize, [ref]$Written, [IntPtr]::Zero)
            }
            if (-not $Success) {
                $ErrCode = [System.Runtime.InteropServices.Marshal]::GetLastWin32Error()
                Write-Err "Write failed at offset $TotalBytes (Win32 error $ErrCode)."
            }
            $TotalBytes += $Written
            $Pct = [math]::Round(($TotalBytes / $TotalSize) * 100, 1)
            $Elapsed = (Get-Date) - $StartTime
            if ($Elapsed.TotalSeconds -gt 0) {
                $Rate = [math]::Round(($TotalBytes / 1MB) / $Elapsed.TotalSeconds, 1)
                $WrittenMB = [math]::Round($TotalBytes / 1MB)
                $TotalMB = [math]::Round($TotalSize / 1MB)
                # Build the progress string with explicit concatenation to avoid
                # PowerShell 5.1 parser issues with parentheses inside interpolation.
                $ProgressLine = "`r  " + $Pct + "% " + $WrittenMB + " MB / " + $TotalMB + " MB @ " + $Rate + " MB/s"
                Write-Host $ProgressLine -NoNewline
            }
        }
        Write-Host ""
        $Stream.Close()
    } finally {
        [void][Win32.Native]::FlushFileBuffers($Handle)
        [void][Win32.Native]::CloseHandle($Handle)
    }

    Write-OK "Write complete."

    # --- Next steps -----------------------------------------------
    Write-Host ""
    Write-Host "Next steps:" -ForegroundColor White
    Write-Host ""
    Write-Host "  1. Plug the USB into the test machine."
    Write-Host "  2. Reboot. Enter the boot menu (F12, F8, F11, or Esc - depends on vendor)."
    Write-Host "  3. Pick the USB from the list."
    Write-Host "  4. (If it refuses to boot) Disable Secure Boot - testOS UKIs are unsigned for now."
    Write-Host "  5. testOS boots, shows a menu of benchmarks."
    Write-Host "  6. Pick 'Run all' (0) or specific test numbers. Press Esc to abort."
    Write-Host "  7. When done, testOS syncs the USB and reboots back to the host OS."
    Write-Host "  8. Plug the USB back here, then pull the results:"
    Write-Host ""
    Write-Host "     # On Windows, the USB mounts with a drive letter (e.g. E:)."
    Write-Host "     # Copy the results off:"
    Write-Host "     Copy-Item -Path E:\testos-results\ -Destination .\ -Recurse"
    Write-Host ""
    Write-Host "     # Or, on a Linux machine:"
    Write-Host "     sudo testos-ingest pull /dev/sdX"
    Write-Host "     testos-ingest format"
    Write-Host "     testos-ingest commit"
    Write-Host "     git push"
    Write-Host ""
    Write-Host "  Results land in benchmarks/results/<date>/<host-fingerprint>/."

} finally {
    # Clean up the temp directory.
    if (Test-Path $WorkDir) {
        Remove-Item -Path $WorkDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
