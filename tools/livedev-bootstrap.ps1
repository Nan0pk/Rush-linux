# tools/livedev-bootstrap.ps1 - ONE-command Rush LiveDev workflow for Windows.
#
# Usage:
#   .\livedev-bootstrap.ps1                       # SMART: auto-detect and do everything
#   .\livedev-bootstrap.ps1 -Smart                # same as above (explicit)
#   .\livedev-bootstrap.ps1 -Auto                 # force USB/testOS prepare path
#   .\livedev-bootstrap.ps1 -Resume               # force resume path
#   .\livedev-bootstrap.ps1 -Resume -Submit       # resume + open real PR
#   .\livedev-bootstrap.ps1 -DryRun               # show what would run
#
# SMART mode (default) auto-detects:
#   1. If a USB with testOS results is plugged in → resume + validate + submit.
#   2. Else → prepare USB (testOS path), print boot instructions.
#      After reboot, re-running the same command resumes (step 1).
#
# What this script does NOT do:
#   - Never auto-merge. PRs are opened for maintainer review only.
#   - Never mark milestones verified.
#   - Never edit release truth.
#   - Never fabricate hardware evidence.
#   - Never print or store tokens.

[CmdletBinding()]
param(
    [switch]$Smart,
    [switch]$Auto,
    [switch]$Resume,
    [switch]$DryRun,
    [switch]$SkipMock,
    [switch]$Submit,
    [string]$Device,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

$RepoUrl = "https://github.com/Nan0pk/Rush-linux.git"
$RepoHost = "Nan0pk/Rush-linux"
$WorkDirName = "Rush-linux"

# --- Env overrides ----------------------------------------------------------
# RUSH_LIVEDEV_REPO_DIR:     use this path as the repo. If missing, clone there.
# RUSH_LIVEDEV_SOURCE_REPO:  clone from this local path instead of GitHub.
#                            (Used by RUSH_LIVEDEV_TEST_STUB so tests do not
#                            touch the network.)
# RUSH_LIVEDEV_TEST_STUB:    real repo resolution still runs, but USB write,
#                            reboot instructions requiring action, PR
#                            submission, and real hardware are skipped.
$TestStub = $env:RUSH_LIVEDEV_TEST_STUB
$SourceRepo = $env:RUSH_LIVEDEV_SOURCE_REPO
$RepoDirOverride = $env:RUSH_LIVEDEV_REPO_DIR

# --- Helpers ------------------------------------------------------
function Write-Info  { param([string]$msg) Write-Host ">> $msg" -ForegroundColor White }
function Write-OK    { param([string]$msg) Write-Host "[OK] $msg" -ForegroundColor Green }
function Write-Warn  { param([string]$msg) Write-Host "[!]  $msg" -ForegroundColor Yellow }
function Write-Err   { param([string]$msg) Write-Host "[X]  $msg" -ForegroundColor Red; exit 1 }

# --- Python detection (Windows) -------------------------------------------
# On Windows, `python3` often hits the Microsoft Store stub which prints
# "Python was not found" and exits non-zero. We try `python`, `py -3`,
# `python3`, in that order, and verify each actually works by running
# `--version`. The first one that works is cached in $script:PythonBin.
$script:PythonBin = ""

function Get-Python {
    if ($script:PythonBin -ne "") { return $script:PythonBin }
    $candidates = @("python", "py", "python3")
    foreach ($c in $candidates) {
        $cmd = $c
        $args = @("--version")
        if ($c -eq "py") { $args = @("-3", "--version") }
        try {
            $out = & $cmd @args 2>&1
            if ($LASTEXITCODE -eq 0 -and "$out" -match "^Python [0-9]") {
                $script:PythonBin = if ($c -eq "py") { "py -3" } else { $c }
                return $script:PythonBin
            }
        } catch {
            # Try next candidate.
        }
    }
    Write-Err "Python not found. Install Python 3 from https://python.org (or 'winget install Python.Python.3') and re-run."
}

function Invoke-Python {
    # Run python with the given args. Handles `py -3` which needs the -3
    # flag before the script path.
    param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Args)
    $py = Get-Python
    if ($py -eq "py -3") {
        & py -3 @Args
    } else {
        & $py @Args
    }
}

function Invoke-PythonCapture {
    param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Args)
    $py = Get-Python
    if ($py -eq "py -3") {
        $output = & py -3 @Args 2>&1
    } else {
        $output = & $py @Args 2>&1
    }
    if ($LASTEXITCODE -ne 0) {
        Write-Err "Python command failed: $($Args -join ' ')`n$($output -join "`n")"
    }
    return ($output -join "`n")
}

if ($Help) {
    @'
livedev-bootstrap.ps1 - one-command Rush LiveDev USB workflow (Windows PowerShell).

Flags:
  -Auto            Full path: clone/fetch repo, mock verify, generate plan,
                    prepare USB using the current testOS backend, print boot instructions.
  -Resume          After rebooting back from testOS: locate USB, copy results,
                    validate, run submit dry-run.
  -DryRun          Print every command that would run. Do not write USB.
  -SkipMock        Skip the mock verification step (used with -Auto).
  -Submit          Used with -Resume: open a real evidence PR for maintainer review.
                    No auto-merge. Requires GH_TOKEN env var.
  -Device <path>   Optional USB device path (e.g. \\.\PhysicalDrive1).
  -Help            Show this message.

Workflow:
  1. .\livedev-bootstrap.ps1 -Auto
  2. (script tells you to boot the USB, run tests, reboot back)
  3. .\livedev-bootstrap.ps1 -Resume
  4. (optional) .\livedev-bootstrap.ps1 -Resume -Submit

Safety:
  - Never auto-merges. PRs are opened for maintainer review.
  - Never marks milestones verified.
  - Never edits release truth.
  - Never fabricates hardware evidence.
'@ | Write-Host
    exit 0
}

if (-not $Auto -and -not $Resume -and -not $Smart) {
    # Default to Smart mode when no mode flag given.
    $Smart = $true
}

# --- Locate or clone the repo ------------------------------------
$RepoDir = ""

function Find-RepoRoot {
    $d = (Get-Location).Path
    while ($d -ne "" -and $d -ne $null) {
        $probe1 = Join-Path $d "tools\livedev-next"
        $probe2 = Join-Path $d "testos\install.ps1"
        $probeGit = Join-Path $d ".git"
        if ((Test-Path $probe1) -and (Test-Path $probe2) -and (Test-Path $probeGit)) {
            $script:RepoDir = $d
            return $true
        }
        $parent = Split-Path -Path $d -Parent
        if ($parent -eq $d) { break }
        $d = $parent
    }
    return $false
}

function Test-IsGitRepo {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return $false }
    $gitDir = Join-Path $Path ".git"
    if (Test-Path $gitDir) { return $true }
    # .git file (worktree) — verify via git
    if (Test-Path $gitDir -PathType Leaf) {
        try { git -C $Path rev-parse --git-dir 2>$null | Out-Null; return $true } catch {}
    }
    return $false
}

function Get-CloneSource {
    if ($SourceRepo) { return $SourceRepo }
    return $RepoUrl
}

function Invoke-Clone {
    param([string]$Target)
    $src = Get-CloneSource
    if ($TestStub -and $SourceRepo) {
        Write-Info "Cloning from local fixture: $src -> $Target"
    } else {
        Write-Info "Cloning from $src -> $Target"
    }
    git clone --depth 1 $src $Target
    if ($LASTEXITCODE -ne 0) { Write-Err "git clone failed (exit $LASTEXITCODE)." }
}

function Sync-ExistingRepo {
    if ($DryRun) {
        Write-Host "    [dry-run] git fetch origin --prune"
        Write-Host "    [dry-run] git checkout main (if clean and main exists)"
        Write-Host "    [dry-run] git pull --ff-only origin main"
        return
    }
    $remotes = git -C $RepoDir remote 2>$null
    if (-not $remotes) {
        Write-Warn "Repo at $RepoDir has no git remotes. Skipping fetch/pull."
        return
    }
    Write-Info "Fetching latest main ..."
    git -C $RepoDir fetch origin --prune --quiet 2>$null
    if ($LASTEXITCODE -ne 0) {
        Write-Warn "git fetch failed (offline?). Continuing with current state."
    }
    $current = git -C $RepoDir rev-parse --abbrev-ref HEAD 2>$null
    if ($current -ne "main") {
        Write-Warn "Repo is on branch '$current'; preserving the explicit branch."
        Write-Warn "Only a checkout already on main is fast-forwarded automatically."
        return
    }
    git -C $RepoDir pull --ff-only origin main --quiet 2>$null
    if ($LASTEXITCODE -ne 0) {
        Write-Warn "git pull --ff-only failed (diverged or offline?). Continuing with current state."
    }
    Write-OK "Repo synced."
}

function Ensure-Repo {
    # --- Rule E: explicit override ---
    if ($RepoDirOverride) {
        if (Test-Path $RepoDirOverride) {
            if (Test-IsGitRepo -Path $RepoDirOverride) {
                $script:RepoDir = $RepoDirOverride
                Write-OK "Using RUSH_LIVEDEV_REPO_DIR: $RepoDir"
            } else {
                Write-Err "RUSH_LIVEDEV_REPO_DIR exists but is not a git repo: $RepoDirOverride"
            }
        } else {
            Write-Info "RUSH_LIVEDEV_REPO_DIR=$RepoDirOverride does not exist. Cloning there."
            Invoke-Clone -Target $RepoDirOverride
            $script:RepoDir = $RepoDirOverride
            Write-OK "Cloned into: $RepoDir"
        }
        Set-Location $RepoDir
        Sync-ExistingRepo
        return
    }

    # --- Rule A: already inside a Rush-linux repo ---
    if (Find-RepoRoot) {
        Write-OK "Using current Rush-linux repo: $RepoDir"
        Set-Location $RepoDir
        Sync-ExistingRepo
        return
    }

    # --- Rules B/C/D: not inside a repo; consider .\$WorkDirName ---
    $candidate = Join-Path (Get-Location) $WorkDirName
    if (Test-Path $candidate) {
        if (Test-IsGitRepo -Path $candidate) {
            # --- Rule B: reuse existing git repo ---
            $script:RepoDir = $candidate
            Write-OK "Found existing Rush-linux git repo: $RepoDir"
            Set-Location $RepoDir
            Sync-ExistingRepo
            return
        } else {
            # --- Rule C: existing dir but NOT a git repo. Use timestamped alternate. ---
            $stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMdd-HHmmss")
            $alternate = Join-Path (Get-Location) "$WorkDirName-livedev-$stamp"
            Write-Warn "Existing .\$WorkDirName is not a git repo; cloning into $alternate"
            Invoke-Clone -Target $alternate
            $script:RepoDir = $alternate
            Write-OK "Cloned into: $RepoDir"
            Set-Location $RepoDir
            return
        }
    }

    # --- Rule D: clean directory — clone into .\$WorkDirName ---
    $script:RepoDir = $candidate
    Write-Info "No .\$WorkDirName found. Cloning into $RepoDir ..."
    Invoke-Clone -Target $RepoDir
    Write-OK "Cloned into: $RepoDir"
    Set-Location $RepoDir
}

# --- Persistent checkpoint + path-safe helpers ------------------------------
function Save-Checkpoint {
    param(
        [string]$RunId,
        [string]$Phase,
        [string]$RunDir,
        [string]$InventoryPath = "",
        [string]$PlanPath = ""
    )
    $cpArgs = @("tools\rush-livedev-checkpoint.py", "save", "--run-id", $RunId,
                "--phase", $Phase, "--run-dir", $RunDir)
    if ($InventoryPath) { $cpArgs += @("--inventory-path", $InventoryPath) }
    if ($PlanPath) { $cpArgs += @("--plan-path", $PlanPath) }
    Invoke-Python @cpArgs
    if ($LASTEXITCODE -ne 0) { Write-Err "Failed to save LiveDev checkpoint." }
}

function Get-CheckpointData {
    $raw = Invoke-PythonCapture tools\rush-livedev-checkpoint.py load
    try { return ($raw | ConvertFrom-Json) } catch { Write-Err "Checkpoint JSON is invalid: $_" }
}

function Test-ReparsePoint {
    param([string]$Path)
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    return [bool]($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)
}

function Install-RunIntentOnUsb {
    param(
        [string]$UsbDevice,
        [string]$ImagePath,
        [string]$ImageCommit,
        [string]$TestosVersion,
        [string]$PlanPath,
        [string]$RunId
    )
    if ($UsbDevice -notmatch '^\\\\\.\\PhysicalDrive(\d+)$') {
        Write-Err "Installer returned unsafe USB device path: $UsbDevice"
    }
    $diskNumber = [int]$Matches[1]
    $partitions = @()
    for ($attempt = 1; $attempt -le 10; $attempt++) {
        try { Update-HostStorageCache -ErrorAction SilentlyContinue } catch {}
        $partitions = @(Get-Partition -DiskNumber $diskNumber -ErrorAction SilentlyContinue)
        if ($partitions.Count -gt 0) { break }
        Start-Sleep -Milliseconds 500
    }
    if ($partitions.Count -eq 0) { Write-Err "No partitions appeared after writing $UsbDevice." }
    $espGuid = "{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}"
    $part = $partitions | Where-Object { $_.GptType -eq $espGuid } | Select-Object -First 1
    if (-not $part) { $part = $partitions | Sort-Object PartitionNumber | Select-Object -First 1 }

    $mount = Join-Path $env:TEMP "rush-livedev-intent-$RunId"
    if (Test-Path $mount) {
        if (Test-ReparsePoint $mount) { Write-Err "Refusing reparse-point mount path: $mount" }
    } else {
        New-Item -ItemType Directory -Path $mount -Force | Out-Null
    }
    $accessPath = $mount.TrimEnd('\') + '\'
    try {
        Add-PartitionAccessPath -DiskNumber $diskNumber -PartitionNumber $part.PartitionNumber -AccessPath $accessPath -ErrorAction Stop
        Invoke-Python tools\testos_prepare_usb.py --repo-root $RepoDir --plan-path $PlanPath `
            --image-path $ImagePath --testos-image-commit $ImageCommit --run-id $RunId `
            --testos-version $TestosVersion --checkpoint-nonce "ckpt-$RunId" --source-dir $mount
        if ($LASTEXITCODE -ne 0) { Write-Err "Failed to install run-intent.json on USB." }
    } finally {
        Remove-PartitionAccessPath -DiskNumber $diskNumber -PartitionNumber $part.PartitionNumber -AccessPath $accessPath -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $mount -Force -ErrorAction SilentlyContinue
    }
}

# --- AUTO MODE ---------------------------------------------------
function Do-Auto {
    Write-Info "=== Rush LiveDev - one-command USB workflow (-Auto) ==="
    Ensure-Repo

    if ($TestStub) {
        Write-OK "[TEST_STUB] Repo resolution succeeded. Skipping USB/reboot/PR."
        Write-Host "[TEST_STUB] REPO_DIR=$RepoDir"
        return
    }

    $runId = ""
    $persistentRunDir = ""
    $inventoryPath = ""
    $planPath = ""

    Write-Host ""
    Write-Info "Step 0/4: Create persistent run and collect privacy-safe hardware inventory."
    if ($DryRun) {
        Write-Host "    [dry-run] Would use %LOCALAPPDATA%\Rush\livedev-runs\<run-id>."
        Write-Host "    [dry-run] Would collect allow-listed CIM hardware fields only."
    } else {
        $freshOutput = Invoke-PythonCapture tools\rush-livedev-checkpoint.py ensure-fresh
        $runId = (($freshOutput -split "`r?`n") | Where-Object { $_.Trim() } | Select-Object -Last 1).Trim()
        if ($runId -notmatch '^[A-Za-z0-9_.-]{4,128}$') { Write-Err "Checkpoint returned unsafe run_id: $runId" }
        $persistentRunDir = (Invoke-PythonCapture tools\rush-livedev-checkpoint.py init-run --run-id $runId).Trim()
        if (Test-ReparsePoint $persistentRunDir) { Write-Err "Persistent run directory is a reparse point." }
        $inventoryPath = Join-Path $persistentRunDir "hardware-inventory.json"
        Invoke-Python tools\collect-hardware-inventory.py --output $inventoryPath
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path $inventoryPath -PathType Leaf)) {
            Write-Err "Privacy-safe Windows hardware inventory collection failed."
        }
        Save-Checkpoint $runId "preflight" $persistentRunDir $inventoryPath
        Write-OK "Persistent run dir: $persistentRunDir"
    }

    if (-not $SkipMock) {
        Write-Host ""
        Write-Info "Step 1/4: Mock verification (no hardware, no network)."
        if ($DryRun) {
            Write-Host "    [dry-run] python tools/livedev-next --mock"
        } else {
            Invoke-Python tools\livedev-next --mock
            if ($LASTEXITCODE -ne 0) { Write-Err "Mock verification failed (or use -SkipMock)." }
            Write-OK "Mock verification passed."
        }
    } else { Write-Warn "Skipping mock verification (-SkipMock)." }

    Write-Host ""
    Write-Info "Step 2/4: Generate and preserve a real baseline-only plan."
    if ($DryRun) {
        Write-Host "    [dry-run] python tools/livedev-next --plan --baseline-only"
    } else {
        Invoke-Python tools\livedev-next --plan --baseline-only
        if ($LASTEXITCODE -ne 0) { Write-Err "Plan generation failed." }
        $generatedPlan = Join-Path $env:TEMP "rush-livedev-plan.json"
        if (-not (Test-Path $generatedPlan -PathType Leaf) -or (Test-ReparsePoint $generatedPlan)) {
            Write-Err "Generated plan is missing, non-regular, or a reparse point: $generatedPlan"
        }
        $planPath = Join-Path $persistentRunDir "plan.json"
        if (Test-Path $planPath) {
            if ((Get-FileHash $planPath -Algorithm SHA256).Hash -ne (Get-FileHash $generatedPlan -Algorithm SHA256).Hash) {
                Write-Err "Existing persistent plan differs from the newly generated plan. Start a fresh run."
            }
        } else {
            Copy-Item -LiteralPath $generatedPlan -Destination $planPath
        }
        Save-Checkpoint $runId "plan_ready" $persistentRunDir $inventoryPath $planPath
        Write-OK "Persistent plan: $planPath"
    }

    Write-Host ""
    Write-Info "Step 3/4: Prepare USB and install cryptographic run intent."
    Write-Host "Using testOS as the current LiveDev boot backend."
    if ($DryRun) {
        Write-Host "    [dry-run] Would run testos\install.ps1, but would not write USB."
    } else {
        $installArgs = @()
        if ($Device) { $installArgs += @("-Device", $Device) }
        $installOutput = & powershell -ExecutionPolicy Bypass -File testos\install.ps1 @installArgs 2>&1
        $installExit = $LASTEXITCODE
        $installLines = @($installOutput | ForEach-Object { "$_" })
        $installLines | ForEach-Object { Write-Host $_ }
        if ($installExit -ne 0) { Write-Err "testOS installer failed." }
        $imagePath = ([string]($installLines | Where-Object { $_ -like 'TESTOS_RAW_IMAGE: *' } | Select-Object -First 1) -replace '^TESTOS_RAW_IMAGE:\s*','').Trim()
        $usbDevice = ([string]($installLines | Where-Object { $_ -like 'TESTOS_USB_DEVICE: *' } | Select-Object -First 1) -replace '^TESTOS_USB_DEVICE:\s*','').Trim()
        $imageCommit = ([string]($installLines | Where-Object { $_ -like 'TESTOS_IMAGE_COMMIT: *' } | Select-Object -First 1) -replace '^TESTOS_IMAGE_COMMIT:\s*','').Trim()
        $testosVersion = ([string]($installLines | Where-Object { $_ -like 'TESTOS_VERSION: *' } | Select-Object -First 1) -replace '^TESTOS_VERSION:\s*','').Trim()
        if (-not $imagePath -or -not $usbDevice -or $imageCommit -notmatch '^[0-9a-f]{40}$' -or -not $testosVersion) {
            Write-Err "USB was written but verified image identity markers were missing. Refusing to boot it."
        }
        Install-RunIntentOnUsb $usbDevice $imagePath $imageCommit $testosVersion $planPath $runId
        Save-Checkpoint $runId "usb_prepared" $persistentRunDir $inventoryPath $planPath
        Write-OK "USB, run-intent.json, plan.json, and catalog verified by readback."
    }

    Write-Host ""
    Write-Info "Step 4/4: Boot the USB and run tests."
    Print-BootInstructions
}

function Print-BootInstructions {
    Write-Host @"

    --- Reboot instructions ---

    1. Plug the USB into the test machine (the one you want to benchmark).

    2. Reboot. Enter the boot menu:
         - Most vendors: F12, F8, F11, or Esc at the BIOS logo.

    3. Pick the USB from the boot menu.

    4. If it refuses to boot, disable Secure Boot in the BIOS.
       (testOS UKIs are unsigned for now.)

    5. testOS boots to a console menu:
         - Type 0 for "Run all benchmarks".
         - Or pick specific test numbers.
         - Press Esc at any time to abort early (partial results saved).

    6. When tests finish, testOS syncs the USB and auto-reboots
       back to the host OS.

    7. Unplug the USB, plug it back into THIS workstation, and run:

         .\livedev-bootstrap.ps1 -Resume

    ---

    You only approve: USB erase, boot from USB, physical AC/battery
    prompts, and (later) GitHub auth. Everything else is automatic.
"@
}

# --- RESUME MODE -------------------------------------------------
function Copy-MatchingResultsIntoRunDir {
    param([string]$OutDir, [string]$ExpectedRunId, [string]$ExpectedNonce)
    $usbDisks = @(Get-Disk | Where-Object { $_.BusType -eq 'USB' })
    foreach ($disk in $usbDisks) {
        foreach ($part in @(Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue)) {
            $mountedByUs = $false
            $accessPath = ""
            $root = ""
            try {
                if ($part.DriveLetter) {
                    $root = "$($part.DriveLetter):\"
                } else {
                    $mount = Join-Path $env:TEMP "rush-livedev-read-$($disk.Number)-$($part.PartitionNumber)"
                    New-Item -ItemType Directory -Path $mount -Force | Out-Null
                    $accessPath = $mount.TrimEnd('\') + '\'
                    Add-PartitionAccessPath -DiskNumber $disk.Number -PartitionNumber $part.PartitionNumber -AccessPath $accessPath -ErrorAction Stop
                    $root = $mount
                    $mountedByUs = $true
                }
                $resultsRoot = Join-Path $root "testos-results"
                if (-not (Test-Path $resultsRoot -PathType Container)) { continue }
                foreach ($candidate in @(Get-ChildItem -LiteralPath $resultsRoot -Directory -Force)) {
                    if (Test-ReparsePoint $candidate.FullName) { continue }
                    $intentPath = Join-Path $candidate.FullName "run-intent.json"
                    if (-not (Test-Path $intentPath -PathType Leaf) -or (Test-ReparsePoint $intentPath)) { continue }
                    try { $intent = Get-Content -LiteralPath $intentPath -Raw | ConvertFrom-Json } catch { continue }
                    if ($intent.run_id -ne $ExpectedRunId -or $intent.checkpoint_nonce -ne $ExpectedNonce) { continue }
                    Invoke-Python tools\rush-safe-copy-tree.py $candidate.FullName $OutDir
                    if ($LASTEXITCODE -ne 0) { Write-Err "Path-safe USB result copy failed." }
                    Write-OK "Copied matching testOS run: $($candidate.Name)"
                    return $true
                }
            } catch {
                Write-Warn "Could not inspect USB disk $($disk.Number) partition $($part.PartitionNumber): $($_.Exception.Message)"
            } finally {
                if ($mountedByUs) {
                    Remove-PartitionAccessPath -DiskNumber $disk.Number -PartitionNumber $part.PartitionNumber -AccessPath $accessPath -ErrorAction SilentlyContinue
                    Remove-Item -LiteralPath $root -Force -ErrorAction SilentlyContinue
                }
            }
        }
    }
    return $false
}

function Test-SubmitAuth {
    if ($env:GH_TOKEN -or $env:GITHUB_TOKEN) { return $true }
    if (Get-Command gh -ErrorAction SilentlyContinue) {
        & gh auth status 2>$null | Out-Null
        if ($LASTEXITCODE -eq 0) { return $true }
    }
    Write-Host "[TOKEN NEEDED]"
    Write-Host "Authenticate with 'gh auth login' (recommended), or set GH_TOKEN, then rerun."
    return $false
}

function Do-Resume {
    Write-Info "=== Rush LiveDev - resume after reboot (-Resume) ==="
    Ensure-Repo
    if ($Submit -and -not $DryRun -and -not (Test-SubmitAuth)) { exit 2 }

    if ($DryRun) {
        Write-Host "    [dry-run] Would load the persistent checkpoint, copy only the matching USB run,"
        Write-Host "    [dry-run] run the strict validator/privacy gate, then submit in dry-run mode."
        return
    }

    $cp = Get-CheckpointData
    if (-not $cp -or -not $cp.run_id -or -not $cp.run_dir -or -not $cp.plan_path) {
        Write-Err "No complete persistent checkpoint. Refusing to associate arbitrary USB evidence."
    }
    $runId = "$($cp.run_id)"
    $runRoot = "$($cp.run_dir)"
    if (-not (Test-Path $runRoot -PathType Container) -or (Test-ReparsePoint $runRoot)) {
        Write-Err "Persistent checkpoint run directory is missing or a reparse point."
    }
    $runDir = Join-Path $runRoot "results"
    New-Item -ItemType Directory -Path $runDir -Force | Out-Null
    if (Test-ReparsePoint $runDir) { Write-Err "Persistent results directory is a reparse point." }

    Write-Host ""
    Write-Info "Step 1/3: Copy the USB run matching checkpoint $runId."
    if (-not (Copy-MatchingResultsIntoRunDir $runDir $runId "ckpt-$runId")) {
        Write-Err "No USB result matched run_id=$runId and its checkpoint nonce."
    }

    $usbPlan = Join-Path $runDir "plan.json"
    if (-not (Test-Path $usbPlan -PathType Leaf) -or (Test-ReparsePoint $usbPlan)) {
        Write-Err "Collected evidence has no regular plan.json."
    }
    if ((Get-FileHash $usbPlan -Algorithm SHA256).Hash -ne (Get-FileHash $cp.plan_path -Algorithm SHA256).Hash) {
        Write-Err "USB plan does not match the persistent pre-reboot plan."
    }
    if ($cp.inventory_path) {
        if (-not (Test-Path $cp.inventory_path -PathType Leaf) -or (Test-ReparsePoint $cp.inventory_path)) {
            Write-Err "Persistent hardware inventory is missing or unsafe."
        }
        Copy-Item -LiteralPath $cp.inventory_path -Destination (Join-Path $runDir "hardware-inventory.json") -Force
    }
    Save-Checkpoint $runId "collected" $runRoot $cp.inventory_path $cp.plan_path

    Write-Host ""
    Write-Info "Step 2/3: Strict provenance, schema, hash, path, and privacy validation."
    Invoke-Python tools\validate-testos-evidence.py --run-dir $runDir --strict
    if ($LASTEXITCODE -ne 0) { Write-Err "Strict evidence validation failed; submission blocked." }
    Save-Checkpoint $runId "validated" $runRoot $cp.inventory_path $cp.plan_path

    Write-Host ""
    Write-Info "Step 3/3: Unified draft-only evidence submission."
    $submitArgs = @("tools\rush-submit-evidence", $runDir, "--submit-mode", "auto")
    if (-not $Submit) { $submitArgs += "--dry-run" }
    Invoke-Python @submitArgs
    if ($LASTEXITCODE -ne 0) { Write-Err "Unified evidence submission failed." }
    if ($Submit) {
        Save-Checkpoint $runId "submitted" $runRoot $cp.inventory_path $cp.plan_path
        Write-OK "Draft evidence PR opened. This script never merges it."
    }
}

# --- USB result detection (Windows) -------------------------------
function Test-UsbHasResults {
    # Inspect every partition on every USB disk. testOS uses an ESP, which
    # Windows commonly leaves without a drive letter, so logical-disk-only
    # detection would incorrectly offer to overwrite an evidence USB.
    foreach ($disk in @(Get-Disk | Where-Object { $_.BusType -eq 'USB' })) {
        foreach ($part in @(Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue)) {
            $mountedByUs = $false
            $accessPath = ""
            $root = ""
            try {
                if ($part.DriveLetter) {
                    $root = "$($part.DriveLetter):\"
                } else {
                    $mount = Join-Path $env:TEMP "rush-livedev-detect-$($disk.Number)-$($part.PartitionNumber)"
                    New-Item -ItemType Directory -Path $mount -Force | Out-Null
                    $accessPath = $mount.TrimEnd('\') + '\'
                    Add-PartitionAccessPath -DiskNumber $disk.Number -PartitionNumber $part.PartitionNumber -AccessPath $accessPath -ErrorAction Stop
                    $root = $mount
                    $mountedByUs = $true
                }
                if (Test-Path (Join-Path $root "testos-results") -PathType Container) {
                    return $true
                }
            } catch {
                # Detection is best-effort; Do-Resume reports actionable errors.
            } finally {
                if ($mountedByUs) {
                    Remove-PartitionAccessPath -DiskNumber $disk.Number -PartitionNumber $part.PartitionNumber -AccessPath $accessPath -ErrorAction SilentlyContinue
                    Remove-Item -LiteralPath $root -Force -ErrorAction SilentlyContinue
                }
            }
        }
    }
    return $false
}

# --- Smart dispatcher ---------------------------------------------
function Do-Smart {
    Write-Info "=== Rush LiveDev - SMART mode ==="

    Ensure-Repo

    if ($TestStub -eq "1") {
        Write-OK "[TEST_STUB] Skipping smart dispatch."
        return
    }

    # Detect what's available.
    $haveUsb = $false
    if (Test-UsbHasResults) { $haveUsb = $true }

    # Build menu options.
    $choices = @()
    $descriptions = @()
    if ($haveUsb) {
        $choices += "resume"
        $descriptions += "Copy results from USB, validate, submit evidence PR"
    }
    $choices += "usb"
    $descriptions += "Prepare a USB via testOS (for real-hardware testing)"

    # Non-interactive: auto-pick.
    if (-not [Environment]::UserInteractive -or $env:CI) {
        if ($haveUsb) {
            Write-OK "Non-interactive + USB detected - resuming."
            Do-Resume
            return
        }
        Do-Auto
        return
    }

    # Interactive: show menu.
    Write-Host ""
    Write-Host "  What would you like to do?"
    Write-Host ""
    for ($i = 0; $i -lt $choices.Count; $i++) {
        $n = $i + 1
        Write-Host ("  [{0}] {1} - {2}" -f $n, $choices[$i], $descriptions[$i])
    }
    Write-Host ""
    $default = 1
    $reply = Read-Host ("  Pick [1-{0}] (default {1})" -f $choices.Count, $default)
    if ([string]::IsNullOrWhiteSpace($reply)) { $reply = $default }
    $n = 0
    if (-not [int]::TryParse($reply, [ref]$n) -or $n -lt 1 -or $n -gt $choices.Count) {
        Write-Err "Invalid choice: $reply"
    }
    $pick = $choices[$n - 1]
    Write-Host ""
    switch ($pick) {
        "resume" {
            Write-OK "Resuming - copy results from USB, validate, submit."
            Do-Resume
        }
        "usb" {
            Write-OK "Preparing USB via testOS."
            Do-Auto
        }
    }
}

# --- Dispatch ----------------------------------------------------
if ($Smart) {
    Do-Smart
} elseif ($Resume) {
    Do-Resume
} else {
    Do-Auto
}
