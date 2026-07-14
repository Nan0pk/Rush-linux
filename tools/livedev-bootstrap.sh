#!/usr/bin/env bash
# tools/livedev-bootstrap.sh — ONE-command Rush LiveDev workflow for Linux/macOS.
#
# Usage:
#   bash livedev-bootstrap.sh                # SMART: auto-detect and do everything
#   bash livedev-bootstrap.sh --smart        # same as above (explicit)
#   bash livedev-bootstrap.sh --auto         # force USB/testOS path (prepare USB, print boot instructions)
#   bash livedev-bootstrap.sh --resume       # force resume path (copy results from USB)
#   bash livedev-bootstrap.sh --resume --submit  # resume + open real evidence PR
#   bash livedev-bootstrap.sh --vm           # force QEMU/--run-vm path
#   bash livedev-bootstrap.sh --dry-run      # show commands, do not write/build/submit
#
# SMART mode (default) auto-detects:
#   1. If a USB with testOS results is plugged in → resume + validate + submit.
#   2. Else if qemu-system-x86_64 is available:
#      a. If livedev image is missing → build it (needs sudo).
#      b. Run --run-vm with --submit-mode auto.
#   3. Else → prepare USB (testOS path), print boot instructions.
#      After reboot, re-running the same command resumes (step 1).
#
# What this script does NOT do:
#   - Never auto-merge. PRs are opened for maintainer review only.
#   - Never mark milestones verified.
#   - Never edit release truth.
#   - Never fabricate hardware evidence.
#   - Never print or store tokens.

set -euo pipefail

REPO_URL="https://github.com/Nan0pk/Rush-linux.git"
REPO_HOST="Nan0pk/Rush-linux"
WORK_DIR_NAME="Rush-linux"

# --- Env overrides ----------------------------------------------------------
# RUSH_LIVEDEV_REPO_DIR:     use this path as the repo. If missing, clone there.
# RUSH_LIVEDEV_SOURCE_REPO:  clone from this local path instead of GitHub.
#                            (Used by RUSH_LIVEDEV_TEST_STUB so tests do not
#                            touch the network.)
# RUSH_LIVEDEV_TEST_STUB:    real repo resolution still runs, but USB write,
#                            reboot instructions requiring action, PR
#                            submission, and real hardware are skipped.
TEST_STUB="${RUSH_LIVEDEV_TEST_STUB:-0}"
SOURCE_REPO="${RUSH_LIVEDEV_SOURCE_REPO:-}"
REPO_DIR_OVERRIDE="${RUSH_LIVEDEV_REPO_DIR:-}"

# --- Args -------------------------------------------------------------------
AUTO=false
RESUME=false
VM=false
SMART=false
DRY_RUN=false
SKIP_MOCK=false
SUBMIT=false
DEVICE=""

usage() {
    cat <<'EOF'
livedev-bootstrap.sh — ONE-command Rush LiveDev workflow (Linux/macOS).

Default (no args): SMART mode. Auto-detects what to do:
  - USB with results plugged in  → resume + validate + submit
  - QEMU available               → build image (if needed) + run --run-vm
  - Neither                       → prepare USB, print boot instructions

Flags:
  --smart         Smart mode (default when no mode flag given).
  --vm            Force QEMU/--run-vm path (build image if missing).
  --auto          Force USB/testOS path (prepare USB, print boot instructions).
  --resume        Force resume path (copy results from USB).
  --submit        With --resume: open a real evidence PR (needs GH_TOKEN).
  --dry-run       Show commands, do not write/build/submit.
  --skip-mock     Skip mock verification (used with --auto/--smart).
  --device /dev/sdX  Optional USB device path.
  --help          Show this message.

Safety:
  - Never auto-merges. PRs are opened for maintainer review.
  - Never marks milestones verified.
  - Never edits release truth.
  - Never fabricates hardware evidence.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --smart)     SMART=true; shift ;;
        --vm)        VM=true; shift ;;
        --auto)      AUTO=true; shift ;;
        --resume)    RESUME=true; shift ;;
        --dry-run)   DRY_RUN=true; shift ;;
        --skip-mock) SKIP_MOCK=true; shift ;;
        --submit)    SUBMIT=true; shift ;;
        --device)    [[ $# -ge 2 ]] || { echo "--device requires an argument" >&2; exit 2; }
                     DEVICE="$2"; shift 2 ;;
        --help|-h)   usage; exit 0 ;;
        *)           echo "Unknown argument: $1 (try --help)" >&2; exit 2 ;;
    esac
done

# If no mode flag given, default to SMART.
if [[ "$AUTO" != "true" && "$RESUME" != "true" && "$VM" != "true" && "$SMART" != "true" ]]; then
    SMART=true
fi

# --- Helpers ----------------------------------------------------------------
log()  { echo ">> $*"; }
ok()   { echo "[OK] $*"; }
warn() { echo "[!] $*" >&2; }
die()  { echo "[X] $*" >&2; exit 1; }

# --- Locate or clone the repo ----------------------------------------------
REPO_DIR=""

# Walk up from $PWD looking for tools/livedev-next + testos/install.sh inside a .git tree.
find_repo_root() {
    local d="$PWD"
    while [[ "$d" != "/" ]]; do
        if [[ -f "$d/tools/livedev-next" && -f "$d/testos/install.sh" && -d "$d/.git" ]]; then
            REPO_DIR="$d"
            return 0
        fi
        d="$(dirname "$d")"
    done
    return 1
}

# Returns 0 if $1 is a git repo (has a .git dir or is a valid gitdir), 1 otherwise.
is_git_repo() {
    local p="$1"
    [[ -d "$p" ]] || return 1
    if [[ -d "$p/.git" ]]; then
        return 0
    fi
    if [[ -f "$p/.git" ]] && git -C "$p" rev-parse --git-dir >/dev/null 2>&1; then
        return 0
    fi
    return 1
}

# Choose the clone source: local override if provided, else GitHub.
clone_source_url() {
    if [[ -n "$SOURCE_REPO" ]]; then
        echo "$SOURCE_REPO"
    else
        echo "$REPO_URL"
    fi
}

# Clone into $1. Uses clone_source_url. Real clone (not dry-run).
do_clone() {
    local target="$1"
    local src
    src="$(clone_source_url)"
    if [[ "$TEST_STUB" == "1" && -n "$SOURCE_REPO" ]]; then
        log "Cloning from local fixture: $src -> $target"
    else
        log "Cloning from $src -> $target"
    fi
    git clone --depth 1 "$src" "$target"
}

# Sync the existing repo at $REPO_DIR: fetch origin, try to fast-forward main.
# Never destroys user work. If dirty or on a feature branch, warns and stays.
sync_existing_repo() {
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "    [dry-run] git fetch origin --prune"
        echo "    [dry-run] git checkout main (if clean and main exists)"
        echo "    [dry-run] git pull --ff-only origin main"
        return 0
    fi
    local remote_count
    remote_count="$(git -C "$REPO_DIR" remote 2>/dev/null | wc -l || echo 0)"
    if [[ "$remote_count" -eq 0 ]]; then
        warn "Repo at $REPO_DIR has no git remotes. Skipping fetch/pull."
        return 0
    fi
    log "Fetching latest main ..."
    git -C "$REPO_DIR" fetch origin --prune --quiet 2>/dev/null || \
        warn "git fetch failed (offline?). Continuing with current state."
    local current
    current="$(git -C "$REPO_DIR" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")"
    if [[ "$current" != "main" ]]; then
        if git -C "$REPO_DIR" diff --quiet 2>/dev/null && git -C "$REPO_DIR" diff --cached --quiet 2>/dev/null; then
            if git -C "$REPO_DIR" rev-parse --verify main >/dev/null 2>&1; then
                git -C "$REPO_DIR" checkout main --quiet 2>/dev/null || \
                    warn "Could not switch to main. Staying on '$current'."
            else
                warn "Branch 'main' does not exist in $REPO_DIR. Staying on '$current'."
            fi
        else
            warn "Working tree is dirty on branch '$current'. Staying on this branch."
            warn "Your local work is preserved."
        fi
    fi
    git -C "$REPO_DIR" pull --ff-only origin main --quiet 2>/dev/null || \
        warn "git pull --ff-only failed (diverged or offline?). Continuing with current state."
    ok "Repo synced."
}

ensure_repo() {
    # --- Rule E: explicit override ---
    if [[ -n "$REPO_DIR_OVERRIDE" ]]; then
        if [[ -d "$REPO_DIR_OVERRIDE" ]]; then
            if is_git_repo "$REPO_DIR_OVERRIDE"; then
                REPO_DIR="$REPO_DIR_OVERRIDE"
                ok "Using RUSH_LIVEDEV_REPO_DIR: $REPO_DIR"
            else
                die "RUSH_LIVEDEV_REPO_DIR exists but is not a git repo: $REPO_DIR_OVERRIDE"
            fi
        else
            log "RUSH_LIVEDEV_REPO_DIR=$REPO_DIR_OVERRIDE does not exist. Cloning there."
            do_clone "$REPO_DIR_OVERRIDE"
            REPO_DIR="$REPO_DIR_OVERRIDE"
            ok "Cloned into: $REPO_DIR"
        fi
        cd "$REPO_DIR"
        sync_existing_repo
        return 0
    fi

    # --- Rule A: already inside a Rush-linux repo ---
    if find_repo_root; then
        ok "Using current Rush-linux repo: $REPO_DIR"
        cd "$REPO_DIR"
        sync_existing_repo
        return 0
    fi

    # --- Rules B/C/D: not inside a repo; consider ./$WORK_DIR_NAME ---
    local candidate="$PWD/$WORK_DIR_NAME"
    if [[ -d "$candidate" ]]; then
        if is_git_repo "$candidate"; then
            # --- Rule B: reuse existing git repo ---
            REPO_DIR="$candidate"
            ok "Found existing Rush-linux git repo: $REPO_DIR"
            cd "$REPO_DIR"
            sync_existing_repo
            return 0
        else
            # --- Rule C: existing dir but NOT a git repo. Use timestamped alternate. ---
            local stamp alternate
            stamp="$(date -u +%Y%m%d-%H%M%S)"
            alternate="$PWD/${WORK_DIR_NAME}-livedev-${stamp}"
            warn "Existing ./$WORK_DIR_NAME is not a git repo; cloning into $alternate"
            do_clone "$alternate"
            REPO_DIR="$alternate"
            ok "Cloned into: $REPO_DIR"
            cd "$REPO_DIR"
            return 0
        fi
    fi

    # --- Rule D: clean directory — clone into ./$WORK_DIR_NAME ---
    REPO_DIR="$candidate"
    log "No ./$WORK_DIR_NAME found. Cloning into $REPO_DIR ..."
    do_clone "$REPO_DIR"
    ok "Cloned into: $REPO_DIR"
    cd "$REPO_DIR"
}

# --- AUTO MODE --------------------------------------------------------------
do_auto() {
    log "=== Rush LiveDev — one-command USB workflow (--auto) ==="
    echo
    echo "This writes a USB, boots the test environment, runs tests, reboots,"
    echo "resumes collection, validates results, and opens an evidence PR for"
    echo "maintainer review."
    echo
    echo "You only approve USB erase, boot from USB, physical AC/battery"
    echo "prompts, and GitHub auth."
    if [[ "$TEST_STUB" == "1" ]]; then
        echo
        warn "RUSH_LIVEDEV_TEST_STUB=1: USB write, reboot, PR, and hardware are skipped."
        warn "Repo resolution still runs for real."
    fi

    # Repo resolution ALWAYS runs (real), even in TEST_STUB mode.
    ensure_repo

    # In TEST_STUB mode: skip mock/plan/USB/boot — we only needed to prove
    # repo resolution worked end-to-end without USB/network/PR side effects.
    if [[ "$TEST_STUB" == "1" ]]; then
        echo
        ok "[TEST_STUB] Repo resolution succeeded. Skipping USB/reboot/PR."
        echo "[TEST_STUB] REPO_DIR=$REPO_DIR"
        return 0
    fi

    # Step 1: mock verification.
    if [[ "$SKIP_MOCK" != "true" ]]; then
        echo
        log "Step 1/4: Mock verification (no hardware, no network)."
        if [[ "$DRY_RUN" == "true" ]]; then
            echo "    [dry-run] python3 tools/livedev-next --mock"
        else
            python3 tools/livedev-next --mock || die "Mock verification failed. Fix before proceeding (or use --skip-mock)."
            ok "Mock verification passed."
        fi
    else
        warn "Skipping mock verification (--skip-mock)."
    fi

    # Step 2: generate plan.
    echo
    log "Step 2/4: Generate benchmark plan."
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "    [dry-run] python3 tools/livedev-next --plan"
    else
        python3 tools/livedev-next --plan || die "Plan generation failed."
        ok "Plan generated: /tmp/rush-livedev-plan.json"
    fi

    # Step 3: prepare USB using the current testOS backend.
    echo
    log "Step 3/4: Prepare USB test environment."
    echo "Using testOS as the current LiveDev boot backend."
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "    [dry-run] Would run:"
        if [[ -n "$DEVICE" ]]; then
            echo "      sudo bash testos/install.sh $DEVICE"
        else
            echo "      sudo bash testos/install.sh"
        fi
        echo "    [dry-run] Not writing USB."
    else
        if [[ -n "$DEVICE" ]]; then
            sudo bash testos/install.sh "$DEVICE"
        else
            sudo bash testos/install.sh
        fi
        ok "USB prepared."
    fi

    # Step 4: print exact reboot instructions.
    echo
    log "Step 4/4: Boot the USB and run tests."
    print_boot_instructions

    # Save checkpoint: USB is prepared, operator needs to boot.
    # After reboot, the operator runs the resume command to continue.
    local run_id
    run_id="usb-$(date -u +%Y%m%d-%H%M%S)"
    if [[ "$DRY_RUN" != "true" ]]; then
        checkpoint_save "$run_id" "usb_prepared"
        echo
        log "Checkpoint saved. After reboot, resume with one command:"
        echo "    python3 tools/rush-livedev-checkpoint.py resume-command"
    fi

    echo
    log "After testOS reboots the test machine back to its host OS, plug the USB"
    log "back into this workstation and run:"
    echo
    echo "    bash livedev-bootstrap.sh --resume"
    echo
    log "That will copy results, validate them, and run a submit dry-run."
    log "To open a real evidence PR for maintainer review (no auto-merge):"
    echo
    echo "    bash livedev-bootstrap.sh --resume --submit"
    echo
}

print_boot_instructions() {
    cat <<'EOF'

    --- Reboot instructions ---

    1. Plug the USB into the test machine (the one you want to benchmark).

    2. Reboot. Enter the boot menu:
         - Most vendors: F12, F8, F11, or Esc at the BIOS logo.
         - On Mac: hold Option immediately after the chime.

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

         bash livedev-bootstrap.sh --resume

       Or use the checkpoint resume command (prints the exact command):

         python3 tools/rush-livedev-checkpoint.py resume-command

    ---

    You only approve: USB erase, boot from USB, physical AC/battery
    prompts, and (later) GitHub auth. Everything else is automatic.
EOF
}

# --- Checkpoint integration --------------------------------------------------
# Save persistent state so the operator can resume with one command after
# reboot. The checkpoint tool stores state in ~/.local/share/rush-livedev/
# which survives reboot (unlike /tmp). NEVER stores tokens.
checkpoint_save() {
    local run_id="$1" phase="$2" run_dir="${3:-}" inventory_path="${4:-}"
    if [[ -f "$REPO_DIR/tools/rush-livedev-checkpoint.py" ]]; then
        local cmd=(python3 "$REPO_DIR/tools/rush-livedev-checkpoint.py" save
                   --run-id "$run_id" --phase "$phase")
        [[ -n "$run_dir" ]] && cmd+=(--run-dir "$run_dir")
        [[ -n "$inventory_path" ]] && cmd+=(--inventory-path "$inventory_path")
        "${cmd[@]}" 2>/dev/null || true
    fi
}

checkpoint_resume_command() {
    if [[ -f "$REPO_DIR/tools/rush-livedev-checkpoint.py" ]]; then
        python3 "$REPO_DIR/tools/rush-livedev-checkpoint.py" resume-command 2>/dev/null || true
    fi
}

# --- RESUME MODE ------------------------------------------------------------
do_resume() {
    log "=== Rush LiveDev — resume/submit ==="

    ensure_repo

    # PRE-FLIGHT: if --submit was requested, verify we can authenticate
    # BEFORE doing any work.
    if [[ "$SUBMIT" == "true" && "$DRY_RUN" != "true" ]]; then
        if ! preflight_submit_auth; then
            exit 2
        fi
    fi

    # Step 1: locate results. TWO sources:
    #   A. USB with testos-results/ (real-hardware path)
    #   B. artifacts/livedev/<run_id>/ (VM --run-vm path)
    # If neither has results, abort — do NOT fall through to submit on empty.
    echo
    log "Step 1/3: Locate results."
    RUN_DIR="$(mktemp -d -t rush-livedev-resume.XXXXXX)"

    # Try USB first.
    local usb_found=false
    if [[ "$DRY_RUN" != "true" ]]; then
        log "Checking for USB with testOS results..."
        copy_results_into_run_dir "$RUN_DIR"
        if [[ -n "$(ls -A "$RUN_DIR" 2>/dev/null)" ]]; then
            usb_found=true
            ok "Copied USB results to: $RUN_DIR"
        fi
    fi

    # If USB had nothing, try the latest VM run dir.
    if [[ "$usb_found" != "true" ]]; then
        local vm_dir=""
        if [[ -d "${REPO_DIR}/artifacts/livedev" ]]; then
            vm_dir="$(find "${REPO_DIR}/artifacts/livedev" -mindepth 1 -maxdepth 1 -type d \
                       -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2- || true)"
        fi
        if [[ -n "$vm_dir" && -f "$vm_dir/summary.json" ]]; then
            ok "Found VM run: $vm_dir"
            if [[ "$DRY_RUN" != "true" ]]; then
                # Copy the VM run dir into RUN_DIR so the submit tool can
                # find it. We also synthesize a manifest.json from the
                # summary.json so rush-submit-evidence's validator passes.
                cp -a "$vm_dir/." "$RUN_DIR/" 2>/dev/null || true
                python3 - "$RUN_DIR" <<'PYEOF'
import json, sys, os
from pathlib import Path
run_dir = Path(sys.argv[1])
summary_path = run_dir / "summary.json"
manifest_path = run_dir / "manifest.json"
if summary_path.is_file() and not manifest_path.is_file():
    try:
        s = json.loads(summary_path.read_text())
        m = {
            "schema_version": 1,
            "started_at": s.get("started_at", ""),
            "finished_at": s.get("finished_at", ""),
            "mode": "livedev-vm",
            "attempted": ["vm-test"],
            "passed": ["vm-test"] if s.get("status") == "passed" else [],
            "failed": ["vm-test"] if s.get("status") in ("failed", "timeout", "guest_failure") else [],
            "skipped": [],
            "host": {
                "fingerprint": "vm-" + s.get("run_id", "unknown")[:12],
                "kernel": s.get("host", {}).get("kernel", "unknown"),
                "cpu_model": "QEMU VM",
            },
            "testos_version": "livedev-vm",
            "_source": "vm-run",
            "_original_summary": s,
        }
        manifest_path.write_text(json.dumps(m, indent=2))
    except Exception as e:
        sys.stderr.write(f"WARNING: could not synthesize manifest.json: {e}\n")
PYEOF
            fi
        fi
    fi

    # If STILL no results, abort — do NOT fall through to submit.
    # Exception: in --dry-run mode, exit 0 (it's just an inspection).
    if [[ -z "$(ls -A "$RUN_DIR" 2>/dev/null)" ]]; then
        if [[ "$DRY_RUN" == "true" ]]; then
            echo "    [dry-run] No results found (would look for USB or VM artifacts)."
            rmdir "$RUN_DIR" 2>/dev/null || true
            return 0
        fi
        warn "No results found."
        warn "  Looked for:"
        warn "    - USB with testos-results/ (none found)"
        warn "    - artifacts/livedev/*/ (none found)"
        warn ""
        warn "  To produce results, run:"
        warn "    bash livedev-bootstrap.sh        # then pick 'vm' or 'usb'"
        rmdir "$RUN_DIR" 2>/dev/null || true
        return 1
    fi

    # Step 2: validate.
    echo
    log "Step 2/3: Validate results."
    if [[ "$DRY_RUN" != "true" ]]; then
        validate_results "$RUN_DIR"
    fi

    # Save checkpoint: results collected, ready to submit.
    if [[ "$DRY_RUN" != "true" ]]; then
        checkpoint_save "resume-$(date -u +%Y%m%d-%H%M%S)" "collected" "$RUN_DIR"
    fi

    # Step 3: submit.
    echo
    log "Step 3/3: Submit evidence."
    if [[ "$SUBMIT" == "true" ]]; then
        do_real_submit "$RUN_DIR"
    else
        do_dry_run_submit "$RUN_DIR"
    fi
}

copy_results_into_run_dir() {
    local out="$1"
    local disk part mnt latest
    # Find first removable USB disk.
    disk="$(lsblk -b -d -P -o NAME,RM,TRAN 2>/dev/null \
            | awk 'BEGIN{FS="\""} /RM="1"/ && /TRAN="usb"/ {print $2; exit}' || true)"
    if [[ -z "$disk" ]]; then
        warn "No removable USB disk auto-detected."
        return 0
    fi
    local dev="/dev/$disk"
    # Find first FAT/vfat partition on the USB.
    part="$(lsblk -ln -o NAME,TYPE,FSTYPE "$dev" 2>/dev/null \
            | awk '$2=="part" && $3 ~ /^(vfat|fat|msdos|exfat)$/ {print "/dev/"$1; exit}' || true)"
    if [[ -z "$part" ]]; then
        warn "No FAT partition found on $dev."
        return 0
    fi
    mnt="$(mktemp -d -t rush-livedev-usb.XXXXXX)"
    sudo mount -o ro "$part" "$mnt" 2>/dev/null || true
    if [[ -d "$mnt/testos-results" ]]; then
        latest="$(find "$mnt/testos-results" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' 2>/dev/null | sort | tail -1 || true)"
        if [[ -n "$latest" ]]; then
            cp -a "$mnt/testos-results/$latest/." "$out/" 2>/dev/null || true
            ok "Copied testOS run: $latest"
        fi
    fi
    sudo umount "$mnt" 2>/dev/null || true
    rmdir "$mnt" 2>/dev/null || true
}

validate_results() {
    local run_dir="$1"
    local manifest="$run_dir/manifest.json"
    if [[ ! -f "$manifest" ]]; then
        warn "No manifest.json in run dir. testOS schema validation skipped."
        warn "(LiveDev hardware-evidence validator requires run-record.json format."
        warn " testOS produces manifest.json — only basic checks are run here.)"
        return 0
    fi
    log "Validating testOS manifest: $manifest"
    if ! python3 -c "
import json
with open('$manifest') as f:
    m = json.load(f)
assert 'host' in m, 'missing host fingerprint'
assert 'passed' in m and 'failed' in m and 'skipped' in m, 'missing pass/fail/skip counts'
passed, failed, skipped = len(m['passed']), len(m['failed']), len(m['skipped'])
print('  manifest parses OK')
print('  passed=%d failed=%d skipped=%d' % (passed, failed, skipped))
if failed:
    print('  NOTE: failed tests are preserved as evidence; submit is allowed for maintainer review.')
"; then
        die "Manifest validation failed."
    fi
    ok "Results validated (basic schema check)."
    # Also try the LiveDev validator if the bundle has the right shape.
    if [[ -f "$run_dir/run-record.json" ]]; then
        log "LiveDev run-record.json detected — running full evidence validator ..."
        python3 tools/validate-hwtest-evidence.py --bundle "$run_dir" || \
            warn "LiveDev validator reported issues. Submit will still proceed in dry-run."
    fi
}

do_dry_run_submit() {
    local run_dir="$1"
    log "Submit dry-run (no push, no PR, no merge)."
    # Use the unified submit tool in dry-run mode.
    python3 tools/rush-submit-evidence "$run_dir" --submit-mode auto --dry-run || \
        warn "dry-run submit reported issues."
    echo
    log "To open a real evidence PR for maintainer review (no auto-merge):"
    echo
    echo "    bash livedev-bootstrap.sh --resume --submit"
    echo
    log "The PR will have a rich body (badge, host table, bench table, bundle)."
    log "A maintainer reviews and merges. This script never merges."
}

# --- Submit auth pre-flight --------------------------------------------------

preflight_submit_auth() {
    # Returns 0 if we can authenticate to GitHub, 1 otherwise.
    # Tries, in order:
    #   1. GH_TOKEN / GITHUB_TOKEN env var (already set).
    #   2. `gh auth status` (user has gh CLI installed and logged in).
    #   3. Interactive prompt (reads token from /dev/tty, never echoed).
    #   4. Offer to run `gh auth login` (browser-based, no token pasted).
    # If none work and no TTY, print the classic [TOKEN NEEDED] message.

    # (1) Env var already set?
    if [[ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]]; then
        ok "GH_TOKEN env var is set (will use it for submit)."
        return 0
    fi

    # (2) gh CLI installed and authenticated?
    if command -v gh >/dev/null 2>&1; then
        if gh auth status >/dev/null 2>&1; then
            ok "gh CLI is authenticated (will use it for submit — no token needed)."
            return 0
        fi
        # gh installed but not authed — offer to log in.
        if [[ -t 0 ]]; then
            echo
            log "gh CLI is installed but not authenticated."
            echo "  You can authenticate with a browser (no token pasted):"
            echo "    gh auth login"
            echo
            echo "  Or paste a GitHub token now (it won't be echoed or stored)."
            echo "  Or press Ctrl-C to cancel."
            echo
            printf "  Authenticate with gh now? [y/N] "
            local reply
            read -r reply < /dev/tty
            if [[ "$reply" =~ ^[Yy]$ ]]; then
                if gh auth login --git-protocol https --web </dev/tty; then
                    ok "gh CLI authenticated."
                    return 0
                fi
                warn "gh auth login failed."
            fi
            # Fall through to interactive token prompt.
        fi
    fi

    # (3) Interactive token prompt (only if we have a TTY).
    if [[ -t 0 ]]; then
        echo
        log "No GH_TOKEN env var and no authenticated gh CLI."
        echo "  Paste a GitHub Personal Access Token now."
        echo "  (It will NOT be echoed, NOT stored, NOT logged.)"
        echo "  Scopes needed: repo, workflow (if .github/workflows changed)."
        echo "  Press Ctrl-C to cancel."
        echo
        printf "  Token: "
        local typed
        read -rs typed < /dev/tty
        echo
        if [[ -n "$typed" ]]; then
            export GH_TOKEN="$typed"
            ok "Token accepted (set in this process's env, not stored on disk)."
            return 0
        fi
        warn "Empty token."
    fi

    # (4) Non-interactive — print the classic message.
    echo "[TOKEN NEEDED]"
    echo "No GH_TOKEN env var, no authenticated gh CLI, no terminal for prompt."
    echo "Options (best first):"
    echo "  1. Install gh CLI and run: gh auth login"
    echo "     (browser-based, no token pasted; works for all future runs)"
    echo "  2. Export a token in your shell:"
    echo "       export GH_TOKEN=github_pat_xxx"
    echo "     (typed in your terminal, not pasted from chat)"
    echo "  3. Submit locally instead of to GitHub:"
    echo "       bash livedev-bootstrap.sh --resume"
    echo "     (produces a bundle in /tmp; you can attach it to an issue manually)"
    return 1
}

do_real_submit() {
    local run_dir="$1"
    # Auth was already pre-flighted at the start of do_resume.
    log "Submit: validate, generate rich PR body, push, open/update PR."

    if [[ "$DRY_RUN" == "true" ]]; then
        echo "    [dry-run] Would run: python3 tools/rush-submit-evidence $run_dir --submit-mode auto --dry-run"
        python3 tools/rush-submit-evidence "$run_dir" --submit-mode auto --dry-run || true
        return 0
    fi

    # Use the new unified submission tool. It handles:
    #   - validation (rejects broken run dirs)
    #   - rich PR body (badge, host table, bench table, validation)
    #   - deterministic branch naming (evidence/<date>/<host>)
    #   - dedup (updates existing PR instead of creating duplicate)
    #   - auto-labeling (evidence, livedev, pass/fail)
    #   - bundle creation
    #   - GitHub auth via gh CLI or GH_TOKEN env
    python3 tools/rush-submit-evidence "$run_dir" --submit-mode auto
    local rc=$?
    if [[ $rc -ne 0 ]]; then
        die "rush-submit-evidence failed (exit $rc). Run with --dry-run to see what it would do."
    fi
    ok "Submit complete. A maintainer reviews and merges. This script never merges."
}

submit_testos_results() {
    local run_dir="$1"
    local token="$2"
    local work_dir branch date_part host_fp manifest commit_msg
    manifest="$run_dir/manifest.json"

    if [[ ! -f "$manifest" ]]; then
        die "No manifest.json in $run_dir. Cannot submit testOS results."
    fi

    # Extract metadata for the branch name and commit message.
    date_part="$(python3 -c "
import json
m = json.load(open('$manifest'))
print(m.get('started_at', '').split('T')[0] or 'unknown-date')
")"
    host_fp="$(python3 -c "
import json
m = json.load(open('$manifest'))
print(m.get('host', {}).get('fingerprint', 'unknown-host'))
")"
    branch="benchmarks/testos-${date_part}-$(date -u +%H%M%S)"

    work_dir="$(mktemp -d -t rush-livedev-submit.XXXXXX)"
    log "Cloning repo shallow into $work_dir ..."
    git clone --depth 1 "$REPO_URL" "$work_dir"
    cd "$work_dir"
    git checkout -b "$branch"

    # Copy results into the clone.
    local dest="benchmarks/results/$date_part/$host_fp"
    mkdir -p "$dest"
    cp -a "$run_dir/." "$dest/"
    find "$dest" -type d -exec chmod 755 {} +
    find "$dest" -type f -exec chmod 644 {} +

    git add benchmarks/results/
    commit_msg="evidence(bench): testOS run $date_part host=$host_fp"
    git -c user.email="livedev-bootstrap@local" -c user.name="Rush LiveDev bootstrap" commit -m "$commit_msg"
    ok "Committed: $commit_msg"

    # Push to a branch using the token. Never store the token in git config.
    local push_url="https://x-access-token:${token}@github.com/${REPO_HOST}.git"
    git push "$push_url" "$branch" >/dev/null 2>&1 || die "git push failed."
    # Scrub the URL from git config.
    git remote set-url origin "$REPO_URL" 2>/dev/null || true
    ok "Pushed branch: $branch"

    # Open a PR via the GitHub API. NO merge call.
    local pr_body
    pr_body="Auto-collected by livedev-bootstrap.sh --resume --submit.

Run summary:
- Date: $date_part
- Host: $host_fp

Includes per-benchmark JSON results and manifest.json. No auto-merge —
opened for maintainer review per Rush LiveDev policy."
    local pr_json
    pr_json="$(python3 -c "
import json, sys
print(json.dumps({
    'title': 'benchmarks(testos): results from $date_part',
    'head': '$branch',
    'base': 'main',
    'body': sys.argv[1],
}))
" "$pr_body")"
    local pr_resp
    pr_resp="$(curl -fsSL -X POST \
        -H "Authorization: Bearer ${token}" \
        -H "Accept: application/vnd.github+json" \
        "https://api.github.com/repos/${REPO_HOST}/pulls" \
        -d "$pr_json")" || die "GitHub PR API call failed."
    local pr_url
    pr_url="$(printf '%s' "$pr_resp" | python3 -c "import json,sys; print(json.load(sys.stdin).get('html_url',''))")"
    [[ -n "$pr_url" ]] || die "Could not parse PR URL from API response."
    ok "PR opened: $pr_url"
    log "No merge API call made. A maintainer reviews and merges."

    cd - >/dev/null
    rm -rf "$work_dir"
}

# --- USB result detection ---------------------------------------------------

usb_has_results() {
    # Returns 0 if a removable USB with testos-results/ is plugged in.
    local disk part mnt
    disk="$(lsblk -b -d -P -o NAME,RM,TRAN 2>/dev/null \
            | awk 'BEGIN{FS="\""} /RM="1"/ && /TRAN="usb"/ {print $2; exit}' || true)"
    [[ -n "$disk" ]] || return 1
    local dev="/dev/$disk"
    part="$(lsblk -ln -o NAME,TYPE,FSTYPE "$dev" 2>/dev/null \
            | awk '$2=="part" && $3 ~ /^(vfat|fat|msdos|exfat)$/ {print "/dev/"$1; exit}' || true)"
    [[ -n "$part" ]] || return 1
    mnt="$(mktemp -d -t rush-livedev-usb-scan.XXXXXX)"
    sudo mount -o ro "$part" "$mnt" 2>/dev/null || { rmdir "$mnt" 2>/dev/null || true; return 1; }
    local found=0
    [[ -d "$mnt/testos-results" ]] && found=1
    sudo umount "$mnt" 2>/dev/null || true
    rmdir "$mnt" 2>/dev/null || true
    [[ "$found" == "1" ]]
}

# --- QEMU/--run-vm path -----------------------------------------------------

# --- Image validation -------------------------------------------------------
# The #1 silent-failure cause: --run-vm uses the wrong image edition.
# The server image has no rush-livedev-test.service, so the VM boots to
# multi-user.target, starts getty, and the orchestrator times out 180s
# later with no markers. This function validates the image BEFORE booting.

image_has_livedev_service() {
    # Returns 0 if the image contains rush-livedev-test.service.
    # Tries guestfish first (no root), then root loopback mount.
    local img="$1"
    [[ -f "$img" ]] || return 1
    if command -v guestfish >/dev/null 2>&1; then
        guestfish -a "$img" -i exists /usr/lib/systemd/system/rush-livedev-test.service 2>/dev/null
        return $?
    fi
    if [[ "$(id -u)" == "0" ]]; then
        local mnt
        mnt="$(mktemp -d -t rush-img-scan.XXXXXX)"
        mount -o loop,offset=1048576,ro "$img" "$mnt" 2>/dev/null || {
            rmdir "$mnt" 2>/dev/null || true
            return 1
        }
        local found=1
        [[ -f "$mnt/usr/lib/systemd/system/rush-livedev-test.service" ]] && found=0
        umount "$mnt" 2>/dev/null || true
        rmdir "$mnt" 2>/dev/null || true
        return $found
    fi
    # Can't validate without guestfish or root — assume not present (safe).
    return 1
}

find_livedev_image() {
    # Find a usable livedev image. Returns path, or empty string if none.
    # Checks (in order):
    #   1. build/rush-linux-livedev.raw
    #   2. build/rush-linux.raw (symlink target)
    #   3. Any build/*.raw that has the livedev service installed.
    local candidates=(
        "${REPO_DIR}/build/rush-linux-livedev.raw"
        "${REPO_DIR}/build/rush-linux.raw"
    )
    for c in "${candidates[@]}"; do
        if [[ -f "$c" ]] && image_has_livedev_service "$c"; then
            echo "$c"
            return 0
        fi
    done
    # Scan all .raw files in build/.
    if [[ -d "${REPO_DIR}/build" ]]; then
        for c in "${REPO_DIR}"/build/*.raw; do
            [[ -f "$c" ]] || continue
            if image_has_livedev_service "$c"; then
                echo "$c"
                return 0
            fi
        done
    fi
    return 1
}

do_vm() {
    log "=== Rush LiveDev — QEMU/--run-vm path ==="
    ensure_repo

    if [[ "$TEST_STUB" == "1" ]]; then
        ok "[TEST_STUB] Skipping VM run."
        return 0
    fi

    # Step 1: Find a USABLE livedev image (has rush-livedev-test.service).
    # This is the fix for the silent-timeout bug: we validate the image
    # edition BEFORE booting, instead of timing out 180s later.
    local img=""
    img="$(find_livedev_image)" || true
    if [[ -z "$img" ]]; then
        # No usable image. Try to build one.
        if [[ "$DRY_RUN" == "true" ]]; then
            echo "    [dry-run] No livedev image found. Would build:"
            echo "    [dry-run]   sudo bash tools/build-mkosi-image.sh --edition livedev"
            return 0
        fi
        if ! command -v mkosi >/dev/null 2>&1; then
            die "No livedev image found (one with rush-livedev-test.service installed).
>> The server image won't work — it has no test runner.
>> Install mkosi and build the livedev image:
>>   sudo bash tools/build-mkosi-image.sh --edition livedev"
        fi
        log "No livedev image found. Building one (needs sudo)..."
        sudo bash tools/build-mkosi-image.sh --edition livedev
        img="$(find_livedev_image)" || die "build succeeded but image still not found"
    fi
    ok "Using livedev image: $img"

    # Determine submit mode.
    local submit_mode="auto"
    if [[ "$SUBMIT" == "true" ]]; then
        submit_mode="github"
    fi

    # State injection needs either guestfish (libguestfs) or root (for
    # loopback mount). If neither is available, auto-sudo the orchestrator
    # so the loopback-mount path works. KVM access (/dev/kvm) also often
    # needs root or the kvm group; auto-sudo covers that too.
    local need_sudo=false
    if [[ "$(id -u)" != "0" ]]; then
        if ! command -v guestfish >/dev/null 2>&1; then
            need_sudo=true
        elif [[ ! -r /dev/kvm ]]; then
            need_sudo=true
        fi
    fi

    # Build the orchestrator command.
    local cmd=(python3 tools/livedev-next --run-vm --image "$img" --submit-mode "$submit_mode")
    if [[ "$DRY_RUN" == "true" ]]; then
        cmd+=(--verbose)
    fi

    if [[ "$need_sudo" == "true" ]]; then
        log "State injection or KVM needs root — re-running orchestrator with sudo."
        log "sudo may ask for your password."
        # Preserve GH_TOKEN if set (for github submit mode).
        local sudo_env=()
        if [[ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]]; then
            sudo_env+=(GH_TOKEN="${GH_TOKEN:-${GITHUB_TOKEN}}")
        fi
        # Preserve PYTHONPATH if set (for the rush libs).
        if [[ -n "${PYTHONPATH:-}" ]]; then
            sudo_env+=(PYTHONPATH="$PYTHONPATH")
        fi
        # Run with sudo, preserving env vars explicitly.
        if [[ ${#sudo_env[@]} -gt 0 ]]; then
            local env_args=()
            for e in "${sudo_env[@]}"; do
                env_args+=(env "$e")
            done
            sudo "${env_args[@]}" "${cmd[@]}"
        else
            sudo "${cmd[@]}"
        fi
    else
        log "Running: ${cmd[*]}"
        "${cmd[@]}"
    fi
}

# --- Smart dispatcher -------------------------------------------------------

do_smart() {
    log "=== Rush LiveDev — SMART mode ==="

    ensure_repo

    if [[ "$TEST_STUB" == "1" ]]; then
        ok "[TEST_STUB] Skipping smart dispatch."
        return 0
    fi

    # Detect what's available, WITHOUT prompting for sudo yet.
    local have_qemu=false have_usb=false have_vm_image=false
    if command -v qemu-system-x86_64 >/dev/null 2>&1; then
        have_qemu=true
    fi
    # Only scan USB if we have a TTY (interactive).
    if [[ -t 0 ]] && usb_has_results; then
        have_usb=true
    fi
    # Check if a livedev image exists OR can be built (mkosi installed).
    if [[ "$have_qemu" == "true" ]]; then
        if find_livedev_image >/dev/null 2>&1; then
            have_vm_image=true
        elif command -v mkosi >/dev/null 2>&1; then
            have_vm_image=true  # can build it
        fi
    fi

    # Build the menu of available options.
    local choices=()
    local descriptions=()
    if [[ "$have_usb" == "true" ]]; then
        choices+=("resume")
        descriptions+=("Copy results from USB, validate, submit evidence PR")
    fi
    if [[ "$have_vm_image" == "true" ]]; then
        choices+=("vm")
        descriptions+=("Run deterministic QEMU test cycle (no USB, no reboot)")
    fi
    choices+=("usb")
    descriptions+=("Prepare a USB via testOS (for real-hardware testing)")
    choices+=("submit-vm")
    descriptions+=("Submit the most recent VM run (from artifacts/livedev/)")

    # Non-interactive (no TTY, or piped stdin): pick automatically.
    if [[ ! -t 0 ]]; then
        if [[ "$have_usb" == "true" ]]; then
            ok "Non-interactive + USB detected — resuming."
            do_resume
            return $?
        fi
        if [[ "$have_vm_image" == "true" ]]; then
            ok "Non-interactive + QEMU detected — using --run-vm."
            do_vm
            return $?
        fi
        do_auto
        return $?
    fi

    # Interactive: show a short menu and ask.
    echo
    echo "  What would you like to do?"
    echo
    local i=1
    for idx in "${!choices[@]}"; do
        printf "  [%d] %s — %s\n" "$i" "${choices[$idx]}" "${descriptions[$idx]}"
        i=$((i + 1))
    done
    echo
    printf "  Pick [1-%d] (or press Enter for default 1): " "${#choices[@]}"
    local reply
    read -r reply < /dev/tty
    [[ -z "$reply" ]] && reply=1
    if ! [[ "$reply" =~ ^[0-9]+$ ]] || (( reply < 1 || reply > ${#choices[@]} )); then
        die "Invalid choice: $reply"
    fi
    local pick="${choices[$((reply - 1))]}"
    echo
    case "$pick" in
        resume)
            ok "Resuming — copy results from USB, validate, submit."
            do_resume
            ;;
        vm)
            ok "Running QEMU/--run-vm cycle."
            do_vm
            ;;
        usb)
            ok "Preparing USB via testOS."
            do_auto
            ;;
        submit-vm)
            ok "Submitting the most recent VM run."
            do_resume  # do_resume now checks artifacts/livedev/ too
            ;;
    esac
}

# --- Dispatch ---------------------------------------------------------------
if [[ "$SMART" == "true" ]]; then
    do_smart
elif [[ "$VM" == "true" ]]; then
    do_vm
elif [[ "$RESUME" == "true" ]]; then
    do_resume
else
    do_auto
fi
