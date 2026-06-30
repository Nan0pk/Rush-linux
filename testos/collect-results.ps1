# testos/collect-results.ps1 - collect testOS benchmark results from a USB on Windows.
#
# The Linux side has `testos-ingest` (a Rust binary) that mounts the USB's
# ESP partition, finds testos-results/, and copies it into the repo. This
# script is the Windows equivalent - pure PowerShell, no Linux binary needed.
#
# What it does:
#   1. Finds the USB disk that has a partition labeled RUSHESP (or just the
#      first FAT32 partition on a USB disk if labeling failed).
#   2. Mounts that partition to a drive letter if Windows didn't auto-mount.
#   3. Copies testos-results/ into the repo (default: ./benchmarks/results/).
#   4. Prints a summary of what was collected.
#
# Usage:
#   .\collect-results.ps1                          # auto-find USB, copy to ./benchmarks/results/
#   .\collect-results.ps1 -DiskNumber 1            # specify which USB disk
#   .\collect-results.ps1 -Destination C:\repo     # specify repo root
#   .\collect-results.ps1 -Diagnose                # just print diagnostics, don't copy
#   .\collect-results.ps1 -List                    # list results on USB, don't copy
#
# Requirements:
#   - Windows 10/11 with PowerShell 5.1+
#   - Administrator privileges (to mount partitions)
#
# Why this exists: after booting testOS on a test machine and running benchmarks,
# the results are written to the USB's ESP partition at testos-results/. Windows
# often doesn't auto-mount the ESP (it's a System partition type), so users had
# to manually run Get-Disk / Get-Partition / Add-PartitionAccessPath every time.
# This script automates that.

[CmdletBinding()]
param(
    [Parameter(Mandatory=$false)]
    [int]$DiskNumber,

    [Parameter(Mandatory=$false)]
    [string]$Destination = (Join-Path $PWD "benchmarks\results"),

    [switch]$Diagnose,
    [switch]$List,
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
testOS results collector for Windows - pull benchmark results off a USB.

Usage:
  .\collect-results.ps1                              Auto-find USB, copy to .\benchmarks\results\
  .\collect-results.ps1 -DiskNumber 1                Specify which USB disk
  .\collect-results.ps1 -Destination C:\repo\bench   Specify destination
  .\collect-results.ps1 -Diagnose                    Print diagnostics only, don't copy
  .\collect-results.ps1 -List                        List results on USB, don't copy
  .\collect-results.ps1 -Help                        This message

The script:
  1. Finds a USB disk with an ESP partition (labeled RUSHESP, or first FAT32).
  2. Mounts it to a drive letter if Windows didn't auto-mount.
  3. Copies testos-results\ into the destination.
  4. Prints a summary.

Requirements:
  - Administrator privileges (to mount partitions)
  - The USB must have been written by install.ps1 and booted on a test machine
'@ | Write-Host
    exit 0
}

# --- Admin check --------------------------------------------------
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Warn "Not running as Administrator. Partition mounting may fail."
    Write-Warn "Re-run from an elevated PowerShell: right-click PowerShell -> 'Run as Administrator'."
    Write-Warn "Continuing anyway in 3 seconds... (Ctrl-C to abort)"
    Start-Sleep -Seconds 3
}

# --- Diagnose mode: print everything and exit --------------------
if ($Diagnose) {
    Write-Host ""
    Write-Host "=== All disks ===" -ForegroundColor Cyan
    Get-Disk | Format-Table Number, FriendlyName, @{Name="SizeGB";Expression={[math]::Round($_.Size/1GB,1)}}, PartitionStyle, BusType, OperationalStatus -AutoSize

    Write-Host "=== USB disks only ===" -ForegroundColor Cyan
    $usbDisks = @(Get-Disk | Where-Object { $_.BusType -eq 'USB' })
    if ($usbDisks.Count -eq 0) {
        Write-Host "No USB disks found." -ForegroundColor Yellow
    } else {
        $usbDisks | Format-Table Number, FriendlyName, @{Name="SizeGB";Expression={[math]::Round($_.Size/1GB,1)}}, PartitionStyle, OperationalStatus -AutoSize
        foreach ($d in $usbDisks) {
            Write-Host "=== Partitions on Disk $($d.Number) ===" -ForegroundColor Cyan
            Get-Partition -DiskNumber $d.Number -ErrorAction SilentlyContinue | Format-Table PartitionNumber, DriveLetter, @{Name="SizeGB";Expression={[math]::Round($_.Size/1GB,2)}}, Type, GptType -AutoSize
        }
    }

    Write-Host "=== All volumes ===" -ForegroundColor Cyan
    Get-Volume | Format-Table DriveLetter, FileSystemLabel, FileSystem, @{Name="SizeGB";Expression={[math]::Round($_.Size/1GB,2)}}, DriveType -AutoSize

    Write-Host "=== Done ===" -ForegroundColor Cyan
    exit 0
}

# --- Find the USB disk -------------------------------------------
Write-Info "Scanning for USB disks..."
$usbDisks = @(Get-Disk | Where-Object { $_.BusType -eq 'USB' } | Sort-Object Number)

if ($usbDisks.Count -eq 0) {
    Write-Err "No USB disks found. Plug in the testOS USB and re-run. (Run with -Diagnose to see all disks.)"
}

if ($DiskNumber) {
    $TargetDisk = $usbDisks | Where-Object { $_.Number -eq $DiskNumber } | Select-Object -First 1
    if (-not $TargetDisk) {
        Write-Err "Disk $DiskNumber is not a USB disk (or doesn't exist). Run with -Diagnose to see all disks."
    }
    Write-OK "Using specified disk: Disk $($TargetDisk.Number) - $($TargetDisk.FriendlyName)"
} else {
    if ($usbDisks.Count -eq 1) {
        $TargetDisk = $usbDisks[0]
        Write-OK "Found 1 USB disk: Disk $($TargetDisk.Number) - $($TargetDisk.FriendlyName) ($([math]::Round($TargetDisk.Size/1GB,1)) GB)"
    } else {
        Write-Host "Multiple USB disks found:" -ForegroundColor White
        for ($i = 0; $i -lt $usbDisks.Count; $i++) {
            $d = $usbDisks[$i]
            Write-Host ("  [{0}] Disk {1} - {2} ({3} GB)" -f ($i+1), $d.Number, $d.FriendlyName, [math]::Round($d.Size/1GB,1))
        }
        $Choice = Read-Host "Select a USB disk by number (1-$($usbDisks.Count))"
        $ChoiceNum = 0
        if (-not [int]::TryParse($Choice, [ref]$ChoiceNum) -or $ChoiceNum -lt 1 -or $ChoiceNum -gt $usbDisks.Count) {
            Write-Err "Invalid selection '$Choice'. Enter a number 1-$($usbDisks.Count)."
        }
        $TargetDisk = $usbDisks[$ChoiceNum - 1]
    }
}

# --- Find the ESP partition on that disk -------------------------
Write-Info "Looking for the testOS ESP partition on Disk $($TargetDisk.Number)..."
$Partitions = @(Get-Partition -DiskNumber $TargetDisk.Number -ErrorAction SilentlyContinue)

if ($Partitions.Count -eq 0) {
    Write-Err "Disk $($TargetDisk.Number) has no partitions. The testOS image may not have been written correctly. Run with -Diagnose to inspect."
}

# The ESP partition is GPT type {c12a7328-f81f-11d2-ba4b-00a0c93ec93b}.
# Fall back to: any partition with a drive letter that has testos-results\,
# or the first FAT32 partition.
$ESP_GPT_TYPE = "{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}"
$EspPartition = $null

# Try 1: GPT type match
$EspPartition = $Partitions | Where-Object { $_.GptType -eq $ESP_GPT_TYPE } | Select-Object -First 1

# Try 2: already-mounted partition with testos-results
if (-not $EspPartition) {
    foreach ($p in $Partitions) {
        if ($p.DriveLetter) {
            $testPath = "$($p.DriveLetter):\testos-results"
            if (Test-Path $testPath) {
                $EspPartition = $p
                Write-Info "Found testos-results on already-mounted drive $($p.DriveLetter):"
                break
            }
        }
    }
}

# Try 3: first partition (ESP is usually partition 1 on testOS images)
if (-not $EspPartition) {
    $EspPartition = $Partitions | Sort-Object PartitionNumber | Select-Object -First 1
    Write-Warn "Could not find ESP by GPT type. Trying first partition (PartitionNumber $($EspPartition.PartitionNumber))."
}

if (-not $EspPartition) {
    Write-Err "Could not identify the ESP partition on Disk $($TargetDisk.Number). Run with -Diagnose to inspect."
}

Write-Info "ESP partition: PartitionNumber $($EspPartition.PartitionNumber), current drive letter: '$($EspPartition.DriveLetter)'"

# --- Mount the partition if needed --------------------------------
$DriveLetter = $EspPartition.DriveLetter
$MountedByUs = $false

if (-not $DriveLetter) {
    # Find an available drive letter (E: through Z:)
    $UsedLetters = @(Get-Volume | Where-Object { $_.DriveLetter } | ForEach-Object { $_.DriveLetter })
    $CandidateLetters = 69..90 | ForEach-Object { [char]$_ }  # E through Z
    $FreeLetter = $CandidateLetters | Where-Object { $_ -notin $UsedLetters } | Select-Object -First 1

    if (-not $FreeLetter) {
        Write-Err "No free drive letters available (E: through Z: all in use). Unmount something and re-run."
    }

    Write-Info "Mounting partition $($EspPartition.PartitionNumber) at ${FreeLetter}:\ ..."
    try {
        Add-PartitionAccessPath -DiskNumber $TargetDisk.Number -PartitionNumber $EspPartition.PartitionNumber -AccessPath "${FreeLetter}:\" -ErrorAction Stop
        $DriveLetter = $FreeLetter
        $MountedByUs = $true
        Write-OK "Mounted at ${DriveLetter}:\"
        # Give Windows a moment to recognize the volume
        Start-Sleep -Seconds 1
    } catch {
        Write-Err "Failed to mount partition $($EspPartition.PartitionNumber) at ${FreeLetter}:\ : $($_.Exception.Message). Try running 'diskpart' as admin, 'select disk $($TargetDisk.Number)', 'select partition $($EspPartition.PartitionNumber)', 'assign'."
    }
} else {
    Write-OK "Partition already mounted at ${DriveLetter}:\"
}

# --- Verify the partition has results -----------------------------
$ResultsRoot = "${DriveLetter}:\testos-results"

if (-not (Test-Path $ResultsRoot)) {
    Write-Host ""
    Write-Warn "No testos-results\ folder found at $ResultsRoot"
    Write-Warn "The USB's ESP partition is mounted but doesn't contain benchmark results."
    Write-Host ""
    Write-Host "Contents of ${DriveLetter}:\ :" -ForegroundColor White
    Get-ChildItem "${DriveLetter}\" -Force -ErrorAction SilentlyContinue | Format-Table Name, Length, LastWriteTime -AutoSize
    Write-Host ""
    Write-Host "Possible reasons:" -ForegroundColor White
    Write-Host "  - testOS didn't run any benchmarks (you quit before they completed)"
    Write-Host "  - testOS wrote results to a different path (check the testOS menu next time)"
    Write-Host "  - The USB was reformatted after the benchmark run"
    if ($MountedByUs) {
        Write-Host ""
        Write-Info "Leaving the partition mounted at ${DriveLetter}:\ so you can inspect it."
    }
    exit 1
}

Write-OK "Found results at $ResultsRoot"

# --- List mode: show what's there and exit -----------------------
if ($List) {
    Write-Host ""
    Write-Host "=== Results on USB ===" -ForegroundColor Cyan
    Get-ChildItem $ResultsRoot -Recurse | Format-Table FullName, Length, LastWriteTime -AutoSize
    exit 0
}

# --- Copy results to destination ----------------------------------
Write-Info "Copying results to: $Destination"

if (-not (Test-Path $Destination)) {
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
}

# Copy the entire testos-results tree. -Recurse to get subfolders,
# -Force to overwrite. The structure is testos-results/<date>/<host>/...
# Each run's directory also contains a system-logs/ subfolder with dmesg,
# journal, cpuinfo, etc. captured by the runner at the end of the run.
$CopiedFiles = 0
$CopiedBytes = 0

Get-ChildItem $ResultsRoot -Recurse -File | ForEach-Object {
    $RelativePath = $_.FullName.Substring($ResultsRoot.Length)
    $DestPath = Join-Path $Destination $RelativePath
    $DestDir = Split-Path $DestPath -Parent
    if (-not (Test-Path $DestDir)) {
        New-Item -ItemType Directory -Path $DestDir -Force | Out-Null
    }
    Copy-Item -Path $_.FullName -Destination $DestPath -Force
    $script:CopiedFiles++
    $script:CopiedBytes += $_.Length
}

Write-OK "Copied $CopiedFiles file(s) ($([math]::Round($CopiedBytes/1KB,1)) KB) to $Destination"

# --- Also copy install logs from the cache dir -------------------
# install.ps1 writes a transcript of every install session to
# %LOCALAPPDATA%\testos-installer\install-log-*.txt. Copy these into
# the destination's install-logs/ folder so they're collected alongside
# the benchmark results. This gives a complete picture: what was
# installed, when, and what the results were.
$InstallLogsDir = Join-Path $env:LOCALAPPDATA "testos-installer"
$DestInstallLogs = Join-Path (Split-Path $Destination -Parent) "install-logs"
if (Test-Path $InstallLogsDir) {
    $InstallLogs = Get-ChildItem $InstallLogsDir -Filter "install-log-*.txt" -ErrorAction SilentlyContinue
    if ($InstallLogs) {
        if (-not (Test-Path $DestInstallLogs)) {
            New-Item -ItemType Directory -Path $DestInstallLogs -Force | Out-Null
        }
        $LogCount = 0
        foreach ($log in $InstallLogs) {
            Copy-Item -Path $log.FullName -Destination $DestInstallLogs -Force
            $LogCount++
        }
        Write-OK "Copied $LogCount install log(s) to $DestInstallLogs"
    }
}

# --- Summary ------------------------------------------------------
Write-Host ""
Write-Host "=== Collected results ===" -ForegroundColor Cyan
Get-ChildItem $Destination -Recurse -File | Format-Table @{Name="Path";Expression={$_.FullName.Substring($Destination.Length)}}, @{Name="Size";Expression={"$([math]::Round($_.Length/1KB,1)) KB"}}, LastWriteTime -AutoSize

Write-Host ""
Write-Host "Next steps:" -ForegroundColor White
Write-Host "  1. Review the results in $Destination"
Write-Host "  2. Commit them to the repo:"
Write-Host "       git add $Destination"
Write-Host "       git commit -m `"benchmarks: add testOS results from $(Get-Date -Format 'yyyy-MM-dd')`""
Write-Host "       git push"
Write-Host ""
if ($MountedByUs) {
    Write-Host "The USB partition is still mounted at ${DriveLetter}:\. You can:"
    Write-Host "  - Browse it:  explorer ${DriveLetter}:\"
    Write-Host "  - Unmount it: Remove-PartitionAccessPath -DiskNumber $($TargetDisk.Number) -PartitionNumber $($EspPartition.PartitionNumber) -AccessPath ${DriveLetter}:\"
}
