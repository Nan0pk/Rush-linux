#!/usr/bin/env bash
# testos/collect-results.sh — collect Linux results and open a reviewable PR.

set -euo pipefail

REPO="Nan0pk/Rush-linux"
TOKEN="${GITHUB_TOKEN:-}"
DISK=""
DRY_RUN=false
DIAGNOSE=false
LIST_ONLY=false
SOURCE_DIR=""
BRANCH=""
WORK_DIR=""
MOUNT_DIR=""
MOUNTED_BY_US=false
KEEP_WORK=false


log()  { echo ">> $*"; }
ok()   { echo "[OK] $*"; }
warn() { echo "[!] $*" >&2; }
die()  { echo "[X] $*" >&2; exit 1; }

read_tty() {
    local prompt="$1" var="$2"
    if [[ -r /dev/tty ]]; then
        read -r -p "$prompt" "$var" </dev/tty
    else
        read -r -p "$prompt" "$var"
    fi
}

read_secret_tty() {
    local prompt="$1" var="$2"
    if [[ -r /dev/tty ]]; then
        read -r -s -p "$prompt" "$var" </dev/tty
    else
        read -r -s -p "$prompt" "$var"
    fi
    echo
}

usage() {
    cat <<'EOF'
testOS results collector — ONE command, end to end.

Usage:
  sudo bash collect-results.sh                         Auto: find USB, copy, commit, push, open PR
  sudo bash collect-results.sh --disk /dev/sdX         Specify which USB
  sudo bash collect-results.sh --dry-run               Do everything except push/open PR
  sudo bash collect-results.sh --diagnose              Print disk diagnostics only
  sudo bash collect-results.sh --list                  List results on USB only
  sudo bash collect-results.sh --repo Nan0pk/Rush-linux
Testing helper:
  sudo bash collect-results.sh --source /tmp/mock-usb --dry-run

Environment:
  GITHUB_TOKEN must be set explicitly. With sudo, use
  `sudo --preserve-env=GITHUB_TOKEN`. The token is used only for git push and
  PR creation; it is not written to git config. This script never merges or
  enables auto-merge.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --disk)      [[ $# -ge 2 ]] || die "--disk requires /dev/sdX"; DISK="$2"; shift 2 ;;
        --repo)      [[ $# -ge 2 ]] || die "--repo requires owner/repo"; REPO="$2"; shift 2 ;;
        --source)    [[ $# -ge 2 ]] || die "--source requires a mounted testOS root"; SOURCE_DIR="$2"; shift 2 ;;
        --dry-run)   DRY_RUN=true; shift ;;
        --diagnose)  DIAGNOSE=true; shift ;;
        --list)      LIST_ONLY=true; shift ;;
        --help|-h)   usage; exit 0 ;;
        *) die "Unknown argument: $1 (try --help)" ;;
    esac
done

need_cmd() { command -v "$1" >/dev/null || die "$1 is required."; }
for c in curl jq git; do need_cmd "$c"; done
if [[ -z "$SOURCE_DIR" ]]; then
    for c in lsblk blkid findmnt mount umount; do need_cmd "$c"; done
fi

if $DIAGNOSE; then
    echo "=== all disks ==="
    lsblk -b -o NAME,SIZE,TYPE,FSTYPE,LABEL,MOUNTPOINTS,RM,TRAN,VENDOR,MODEL
    echo
    echo "=== removable USB disks ==="
    lsblk -b -d -o NAME,SIZE,RM,TRAN,VENDOR,MODEL | awk 'NR==1 || ($3==1 && $4=="usb")'
    echo
    echo "=== filesystem labels ==="
    blkid || true
    exit 0
fi

if [[ -z "$TOKEN" && "$DRY_RUN" != "true" && "$LIST_ONLY" != "true" ]]; then
    if [[ -t 0 ]]; then
        read_secret_tty "GitHub token (used for this process only): " TOKEN
    fi
    [[ -n "$TOKEN" ]] || die "No GitHub token. Export GITHUB_TOKEN; with sudo, use --preserve-env=GITHUB_TOKEN."
fi

cleanup() {
    if [[ "$MOUNTED_BY_US" == "true" && -n "$MOUNT_DIR" ]]; then
        umount "$MOUNT_DIR" >/dev/null 2>&1 || true
    fi
    if [[ -n "$MOUNT_DIR" && -d "$MOUNT_DIR" && "$MOUNTED_BY_US" == "true" ]]; then
        rmdir "$MOUNT_DIR" >/dev/null 2>&1 || true
    fi
    if [[ "$KEEP_WORK" != "true" && -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT

partition_paths() {
    local dev="$1" name type
    lsblk -ln -o NAME,TYPE "$dev" 2>/dev/null | while read -r name type _; do
        [[ "$type" == "part" ]] && printf '/dev/%s\n' "$name"
    done
}

select_usb_disk() {
    log "Scanning for removable USB disks..."
    mapfile -t usb_lines < <(lsblk -b -d -P -o NAME,SIZE,RM,TRAN,MODEL,VENDOR 2>/dev/null | awk 'BEGIN{FS="\""} /RM="1"/ && /TRAN="usb"/ {print}')
    if [[ ${#usb_lines[@]} -eq 0 ]]; then
        lsblk -b -d -o NAME,SIZE,RM,TRAN,VENDOR,MODEL 2>/dev/null || true
        die "No removable USB disks found. Plug in the testOS USB and re-run."
    fi
    if [[ ${#usb_lines[@]} -eq 1 ]]; then
        eval "${usb_lines[0]}"
        DISK="/dev/${NAME}"
        ok "Found 1 USB disk: ${DISK} (${SIZE} bytes, ${VENDOR:-} ${MODEL:-})"
        return
    fi
    echo "Multiple USB disks found:"
    local i choice
    for i in "${!usb_lines[@]}"; do
        eval "${usb_lines[$i]}"
        printf '  [%d] /dev/%s  %s bytes  %s %s\n' "$((i+1))" "$NAME" "$SIZE" "${VENDOR:-}" "${MODEL:-}"
    done
    read_tty "Select a USB disk by number (1-${#usb_lines[@]}): " choice
    [[ "$choice" =~ ^[0-9]+$ && "$choice" -ge 1 && "$choice" -le ${#usb_lines[@]} ]] || die "Invalid selection: $choice"
    eval "${usb_lines[$((choice-1))]}"
    DISK="/dev/${NAME}"
}

find_esp_partition() {
    local part label fstype
    while read -r part; do
        [[ -n "$part" ]] || continue
        label="$(blkid -s LABEL -o value "$part" 2>/dev/null || true)"
        if [[ "$label" == "RUSHESP" ]]; then
            printf '%s\n' "$part"
            return 0
        fi
    done < <(partition_paths "$DISK")
    while read -r part; do
        [[ -n "$part" ]] || continue
        fstype="$(blkid -s TYPE -o value "$part" 2>/dev/null || true)"
        case "$fstype" in
            vfat|fat|msdos|exfat) printf '%s\n' "$part"; return 0 ;;
        esac
    done < <(partition_paths "$DISK")
    return 1
}

mount_results_root() {
    if [[ -n "$SOURCE_DIR" ]]; then
        [[ -d "$SOURCE_DIR/testos-results" ]] || die "--source must point at a mounted root containing testos-results/: $SOURCE_DIR"
        MOUNT_DIR="$SOURCE_DIR"
        ok "Using mounted source: $MOUNT_DIR"
        return
    fi
    [[ -n "$DISK" ]] || select_usb_disk
    [[ -b "$DISK" ]] || die "$DISK is not a block device."
    local esp
    esp="$(find_esp_partition || true)"
    [[ -n "$esp" ]] || die "Could not find RUSHESP or a FAT partition on $DISK."
    MOUNT_DIR="$(mktemp -d -t testos-results.XXXXXX)"
    log "Mounting $esp read-only at $MOUNT_DIR ..."
    mount -o ro "$esp" "$MOUNT_DIR" || die "Failed to mount $esp read-only."
    MOUNTED_BY_US=true
    [[ -d "$MOUNT_DIR/testos-results" ]] || die "No testos-results/ on $esp. Did testOS run benchmarks?"
    ok "Mounted results at $MOUNT_DIR/testos-results"
}

latest_run_dir() {
    find "$MOUNT_DIR/testos-results" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort | tail -1
}

manifest_value() { jq -r "$1 // empty" "$2"; }

# Validate that a string is safe to use as a single path component under
# the repo's benchmarks/results/ tree. Rejects:
#   - empty strings
#   - anything containing / or \ (path separators)
#   - '.' or '..' (traversal)
#   - strings starting with '-' (could be misread as a flag by downstream tools)
#   - strings containing NUL, newline, or any byte outside the safe set
# Allowed characters: ASCII letters, digits, '_', '-', '.', ':', and '+'.
# (':' and '+' are included because host fingerprints and ISO timestamps use them.)
safe_path_component() {
    local name="$1" label="$2"
    [[ -n "$name" ]] || die "Refusing to use empty value for $label (possible manifest tampering)."
    [[ "$name" != "." && "$name" != ".." ]] || die "Refusing to use traversal name '$name' for $label."
    [[ "$name" != */* && "$name" != *\\* ]] || die "Refusing to use path-containing value '$name' for $label."
    [[ "$name" != -* ]] || die "Refusing to use dash-leading value '$name' for $label (flag injection risk)."
    # Reject any byte that isn't in the safe set. Use printf to handle NUL.
    local bad
    bad="$(printf '%s' "$name" | LC_ALL=C tr -d 'A-Za-z0-9_.:-+' )"
    [[ -z "$bad" ]] || die "Refusing to use $label with disallowed characters: '$name' (unexpected bytes: '$bad')."
}

# Verify that a resolved path is strictly under a parent directory.
# Uses realpath(1) so symlinks are resolved; if realpath is unavailable,
# falls back to readlink -f. If neither exists, the check is skipped with a warning.
is_strictly_under() {
    local child="$1" parent="$2"
    local child_resolved parent_resolved
    if command -v realpath >/dev/null 2>&1; then
        child_resolved="$(realpath "$child" 2>/dev/null || true)"
        parent_resolved="$(realpath "$parent" 2>/dev/null || true)"
    elif command -v readlink >/dev/null 2>&1; then
        child_resolved="$(readlink -f "$child" 2>/dev/null || true)"
        parent_resolved="$(readlink -f "$parent" 2>/dev/null || true)"
    else
        warn "realpath/readlink unavailable; cannot verify path containment for $child. Proceeding with caution."
        return 0
    fi
    [[ -n "$child_resolved" && -n "$parent_resolved" ]] || return 1
    # child must equal parent, or start with parent + '/'
    [[ "$child_resolved" == "$parent_resolved" || "$child_resolved" == "$parent_resolved/"* ]]
}

copy_tree() {
    local src="$1" dst="$2"
    mkdir -p "$dst"
    # Security: reject symlinks in the source tree before copying. A hostile
    # USB could include a symlink pointing outside the run dir (e.g. to
    # /etc/shadow or /dev/zero); tar would faithfully reproduce it, and a
    # downstream tool writing through the symlink would escape the sandbox.
    # --no-symlinks is a GNU tar extension; if unavailable, fall back to a
    # find-based symlink rejection.
    if (cd "$src" && tar --help 2>&1 | grep -q -- '--no-symlinks'); then
        (cd "$src" && tar --no-symlinks -cf - .) | (cd "$dst" && tar -xf -)
    else
        # find -type l lists any symlinks; if any exist, abort.
        local symcount
        symcount="$(find "$src" -type l 2>/dev/null | wc -l)"
        if [[ "$symcount" -gt 0 ]]; then
            die "Refusing to copy: source tree contains $symcount symlink(s). Hostile media may try to escape the ingestion directory."
        fi
        (cd "$src" && tar -cf - .) | (cd "$dst" && tar -xf -)
    fi
}

write_summary() {
    local dir="$1" manifest="$2" out
    out="$dir/SUMMARY.md"
    {
        echo "# testOS Benchmark Results — $(manifest_value '.host.fingerprint' "$manifest")"
        echo
        echo "- **Run started**: $(manifest_value '.started_at' "$manifest")"
        echo "- **Run finished**: $(manifest_value '.finished_at' "$manifest")"
        echo "- **Mode**: $(manifest_value '.mode' "$manifest")"
        echo "- **testOS version**: $(manifest_value '.testos_version' "$manifest")"
        echo "- **Host CPU**: $(manifest_value '.host.cpu_model' "$manifest")"
        echo "- **Passed / Failed / Skipped**: $(jq '.passed|length' "$manifest") / $(jq '.failed|length' "$manifest") / $(jq '.skipped|length' "$manifest")"
        echo
        echo "## Files"
        echo
        find "$dir" -maxdepth 1 -type f -name '*.json' -printf '- %f\n' | sort
    } > "$out"
}

mount_results_root
RESULTS_ROOT="$MOUNT_DIR/testos-results"

if $LIST_ONLY; then
    echo "=== Results on USB ==="
    find "$RESULTS_ROOT" -maxdepth 3 -type f -printf '%p\n' | sort
    exit 0
fi

RUN_NAME="$(latest_run_dir)"
[[ -n "$RUN_NAME" ]] || die "No run directories found under $RESULTS_ROOT."
# RUN_NAME comes from find -maxdepth 1 -type d -printf '%f', so it cannot
# contain '/'. But it could theoretically be '.' or '..' if the filesystem
# is hostile; validate defensively.
safe_path_component "$RUN_NAME" "run directory name"
RUN_DIR="$RESULTS_ROOT/$RUN_NAME"
MANIFEST="$RUN_DIR/manifest.json"
[[ -f "$MANIFEST" ]] || die "No manifest.json in latest run: $RUN_DIR"

PASSED="$(jq '.passed|length' "$MANIFEST")"
FAILED="$(jq '.failed|length' "$MANIFEST")"
SKIPPED="$(jq '.skipped|length' "$MANIFEST")"
TOTAL=$((PASSED + FAILED + SKIPPED))
STARTED_AT="$(manifest_value '.started_at' "$MANIFEST")"
HOST_FP="$(manifest_value '.host.fingerprint' "$MANIFEST")"
DATE_PART="${STARTED_AT%%T*}"
[[ -n "$DATE_PART" && "$DATE_PART" != "$STARTED_AT" ]] || DATE_PART="${RUN_NAME%%T*}"
[[ -n "$HOST_FP" ]] || HOST_FP="unknown-host"

# Security (audit finding #2): DATE_PART and HOST_FP are derived from
# untrusted manifest fields on hostile media. They are used to construct
# the destination path for rm -rf + copy_tree. Validate them BEFORE any
# filesystem mutation to prevent path traversal.
safe_path_component "$DATE_PART" "manifest started_at date"
safe_path_component "$HOST_FP" "manifest host.fingerprint"

ok "Latest run: $RUN_NAME — $PASSED passed, $FAILED failed, $SKIPPED skipped ($TOTAL total)"

STAMP="$(date -u +%Y%m%d-%H%M%SZ)"
BRANCH="benchmarks/testos-${STAMP}"
WORK_DIR="$(mktemp -d -t testos-collect.XXXXXX)"
SAFE_URL="https://github.com/${REPO}.git"
PUSH_URL="https://x-access-token:${TOKEN}@github.com/${REPO}.git"

log "Cloning $REPO shallow into $WORK_DIR ..."
git clone --depth 1 "$SAFE_URL" "$WORK_DIR"

DEST_RESULTS="$WORK_DIR/benchmarks/results/$DATE_PART/$HOST_FP"
# Security: verify the computed DEST_RESULTS is strictly under the repo's
# benchmarks/results/ directory before the rm -rf. This is a redundant
# post-check — safe_path_component above already validated each component —
# but defense in depth: a future bug in DATE_PART/HOST_FP derivation that
# re-introduces traversal would be caught here instead of causing a
# destructive rm -rf outside the intended tree.
DEST_PARENT="$WORK_DIR/benchmarks/results"
mkdir -p "$DEST_PARENT"
if ! is_strictly_under "$DEST_RESULTS" "$DEST_PARENT"; then
    die "Refusing to proceed: destination '$DEST_RESULTS' is not strictly under '$DEST_PARENT'. Possible path traversal."
fi
log "Copying latest run to benchmarks/results/$DATE_PART/$HOST_FP ..."
rm -rf "$DEST_RESULTS"
copy_tree "$RUN_DIR" "$DEST_RESULTS"
# Post-copy verification: ensure nothing escaped during the tar copy.
if ! is_strictly_under "$DEST_RESULTS" "$DEST_PARENT"; then
    die "Post-copy containment check failed: '$DEST_RESULTS' resolved outside '$DEST_PARENT'."
fi
write_summary "$DEST_RESULTS" "$DEST_RESULTS/manifest.json"

INSTALL_LOG_SRC="${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}/testos-installer"
if [[ -d "$INSTALL_LOG_SRC" ]] && compgen -G "$INSTALL_LOG_SRC/install-log-*.txt" >/dev/null; then
    mkdir -p "$WORK_DIR/install-logs"
    cp "$INSTALL_LOG_SRC"/install-log-*.txt "$WORK_DIR/install-logs/"
    ok "Copied install logs."
fi

cd "$WORK_DIR"
git checkout -b "$BRANCH"
git add benchmarks/results/ install-logs/ 2>/dev/null || git add benchmarks/results/
if git diff --cached --quiet; then
    die "No changes to commit after copying results."
fi
COMMIT_MSG="evidence(bench): testOS run ${DATE_PART} pass=${PASSED} fail=${FAILED} skip=${SKIPPED}"
git -c user.email="testos-bot@local" -c user.name="testOS collector" commit -m "$COMMIT_MSG"
ok "Committed: $COMMIT_MSG"

if $DRY_RUN; then
    KEEP_WORK=true
    ok "Dry run complete. Temp clone kept at $WORK_DIR"
    exit 0
fi

log "Pushing $BRANCH ..."
git push "$PUSH_URL" "$BRANCH"
git config --local --unset "branch.${BRANCH}.remote" 2>/dev/null || true
git config --local --unset "branch.${BRANCH}.merge" 2>/dev/null || true
git remote set-url origin "$SAFE_URL" 2>/dev/null || true
ok "Pushed and scrubbed local git config."

PR_BODY="Auto-collected from USB by collect-results.sh.\n\nRun summary:\n- Date: ${STARTED_AT}\n- Host: ${HOST_FP}\n- Passed: ${PASSED}\n- Failed: ${FAILED}\n- Skipped: ${SKIPPED}\n\nIncludes per-benchmark JSON results, generated SUMMARY.md, system logs captured by testOS, and installer logs when available."
PR_JSON="$(jq -n --arg title "benchmarks(testos): results from ${DATE_PART}" --arg head "$BRANCH" --arg base main --arg body "$PR_BODY" '{title:$title, head:$head, base:$base, body:$body}')"
log "Opening PR ..."
PR_RESP="$(curl -fsSL -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Accept: application/vnd.github+json" \
    "https://api.github.com/repos/${REPO}/pulls" \
    -d "$PR_JSON")"
PR_NUM="$(printf '%s' "$PR_RESP" | jq -r '.number')"
PR_URL="$(printf '%s' "$PR_RESP" | jq -r '.html_url')"
[[ "$PR_NUM" != "null" && -n "$PR_NUM" ]] || die "Failed to open PR: $PR_RESP"
ok "Opened PR #$PR_NUM — $PR_URL"
ok "Collection complete. CI and the maintainer now review the PR; this script will not merge it."
