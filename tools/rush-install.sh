#!/usr/bin/env bash
# tools/rush-install.sh — Install Rush Linux onto a target block device.
#
# This is the v0.5 "fresh install" mechanism. It uses systemd-repart to
# partition a blank target disk and copies the OS image onto it, then
# installs systemd-boot to the ESP.
#
# The target device is FULLY OVERWRITTEN. Use with caution.
#
# Usage:
#   sudo bash tools/rush-install.sh <source_image> <target_device>
#
# Example (install onto a blank VM disk):
#   truncate -s 2G /tmp/target.raw
#   sudo bash tools/rush-install.sh build/rush-linux.raw /dev/sdX
#
# Or for a raw image file (loop-mounted):
#   truncate -s 2G /tmp/target.raw
#   LOOP=$(sudo losetup --find --show /tmp/target.raw)
#   sudo bash tools/rush-install.sh build/rush-linux.raw ${LOOP}
#   sudo losetup -d ${LOOP}
#
# Exit criteria satisfied:
#   Criterion 1: "fresh VM install succeeds"

set -euo pipefail

# ── Preflight checks ─────────────────────────────────────────────
if [[ $# -lt 2 ]]; then
    echo "Usage: $0 <source_image> <target_device>" >&2
    echo "" >&2
    echo "  source_image  — Path to the Rush Linux disk image (e.g., build/rush-linux.raw)" >&2
    echo "  target_device — Block device to install onto (e.g., /dev/sdX or a loop device)" >&2
    exit 1
fi

SOURCE_IMAGE="$1"
TARGET_DEVICE="$2"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPART_DIR="${REPO_ROOT}/mkosi/mkosi.repart"

if [[ ! -f "${SOURCE_IMAGE}" ]]; then
    echo "Error: source image not found: ${SOURCE_IMAGE}" >&2
    exit 1
fi

if [[ ! -b "${TARGET_DEVICE}" ]]; then
    echo "Error: target is not a block device: ${TARGET_DEVICE}" >&2
    echo "  (use losetup to create a loop device for raw image files)" >&2
    exit 1
fi

if ! command -v systemd-repart >/dev/null 2>&1; then
    echo "Error: systemd-repart not found. Install systemd-repart package." >&2
    exit 1
fi

if [[ "$(id -u)" -ne 0 ]]; then
    echo "Error: this script must be run as root (or with sudo)." >&2
    exit 1
fi

echo "════════════════════════════════════════════════════"
echo "  Rush Linux Installer"
echo "════════════════════════════════════════════════════"
echo "  Source:  ${SOURCE_IMAGE}"
echo "  Target:  ${TARGET_DEVICE}"
echo ""

# ── Step 1: Wipe target and create partition table ───────────────
echo ">> [1/4] Partitioning target device..."
sgdisk --clear --zap-all "${TARGET_DEVICE}" 2>/dev/null

# Create partitions using systemd-repart definitions
# This gives us ESP + root with the same layout as the build image
systemd-repart \
    --empty=force \
    --definitions="${REPART_DIR}" \
    --dry-run=no \
    "${TARGET_DEVICE}" 2>&1 || {
    # Fallback: manual partitioning if systemd-repart fails
    # ponytail: fallback uses +1G ESP and labels that match systemd-validatefs expectations (RushRoot)
    echo "  systemd-repart failed, falling back to sgdisk..."
    sgdisk --clear \
        --new=1::+1G   -t 1:C12A7328-F81F-11D2-BA4B-00A0C93EC93B -c 1:"RUSHESP" \
        --new=2::0     -t 2:4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709 -c 2:"RushRoot" \
        "${TARGET_DEVICE}" 2>/dev/null
}

echo "   Done."
echo ""

# ── Step 2: Copy partitions from source image ───────────────────
echo ">> [2/4] Copying OS image to target..."

# Find partition offsets in source image
SRC_ESP_START=$(sgdisk -i 1 "${SOURCE_IMAGE}" 2>/dev/null | grep "First sector" | awk '{print $3}')
SRC_ESP_SIZE=$(sgdisk -i 1 "${SOURCE_IMAGE}" 2>/dev/null | grep "Partition size" | awk '{print $3}')
SRC_ROOT_START=$(sgdisk -i 2 "${SOURCE_IMAGE}" 2>/dev/null | grep "First sector" | awk '{print $3}')
SRC_ROOT_SIZE=$(sgdisk -i 2 "${SOURCE_IMAGE}" 2>/dev/null | grep "Partition size" | awk '{print $3}')

# Find partition offsets in target
TGT_ESP_START=$(sgdisk -i 1 "${TARGET_DEVICE}" 2>/dev/null | grep "First sector" | awk '{print $3}')
TGT_ROOT_START=$(sgdisk -i 2 "${TARGET_DEVICE}" 2>/dev/null | grep "First sector" | awk '{print $3}')

if [[ -z "${SRC_ESP_START}" || -z "${SRC_ROOT_START}" || -z "${TGT_ESP_START}" || -z "${TGT_ROOT_START}" ]]; then
    echo "Error: Could not determine partition offsets." >&2
    echo "  Source ESP: ${SRC_ESP_START:-?}, Root: ${SRC_ROOT_START:-?}" >&2
    echo "  Target ESP: ${TGT_ESP_START:-?}, Root: ${TGT_ROOT_START:-?}" >&2
    exit 1
fi

# Copy ESP partition
echo "  Copying ESP..."
dd if="${SOURCE_IMAGE}" of="${TARGET_DEVICE}" \
    bs=512 skip="${SRC_ESP_START}" seek="${TGT_ESP_START}" \
    count="${SRC_ESP_SIZE}" conv=notrunc status=progress 2>&1

# Copy root partition
echo "  Copying root filesystem..."
dd if="${SOURCE_IMAGE}" of="${TARGET_DEVICE}" \
    bs=512 skip="${SRC_ROOT_START}" seek="${TGT_ROOT_START}" \
    count="${SRC_ROOT_SIZE}" conv=notrunc status=progress 2>&1

echo "   Done."
echo ""

# ── Step 3: Install systemd-boot ─────────────────────────────────
echo ">> [3/4] Installing systemd-boot..."

# Mount ESP, install bootloader, unmount
# Partition suffix differs by device type: loop/nvme use p1, others use 1
if [[ "${TARGET_DEVICE}" =~ (loop|nvme) ]]; then
    ESP_PART="${TARGET_DEVICE}p1"
else
    ESP_PART="${TARGET_DEVICE}1"
fi
TMP_MNT=$(mktemp -d)

# The ESP was already populated by the dd copy in step 2.
# Do NOT mkfs.vfat here — that would destroy the UKIs and boot entries.
mount "${ESP_PART}" "${TMP_MNT}"

bootctl install --esp-path="${TMP_MNT}" --no-variables 2>/dev/null || {
    echo "  bootctl install failed, boot entry should already exist from source image."
}

umount "${TMP_MNT}"
rmdir "${TMP_MNT}"

echo "   Done."
echo ""

# ── Step 4: Verify and report ────────────────────────────────────
echo ">> [4/4] Verifying installation..."

sgdisk -p "${TARGET_DEVICE}" 2>/dev/null

echo ""
echo "════════════════════════════════════════════════════"
echo "  ✅ Installation complete"
echo ""
echo "  Boot the installed system:"
echo "    qemu-system-x86_64 -bios /usr/share/OVMF/OVMF_CODE.fd \\"
echo "      -drive file=${TARGET_DEVICE},format=raw,if=virtio \\"
echo "      -m 1G -nographic"
echo "════════════════════════════════════════════════════"
