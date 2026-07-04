#!/usr/bin/env bash
# tools/livedev-bootstrap.sh — one-command Rush LiveDev USB workflow for Linux/macOS.
#
# Usage:
#   bash livedev-bootstrap.sh --auto                # full path: mock -> plan -> USB -> boot prompt
#   bash livedev-bootstrap.sh --auto --dry-run      # print every command, do not write USB
#   bash livedev-bootstrap.sh --resume              # after reboot: copy results -> validate -> submit dry-run
#   bash livedev-bootstrap.sh --resume --submit     # after validation: open PR for maintainer review
#   bash livedev-bootstrap.sh --skip-mock --auto    # skip the mock verification step
#
# What this script does NOT do:
#   - Never auto-merge. PRs are opened for maintainer review only.
#   - Never mark milestones verified.
#   - Never edit release truth (VERSION, RELEASES.md, milestones.toml, ADRs, CI workflow).
#   - Never fabricate hardware evidence. Results only come from the USB.
#   - Never print or store tokens. If a token is needed, prints exactly: [TOKEN NEEDED]

set -euo pipefail

REPO_URL="https://github.com/Nan0pk/Rush-linux.git"
REPO_HOST="Nan0pk/Rush-linux"
WORK_DIR_NAME="Rush-linux"

# --- Args -------------------------------------------------------------------
AUTO=false
RESUME=false
DRY_RUN=false
SKIP_MOCK=false
SUBMIT=false
DEVICE=""

usage() {
    cat <<'EOF'
livedev-bootstrap.sh — one-command Rush LiveDev USB workflow (Linux/macOS).

Flags:
  --auto           Full path: clone/fetch repo, mock verify, generate plan,
                   prepare USB using the current testOS backend, print boot instructions.
  --resume         After rebooting back from testOS: locate USB, copy results,
                   validate, run submit dry-run.
  --dry-run        Print every command that would run. Do not write USB.
  --skip-mock      Skip the mock verification step (used with --auto).
  --submit         Used with --resume: open a real evidence PR for maintainer review.
                   No auto-merge. Requires GH_TOKEN.
  --device /dev/sdX  Optional USB device path (otherwise testOS auto-detects).
  --help           Show this message.

Workflow:
  1. bash livedev-bootstrap.sh --auto
  2. (script tells you to boot the USB, run tests, reboot back)
  3. bash livedev-bootstrap.sh --resume
  4. (optional) bash livedev-bootstrap.sh --resume --submit

Safety:
  - Never auto-merges. PRs are opened for maintainer review.
  - Never marks milestones verified.
  - Never edits release truth.
  - Never fabricates hardware evidence.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
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

if [[ "$AUTO" != "true" && "$RESUME" != "true" ]]; then
    echo ">> No mode selected. Use --auto or --resume." >&2
    echo ">>   bash livedev-bootstrap.sh --auto" >&2
    echo ">>   bash livedev-bootstrap.sh --resume" >&2
    exit 2
fi

# --- Helpers ----------------------------------------------------------------
log()  { echo ">> $*"; }
ok()   { echo "[OK] $*"; }
warn() { echo "[!] $*" >&2; }
die()  { echo "[X] $*" >&2; exit 1; }

# --- Locate or clone the repo ----------------------------------------------
REPO_DIR=""
find_repo_root() {
    local d="$PWD"
    while [[ "$d" != "/" ]]; do
        if [[ -f "$d/tools/livedev-next" && -f "$d/testos/install.sh" ]]; then
            REPO_DIR="$d"
            return 0
        fi
        d="$(dirname "$d")"
    done
    return 1
}

ensure_repo() {
    if find_repo_root; then
        ok "Inside repo: $REPO_DIR"
    else
        log "Not inside repo. Cloning into ./$WORK_DIR_NAME ..."
        if [[ "$DRY_RUN" == "true" ]]; then
            echo "    [dry-run] git clone --depth 1 $REPO_URL $WORK_DIR_NAME"
            REPO_DIR="$PWD/$WORK_DIR_NAME"
        else
            git clone --depth 1 "$REPO_URL" "$WORK_DIR_NAME"
            REPO_DIR="$PWD/$WORK_DIR_NAME"
        fi
    fi

    cd "$REPO_DIR"

    if [[ "$DRY_RUN" != "true" ]]; then
        log "Fetching latest main ..."
        git fetch origin --prune --quiet
        local current
        current="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")"
        if [[ "$current" != "main" ]]; then
            if git diff --quiet && git diff --cached --quiet; then
                git checkout main --quiet 2>/dev/null || true
            else
                warn "Working tree is dirty on branch '$current'. Staying on this branch."
            fi
        fi
        git pull --ff-only origin main --quiet 2>/dev/null || true
        ok "Repo is up to date."
    else
        echo "    [dry-run] git fetch origin --prune"
        echo "    [dry-run] git checkout main"
        echo "    [dry-run] git pull --ff-only origin main"
    fi
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

    ensure_repo

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
print('  manifest parses OK')
print('  passed=%d failed=%d skipped=%d' % (len(m['passed']), len(m['failed']), len(m['skipped'])))
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
        echo "Export GH_TOKEN, then rerun:"
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

# --- Dispatch ---------------------------------------------------------------
if [[ "$RESUME" == "true" ]]; then
    do_resume
else
    do_auto
fi
