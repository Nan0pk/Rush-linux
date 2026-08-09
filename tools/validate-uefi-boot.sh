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

# The boot proof must not modify the image under test. QEMU snapshot mode keeps
# guest writes in a temporary overlay, so an abrupt timeout cannot dirty the ESP
# and no post-boot partition surgery is required.
set +e
timeout "${TIMEOUT_SEC}s" \
    stdbuf -oL -eL \
    qemu-system-x86_64 \
        "${QEMU_ACCEL_ARGS[@]}" \
        -bios "${FIRMWARE}" \
        -drive "file=${DISK},format=raw,if=virtio,snapshot=on" \
        -m 1G \
        -nographic \
        -no-reboot \
    </dev/null 2>&1 | tee "${LOG}"
QEMU_STATUS=${PIPESTATUS[0]}
set -e

if [ "${QEMU_STATUS}" -ne 0 ] && [ "${QEMU_STATUS}" -ne 124 ]; then
    echo "::error title=UEFI boot QEMU failed::QEMU exited unexpectedly with status ${QEMU_STATUS}"
    echo "Error: QEMU exited unexpectedly with status ${QEMU_STATUS}" >&2
    exit "${QEMU_STATUS}"
fi

PROOF_FAILURES=0
require_log() {
    local pattern="$1"
    local description="$2"
    if grep -aEq "${pattern}" "${LOG}"; then
        echo "  ✅ ${description}"
    else
        echo "::error title=UEFI boot proof missing::${description}"
        echo "  ❌ Missing: ${description}" >&2
        PROOF_FAILURES=$((PROOF_FAILURES + 1))
    fi
}

# Do not depend on firmware debug strings such as "BdsDxe: starting"; those are
# OVMF-build-specific. Reaching the systemd-boot entry through the OVMF firmware
# supplied above is the stable UEFI proof boundary.
require_log "Rush Linux" "systemd-boot displayed the Rush Linux entry"
require_log "Booting initrd|EFI stub: Loaded initrd" "UKI loaded its embedded initrd"
require_log "[Cc]ommand [Ll]ine|Generate Network Units from Kernel Command Line" "UKI command line selected the VM root partition"
require_log "Reached target .*multi-user\.target|Reached target .*Multi-User System" "systemd reached multi-user.target"
require_log "Started .*optid\.service|Started .*Rush Linux optimization daemon" "optid.service started"

if (( PROOF_FAILURES != 0 )); then
    echo "Error: ${PROOF_FAILURES} UEFI boot proof(s) missing; see ${LOG}" >&2
    exit 1
fi

echo "✅ UEFI UKI boot validation passed"
