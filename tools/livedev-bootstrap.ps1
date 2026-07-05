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
        $dirty = git -C $RepoDir status --porcelain 2>$null
        if (-not $dirty) {
            $hasMain = git -C $RepoDir rev-parse --verify main 2>$null
            if ($hasMain) {
                git -C $RepoDir checkout main --quiet 2>$null
                if ($LASTEXITCODE -ne 0) { Write-Warn "Could not switch to main. Staying on '$current'." }
            } else {
                Write-Warn "Branch 'main' does not exist in $RepoDir. Staying on '$current'."
            }
        } else {
            Write-Warn "Working tree is dirty on branch '$current'. Staying on this branch."
            Write-Warn "Your local work is preserved."
        }
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

# --- AUTO MODE ---------------------------------------------------
function Do-Auto {
    Write-Info "=== Rush LiveDev - one-command USB workflow (-Auto) ==="
    Write-Host ""
    Write-Host "This writes a USB, boots the test environment, runs tests, reboots,"
    Write-Host "resumes collection, validates results, and opens an evidence PR for"
    Write-Host "maintainer review."
    Write-Host ""
    Write-Host "You only approve USB erase, boot from USB, physical AC/battery"
    Write-Host "prompts, and GitHub auth."
    if ($TestStub) {
        Write-Host ""
        Write-Warn "RUSH_LIVEDEV_TEST_STUB=1: USB write, reboot, PR, and hardware are skipped."
        Write-Warn "Repo resolution still runs for real."
    }

    # Repo resolution ALWAYS runs (real), even in TEST_STUB mode.
    Ensure-Repo

    # In TEST_STUB mode: skip mock/plan/USB/boot — we only needed to prove
    # repo resolution worked end-to-end without USB/network/PR side effects.
    if ($TestStub) {
        Write-Host ""
        Write-OK "[TEST_STUB] Repo resolution succeeded. Skipping USB/reboot/PR."
        Write-Host "[TEST_STUB] REPO_DIR=$RepoDir"
        return
    }

    # Step 1: mock verification.
    if (-not $SkipMock) {
        Write-Host ""
        Write-Info "Step 1/4: Mock verification (no hardware, no network)."
        if ($DryRun) {
            Write-Host "    [dry-run] python3 tools/livedev-next --mock"
        } else {
            $ok = $false
            try {
                & python3 tools/livedev-next --mock
                if ($LASTEXITCODE -eq 0) { $ok = $true }
            } catch {}
            if (-not $ok) {
                Write-Err "Mock verification failed. Fix before proceeding (or use -SkipMock)."
            }
            Write-OK "Mock verification passed."
        }
    } else {
        Write-Warn "Skipping mock verification (-SkipMock)."
    }

    # Step 2: generate plan.
    Write-Host ""
    Write-Info "Step 2/4: Generate benchmark plan."
    if ($DryRun) {
        Write-Host "    [dry-run] python3 tools/livedev-next --plan"
    } else {
        $ok = $false
        try {
            & python3 tools/livedev-next --plan
            if ($LASTEXITCODE -eq 0) { $ok = $true }
        } catch {}
        if (-not $ok) { Write-Err "Plan generation failed." }
        Write-OK "Plan generated: /tmp/rush-livedev-plan.json (or %TEMP%\rush-livedev-plan.json)"
    }

    # Step 3: prepare USB.
    Write-Host ""
    Write-Info "Step 3/4: Prepare USB test environment."
    Write-Host "Using testOS as the current LiveDev boot backend."
    if ($DryRun) {
        Write-Host "    [dry-run] Would run:"
        if ($Device) {
            Write-Host "      powershell -ExecutionPolicy Bypass -File testos\install.ps1 -Device $Device"
        } else {
            Write-Host "      powershell -ExecutionPolicy Bypass -File testos\install.ps1"
        }
        Write-Host "    [dry-run] Not writing USB."
    } else {
        $args = @()
        if ($Device) { $args += @("-Device", $Device) }
        & powershell -ExecutionPolicy Bypass -File testos\install.ps1 @args
        if ($LASTEXITCODE -ne 0) { Write-Err "testOS installer failed." }
        Write-OK "USB prepared."
    }

    # Step 4: reboot instructions.
    Write-Host ""
    Write-Info "Step 4/4: Boot the USB and run tests."
    Print-BootInstructions

    Write-Host ""
    Write-Info "After testOS reboots the test machine back to its host OS, plug the USB"
    Write-Info "back into this workstation and run:"
    Write-Host ""
    Write-Host "    .\livedev-bootstrap.ps1 -Resume"
    Write-Host ""
    Write-Info "That will copy results, validate them, and run a submit dry-run."
    Write-Info "To open a real evidence PR for maintainer review (no auto-merge):"
    Write-Host ""
    Write-Host "    .\livedev-bootstrap.ps1 -Resume -Submit"
    Write-Host ""
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
function Do-Resume {
    Write-Info "=== Rush LiveDev - resume after reboot (-Resume) ==="

    Ensure-Repo

    # Step 1: locate USB and copy results.
    Write-Host ""
    Write-Info "Step 1/3: Locate USB and copy results."
    $RunDir = New-Item -ItemType Directory -Path "$env:TEMP\rush-livedev-resume-$(Get-Date -Format yyyyMMddHHmmss)" -Force
    if ($DryRun) {
        Write-Host "    [dry-run] Would scan for USB, mount its ESP read-only,"
        Write-Host "    [dry-run]   and copy testos-results\<latest>\ into: $($RunDir.FullName)"
    } else {
        Copy-ResultsIntoRunDir $RunDir.FullName
        if ((Get-ChildItem -Path $RunDir.FullName -Force | Measure-Object).Count -eq 0) {
            Write-Warn "No results copied (USB may not be plugged in, or no testos-results\ on it)."
            Write-Warn "Run dir kept for inspection: $($RunDir.FullName)"
        } else {
            Write-OK "Results copied to: $($RunDir.FullName)"
        }
    }

    # Step 2: validate.
    Write-Host ""
    Write-Info "Step 2/3: Validate results."
    if ($DryRun) {
        Write-Host "    [dry-run] Would validate testOS manifest.json (parses, has passed/failed counts)."
        Write-Host "    [dry-run] Would run: python3 tools\validate-hwtest-evidence.py --bundle <run_dir> (if applicable)"
    } else {
        Validate-Results $RunDir.FullName
    }

    # Step 3: submit.
    Write-Host ""
    Write-Info "Step 3/3: Submit evidence."
    if ($Submit) {
        Do-RealSubmit $RunDir.FullName
    } else {
        Do-DryRunSubmit $RunDir.FullName
    }
}

function Copy-ResultsIntoRunDir {
    param([string]$OutDir)

    # Find first USB disk.
    $usbDisks = @(Get-Disk | Where-Object { $_.BusType -eq 'USB' })
    if ($usbDisks.Count -eq 0) {
        Write-Warn "No USB disks detected."
        return
    }
    $disk = $usbDisks[0]

    # Find first FAT partition on the USB.
    $parts = @(Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue | Where-Object { $_.Type -match 'FAT|ESP|EFI' })
    if ($parts.Count -eq 0) {
        # Fallback: try Get-Volume and look for FAT.
        $vols = @(Get-Volume | Where-Object { $_.DriveType -eq 'Removable' -and $_.FileSystem -match 'FAT' })
        if ($vols.Count -eq 0) {
            Write-Warn "No FAT partition found on USB disk $($disk.Number)."
            return
        }
        $drive = "$($vols[0].DriveLetter):"
        if (-not $drive -or $drive -eq ":") {
            Write-Warn "USB volume has no drive letter. Mount it in Disk Management and rerun."
            return
        }
        $resultsRoot = Join-Path $drive "testos-results"
        if (-not (Test-Path $resultsRoot)) {
            Write-Warn "No testos-results\ on $drive."
            return
        }
        $latest = Get-ChildItem -Path $resultsRoot -Directory | Sort-Object Name -Descending | Select-Object -First 1
        if ($latest) {
            Copy-Item -Path (Join-Path $latest.FullName "*") -Destination $OutDir -Recurse -Force
            Write-OK "Copied testOS run: $($latest.Name)"
        }
        return
    }

    # Mount the ESP partition temporarily if it has no drive letter.
    $part = $parts[0]
    $drive = "$($part.DriveLetter):"
    $assignedDrive = $false
    if (-not $part.DriveLetter -or $part.DriveLetter -eq 0) {
        # Add an access path instead of a drive letter to avoid conflicts.
        $mount = Join-Path $env:TEMP "rush-livedev-mount-$(Get-Random)"
        New-Item -ItemType Directory -Path $mount -Force | Out-Null
        Add-PartitionAccessPath -DiskNumber $disk.Number -PartitionNumber $part.PartitionNumber -AccessPath $mount
        $drive = $mount
        $assignedDrive = $true
    }

    try {
        $resultsRoot = Join-Path $drive "testos-results"
        if (Test-Path $resultsRoot) {
            $latest = Get-ChildItem -Path $resultsRoot -Directory | Sort-Object Name -Descending | Select-Object -First 1
            if ($latest) {
                Copy-Item -Path (Join-Path $latest.FullName "*") -Destination $OutDir -Recurse -Force
                Write-OK "Copied testOS run: $($latest.Name)"
            }
        }
    } finally {
        if ($assignedDrive) {
            Remove-PartitionAccessPath -DiskNumber $disk.Number -PartitionNumber $part.PartitionNumber -AccessPath $drive -ErrorAction SilentlyContinue
            Remove-Item -Path $drive -Force -ErrorAction SilentlyContinue
        }
    }
}

function Validate-Results {
    param([string]$RunDir)

    $manifest = Join-Path $RunDir "manifest.json"
    if (-not (Test-Path $manifest)) {
        Write-Warn "No manifest.json in run dir. testOS schema validation skipped."
        Write-Warn "(LiveDev hardware-evidence validator requires run-record.json format."
        Write-Warn " testOS produces manifest.json - only basic checks are run here.)"
        return
    }
    Write-Info "Validating testOS manifest: $manifest"
    $m = Get-Content $manifest -Raw | ConvertFrom-Json
    if (-not $m.host) { Write-Err "manifest missing host fingerprint" }
    if ($null -eq $m.passed -or $null -eq $m.failed -or $null -eq $m.skipped) {
        Write-Err "manifest missing pass/fail/skip counts"
    }
    Write-Host "  manifest parses OK"
    Write-Host "  passed=$($m.passed.Count) failed=$($m.failed.Count) skipped=$($m.skipped.Count)"
    Write-OK "Results validated (basic schema check)."

    # Also try the LiveDev validator if the bundle has the right shape.
    $runRecord = Join-Path $RunDir "run-record.json"
    if (Test-Path $runRecord) {
        Write-Info "LiveDev run-record.json detected - running full evidence validator ..."
        & python3 tools/validate-hwtest-evidence.py --bundle $RunDir
        if ($LASTEXITCODE -ne 0) {
            Write-Warn "LiveDev validator reported issues. Submit will still proceed in dry-run."
        }
    }
}

function Do-DryRunSubmit {
    param([string]$RunDir)

    Write-Info "Submit dry-run (no push, no PR, no merge)."
    if ($DryRun) {
        Write-Host "    [dry-run] Would run: python3 tools/livedev-next --submit $RunDir --dry-run"
        return
    }
    $runRecord = Join-Path $RunDir "run-record.json"
    if (Test-Path $runRecord) {
        & python3 tools/livedev-next --submit $RunDir --dry-run
        if ($LASTEXITCODE -ne 0) {
            Write-Warn "livedev-next --submit --dry-run reported issues."
        }
    } else {
        Write-OK "testOS results staged in: $RunDir"
        Write-Host ""
        Write-Info "To open a real evidence PR for maintainer review (no auto-merge):"
        Write-Host ""
        Write-Host "    .\livedev-bootstrap.ps1 -Resume -Submit"
        Write-Host ""
        Write-Info "The PR will be opened on a branch. A maintainer reviews and merges."
    }
}

function Do-RealSubmit {
    param([string]$RunDir)

    # Token check. Never print the token.
    $token = $env:GH_TOKEN
    if (-not $token) { $token = $env:GITHUB_TOKEN }
    if (-not $token) {
        Write-Host "[TOKEN NEEDED]"
        Write-Host "Set `$env:GH_TOKEN, then rerun:"
        Write-Host "    .\livedev-bootstrap.ps1 -Resume -Submit"
        exit 2
    }

    Write-Info "Real submit: open evidence PR for maintainer review (no auto-merge)."
    if ($DryRun) {
        Write-Host "    [dry-run] Would push branch and open PR via GitHub API."
        Write-Host "    [dry-run] Token present (not printed). No merge API call would be made."
        return
    }

    $runRecord = Join-Path $RunDir "run-record.json"
    if (Test-Path $runRecord) {
        # LiveDev-shaped run: use livedev-next --submit (no --dry-run).
        # rush_pr_lib.py never calls the merge API.
        $env:GH_TOKEN = $token
        & python3 tools/livedev-next --submit $RunDir
        if ($LASTEXITCODE -ne 0) { Write-Err "livedev-next --submit failed." }
    } else {
        # testOS-shaped run: self-contained push + PR open. No merge API call.
        Submit-TestosResults $RunDir $token
    }
    Write-OK "Submit complete. PR opened for maintainer review."
    Write-Info "A maintainer reviews and merges the PR. This script never merges."
}

function Submit-TestosResults {
    param([string]$RunDir, [string]$Token)

    $manifest = Join-Path $RunDir "manifest.json"
    if (-not (Test-Path $manifest)) {
        Write-Err "No manifest.json in $RunDir. Cannot submit testOS results."
    }

    # Extract metadata.
    $m = Get-Content $manifest -Raw | ConvertFrom-Json
    $datePart = "unknown-date"
    if ($m.started_at) {
        $datePart = ($m.started_at -split "T")[0]
    }
    $hostFp = "unknown-host"
    if ($m.host -and $m.host.fingerprint) {
        $hostFp = $m.host.fingerprint
    }
    $stamp = Get-Date -Format "yyyyMMddHHmmss"
    $branch = "benchmarks/testos-$datePart-$stamp"

    $workDir = Join-Path $env:TEMP "rush-livedev-submit-$stamp"
    Write-Info "Cloning repo shallow into $workDir ..."
    git clone --depth 1 $RepoUrl $workDir
    if ($LASTEXITCODE -ne 0) { Write-Err "git clone failed." }
    Set-Location $workDir
    git checkout -b $branch

    # Copy results into the clone.
    $dest = "benchmarks/results/$datePart/$hostFp"
    New-Item -ItemType Directory -Path $dest -Force | Out-Null
    Copy-Item -Path (Join-Path $RunDir "*") -Destination $dest -Recurse -Force

    git add benchmarks/results/
    $commitMsg = "evidence(bench): testOS run $datePart host=$hostFp"
    git -c user.email="livedev-bootstrap@local" -c user.name="Rush LiveDev bootstrap" commit -m $commitMsg
    Write-OK "Committed: $commitMsg"

    # Push using the token. Never store the token in git config.
    $pushUrl = "https://x-access-token:$Token@github.com/$RepoHost.git"
    git push $pushUrl $branch 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Err "git push failed." }
    git remote set-url origin $RepoUrl 2>$null
    Write-OK "Pushed branch: $branch"

    # Open a PR via the GitHub API. NO merge call.
    $prBody = @"
Auto-collected by livedev-bootstrap.ps1 -Resume -Submit.

Run summary:
- Date: $datePart
- Host: $hostFp

Includes per-benchmark JSON results and manifest.json. No auto-merge -
opened for maintainer review per Rush LiveDev policy.
"@
    $prPayload = @{
        title = "benchmarks(testos): results from $datePart"
        head = $branch
        base = "main"
        body = $prBody
    } | ConvertTo-Json -Depth 5

    $headers = @{
        Authorization = "Bearer $Token"
        Accept = "application/vnd.github+json"
    }
    try {
        $resp = Invoke-RestMethod -Uri "https://api.github.com/repos/$RepoHost/pulls" -Method Post -Headers $headers -Body $prPayload -ContentType "application/json"
    } catch {
        Write-Err "GitHub PR API call failed: $_"
    }
    $prUrl = $resp.html_url
    if (-not $prUrl) { Write-Err "Could not parse PR URL from API response." }
    Write-OK "PR opened: $prUrl"
    Write-Info "No merge API call made. A maintainer reviews and merges."
}

# --- USB result detection (Windows) -------------------------------
function Test-UsbHasResults {
    # Returns $true if a removable drive with testos-results\ is plugged in.
    $drives = Get-CimInstance Win32_LogicalDisk | Where-Object { $_.DriveType -eq 2 }
    foreach ($d in $drives) {
        $resultsRoot = Join-Path $d.DeviceID "testos-results"
        if (Test-Path $resultsRoot) { return $true }
    }
    return $false
}

# --- Smart dispatcher ---------------------------------------------
function Do-Smart {
    Write-Info "=== Rush LiveDev - SMART mode (auto-detect) ==="

    Ensure-Repo

    if ($TestStub -eq "1") {
        Write-OK "[TEST_STUB] Skipping smart dispatch."
        return
    }

    # Step 1: USB with results? -> resume.
    if (Test-UsbHasResults) {
        Write-OK "Detected USB with testOS results - resuming."
        Do-Resume
        return
    }

    # Step 2: No QEMU on Windows by default; fall back to USB/testOS path.
    Write-Warn "No USB results detected - falling back to USB/testOS path."
    Do-Auto
}

# --- Dispatch ----------------------------------------------------
if ($Smart) {
    Do-Smart
} elseif ($Resume) {
    Do-Resume
} else {
    Do-Auto
}
