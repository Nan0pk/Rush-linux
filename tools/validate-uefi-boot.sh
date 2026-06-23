#!/usr/bin/env bash
# Validate that build/disk.raw boots through UEFI/systemd-boot/UKI to
# multi-user.target with optid.service active.
#
# Usage:
#   tools/validate-uefi-boot.sh [build/disk.raw]
#
# Environment:
#   OVMF_FIRMWARE       Override firmware path.
#   RUSH_BOOT_TIMEOUT   QEMU runtime timeout in seconds (default: 150).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DISK="${1:-${ROOT}/build/disk.raw}"
TIMEOUT_SEC="${RUSH_BOOT_TIMEOUT:-150}"
LOG="${ROOT}/build/uefi-boot.log"

if [ ! -f "${DISK}" ]; then
    echo "Error: disk image not found: ${DISK}" >&2
    echo "Build it first with: sudo bash tools/build-vm-final.sh" >&2
    exit 1
fi

if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
    echo "Error: qemu-system-x86_64 not found" >&2
    exit 1
fi

FIRMWARE="${OVMF_FIRMWARE:-}"
if [ -z "${FIRMWARE}" ]; then
    for candidate in \
        /usr/share/ovmf/OVMF.fd \
        /usr/share/OVMF/OVMF_CODE.fd \
        /usr/share/OVMF/OVMF_CODE_4M.fd; do
        if [ -f "${candidate}" ]; then
            FIRMWARE="${candidate}"
            break
        fi
    done
fi

if [ -z "${FIRMWARE}" ] || [ ! -f "${FIRMWARE}" ]; then
    echo "Error: OVMF firmware not found. Set OVMF_FIRMWARE=/path/to/OVMF.fd" >&2
    exit 1
fi

mkdir -p "$(dirname "${LOG}")"
rm -f "${LOG}"

echo "Validating UEFI UKI boot"
echo "  disk:     ${DISK}"
echo "  firmware: ${FIRMWARE}"
echo "  timeout:  ${TIMEOUT_SEC}s"
echo "  log:      ${LOG}"

QEMU_ACCEL_ARGS=()
if [ -e /dev/kvm ] && [ -r /dev/kvm ] && [ -w /dev/kvm ]; then
    QEMU_ACCEL_ARGS=(-enable-kvm)
    echo "  accel:    KVM enabled"
else
    echo "  accel:    TCG (set permissions on /dev/kvm to enable KVM)"
fi

set +e
timeout "${TIMEOUT_SEC}s" \
    stdbuf -oL -eL \
    qemu-system-x86_64 \
        "${QEMU_ACCEL_ARGS[@]}" \
        -bios "${FIRMWARE}" \
        -drive "file=${DISK},format=raw,if=virtio" \
        -m 1G \
        -nographic \
        -no-reboot \
    </dev/null 2>&1 | tee "${LOG}"
QEMU_STATUS=${PIPESTATUS[0]}
set -e

if [ "${QEMU_STATUS}" -ne 0 ] && [ "${QEMU_STATUS}" -ne 124 ]; then
    echo "Error: QEMU exited unexpectedly with status ${QEMU_STATUS}" >&2
    exit "${QEMU_STATUS}"
fi

require_log() {
    local pattern="$1"
    local description="$2"
    if grep -aEq "${pattern}" "${LOG}"; then
        echo "  ✅ ${description}"
    else
        echo "  ❌ Missing: ${description}" >&2
        exit 1
    fi
}

require_log "BdsDxe: starting" "OVMF started the fallback UEFI boot path"
require_log "Rush Linux" "systemd-boot displayed the Rush Linux entry"
require_log "Booting initrd|EFI stub: Loaded initrd" "UKI loaded its embedded initrd"
require_log "[Cc]ommand [Ll]ine|Generate Network Units from Kernel Command Line" "UKI command line selected the VM root partition"
require_log "Reached target .*multi-user\.target|Reached target .*Multi-User System" "systemd reached multi-user.target"
require_log "Started .*optid\.service|Started .*Rush Linux optimization daemon" "optid.service started"

echo "✅ UEFI UKI boot validation passed"
