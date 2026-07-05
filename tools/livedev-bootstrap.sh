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

    ---

    You only approve: USB erase, boot from USB, physical AC/battery
    prompts, and (later) GitHub auth. Everything else is automatic.
EOF
}

# --- RESUME MODE ------------------------------------------------------------
do_resume() {
    log "=== Rush LiveDev — resume after reboot (--resume) ==="

    ensure_repo

    # Step 1: locate USB and copy results into a run dir.
    echo
    log "Step 1/3: Locate USB and copy results."
    RUN_DIR="$(mktemp -d -t rush-livedev-resume.XXXXXX)"
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "    [dry-run] Would scan for removable USB, mount its ESP read-only,"
        echo "    [dry-run]   and copy testos-results/<latest>/ into: $RUN_DIR"
    else
        log "sudo may ask for your password to mount the USB read-only."
        copy_results_into_run_dir "$RUN_DIR"
        if [[ -z "$(ls -A "$RUN_DIR" 2>/dev/null)" ]]; then
            warn "No results copied (USB may not be plugged in, or no testos-results/ on it)."
            warn "Run dir kept for inspection: $RUN_DIR"
        else
            ok "Results copied to: $RUN_DIR"
        fi
    fi

    # Step 2: validate copied results where possible.
    echo
    log "Step 2/3: Validate results."
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "    [dry-run] Would validate testOS manifest.json (parses, has passed/failed counts)."
        echo "    [dry-run] Would run: python3 tools/validate-hwtest-evidence.py --bundle <run_dir> (if applicable)"
    else
        validate_results "$RUN_DIR"
    fi

    # Step 3: submit dry-run by default; real submit only if --submit.
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
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "    [dry-run] Would run: python3 tools/livedev-next --submit $run_dir --dry-run"
        return 0
    fi
    if [[ -f "$run_dir/run-record.json" ]]; then
        python3 tools/livedev-next --submit "$run_dir" --dry-run || \
            warn "livedev-next --submit --dry-run reported issues."
    else
        ok "testOS results staged in: $run_dir"
        echo
        log "To open a real evidence PR for maintainer review (no auto-merge):"
        echo
        echo "    bash livedev-bootstrap.sh --resume --submit"
        echo
        log "The PR will be opened on a branch. A maintainer reviews and merges."
    fi
}

do_real_submit() {
    local run_dir="$1"
    # Token check. Never print the token.
    local token="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
    if [[ -z "$token" ]]; then
        echo "[TOKEN NEEDED]"
        echo "Export GH_TOKEN or GITHUB_TOKEN, then rerun:"
        echo "    export GH_TOKEN=github_pat_xxx"
        echo "    bash livedev-bootstrap.sh --resume --submit"
        exit 2
    fi

    log "Real submit: open evidence PR for maintainer review (no auto-merge)."
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "    [dry-run] Would push branch and open PR via GitHub API."
        echo "    [dry-run] Token present (not printed). No merge API call would be made."
        return 0
    fi

    if [[ -f "$run_dir/run-record.json" ]]; then
        # LiveDev-shaped run: use livedev-next --submit (no --dry-run).
        # rush_pr_lib.py never calls the merge API.
        GH_TOKEN="$token" python3 tools/livedev-next --submit "$run_dir" || \
            die "livedev-next --submit failed."
    else
        # testOS-shaped run: do a self-contained push + PR open.
        # We never call the GitHub merge API.
        submit_testos_results "$run_dir" "$token"
    fi
    ok "Submit complete. PR opened for maintainer review."
    log "A maintainer reviews and merges the PR. This script never merges."
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

do_vm() {
    log "=== Rush LiveDev — QEMU/--run-vm path ==="
    ensure_repo

    if [[ "$TEST_STUB" == "1" ]]; then
        ok "[TEST_STUB] Skipping VM run."
        return 0
    fi

    # Locate or build the livedev image.
    local img="${REPO_DIR}/build/rush-linux-livedev.raw"
    if [[ ! -f "$img" ]]; then
        # Fall back to the server image if livedev wasn't built.
        local server_img="${REPO_DIR}/build/rush-linux-server.raw"
        if [[ -f "$server_img" ]]; then
            img="$server_img"
            warn "livedev image not found; using server image: $img"
        else
            if [[ "$DRY_RUN" == "true" ]]; then
                echo "    [dry-run] Would build livedev image: sudo bash tools/build-mkosi-image.sh --edition livedev"
                return 0
            fi
            if ! command -v mkosi >/dev/null 2>&1; then
                die "mkosi not installed and livedev image not found at $img. Install mkosi or build the image manually."
            fi
            log "Building livedev image (needs sudo)..."
            sudo bash tools/build-mkosi-image.sh --edition livedev
        fi
    fi

    # Determine submit mode.
    local submit_mode="auto"
    if [[ "$SUBMIT" == "true" ]]; then
        submit_mode="github"
    fi

    # Hand off to livedev-next --run-vm.
    local cmd=(python3 tools/livedev-next --run-vm --image "$img" --submit-mode "$submit_mode")
    if [[ "$DRY_RUN" == "true" ]]; then
        cmd+=(--verbose)
    fi
    log "Running: ${cmd[*]}"
    python3 tools/livedev-next --run-vm --image "$img" --submit-mode "$submit_mode"
}

# --- Smart dispatcher -------------------------------------------------------

do_smart() {
    log "=== Rush LiveDev — SMART mode (auto-detect) ==="

    ensure_repo

    if [[ "$TEST_STUB" == "1" ]]; then
        ok "[TEST_STUB] Skipping smart dispatch."
        return 0
    fi

    # Step 1: Is a USB with results plugged in? → resume.
    if usb_has_results; then
        ok "Detected USB with testOS results — resuming."
        if [[ "$SUBMIT" == "true" ]]; then
            do_resume
        else
            do_resume
        fi
        return $?
    fi

    # Step 2: Is QEMU available? → --run-vm path.
    if command -v qemu-system-x86_64 >/dev/null 2>&1; then
        ok "QEMU detected — using --run-vm path."
        do_vm
        return $?
    fi

    # Step 3: Fall back to USB/testOS path.
    warn "No USB results detected and no QEMU available — falling back to USB/testOS path."
    do_auto
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
