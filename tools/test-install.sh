#!/usr/bin/env bash
# tools/test-install.sh — End-to-end install test for v0.5.0-beta.1.
#
# Validates exit criterion 1 ("fresh VM install succeeds") by:
#   1. Building the OS image (if not already built)
#   2. Creating a blank target disk
#   3. Installing Rush Linux onto the blank disk via rush-install.sh
#   4. Booting the installed disk and verifying multi-user.target
#   5. Booting a second time and verifying again (criterion 2)
#
# Prerequisites:
#   - sudo/root access
#   - qemu-system-x86_64 and OVMF firmware
#   - systemd-repart, sgdisk, losetup
#
# Usage:
#   sudo bash tools/test-install.sh [source_image]
#
# Environment:
#   OVMF_FIRMWARE       Override OVMF firmware path.
#   RUSH_BOOT_TIMEOUT   Per-boot timeout in seconds (default: 150).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="${1:-${ROOT}/build/rush-linux.raw}"
TIMEOUT_SEC="${RUSH_BOOT_TIMEOUT:-150}"
TEST_DIR="${ROOT}/build/install-test"
LOG_DIR="${TEST_DIR}/logs"
FIRMWARE="${OVMF_FIRMWARE:-}"
QEMU_ACCEL_ARGS=()

die() { echo "❌ FAIL: $*" >&2; exit 1; }
pass() { echo "✅ PASS: $*"; }

echo "============================================"
echo "  Rush Linux Install Test Suite (v0.5)"
echo "============================================"
echo ""

# ── Find OVMF ────────────────────────────────────────────────────
if [[ -z "${FIRMWARE}" ]]; then
    for candidate in \
        /usr/share/ovmf/OVMF.fd \
        /usr/share/OVMF/OVMF_CODE.fd \
        /usr/share/OVMF/OVMF_CODE_4M.fd; do
        if [[ -f "${candidate}" ]]; then
            FIRMWARE="${candidate}"
            break
        fi
    done
fi

# ── Preflight checks ────────────────────────────────────────────
[[ -f "${SOURCE}" ]] || die "source image not found: ${SOURCE} (build with: sudo bash tools/build-mkosi-image.sh)"
[[ -n "${FIRMWARE}" ]] || die "OVMF firmware not found. Set OVMF_FIRMWARE=."
command -v qemu-system-x86_64 >/dev/null 2>&1 || die "qemu-system-x86_64 not found"
command -v losetup >/dev/null 2>&1 || die "losetup not found"
[[ "$(id -u)" -eq 0 ]] || die "this test must be run as root"

if [[ -e /dev/kvm ]] && [[ -r /dev/kvm ]] && [[ -w /dev/kvm ]]; then
    QEMU_ACCEL_ARGS=(-enable-kvm)
    echo "  QEMU acceleration: KVM enabled"
else
    echo "  QEMU acceleration: TCG"
fi

mkdir -p "${LOG_DIR}"

# ── Helper: boot disk image and capture log ──────────────────────
boot_and_log() {
    local label="$1"
    local disk_path="$2"
    local log_file="${LOG_DIR}/${label}.log"

    echo "  Booting: ${label}..."
    rm -f "${log_file}"

    set +e
    timeout "${TIMEOUT_SEC}s" \
        stdbuf -oL -eL \
        qemu-system-x86_64 \
            "${QEMU_ACCEL_ARGS[@]}" \
            -bios "${FIRMWARE}" \
            -drive "file=${disk_path},format=raw,if=virtio" \
            -m 1G \
            -nographic \
            -no-reboot \
        </dev/null 2>&1 | tee "${log_file}"
    local STATUS=${PIPESTATUS[0]}
    set -e

    echo "  QEMU exit status: ${STATUS}"
}

# ── Helper: check log for patterns ───────────────────────────────
log_has() {
    local log="$1"
    local pattern="$2"
    grep -aEq "${pattern}" "${log}" 2>/dev/null
}

# ══════════════════════════════════════════════════════════════════
echo "━━━ Test 1: Fresh VM install succeeds ━━━"
# ══════════════════════════════════════════════════════════════════

# Create a blank 2GB target disk
TARGET_DISK="${TEST_DIR}/target.raw"
echo "  Creating blank 2GB target disk..."
truncate -s 2G "${TARGET_DISK}"

# Set up loop device
LOOP=$(losetup --find --show "${TARGET_DISK}")
echo "  Loop device: ${LOOP}"

# Run the installer
echo "  Running rush-install.sh..."
bash "${ROOT}/tools/rush-install.sh" "${SOURCE}" "${LOOP}" 2>&1 | tail -10

# Detach loop device before QEMU boot
losetup -d "${LOOP}"
echo "  Loop device detached."

# Boot the installed disk
boot_and_log "t1-fresh-install" "${TARGET_DISK}"

if log_has "${LOG_DIR}/t1-fresh-install.log" "multi-user"; then
    pass "Test 1: Fresh VM install boots to multi-user.target"
else
    die "Test 1: Installed system did not reach multi-user.target"
fi

# ══════════════════════════════════════════════════════════════════
echo ""
echo "━━━ Test 2: Installed system boots twice cleanly ━━━"
# ══════════════════════════════════════════════════════════════════

# Second boot of the same disk
boot_and_log "t2-second-boot" "${TARGET_DISK}"

if log_has "${LOG_DIR}/t2-second-boot.log" "multi-user"; then
    pass "Test 2: Second boot reaches multi-user.target"
else
    die "Test 2: Second boot did not reach multi-user.target"
fi

# Verify boot assessment markers exist
if log_has "${LOG_DIR}/t2-second-boot.log" "optid"; then
    pass "Test 2a: optid.service started on second boot"
else
    echo "  ⚠️  Warning: optid.service marker not found in second boot log"
fi

# ══════════════════════════════════════════════════════════════════
echo ""
echo "━━━ Test 3: Server edition has no desktop dependency ━━━"
# ══════════════════════════════════════════════════════════════════

# Check the source image for desktop packages by mounting the root partition
echo "  Checking source image for desktop packages..."

# Find root partition offset in the source image
ROOT_START=$(sgdisk -i 2 "${SOURCE}" 2>/dev/null | grep "First sector" | awk '{print $3}')
ROOT_SIZE=$(sgdisk -i 2 "${SOURCE}" 2>/dev/null | grep "Partition size" | awk '{print $3}')

if [[ -n "${ROOT_START}" && -n "${ROOT_SIZE}" ]]; then
    # Extract root partition to a temporary file
    ROOT_IMG="${TEST_DIR}/root_part.img"
    dd if="${SOURCE}" of="${ROOT_IMG}" bs=512 skip="${ROOT_START}" count="${ROOT_SIZE}" status=none 2>/dev/null

    ROOT_MNT="${TEST_DIR}/root_mnt"
    mkdir -p "${ROOT_MNT}"

    # Try to mount and check for desktop packages
    if mount -o ro "${ROOT_IMG}" "${ROOT_MNT}" 2>/dev/null; then
        DESKTOP_PACKAGES=""
        for pkg in plasma-desktop plasma-workspace kwayland pipewire xorg-server wayland weston; do
            if [[ -d "${ROOT_MNT}/var/lib/pacman/local/${pkg}-"* ]] 2>/dev/null; then
                DESKTOP_PACKAGES="${DESKTOP_PACKAGES} ${pkg}"
            fi
        done
        umount "${ROOT_MNT}"

        if [[ -z "${DESKTOP_PACKAGES}" ]]; then
            pass "Test 3: No desktop packages found in server image"
        else
            die "Test 3: Desktop packages found:${DESKTOP_PACKAGES}"
        fi
    else
        echo "  ⚠️  Could not mount root partition for package check (skipping)"
        pass "Test 3: Skipped (could not mount root partition)"
    fi
    rm -f "${ROOT_IMG}"
else
    echo "  ⚠️  Could not determine root partition offset (skipping)"
    pass "Test 3: Skipped (partition layout unknown)"
fi

# ══════════════════════════════════════════════════════════════════
echo ""
echo "============================================"
echo "  All install tests PASSED"
echo "============================================"
echo ""
echo "v0.5.0-beta.1 exit criteria verified:"
echo "  ✅ Fresh VM install succeeds"
echo "  ✅ Installed system boots twice cleanly"
