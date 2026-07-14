#!/usr/bin/env bash
# testos/install.sh — download the latest prebuilt testOS image and write it to a USB stick.
#
# Usage (one-liner):
#   curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh | sudo bash
#
# Or specify a disk explicitly:
#   sudo bash install.sh /dev/sdX
#
# What it does:
#   1. Finds the latest testOS release on GitHub.
#   2. Downloads testos-<version>.raw.zst (cached in ~/.cache/testos-installer/),
#      decompresses it, verifies SHA256SUMS.
#   3. Auto-detects the USB stick if no device is specified.
#   4. Refuses the host root disk and disks smaller than the image.
#   5. Auto-unmounts mounted USB partitions, asks for confirmation, writes with dd.
#
# Supported platforms:
#   - Linux x86_64 (uses the prebuilt testos-launcher binary)
#   - macOS / BSD (falls back to plain dd — the launcher binaries are Linux-only)
#
# Windows users: use testos/install.ps1 instead — native PowerShell, no
# WSL, no Rufus, no third-party tools required.
#
# If the prebuilt image isn't available yet (e.g. the release workflow hasn't
# been triggered), this script will tell you to build from source instead.

set -euo pipefail

REPO="Nan0pk/Rush-linux"
# Use /releases (not /releases/latest, which skips prereleases) and fetch
# 10 so we can skip draft releases. GitHub returns drafts first in the
# /releases listing; if a draft exists, per_page=1 would return it instead
# of the latest published release. We filter drafts below.
API_URL="https://api.github.com/repos/${REPO}/releases?per_page=10"

# --- Colors (only if stdout is a terminal) --------------------------------
if [ -t 1 ]; then
    BOLD=$'\033[1m'
    RED=$'\033[31m'
    GREEN=$'\033[32m'
    AMBER=$'\033[33m'
    RESET=$'\033[0m'
else
    BOLD=""; RED=""; GREEN=""; AMBER=""; RESET=""
fi

log()  { echo "${BOLD}>> ${RESET}$*"; }
ok()   { echo "${GREEN}[OK]${RESET} $*"; }
warn() { echo "${AMBER}[!] ${RESET}$*" >&2; }
die()  { echo "${RED}[X] ${RESET}$*" >&2; exit 1; }

read_tty() {
    local prompt="$1" var="$2"
    if [[ -r /dev/tty ]]; then
        read -r -p "$prompt" "$var" </dev/tty
    else
        read -r -p "$prompt" "$var"
    fi
}

# --- Cache + transcript logging --------------------------------------------
CACHE_BASE="${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}"
CACHE_DIR="${CACHE_BASE}/testos-installer"
mkdir -p "${CACHE_DIR}"
LOGFILE="${CACHE_DIR}/install-log-$(date -u +%Y%m%d-%H%M%SZ).txt"
exec > >(tee -a "$LOGFILE") 2>&1
log "Install transcript: ${LOGFILE}"

# --- Argument parsing ------------------------------------------------------
DEVICE=""
IMAGE_FLAG=""          # --image <path>
SKIP_VERIFY=false      # --skip-verification
CLEAN_CACHE=false      # --clean-cache
DRY_RUN=false
LIST_ONLY=false
FORCE=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        /dev/*) DEVICE="$1"; shift ;;
        --image)
            [[ $# -ge 2 ]] || die "--image requires a path argument"
            IMAGE_FLAG="$2"; shift 2 ;;
        --skip-verification) SKIP_VERIFY=true; shift ;;
        --clean-cache)       CLEAN_CACHE=true; shift ;;
        --dry-run)  DRY_RUN=true; shift ;;
        --list)     LIST_ONLY=true; shift ;;
        --force)    FORCE=true; shift ;;
        --help|-h)
            cat <<'EOF'
testOS installer -- download and write the latest testOS image to USB.

Usage:
  sudo bash install.sh                            Auto-detect USB, download + write
  sudo bash install.sh /dev/sdX                   Download + write to /dev/sdX
  sudo bash install.sh --list                     Show latest release assets
  sudo bash install.sh --dry-run                  Download and verify, don't write
  sudo bash install.sh --image /path/to/img.raw   Use a local image, skip download
  sudo bash install.sh --clean-cache              Delete cache and re-download

Options:
  /dev/sdX             Target USB device. If omitted, scans removable USB disks.
  --image <path>       Path to a local .raw or .raw.zst. Skips the GitHub download.
                       SHA256 is still verified against the release SHA256SUMS
                       unless you also pass --skip-verification.
  --skip-verification  Skip SHA256 check (use with --image when offline).
  --clean-cache        Delete the local cache directory before running.
  --dry-run            Download/verify/decompress but don't write to the device.
  --list               Just show what's in the latest release.
  --force              Bypass the removable-media and size-sanity safety checks.
  --help               This message.

Cache directory: ${XDG_CACHE_HOME:-$HOME/.cache}/testos-installer/
  The installer caches the downloaded .raw.zst and decompressed .raw here.
  Second run of the same version skips the 582 MB re-download.
EOF
            exit 0
            ;;
        *) die "Unknown argument: $1 (try --help)" ;;
    esac
done

# --- Preflight checks ------------------------------------------------------
[[ "$(uname -s)" == "Linux" || "$(uname -s)" == "Darwin" ]] || \
    die "This script supports Linux and macOS only. On Windows, use testos/install.ps1."

command -v curl >/dev/null || die "curl is required."
command -v dd   >/dev/null || die "dd is required."
command -v sha256sum >/dev/null || command -v shasum >/dev/null || \
    die "sha256sum (or shasum on macOS) is required."

if [[ -z "$IMAGE_FLAG" && "$LIST_ONLY" != "true" ]]; then
    # We'll need zstd for decompression — check now before the download.
    command -v zstd >/dev/null || \
        die "zstd is required to decompress the image. Install with: apt install zstd / pacman -S zstd / brew install zstd"
fi

# --- Cache directory -------------------------------------------------------
if $CLEAN_CACHE; then
    log "Cleaning cached images at ${CACHE_DIR} ..."
    rm -f "${CACHE_DIR}"/testos-*.raw "${CACHE_DIR}"/testos-*.raw.zst 2>/dev/null || true
    ok "Cached images cleared. Install logs kept."
fi

mkdir -p "${CACHE_DIR}"

# --- Find the latest release -----------------------------------------------
log "Finding the latest testOS release..."
RELEASE_JSON_RAW="$(curl -fsSL "$API_URL" || true)"
[[ -n "$RELEASE_JSON_RAW" ]] || \
    die "Could not fetch release info from $API_URL. Either there are no releases yet, or you're rate-limited. Try again in a few minutes, or build from source: see the README's 'Build from source' section."

# Extract the first non-draft release.
if command -v python3 >/dev/null; then
    RELEASE_JSON="$(printf '%s' "$RELEASE_JSON_RAW" | python3 -c '
import sys, json
r = json.load(sys.stdin)
if isinstance(r, dict): r = [r]
pub = [x for x in r if not x.get("draft")]
if not pub:
    sys.exit(1)
print(json.dumps(pub[0], indent=2))
' 2>/dev/null)" || die "No non-draft releases found at $API_URL. The release workflow may not have run yet."
elif command -v jq >/dev/null; then
    RELEASE_JSON="$(printf '%s' "$RELEASE_JSON_RAW" | jq '[.[] | select(.draft != true)][0]' 2>/dev/null)" || \
        die "No non-draft releases found."
else
    RELEASE_JSON="$(printf '%s' "$RELEASE_JSON_RAW" | awk '
        /"draft": false/ { in_block = 1 }
        in_block { print }
        in_block && /^}/ { in_block = 0; exit }
    ')"
    [[ -n "$RELEASE_JSON" ]] || die "Could not parse releases (no python3/jq). Install one: apt install python3 / brew install python3"
fi

VERSION="$(printf '%s' "$RELEASE_JSON" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": "([^"]+)".*/\1/')"
[[ -n "$VERSION" ]] || die "Could not parse release tag. The release may be malformed."

log "Latest release: ${BOLD}${VERSION}${RESET}"

if $LIST_ONLY; then
    echo
    echo "Assets in this release:"
    printf '%s' "$RELEASE_JSON" | grep '"browser_download_url"' | sed -E 's/.*"browser_download_url": "([^"]+)".*/  \1/'
    echo
    exit 0
fi

# --- Check that a release image exists -------------------------------------
ASSET_URLS="$(printf '%s' "$RELEASE_JSON" | grep '"browser_download_url"' | sed -E 's/.*"browser_download_url": "([^"]+)".*/\1/')"
echo "$ASSET_URLS" | grep -qE 'testos-.*\.raw(\.zst)?$' || {
    warn "The latest release (${VERSION}) does not contain a testOS-*.raw(.zst) image."
    warn "This usually means the release workflow is still running."
    echo
    echo "To build from source instead, see:"
    echo "  https://github.com/${REPO}#build-from-source"
    exit 1
}

IMAGE_URL="$(echo "$ASSET_URLS" | grep -E 'testos-.*\.raw(\.zst)?$' | head -1)"
IMAGE_BASENAME="$(basename "$IMAGE_URL")"
IS_ZST=false
[[ "$IMAGE_BASENAME" == *.zst ]] && IS_ZST=true
RAW_BASENAME="${IMAGE_BASENAME%.zst}"

SUMS_URL="$(echo "$ASSET_URLS" | grep -E 'SHA256SUMS$' | head -1 || true)"

# --- Working directory ------------------------------------------------------
WORK_DIR="$(mktemp -d -t testos-install.XXXXXX)"
trap 'rm -rf "${WORK_DIR}"' EXIT

# --- Helper: sha256 check one file against SHA256SUMS content -------------
# Usage: check_sha256 <filepath> <sums_content>
# Returns 0 if match, 1 if filename not in sums. Exits on mismatch.
check_sha256() {
    local filepath="$1"
    local sums_content="$2"
    local fname
    fname="$(basename "$filepath")"
    local expected
    expected="$(printf '%s' "$sums_content" | grep -E "^[0-9a-fA-F]{64}[[:space:]]+\*?${fname}[[:space:]]*$" | awk '{print $1}' | head -1 || true)"
    if [[ -z "$expected" ]]; then
        return 1  # not found in sums
    fi
    log "Verifying SHA256 for ${fname} ..."
    local actual
    if command -v sha256sum >/dev/null; then
        actual="$(sha256sum "$filepath" | awk '{print $1}')"
    else
        actual="$(shasum -a 256 "$filepath" | awk '{print $1}')"
    fi
    if [[ "${actual,,}" == "${expected,,}" ]]; then
        ok "SHA256 OK for ${fname}"
        return 0
    else
        warn "SHA256 MISMATCH for ${fname}"
        warn "  Expected: ${expected}"
        warn "  Actual:   ${actual}"
        die "Checksum verification failed. The file may be corrupted or stale."
    fi
}

# --- Fetch SHA256SUMS (always fresh, it's tiny) ----------------------------
SUMS_CONTENT=""
if [[ -n "$SUMS_URL" ]] && ! $SKIP_VERIFY; then
    log "Downloading SHA256SUMS..."
    curl -fsSL --progress-bar -o "${WORK_DIR}/SHA256SUMS" "$SUMS_URL" || die "Failed to download SHA256SUMS."
    SUMS_CONTENT="$(cat "${WORK_DIR}/SHA256SUMS")"
fi

# --- Cache paths -----------------------------------------------------------
ZST_CACHE="${CACHE_DIR}/${IMAGE_BASENAME}"    # e.g. cache/testos-0.7.0-beta.4.raw.zst
RAW_CACHE="${CACHE_DIR}/${RAW_BASENAME}"      # e.g. cache/testos-0.7.0-beta.4.raw

RESOLVED_RAW=""  # will be set to the final .raw path before writing

# --- PATH A: user supplied a local file ------------------------------------
if [[ -n "$IMAGE_FLAG" ]]; then
    [[ -f "$IMAGE_FLAG" ]] || die "The file specified with --image does not exist: ${IMAGE_FLAG}"
    log "Using local file: ${IMAGE_FLAG}"

    if [[ "$IMAGE_FLAG" == *.zst ]]; then
        if $SKIP_VERIFY; then
            warn "Skipping SHA256 verification (--skip-verification)."
        elif [[ -n "$SUMS_CONTENT" ]]; then
            check_sha256 "$IMAGE_FLAG" "$SUMS_CONTENT" || {
                warn "$(basename "$IMAGE_FLAG") not found in SHA256SUMS. Pass --skip-verification to proceed anyway."
                exit 1
            }
        fi
        log "Decompressing $(basename "$IMAGE_FLAG") ..."
        zstd -d -f "$IMAGE_FLAG" -o "${RAW_CACHE}" || die "zstd decompression failed."
        RESOLVED_RAW="${RAW_CACHE}"
    else
        if ! $SKIP_VERIFY && [[ -n "$SUMS_CONTENT" ]]; then
            check_sha256 "$IMAGE_FLAG" "$SUMS_CONTENT" || {
                warn "$(basename "$IMAGE_FLAG") not found in SHA256SUMS. Pass --skip-verification to proceed anyway."
                exit 1
            }
        elif $SKIP_VERIFY; then
            warn "Skipping SHA256 verification (--skip-verification)."
        fi
        RESOLVED_RAW="$IMAGE_FLAG"
    fi
else
    # --- PATH B: download with cache --------------------------------------
    log "Checking cache at ${CACHE_DIR} ..."

    # B1: both .raw and .zst cached, .zst hash OK -> skip download + decompress
    if $IS_ZST && [[ -f "${RAW_CACHE}" && -f "${ZST_CACHE}" ]] && ! $SKIP_VERIFY && [[ -n "$SUMS_CONTENT" ]]; then
        if check_sha256 "${ZST_CACHE}" "$SUMS_CONTENT" 2>/dev/null; then
            ok "Cache hit (raw+zst): using ${RAW_CACHE} (skipping download and decompression)."
            RESOLVED_RAW="${RAW_CACHE}"
        else
            warn "Cached .zst hash mismatch - stale cache. Deleting and re-downloading."
            rm -f "${ZST_CACHE}" "${RAW_CACHE}"
        fi
    fi

    # B2: .zst cached, hash OK -> skip download, decompress to .raw
    if [[ -z "$RESOLVED_RAW" ]] && $IS_ZST && [[ -f "${ZST_CACHE}" ]] && ! $SKIP_VERIFY && [[ -n "$SUMS_CONTENT" ]]; then
        if check_sha256 "${ZST_CACHE}" "$SUMS_CONTENT" 2>/dev/null; then
            ok "Cache hit (.zst): using cached ${ZST_CACHE}"
            log "Decompressing cached ${IMAGE_BASENAME} ..."
            zstd -d -f -v "${ZST_CACHE}" -o "${RAW_CACHE}" || die "zstd decompression of cached file failed."
            RESOLVED_RAW="${RAW_CACHE}"
        else
            warn "Cached .zst hash mismatch - deleting stale file."
            rm -f "${ZST_CACHE}"
        fi
    fi

    # B3: cache miss - download from GitHub
    if [[ -z "$RESOLVED_RAW" ]]; then
        log "Cache miss - downloading from GitHub..."
        DOWNLOAD_DEST="${WORK_DIR}/${IMAGE_BASENAME}"
        curl -fsSL --progress-bar -o "${DOWNLOAD_DEST}" "$IMAGE_URL" || die "Download failed: $IMAGE_URL"

        if ! $SKIP_VERIFY && [[ -n "$SUMS_CONTENT" ]]; then
            check_sha256 "${DOWNLOAD_DEST}" "$SUMS_CONTENT" || \
                warn "Image filename not found in SHA256SUMS - skipping verification."
        fi

        log "Caching downloaded image to ${ZST_CACHE} ..."
        cp "${DOWNLOAD_DEST}" "${ZST_CACHE}"

        if $IS_ZST; then
            log "Decompressing ${IMAGE_BASENAME} ..."
            zstd -d -f -v "${ZST_CACHE}" -o "${RAW_CACHE}" || die "zstd decompression failed."
            RESOLVED_RAW="${RAW_CACHE}"
        else
            RESOLVED_RAW="${ZST_CACHE}"
        fi
    fi
fi

# --- Download side-car binaries (ingest + bench-list) ---------------------
if [[ "$(uname -s)" == "Linux" ]]; then
    LAUNCHER_URL="$(echo "$ASSET_URLS" | grep -E 'testos-launcher-.*-linux-x86_64$' | head -1 || true)"
    INGEST_URL="$(echo "$ASSET_URLS"   | grep -E 'testos-ingest-.*-linux-x86_64$'   | head -1 || true)"
    if [[ -n "$LAUNCHER_URL" ]]; then
        log "Downloading testos-launcher..."
        curl -fsSL --progress-bar -o "${WORK_DIR}/testos-launcher" "$LAUNCHER_URL"
        chmod +x "${WORK_DIR}/testos-launcher"
    fi
    if [[ -n "$INGEST_URL" ]]; then
        log "Downloading testos-ingest..."
        curl -fsSL --progress-bar -o "${WORK_DIR}/testos-ingest" "$INGEST_URL"
        chmod +x "${WORK_DIR}/testos-ingest"
    fi
fi
BENCH_URL="$(echo "$ASSET_URLS" | grep 'bench-list.toml' | head -1 || true)"
if [[ -n "$BENCH_URL" ]]; then
    curl -fsSL --progress-bar -o "${WORK_DIR}/bench-list.toml" "$BENCH_URL" 2>/dev/null || true
fi

IMAGE_SIZE_BYTES="$(stat -c %s "${RESOLVED_RAW}" 2>/dev/null || stat -f %z "${RESOLVED_RAW}" 2>/dev/null || echo 0)"
IMAGE_SIZE_MB=$(( IMAGE_SIZE_BYTES / 1024 / 1024 ))

# --- Device helpers --------------------------------------------------------
partition_paths() {
    local dev="$1" name type
    lsblk -ln -o NAME,TYPE "$dev" 2>/dev/null | while read -r name type _; do
        [[ "$type" == "part" ]] && printf '/dev/%s\n' "$name"
    done
}

root_base_device() {
    local src pk
    src="$(findmnt -n -o SOURCE / 2>/dev/null || true)"
    [[ -n "$src" ]] || return 0
    pk="$(lsblk -no PKNAME "$src" 2>/dev/null | head -1 || true)"
    [[ -n "$pk" ]] && printf '/dev/%s\n' "$pk" || true
}

select_usb_device() {
    log "No device specified. Scanning for removable USB disks..."
    mapfile -t usb_lines < <(lsblk -b -d -P -o NAME,SIZE,RM,TRAN,MODEL,VENDOR 2>/dev/null | awk 'BEGIN{FS="\""} /RM="1"/ && /TRAN="usb"/ {print}')
    if [[ ${#usb_lines[@]} -eq 0 ]]; then
        echo
        echo "No removable USB disks found."
        echo
        echo "All disks currently visible:"
        lsblk -b -d -o NAME,SIZE,RM,TRAN,MODEL,VENDOR 2>/dev/null || lsblk
        die "Plug in a USB stick and re-run, or pass /dev/sdX explicitly."
    fi
    if [[ ${#usb_lines[@]} -eq 1 ]]; then
        eval "${usb_lines[0]}"
        DEVICE="/dev/${NAME}"
        ok "Found 1 USB disk: ${DEVICE} (${SIZE} bytes, ${VENDOR:-} ${MODEL:-})"
        return
    fi
    echo
    echo "Multiple USB disks found:"
    local i line
    for i in "${!usb_lines[@]}"; do
        eval "${usb_lines[$i]}"
        printf '  [%d] /dev/%s  %s bytes  %s %s\n' "$((i+1))" "$NAME" "$SIZE" "${VENDOR:-}" "${MODEL:-}"
    done
    echo
    local choice
    read_tty "Select a USB disk by number (1-${#usb_lines[@]}): " choice
    [[ "$choice" =~ ^[0-9]+$ && "$choice" -ge 1 && "$choice" -le ${#usb_lines[@]} ]] || die "Invalid selection: $choice"
    eval "${usb_lines[$((choice-1))]}"
    DEVICE="/dev/${NAME}"
    log "Selected ${DEVICE}"
}

auto_unmount_partitions() {
    local dev="$1" part targets target
    while read -r part; do
        [[ -n "$part" ]] || continue
        mapfile -t targets < <(findmnt -rn -S "$part" -o TARGET 2>/dev/null || true)
        for target in "${targets[@]}"; do
            [[ -n "$target" ]] || continue
            log "Unmounting ${part} from ${target} ..."
            umount "$target" || die "Failed to unmount ${part} from ${target}. Close open files and retry."
        done
    done < <(partition_paths "$dev")
}

# --- Dry-run stops here ---------------------------------------------------
if $DRY_RUN; then
    ok "Dry run complete."
    echo
    echo "Image: ${RESOLVED_RAW} (${IMAGE_SIZE_MB} MB)"
    echo "Cache: ${CACHE_DIR}"
    echo
    echo "Re-run without --dry-run to auto-detect and write a USB:"
    echo "  sudo bash $0"
    exit 0
fi

# --- Device selection and safety checks -----------------------------------
[[ -n "$DEVICE" ]] || select_usb_device
[[ -e "$DEVICE" ]] || die "Device $DEVICE does not exist. Check with 'lsblk'."

# Refuse to write to the host's root disk.
if [[ "$(uname -s)" == "Linux" ]]; then
    ROOT_BASE="$(root_base_device)"
    if [[ -n "$ROOT_BASE" && "$DEVICE" == "$ROOT_BASE" ]]; then
        die "Device $DEVICE is the host's root disk. Refusing to overwrite."
    fi
fi

# Refuse non-removable disks unless --force.
if [[ "$FORCE" != "true" ]] && command -v lsblk >/dev/null; then
    RM_FLAG="$(lsblk -d -n -o RM "$DEVICE" 2>/dev/null | tr -d ' ' || true)"
    if [[ "$RM_FLAG" == "0" ]]; then
        warn "Device $DEVICE reports RM=0 (not removable). This looks like an internal disk."
        die "Refusing to write to a non-removable disk. Re-run with --force if intentional."
    fi
fi

# Size sanity.
if command -v lsblk >/dev/null; then
    DISK_SIZE_BYTES="$(lsblk -b -d -n -o SIZE "$DEVICE" 2>/dev/null | head -1 || echo 0)"
    if [[ -n "$DISK_SIZE_BYTES" && "$DISK_SIZE_BYTES" -gt 0 ]]; then
        DISK_SIZE_MB=$(( DISK_SIZE_BYTES / 1024 / 1024 ))
        if [[ "$DISK_SIZE_MB" -gt $(( IMAGE_SIZE_MB * 4 )) ]]; then
            log "Note: target disk is ${DISK_SIZE_MB} MB, image is ${IMAGE_SIZE_MB} MB; remaining space will stay unallocated."
        fi
        if [[ "$DISK_SIZE_MB" -lt "$IMAGE_SIZE_MB" ]]; then
            die "Target disk (${DISK_SIZE_MB} MB) is smaller than the image (${IMAGE_SIZE_MB} MB)."
        fi
    fi
fi

if [[ $EUID -ne 0 ]]; then
    warn "Not running as root. dd will probably fail. Re-run with sudo."
fi

# --- Confirm: show disk identity and ask 'yes' ----------------------------
echo
echo "${BOLD}About to write ${IMAGE_SIZE_MB} MiB to:${RESET}"
echo "  Device:  ${DEVICE}"
if command -v lsblk >/dev/null; then
    DISK_INFO="$(lsblk -d -n -o VENDOR,MODEL,SIZE,TRAN,RM "$DEVICE" 2>/dev/null | head -1 || true)"
    [[ -n "$DISK_INFO" ]] && echo "  Identity: ${DISK_INFO}"
fi
echo
echo "${RED}ALL DATA ON THIS DISK WILL BE LOST.${RESET}"
echo

if [[ "$FORCE" != "true" ]]; then
    read_tty "Is this your USB stick? Type 'yes' to confirm (anything else aborts): " CONFIRM
    [[ "$CONFIRM" == "yes" ]] || die "Confirmation was not 'yes'. Aborting."
fi

# --- Unmount then write the image ------------------------------------------
auto_unmount_partitions "$DEVICE"
log "Writing ${RESOLVED_RAW} to ${DEVICE} with dd..."
dd if="${RESOLVED_RAW}" of="$DEVICE" bs=4M status=progress conv=fsync
sync
command -v blockdev  >/dev/null && blockdev --flushbufs "$DEVICE"  2>/dev/null || true
command -v partprobe >/dev/null && partprobe "$DEVICE"             2>/dev/null || true

ok "Write complete."
echo
echo "${BOLD}Next steps:${RESET}"
echo
echo "  1. Plug the USB into the test machine."
echo "  2. Reboot. Enter the boot menu (F12, F8, F11, or Esc -- depends on vendor)."
echo "  3. Pick the USB from the list."
echo "  4. (If it refuses to boot) Disable Secure Boot -- testOS UKIs are unsigned for now."
echo "  5. testOS boots, shows a menu of benchmarks."
echo "  6. Pick 'Run all' (0) or specific test numbers. Press Esc to abort."
echo "  7. When done, testOS syncs the USB and reboots back to the host OS."
echo "  8. Plug the USB back here, then collect + push results:"
echo
echo "       export GH_TOKEN=github_pat_xxx   # GITHUB_TOKEN is also accepted by LiveDev submit"
echo "       curl -fsSL https://raw.githubusercontent.com/${REPO}/main/testos/collect-results.sh | sudo bash"
echo
echo "  Results land in benchmarks/results/<date>/<host-fingerprint>/ and open a PR for maintainer review."
echo
echo "Install log saved to: ${LOGFILE}"
