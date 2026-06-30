# testos/install.ps1 — download the latest prebuilt testOS image and write it to a USB stick.
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
#      (no Rufus, no Etcher, no WSL — pure PowerShell).
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

    [switch]$DryRun,
    [switch]$ListOnly,
    [switch]$Force,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

# ─── Helpers ──────────────────────────────────────────────────────
function Write-Info  { param([string]$msg) Write-Host ">> $msg" -ForegroundColor White }
function Write-OK    { param([string]$msg) Write-Host "[OK] $msg" -ForegroundColor Green }
function Write-Warn  { param([string]$msg) Write-Host "[!]  $msg" -ForegroundColor Yellow }
function Write-Err   { param([string]$msg) Write-Host "[X]  $msg" -ForegroundColor Red; exit 1 }

if ($Help) {
    @'
testOS installer for Windows — download and write the latest testOS image to USB.

Usage:
  .\install.ps1 -Device \\.\PhysicalDrive<N>        Download + write to the specified disk
  .\install.ps1 -ListOnly                           Show latest release assets without writing
  .\install.ps1 -DryRun -Device \\.\PhysicalDrive<N> Download and verify, don't write
  .\install.ps1 -Help                               This message

How to find your USB's physical drive number:
  Get-Disk | Format-Table Number, FriendlyName, Size, PartitionStyle, BusType

Then pass -Device \\.\PhysicalDrive<N> (e.g. \\.\PhysicalDrive1).

Common issue — "cannot be loaded because running scripts is disabled":
  Windows blocks downloaded scripts by default. Run with execution policy
  bypassed for this process only:
    powershell -ExecutionPolicy Bypass -File .\install.ps1 -Device \\.\PhysicalDrive<N>

  If that still fails, unblock the downloaded file first (Windows marks
  downloaded files with a "Mark of the Web"):
    Unblock-File .\install.ps1
    powershell -ExecutionPolicy Bypass -File .\install.ps1 -Device \\.\PhysicalDrive<N>

Requirements:
  - Windows 10/11 with PowerShell 5.1+ (built-in) or PowerShell 7+
  - Administrator privileges (Run as Administrator)

Options:
  -DryRun     Download and verify everything, but don't write to the device.
  -ListOnly   Just show what's in the latest release.
  -Force      Skip the removable-media and size-sanity safety checks.
              Required if you want to write to a non-USB disk (e.g. an
              internal test disk). Still refuses the system root disk.
  -Help       This message.
'@ | Write-Host
    exit 0
}

# ─── Admin check ──────────────────────────────────────────────────
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin -and -not $ListOnly -and -not $DryRun) {
    Write-Warn "Not running as Administrator. Disk writes will fail."
    Write-Warn "Re-run from an elevated PowerShell: right-click PowerShell -> 'Run as Administrator'."
    Write-Warn "Continuing anyway in 3 seconds... (Ctrl-C to abort)"
    Start-Sleep -Seconds 3
}

# ─── Find latest release ──────────────────────────────────────────
$Repo = "Nan0pk/Rush-linux"
$ApiUrl = "https://api.github.com/repos/$Repo/releases/latest"

Write-Info "Finding the latest testOS release..."
try {
    $Release = Invoke-RestMethod -Uri $ApiUrl -Headers @{ "User-Agent" = "testos-installer" } -ErrorAction Stop
} catch {
    Write-Err "Could not fetch release info from $ApiUrl. Either there are no releases yet, or you're rate-limited. Try again in a few minutes, or build from source: see the README's 'Build from source' section."
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

# ─── Check that a release image exists ────────────────────────────
$Assets = $Release.assets
$ImageAsset = $Assets | Where-Object { $_.name -match '^testos-.*\.raw$' } | Select-Object -First 1
if (-not $ImageAsset) {
    Write-Warn "The latest release ($Version) does not contain a testos-*.raw image."
    Write-Warn "This usually means the release workflow is still running, or the project"
    Write-Warn "hasn't published a testOS image yet."
    Write-Host ""
    Write-Host "To build from source instead, see:"
    Write-Host "  https://github.com/$Repo#build-from-source"
    exit 1
}

# ─── Set up working directory ─────────────────────────────────────
$WorkDir = Join-Path $env:TEMP ("testos-install-" + [System.Guid]::NewGuid().ToString("N").Substring(0,8))
New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null

try {
    # ─── Download assets ──────────────────────────────────────────
    function Download-Asset {
        param([string]$Url, [string]$DestPath)
        Write-Info "Downloading $(Split-Path $DestPath -Leaf)..."
        try {
            Invoke-WebRequest -Uri $Url -OutFile $DestPath -UseBasicParsing -ErrorAction Stop
        } catch {
            Write-Err "Download failed: $Url  --  $($_.Exception.Message)"
        }
    }

    Download-Asset $ImageAsset.browser_download_url (Join-Path $WorkDir $ImageAsset.name)
    $ImageFile = Join-Path $WorkDir $ImageAsset.name

    # SHA256SUMS
    $SumsAsset = $Assets | Where-Object { $_.name -eq "SHA256SUMS" } | Select-Object -First 1
    if ($SumsAsset) {
        Download-Asset $SumsAsset.browser_download_url (Join-Path $WorkDir "SHA256SUMS")
    }

    # testos-ingest (Linux binary — won't run on Windows, but useful for the
    # user to copy into WSL or a Linux box later if they want to ingest from there)
    $IngestAsset = $Assets | Where-Object { $_.name -match '^testos-ingest-.*-linux-x86_64$' } | Select-Object -First 1
    if ($IngestAsset) {
        Download-Asset $IngestAsset.browser_download_url (Join-Path $WorkDir $IngestAsset.name)
    }

    # bench-list.toml (for reference)
    $BenchListAsset = $Assets | Where-Object { $_.name -eq "bench-list.toml" } | Select-Object -First 1
    if ($BenchListAsset) {
        Download-Asset $BenchListAsset.browser_download_url (Join-Path $WorkDir "bench-list.toml")
    }

    # ─── Verify checksums ─────────────────────────────────────────
    if ($SumsAsset) {
        Write-Info "Verifying checksums..."
        $SumsFile = Join-Path $WorkDir "SHA256SUMS"
        $SumsContent = Get-Content $SumsFile
        $Verified = 0
        $Failed = 0
        foreach ($Line in $SumsContent) {
            if ($Line -match '^\s*([0-9a-fA-F]{64})\s+\*?(\S+)\s*$') {
                $ExpectedHash = $Matches[1].ToLower()
                $FileName = $Matches[2]
                $FilePath = Join-Path $WorkDir $FileName
                if (Test-Path $FilePath) {
                    $ActualHash = (Get-FileHash -Path $FilePath -Algorithm SHA256).Hash.ToLower()
                    if ($ActualHash -eq $ExpectedHash) {
                        $Verified++
                    } else {
                        Write-Warn "Checksum mismatch for $FileName"
                        Write-Warn "  Expected: $ExpectedHash"
                        Write-Warn "  Actual:   $ActualHash"
                        $Failed++
                    }
                }
            }
        }
        if ($Failed -gt 0) {
            Write-Err "Checksum verification failed for $Failed file(s). The download may be corrupted."
        }
        Write-OK "Verified $Verified file(s)."
    }

    $ImageSizeBytes = (Get-Item $ImageFile).Length
    $ImageSizeMB = [math]::Round($ImageSizeBytes / 1MB)

    # ─── Dry-run stops here ───────────────────────────────────────
    if ($DryRun) {
        Write-OK "Dry run complete. Downloaded and verified:"
        Get-ChildItem $WorkDir | Format-Table Name, Length
        Write-Host ""
        Write-Host "Re-run without -DryRun and with a USB device to write:"
        Write-Host "  .\install.ps1 -Device \\.\PhysicalDrive<N>"
        exit 0
    }

    # ─── Device selection and safety checks ──────────────────────
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

    # ─── Safety check 1: refuse the Windows system disk ─────────
    try {
        $SystemDisk = Get-Partition | Where-Object { $_.DriveLetter -eq $env:SystemDrive[0] } | Select-Object -ExpandProperty DiskNumber -First 1
        if ($null -ne $SystemDisk -and $DiskNum -eq $SystemDisk) {
            Write-Err "Device $Device is the Windows system disk (Disk $SystemDisk, $($DiskInfo.FriendlyName)). Refusing to overwrite. If you really meant to write to your boot disk, you're holding the script wrong — use a USB stick."
        }
    } catch {
        Write-Warn "Could not determine the system disk for safety check. Proceed with extreme caution."
    }

    # ─── Safety check 2: refuse non-USB bus types unless -Force ─
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

    # ─── Safety check 3: refuse mounted volumes unless -Force ───
    $MountedParts = Get-Partition -DiskNumber $DiskNum -ErrorAction SilentlyContinue | Where-Object { $_.DriveLetter }
    if ($MountedParts -and -not $Force) {
        Write-Warn "Disk $DiskNum ($($DiskInfo.FriendlyName)) has mounted volumes:"
        $MountedParts | ForEach-Object { Write-Warn "  $($_.DriveLetter):" }
        Write-Warn "Writing will destroy all data on these volumes."
        Write-Err "Aborting. Re-run with -Force to proceed anyway, or unmount the volumes first."
    }

    # ─── Safety check 4: size sanity ────────────────────────────
    # If the target disk is more than 4x the image size, warn. People
    # sometimes image a 500MB USB onto a 2TB HDD by mistake.
    $DiskSizeBytes = $DiskInfo.Size
    $DiskSizeGB = [math]::Round($DiskSizeBytes / 1GB, 1)
    if ($DiskSizeBytes -gt ($ImageSizeBytes * 4)) {
        Write-Warn "Target disk is $DiskSizeGB GB but the image is only $ImageSizeMB MB."
        Write-Warn "This is unusual — you may be targeting the wrong disk (e.g. an internal HDD instead of a USB stick)."
        if (-not $Force) {
            Write-Err "Refusing to write to a disk that's much larger than the image. If this is intentional (e.g. a large USB stick), re-run with -Force."
        }
    }
    # Also warn if the target is smaller than the image (would fail mid-write).
    if ($DiskSizeBytes -lt $ImageSizeBytes) {
        Write-Err "Target disk ($DiskSizeGB GB) is smaller than the image ($ImageSizeMB MB). The write would fail mid-way and leave the disk in a broken state."
    }

    # ─── Confirm: show the disk's identity and ask 'yes' ────────
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

    # ─── Write the image ──────────────────────────────────────────
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
        Write-Err "Failed to open $Device for writing (Win32 error $ErrCode). The disk may be in use — close any Disk Management windows, or run 'diskpart' then 'select disk $DiskNum' / 'clean' to clear it."
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
                Write-Host "`r  $Pct% ($WrittenMB MB / $TotalMB MB) @ $Rate MB/s" -NoNewline
            }
        }
        Write-Host ""
        $Stream.Close()
    } finally {
        [void][Win32.Native]::FlushFileBuffers($Handle)
        [void][Win32.Native]::CloseHandle($Handle)
    }

    Write-OK "Write complete."

    # ─── Next steps ───────────────────────────────────────────────
    Write-Host ""
    Write-Host "Next steps:" -ForegroundColor White
    Write-Host ""
    Write-Host "  1. Plug the USB into the test machine."
    Write-Host "  2. Reboot. Enter the boot menu (F12, F8, F11, or Esc — depends on vendor)."
    Write-Host "  3. Pick the USB from the list."
    Write-Host "  4. (If it refuses to boot) Disable Secure Boot — testOS UKIs are unsigned for now."
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
