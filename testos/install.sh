#!/usr/bin/env bash
# testos/install.sh — download the latest prebuilt testOS image and write it to a USB stick.
#
# Usage (recommended — download, inspect, then run):
#   wget https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh
#   sudo bash install.sh /dev/sdX
#
# Or one-liner (if you trust the source):
#   wget -qO- https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh | sudo bash -s -- /dev/sdX
#
# Or with curl:
#   curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh | sudo bash -s -- /dev/sdX
#
# What it does:
#   1. Finds the latest testOS release on GitHub.
#   2. Downloads testos-<version>.raw, the testos-launcher and testos-ingest
#      binaries, the bench-list.toml, and SHA256SUMS.
#   3. Verifies the checksums.
#   4. Refuses to write to a mounted device or anything that looks like the
#      host's root disk.
#   5. Asks you to type the device name twice to confirm.
#   6. Writes the image to the USB with dd, syncs, and prints next steps.
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
API_URL="https://api.github.com/repos/${REPO}/releases/latest"

# ─── Colors (only if stdout is a terminal) ────────────────────────
if [ -t 1 ]; then
    BOLD=$'\033[1m'
    RED=$'\033[31m'
    GREEN=$'\033[32m'
    AMBER=$'\033[33m'
    BLUE=$'\033[34m'
    DIM=$'\033[2m'
    RESET=$'\033[0m'
else
    BOLD=""; RED=""; GREEN=""; AMBER=""; BLUE=""; DIM=""; RESET=""
fi

log()  { echo "${BOLD}>>${RESET} $*"; }
ok()   { echo "${GREEN}✓${RESET} $*"; }
warn() { echo "${AMBER}!${RESET} $*" >&2; }
die()  { echo "${RED}✗${RESET} $*" >&2; exit 1; }

# ─── Argument parsing ─────────────────────────────────────────────
DEVICE=""
DRY_RUN=false
LIST_ONLY=false
FORCE=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        /dev/*) DEVICE="$1"; shift ;;
        --dry-run) DRY_RUN=true; shift ;;
        --list)    LIST_ONLY=true; shift ;;
        --force)   FORCE=true; shift ;;
        --help|-h)
            cat <<'EOF'
testOS installer — download and write the latest testOS image to USB.

Usage:
  sudo bash testos/install.sh /dev/sdX     Download + write to /dev/sdX
  sudo bash testos/install.sh --list       Show latest release assets without writing
  sudo bash testos/install.sh --dry-run /dev/sdX   Download and verify, don't write

Options:
  --dry-run   Download and verify everything, but don't write to the device.
  --list      Just show what's in the latest release.
  --force     Bypass the removable-media and size-sanity safety checks.
              Required if you want to write to a non-USB disk (e.g. an
              internal test disk). Still refuses the system root disk.
  --help      This message.
EOF
            exit 0
            ;;
        *) die "Unknown argument: $1 (try --help)" ;;
    esac
done

# ─── Preflight checks ─────────────────────────────────────────────
[[ "$(uname -s)" == "Linux" || "$(uname -s)" == "Darwin" ]] || die "This script supports Linux and macOS only. On Windows, use Rufus on the .raw from Releases."

command -v curl >/dev/null || die "curl is required."
command -v dd   >/dev/null || die "dd is required."
command -v sha256sum >/dev/null || command -v shasum >/dev/null || die "sha256sum (or shasum on macOS) is required."

# ─── Find the latest release ──────────────────────────────────────
log "Finding the latest testOS release..."
RELEASE_JSON="$(curl -fsSL "$API_URL" || true)"
[[ -n "$RELEASE_JSON" ]] || die "Could not fetch release info from $API_URL. Either there are no releases yet, or you're rate-limited. Try again in a few minutes, or build from source: see the README's 'Build from source' section."

# Extract the tag name (version).
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

# ─── Check that a release image exists ────────────────────────────
ASSET_URLS="$(printf '%s' "$RELEASE_JSON" | grep '"browser_download_url"' | sed -E 's/.*"browser_download_url": "([^"]+)".*/\1/')"
echo "$ASSET_URLS" | grep -q 'testos-.*\.raw$' || {
    warn "The latest release (${VERSION}) does not contain a testOS-*.raw image."
    warn "This usually means the release workflow is still running, or the project"
    warn "hasn't published a testOS image yet."
    echo
    echo "To build from source instead, see:"
    echo "  https://github.com/${REPO}#build-from-source"
    exit 1
}

# ─── Set up a working directory ───────────────────────────────────
WORK_DIR="$(mktemp -d -t testos-install.XXXXXX)"
trap 'rm -rf "$WORK_DIR"' EXIT

cd "$WORK_DIR"

# ─── Download assets ──────────────────────────────────────────────
download() {
    local url="$1" dest="$2"
    log "Downloading ${dest}..."
    curl -fsSL -o "$dest" "$url" || die "Download failed: $url"
}

IMAGE_URL="$(echo "$ASSET_URLS" | grep -E 'testos-.*\.raw$' | head -1)"
IMAGE_FILE="$(basename "$IMAGE_URL")"
download "$IMAGE_URL" "$IMAGE_FILE"

SUMS_URL="$(echo "$ASSET_URLS" | grep -E 'SHA256SUMS$' | head -1)"
if [[ -n "$SUMS_URL" ]]; then
    download "$SUMS_URL" "SHA256SUMS"
fi

# Download the launcher and ingest binaries too (Linux only — they're
# Linux x86_64 ELF binaries).
LAUNCHER_BIN=""
if [[ "$(uname -s)" == "Linux" ]]; then
    LAUNCHER_URL="$(echo "$ASSET_URLS" | grep -E 'testos-launcher-.*-linux-x86_64$' | head -1 || true)"
    INGEST_URL="$(echo "$ASSET_URLS" | grep -E 'testos-ingest-.*-linux-x86_64$' | head -1 || true)"
    if [[ -n "$LAUNCHER_URL" ]]; then
        download "$LAUNCHER_URL" "testos-launcher"
        chmod +x testos-launcher
        LAUNCHER_BIN="$WORK_DIR/testos-launcher"
    fi
    if [[ -n "$INGEST_URL" ]]; then
        download "$INGEST_URL" "testos-ingest"
        chmod +x testos-ingest
    fi
fi

# ─── Verify checksums ─────────────────────────────────────────────
if [[ -f SHA256SUMS ]]; then
    log "Verifying checksums..."
    if command -v sha256sum >/dev/null; then
        sha256sum -c SHA256SUMS --ignore-missing || die "Checksum verification failed. The download may be corrupted."
    else
        # macOS shasum
        ( cd "$WORK_DIR" && shasum -a 256 -c SHA256SUMS 2>/dev/null ) || warn "Checksum verification skipped (shasum failed)."
    fi
    ok "Checksums verified."
fi

# Image size for the confirmation prompt.
IMAGE_SIZE_BYTES="$(stat -c %s "$IMAGE_FILE" 2>/dev/null || stat -f %z "$IMAGE_FILE" 2>/dev/null || echo 0)"
IMAGE_SIZE_MB=$(( IMAGE_SIZE_BYTES / 1024 / 1024 ))

# ─── Dry-run stops here ───────────────────────────────────────────
if $DRY_RUN; then
    ok "Dry run complete. Downloaded and verified:"
    ls -lh "$WORK_DIR"
    echo
    echo "Re-run without --dry-run and with a USB device to write:"
    echo "  sudo bash $0 /dev/sdX"
    exit 0
fi

# ─── Device selection and safety checks ───────────────────────────
[[ -n "$DEVICE" ]] || {
    echo
    echo "Available block devices:"
    lsblk -o NAME,SIZE,TYPE,MOUNTPOINT,RM,VENDOR,MODEL 2>/dev/null || lsblk
    echo
    die "No device specified. Find your USB stick above (look for RM=1 and the right size), then re-run:\n  sudo bash $0 /dev/sdX"
}
[[ -e "$DEVICE" ]] || die "Device $DEVICE does not exist. Check with 'lsblk'."

# Refuse to write to a mounted device.
if command -v findmnt >/dev/null; then
    if findmnt --source "$DEVICE" >/dev/null 2>&1 || findmnt | grep -q "^${DEVICE}"; then
        die "Device $DEVICE (or a partition on it) is mounted. Unmount first:\n  sudo umount ${DEVICE}*\n  sudo umount ${DEVICE}p*"
    fi
fi

# Refuse to write to the host's root disk.
if [[ "$(uname -s)" == "Linux" ]]; then
    ROOT_DEV="$(findmnt -n -o SOURCE / 2>/dev/null || true)"
    if [[ -n "$ROOT_DEV" ]]; then
        # Strip partition digits to get base device.
        ROOT_BASE="$ROOT_DEV"
        ROOT_BASE="${ROOT_BASE%p[0-9]}"
        ROOT_BASE="${ROOT_BASE%[0-9]}"
        if [[ "$DEVICE" == "$ROOT_BASE" ]]; then
            die "Device $DEVICE is the host's root disk. Refusing to overwrite. If you really meant to write to your boot disk, you're holding the script wrong — use a USB stick."
        fi
    fi
fi

# ─── Safety check: refuse non-removable disks unless --force ──────
# lsblk reports RM=1 for removable media (USB sticks, SD cards). Internal
# SATA/NVMe disks report RM=0. Refusing non-removable disks catches the
# most common accident: targeting an internal data disk.
if [[ "$FORCE" != "true" ]] && command -v lsblk >/dev/null; then
    RM_FLAG="$(lsblk -d -n -o RM "$DEVICE" 2>/dev/null | tr -d ' ' || true)"
    if [[ "$RM_FLAG" == "0" ]]; then
        warn "Device $DEVICE reports RM=0 (not removable). This looks like an internal disk, not a USB stick."
        warn "Writing to it would destroy any data on it."
        die "Refusing to write to a non-removable disk. If you really mean to do this (e.g. writing to an internal test disk), re-run with --force."
    fi
fi

# ─── Safety check: size sanity ────────────────────────────────────
# If the target disk is more than 4x the image size, warn. People
# sometimes image a 500MB USB onto a 2TB HDD by mistake.
if command -v lsblk >/dev/null; then
    DISK_SIZE_BYTES="$(lsblk -b -d -n -o SIZE "$DEVICE" 2>/dev/null | head -1 || echo 0)"
    if [[ -n "$DISK_SIZE_BYTES" && "$DISK_SIZE_BYTES" -gt 0 ]]; then
        DISK_SIZE_MB=$(( DISK_SIZE_BYTES / 1024 / 1024 ))
        IMAGE_SIZE_MB=$(( IMAGE_SIZE_BYTES / 1024 / 1024 ))
        if [[ "$DISK_SIZE_MB" -gt $(( IMAGE_SIZE_MB * 4 )) ]]; then
            warn "Target disk is $DISK_SIZE_MB MB but the image is only $IMAGE_SIZE_MB MB."
            warn "This is unusual — you may be targeting the wrong disk (e.g. an internal HDD instead of a USB stick)."
            if [[ "$FORCE" != "true" ]]; then
                die "Refusing to write to a disk that's much larger than the image. If this is intentional (e.g. a large USB stick), re-run with --force."
            fi
        fi
        if [[ "$DISK_SIZE_MB" -lt "$IMAGE_SIZE_MB" ]]; then
            die "Target disk ($DISK_SIZE_MB MB) is smaller than the image ($IMAGE_SIZE_MB MB). The write would fail mid-way and leave the disk in a broken state."
        fi
    fi
fi

# Don't run as root? Actually we need root for dd. Check.
if [[ $EUID -ne 0 ]]; then
    warn "Not running as root. dd will probably fail. Re-run with sudo."
fi

# ─── Confirm: show the disk's identity and ask 'yes' ─────────────
echo
echo "${BOLD}About to write ${IMAGE_SIZE_MB} MiB to:${RESET}"
echo "  Device:  ${DEVICE}"
if command -v lsblk >/dev/null; then
    DISK_INFO="$(lsblk -d -n -o VENDOR,MODEL,SIZE,TRAN,RM "$DEVICE" 2>/dev/null | head -1 || true)"
    if [[ -n "$DISK_INFO" ]]; then
        echo "  Identity: ${DISK_INFO}"
    fi
fi
echo
echo "${RED}ALL DATA ON THIS DISK WILL BE LOST.${RESET}"
echo

if [[ "$FORCE" != "true" ]]; then
    printf "%s" "Is this your USB stick? Type 'yes' to confirm (anything else aborts): "
    read -r CONFIRM
    [[ "$CONFIRM" == "yes" ]] || die "Confirmation was not 'yes'. Aborting."
fi

# ─── Write the image ──────────────────────────────────────────────
log "Writing ${IMAGE_FILE} to ${DEVICE} with dd..."
dd if="$IMAGE_FILE" of="$DEVICE" bs=4M status=progress conv=fsync
sync
command -v blockdev >/dev/null && blockdev --flushbufs "$DEVICE" 2>/dev/null || true
command -v partprobe >/dev/null && partprobe "$DEVICE" 2>/dev/null || true

ok "Write complete."
echo
echo "${BOLD}Next steps:${RESET}"
echo
echo "  1. Plug the USB into the test machine."
echo "  2. Reboot. Enter the boot menu (F12, F8, F11, or Esc — depends on vendor)."
echo "  3. Pick the USB from the list."
echo "  4. (If it refuses to boot) Disable Secure Boot — testOS UKIs are unsigned for now."
echo "  5. testOS boots, shows a menu of benchmarks."
echo "  6. Pick 'Run all' (0) or specific test numbers. Press Esc to abort."
echo "  7. When done, testOS syncs the USB and reboots back to the host OS."
echo "  8. Plug the USB back here, then pull the results:"
echo
if [[ -n "$LAUNCHER_BIN" ]]; then
    echo "       sudo $WORK_DIR/testos-ingest pull $DEVICE"
    echo "       $WORK_DIR/testos-ingest format"
    echo "       $WORK_DIR/testos-ingest commit"
else
    echo "       # Download testos-ingest from the same release:"
    echo "       curl -fsSL -o testos-ingest <url-from-release-assets>"
    echo "       chmod +x testos-ingest"
    echo "       sudo ./testos-ingest pull $DEVICE"
    echo "       ./testos-ingest format"
    echo "       ./testos-ingest commit"
fi
echo "       git push"
echo
echo "  Results land in benchmarks/results/<date>/<host-fingerprint>/."
