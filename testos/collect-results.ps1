# testos/collect-results.ps1 - ONE-COMMAND results collection + commit + push.
#
# Usage (the only command the user needs to run after booting testOS):
#   .\collect-results.ps1
#
# What it does, end to end, no manual steps:
#   1. Finds the USB disk (auto-select if one, picker if multiple).
#   2. Mounts the ESP partition if Windows didn't auto-mount it.
#   3. Copies testos-results\ + install logs into the repo.
#   4. Validates the results (checks manifest.json for pass/fail counts).
#   5. Clones or pulls the repo (so the commit is on top of latest main).
#   6. Commits the results with a conventional commit message.
#   7. Pushes to main via a PR (since main is branch-protected).
#   8. Leaves the PR open for checks and maintainer review.
#   9. Cleans up the temporary clone.
#
# The user just runs this one script. Everything else is automatic.
#
# Requirements:
#   - Windows 10/11 with PowerShell 5.1+
#   - Administrator privileges (to mount partitions)
#   - The GITHUB_TOKEN env var set (or pass -GitHubToken). The token is
#     scoped to the Rush-linux repo and used for git push and PR creation.
#     # SECURITY (audit finding #6): the token is held in the process environment
# and passed to git via http.extraheader (a per-invocation git -c setting).
# It is NOT embedded in the clone URL and is NOT written to .git/config.
# The previous version embedded the token in the clone URL, which Git
# stored in .git/config remote.origin.url — contradicting the script's
# assertion that the token was never written to disk. Dry-run mode
# retained the work directory (including .git/config with the token).
# This version never writes the token to .git/config at all.

[CmdletBinding()]
param(
    [Parameter(Mandatory=$false)]
    [int]$DiskNumber,

    # Repo to commit to. Defaults to the project repo.
    [string]$Repo = "Nan0pk/Rush-linux",

    # GitHub token for git push + PR creation. If not passed, reads from
    # $env:GITHUB_TOKEN. Must have repo + contents:write scope.
    [string]$GitHubToken = $env:GITHUB_TOKEN,

    # Where to put the temporary clone. Default: temp dir.
    [string]$WorkDir = (Join-Path $env:TEMP "testos-collect-$([System.Guid]::NewGuid().ToString('N').Substring(0,8))"),

    [switch]$Diagnose,
    [switch]$List,
    [switch]$DryRun,
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
testOS results collector - ONE command, end to end.

Usage:
  .\collect-results.ps1                              Auto: find USB, copy, commit, push
  .\collect-results.ps1 -DiskNumber 1                Specify which USB
  .\collect-results.ps1 -DryRun                      Do everything except push
  .\collect-results.ps1 -Diagnose                    Just print disk diagnostics
  .\collect-results.ps1 -List                        List results on USB, don't commit
  .\collect-results.ps1 -Help                        This message

Environment:
  $env:GITHUB_TOKEN must be set (or pass -GitHubToken). Needs repo scope.

What it does:
  1. Finds the USB, mounts the ESP, copies testos-results\ + install logs
  2. Validates results (reads manifest.json for pass/fail counts)
  3. Clones/pulls the repo to a temp dir
  4. Commits the results with a conventional message
  5. Pushes to a branch and opens a PR for maintainer review
  6. Cleans up the temp clone

No manual git commands needed. No manual mount commands. No manual PR.
'@ | Write-Host
    exit 0
}

# --- Diagnose mode ------------------------------------------------
if ($Diagnose) {
    Write-Host "=== All disks ===" -ForegroundColor Cyan
    Get-Disk | Format-Table Number, FriendlyName, @{Name="SizeGB";Expression={[math]::Round($_.Size/1GB,1)}}, PartitionStyle, BusType, OperationalStatus -AutoSize
    Write-Host "=== USB disks ===" -ForegroundColor Cyan
    @(Get-Disk | Where-Object { $_.BusType -eq 'USB' }) | Format-Table Number, FriendlyName, @{Name="SizeGB";Expression={[math]::Round($_.Size/1GB,1)}}, OperationalStatus -AutoSize
    foreach ($d in @(Get-Disk | Where-Object { $_.BusType -eq 'USB' })) {
        Write-Host "=== Partitions on Disk $($d.Number) ===" -ForegroundColor Cyan
        Get-Partition -DiskNumber $d.Number -ErrorAction SilentlyContinue | Format-Table PartitionNumber, DriveLetter, @{Name="SizeGB";Expression={[math]::Round($_.Size/1GB,2)}}, Type, GptType -AutoSize
    }
    Write-Host "=== Volumes ===" -ForegroundColor Cyan
    Get-Volume | Format-Table DriveLetter, FileSystemLabel, FileSystem, @{Name="SizeGB";Expression={[math]::Round($_.Size/1GB,2)}}, DriveType -AutoSize
    exit 0
}

# --- Token check --------------------------------------------------
if (-not $GitHubToken) {
    Write-Err "No GitHub token. Set `$env:GITHUB_TOKEN or pass -GitHubToken. The token needs contents and pull-request write access, not merge access."
}

# --- Admin check --------------------------------------------------
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Warn "Not running as Administrator. Partition mounting may fail."
    Write-Warn "Re-run from an elevated PowerShell."
    Start-Sleep -Seconds 2
}

# --- Find the USB disk -------------------------------------------
Write-Info "Scanning for USB disks..."
$usbDisks = @(Get-Disk | Where-Object { $_.BusType -eq 'USB' } | Sort-Object Number)
if ($usbDisks.Count -eq 0) {
    Write-Err "No USB disks found. Plug in the testOS USB and re-run. (Run with -Diagnose to see all disks.)"
}
if ($DiskNumber) {
    $TargetDisk = $usbDisks | Where-Object { $_.Number -eq $DiskNumber } | Select-Object -First 1
    if (-not $TargetDisk) { Write-Err "Disk $DiskNumber is not a USB disk." }
    Write-OK "Using disk $DiskNumber - $($TargetDisk.FriendlyName)"
} elseif ($usbDisks.Count -eq 1) {
    $TargetDisk = $usbDisks[0]
    Write-OK "Found 1 USB disk: Disk $($TargetDisk.Number) - $($TargetDisk.FriendlyName)"
} else {
    Write-Host "Multiple USB disks found:"
    for ($i = 0; $i -lt $usbDisks.Count; $i++) {
        $d = $usbDisks[$i]
        Write-Host ("  [{0}] Disk {1} - {2} ({3} GB)" -f ($i+1), $d.Number, $d.FriendlyName, [math]::Round($d.Size/1GB,1))
    }
    $Choice = Read-Host "Select a USB disk by number (1-$($usbDisks.Count))"
    $ChoiceNum = 0
    if (-not [int]::TryParse($Choice, [ref]$ChoiceNum) -or $ChoiceNum -lt 1 -or $ChoiceNum -gt $usbDisks.Count) {
        Write-Err "Invalid selection."
    }
    $TargetDisk = $usbDisks[$ChoiceNum - 1]
}

# --- Find + mount the ESP partition -------------------------------
$Partitions = @(Get-Partition -DiskNumber $TargetDisk.Number -ErrorAction SilentlyContinue)
if ($Partitions.Count -eq 0) { Write-Err "Disk $($TargetDisk.Number) has no partitions." }
$ESP_GPT = "{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}"
$EspPartition = $Partitions | Where-Object { $_.GptType -eq $ESP_GPT } | Select-Object -First 1
if (-not $EspPartition) {
    foreach ($p in $Partitions) {
        if ($p.DriveLetter -and (Test-Path "$($p.DriveLetter):\testos-results")) {
            $EspPartition = $p; break
        }
    }
}
if (-not $EspPartition) { $EspPartition = $Partitions | Sort-Object PartitionNumber | Select-Object -First 1 }
if (-not $EspPartition) { Write-Err "Could not find ESP partition." }

$DriveLetter = $EspPartition.DriveLetter
$MountedByUs = $false
if (-not $DriveLetter) {
    $Used = @(Get-Volume | Where-Object { $_.DriveLetter } | ForEach-Object { $_.DriveLetter })
    $Free = 69..90 | ForEach-Object { [char]$_ } | Where-Object { $_ -notin $Used } | Select-Object -First 1
    if (-not $Free) { Write-Err "No free drive letters." }
    Write-Info "Mounting partition at ${Free}:\..."
    try {
        Add-PartitionAccessPath -DiskNumber $TargetDisk.Number -PartitionNumber $EspPartition.PartitionNumber -AccessPath "${Free}:\" -ErrorAction Stop
        $DriveLetter = $Free; $MountedByUs = $true
        Start-Sleep -Seconds 1
        Write-OK "Mounted at ${DriveLetter}:\"
    } catch {
        Write-Err "Failed to mount: $($_.Exception.Message)"
    }
} else {
    Write-OK "Already mounted at ${DriveLetter}:\"
}

# --- Verify results exist -----------------------------------------
$ResultsRoot = "${DriveLetter}:\testos-results"
if (-not (Test-Path $ResultsRoot)) {
    Write-Warn "No testos-results\ at $ResultsRoot"
    Write-Host "Contents of ${DriveLetter}:\ :"
    Get-ChildItem "${DriveLetter}\" -Force -ErrorAction SilentlyContinue | Format-Table Name, Length, LastWriteTime -AutoSize
    Write-Err "No results found. Did testOS actually run benchmarks?"
}
Write-OK "Found results at $ResultsRoot"

# --- List mode ----------------------------------------------------
if ($List) {
    Write-Host "=== Results on USB ===" -ForegroundColor Cyan
    Get-ChildItem $ResultsRoot -Recurse | Format-Table FullName, Length, LastWriteTime -AutoSize
    if ($MountedByUs) { try { Remove-PartitionAccessPath -DiskNumber $TargetDisk.Number -PartitionNumber $EspPartition.PartitionNumber -AccessPath "${DriveLetter}:\" -ErrorAction SilentlyContinue } catch {} }
    exit 0
}

# --- Find the latest run ------------------------------------------
$Runs = @(Get-ChildItem $ResultsRoot -Directory | Sort-Object Name -Descending)
if ($Runs.Count -eq 0) { Write-Err "No run directories in $ResultsRoot" }
$LatestRun = $Runs[0]
Write-OK "Latest run: $($LatestRun.Name)"

# --- Validate the results (read manifest.json) --------------------
$ManifestPath = Join-Path $LatestRun.FullName "manifest.json"
$Validation = $null
if (Test-Path $ManifestPath) {
    try {
        $Manifest = Get-Content $ManifestPath -Raw | ConvertFrom-Json
        $passed = $Manifest.passed.Count
        $failed = $Manifest.failed.Count
        $skipped = $Manifest.skipped.Count
        $total = $passed + $failed + $skipped
        Write-OK "Run summary: $passed passed, $failed failed, $skipped skipped ($total total)"
        $Validation = @{
            Passed = $passed
            Failed = $failed
            Skipped = $skipped
            Total = $total
            Host = $Manifest.host.fingerprint
            StartedAt = $Manifest.started_at
        }
    } catch {
        Write-Warn "Could not parse manifest.json: $($_.Exception.Message)"
    }
} else {
    Write-Warn "No manifest.json in run directory."
}

# --- Clone or pull the repo ---------------------------------------
# SECURITY (audit finding #6): the previous version embedded the token in
# the clone URL (https://x-access-token:$GitHubToken@github.com/...),
# which Git stored in .git/config remote.origin.url. Even after the script
# unset branch.*.remote, the token remained in remote.origin.url and was
# retained on disk in dry-run mode.
#
# This version uses git's http.extraheader mechanism via `git -c`:
#   1. Build a base64-encoded Basic auth header with the token.
#   2. Pass it as `git -c http.https://github.com/.extraheader=...`.
#   3. Clone from the SAFE URL (no token in URL).
#   4. The -c setting is process-local; it is NOT persisted to .git/config.
# After push, a defensive `git remote set-url origin $SafeUrl` runs as
# belt-and-braces, and a verification step checks that no token leaked
# into .git/config.
Write-Info "Preparing repo clone at $WorkDir..."
$SafeUrl = "https://github.com/$Repo.git"

# Build the per-invocation extraheader. base64 of "x-access-token:$GitHubToken".
$B64Auth = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes("x-access-token:$GitHubToken"))
$ExtraHeader = "http.https://github.com/.extraheader=Authorization: Basic $B64Auth"

if (Test-Path $WorkDir) {
    Remove-Item $WorkDir -Recurse -Force -ErrorAction SilentlyContinue
}
New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null

Write-Info "Cloning $Repo (shallow, depth 1) from safe URL (no token in URL)..."
$CloneResult = & git -c $ExtraHeader clone --depth 1 $SafeUrl $WorkDir 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Err "git clone failed: $CloneResult"
}
Write-OK "Cloned. Token was passed via http.extraheader (not in URL, not in .git/config)."

# --- Copy results into the clone ----------------------------------
$DestResults = Join-Path $WorkDir "benchmarks\results"
if (-not (Test-Path $DestResults)) { New-Item -ItemType Directory -Path $DestResults -Force | Out-Null }

$CopiedFiles = 0
Get-ChildItem $ResultsRoot -Recurse -File | ForEach-Object {
    $Rel = $_.FullName.Substring($ResultsRoot.Length)
    $Dest = Join-Path $DestResults $Rel
    $DestDir = Split-Path $Dest -Parent
    if (-not (Test-Path $DestDir)) { New-Item -ItemType Directory -Path $DestDir -Force | Out-Null }
    Copy-Item -Path $_.FullName -Destination $Dest -Force
    $script:CopiedFiles++
}
Write-OK "Copied $CopiedFiles result file(s) to clone."

# Also copy install logs from cache
$InstallLogsSrc = Join-Path $env:LOCALAPPDATA "testos-installer"
$InstallLogsDest = Join-Path $WorkDir "install-logs"
if ((Test-Path $InstallLogsSrc) -and (Get-ChildItem $InstallLogsSrc -Filter "install-log-*.txt" -ErrorAction SilentlyContinue)) {
    if (-not (Test-Path $InstallLogsDest)) { New-Item -ItemType Directory -Path $InstallLogsDest -Force | Out-Null }
    $LogCount = 0
    Get-ChildItem $InstallLogsSrc -Filter "install-log-*.txt" | ForEach-Object {
        Copy-Item -Path $_.FullName -Destination $InstallLogsDest -Force
        $LogCount++
    }
    Write-OK "Copied $LogCount install log(s) to clone."
}

# --- Create branch, commit, push ----------------------------------
$DateStr = Get-Date -Format "yyyyMMdd-HHmmss"
$BranchName = "benchmarks/testos-$DateStr"
$CommitMsg = if ($Validation) {
    "benchmarks(testos): add results from $($Validation.StartedAt) - $($Validation.Passed) passed, $($Validation.Failed) failed"
} else {
    "benchmarks(testos): add results from $DateStr"
}

Write-Info "Creating branch $BranchName..."
Push-Location $WorkDir
try {
    & git checkout -b $BranchName 2>&1 | Out-Null
    & git add benchmarks/results/ install-logs/ 2>&1 | Out-Null
    & git -c user.email="testos-bot@local" -c user.name="testOS collector" commit -m $CommitMsg 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Err "git commit failed." }

    if ($DryRun) {
        Write-Info "DryRun: skipping push. Branch $BranchName is ready in $WorkDir"
    } else {
        Write-Info "Pushing branch (token via http.extraheader, not in URL)..."
        $PushResult = & git -c $ExtraHeader push $SafeUrl $BranchName 2>&1
        if ($LASTEXITCODE -ne 0) { Write-Err "git push failed: $PushResult" }
        Write-OK "Pushed."
        # SECURITY (audit finding #6): defensively scrub the remote URL even
        # though we never wrote the token to it. Belt-and-braces: if a future
        # refactor accidentally uses the token-URL approach, this line
        # overwrites it with the safe URL.
        & git remote set-url origin $SafeUrl 2>$null
        & git config --local --unset "branch.$BranchName.remote" 2>$null
        & git config --local --unset "branch.$BranchName.merge" 2>$null
        # Verify no token remains in .git/config. If it does, warn loudly.
        $ConfigCheck = & git config --local --list 2>&1 | Select-String -Pattern 'github_pat','x-access-token' -SimpleMatch
        if ($ConfigCheck) {
            Write-Warn "WARNING: token found in .git/config after scrub. Manual cleanup required:"
            $ConfigCheck | ForEach-Object { Write-Warn $_.ToString() }
        }
    }
} finally {
    Pop-Location
}

if ($DryRun) {
    Write-OK "Dry run complete. Temp clone at $WorkDir (not cleaned up for inspection)."
    if ($MountedByUs) { try { Remove-PartitionAccessPath -DiskNumber $TargetDisk.Number -PartitionNumber $EspPartition.PartitionNumber -AccessPath "${DriveLetter}:\" -ErrorAction SilentlyContinue } catch {} }
    exit 0
}

# --- Open PR ------------------------------------------------------
Write-Info "Opening PR..."
$PrBody = if ($Validation) {
    "Auto-collected from USB by collect-results.ps1.`n`n**Run summary:**`n- Date: $($Validation.StartedAt)`n- Host: $($Validation.Host)`n- Passed: $($Validation.Passed)`n- Failed: $($Validation.Failed)`n- Skipped: $($Validation.Skipped)`n`nIncludes per-benchmark JSON results, system logs (dmesg/journal/cpuinfo), and install logs."
} else {
    "Auto-collected from USB by collect-results.ps1."
}

$PrPayload = @{
    title = "benchmarks(testos): results from $DateStr"
    head = $BranchName
    base = "main"
    body = $PrBody
} | ConvertTo-Json -Depth 5

$PrResponse = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/pulls" -Method Post -Headers @{ "Authorization" = "Bearer $GitHubToken"; "Accept" = "application/vnd.github+json" } -Body $PrPayload -ErrorAction Stop
$PrNumber = $PrResponse.number
Write-OK "Opened PR #$PrNumber - $($PrResponse.html_url)"

# --- Cleanup ------------------------------------------------------
Write-Info "Cleaning up temp clone..."
Remove-Item $WorkDir -Recurse -Force -ErrorAction SilentlyContinue

if ($MountedByUs) {
    Write-Info "Unmounting USB partition..."
    try { Remove-PartitionAccessPath -DiskNumber $TargetDisk.Number -PartitionNumber $EspPartition.PartitionNumber -AccessPath "${DriveLetter}:\" -ErrorAction SilentlyContinue } catch {}
}

Write-Host ""
Write-OK "Done. The evidence PR is open for CI and maintainer review."
if ($PrResponse) {
    Write-Host "PR: $($PrResponse.html_url)" -ForegroundColor Cyan
}
