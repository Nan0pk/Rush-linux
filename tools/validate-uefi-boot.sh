#!/usr/bin/env bash
# Validate that build/disk.raw boots through UEFI/systemd-boot/UKI to
# multi-user.target with optid.service active.
#
# Usage:
#   tools/validate-uefi-boot.sh [build/disk.raw]
#
# Environment:
#   OVMF_FIRMWARE       Override OVMF code/combined firmware path.
#   OVMF_VARS           Override OVMF variable-store template path.
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
VARS_TEMPLATE="${OVMF_VARS:-}"
if [ -z "${FIRMWARE}" ]; then
    for pair in \
        "/usr/share/OVMF/OVMF_CODE_4M.fd:/usr/share/OVMF/OVMF_VARS_4M.fd" \
        "/usr/share/OVMF/OVMF_CODE.fd:/usr/share/OVMF/OVMF_VARS.fd"; do
        code="${pair%%:*}"
        vars="${pair#*:}"
        if [ -f "${code}" ] && [ -f "${vars}" ]; then
            FIRMWARE="${code}"
            VARS_TEMPLATE="${vars}"
            break
        fi
    done
fi

# Retain support for explicit/older monolithic OVMF images, but do not select
# distro-provided convenience OVMF.fd images ahead of a CODE/VARS pair.
if [ -z "${FIRMWARE}" ]; then
    for candidate in \
        /usr/share/ovmf/OVMF.fd \
        /usr/share/qemu/OVMF.fd; do
        if [ -f "${candidate}" ]; then
            FIRMWARE="${candidate}"
            break
        fi
    done
fi

if [ -z "${FIRMWARE}" ] || [ ! -f "${FIRMWARE}" ]; then
    echo "Error: OVMF firmware not found. Set OVMF_FIRMWARE=/path/to/OVMF_CODE.fd" >&2
    exit 1
fi
if [ -n "${VARS_TEMPLATE}" ] && [ ! -f "${VARS_TEMPLATE}" ]; then
    echo "Error: OVMF variable-store template not found: ${VARS_TEMPLATE}" >&2
    exit 1
fi

mkdir -p "$(dirname "${LOG}")"
rm -f "${LOG}"

echo "Validating UEFI UKI boot"
echo "  disk:     ${DISK}"
echo "  firmware: ${FIRMWARE}"
echo "  vars:     ${VARS_TEMPLATE:-<combined firmware>}"
echo "  timeout:  ${TIMEOUT_SEC}s"
echo "  log:      ${LOG}"
echo "::notice title=UEFI boot firmware::firmware=${FIRMWARE} vars=${VARS_TEMPLATE:-combined} disk_bytes=$(stat -Lc %s "${DISK}")"

QEMU_ACCEL_ARGS=()
if [ -e /dev/kvm ] && [ -r /dev/kvm ] && [ -w /dev/kvm ]; then
    QEMU_ACCEL_ARGS=(-enable-kvm)
    echo "  accel:    KVM enabled"
else
    echo "  accel:    TCG (set permissions on /dev/kvm to enable KVM)"
fi

QEMU_FIRMWARE_ARGS=()
VARS_RUNTIME=""
cleanup() {
    if [ -n "${VARS_RUNTIME}" ]; then
        rm -f "${VARS_RUNTIME}"
    fi
}
trap cleanup EXIT

if [ -n "${VARS_TEMPLATE}" ]; then
    VARS_RUNTIME="$(mktemp "${TMPDIR:-/tmp}/ovmf-vars.XXXXXX.fd")"
    cp "${VARS_TEMPLATE}" "${VARS_RUNTIME}"
    QEMU_FIRMWARE_ARGS=(
        -drive "if=pflash,format=raw,unit=0,file=${FIRMWARE},readonly=on"
        -drive "if=pflash,format=raw,unit=1,file=${VARS_RUNTIME}"
    )
else
    QEMU_FIRMWARE_ARGS=(-bios "${FIRMWARE}")
fi

# The boot proof must not modify the image under test. QEMU snapshot mode keeps
# guest disk writes in a temporary overlay, while the UEFI variable store is a
# disposable copy. Keep the serial port separate from QEMU's monitor so the log
# contains only firmware/guest serial evidence.
set +e
timeout "${TIMEOUT_SEC}s" \
    stdbuf -oL -eL \
    qemu-system-x86_64 \
        -machine q35 \
        "${QEMU_ACCEL_ARGS[@]}" \
        "${QEMU_FIRMWARE_ARGS[@]}" \
        -drive "file=${DISK},format=raw,if=virtio,snapshot=on" \
        -m 1G \
        -display none \
        -serial stdio \
        -monitor none \
        -net none \
        -no-reboot \
    </dev/null 2>&1 | tee "${LOG}"
QEMU_STATUS=${PIPESTATUS[0]}

# Capture diagnostics while errexit is disabled so a diagnostic failure can
# never hide the original boot failure. Base64 keeps GitHub workflow-command
# framing safe even when firmware emits control characters or punctuation.
LOG_BYTES="$(stat -Lc %s "${LOG}" 2>/dev/null)"
[ -n "${LOG_BYTES}" ] || LOG_BYTES=0
LOG_TAIL_B64="$(tail -c 1800 "${LOG}" 2>/dev/null | base64 | tr -d '\r\n')"
[ -n "${LOG_TAIL_B64}" ] || LOG_TAIL_B64="PGVtcHR5Pg=="
set -e

echo "::notice title=UEFI QEMU result::qemu_status=${QEMU_STATUS} log_bytes=${LOG_BYTES}"

if [ "${QEMU_STATUS}" -ne 0 ] && [ "${QEMU_STATUS}" -ne 124 ]; then
    echo "::error title=UEFI boot QEMU failed::status=${QEMU_STATUS} log_bytes=${LOG_BYTES} tail_b64=${LOG_TAIL_B64}"
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

# Reaching the systemd-boot entry through the OVMF firmware supplied above is
# the stable UEFI proof boundary; do not depend on OVMF-build-specific debug
# strings.
require_log "Rush Linux" "systemd-boot displayed the Rush Linux entry"
require_log "Booting initrd|EFI stub: Loaded initrd" "UKI loaded its embedded initrd"
require_log "[Cc]ommand [Ll]ine|Generate Network Units from Kernel Command Line" "UKI command line selected the VM root partition"
require_log "Reached target .*multi-user\.target|Reached target .*Multi-User System" "systemd reached multi-user.target"
require_log "Started .*optid\.service|Started .*Rush Linux optimization daemon" "optid.service started"

if (( PROOF_FAILURES != 0 )); then
    echo "::error title=UEFI boot serial evidence::log_bytes=${LOG_BYTES} qemu_status=${QEMU_STATUS} tail_b64=${LOG_TAIL_B64}"
    echo "Error: ${PROOF_FAILURES} UEFI boot proof(s) missing; see ${LOG}" >&2
    exit 1
fi

echo "✅ UEFI UKI boot validation passed"
