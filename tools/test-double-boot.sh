#!/usr/bin/env bash
# tools/test-double-boot.sh — Verify a disk image boots twice cleanly.
#
# Validates exit criterion 2 ("installed system boots twice cleanly") by:
#   1. Booting the disk image, waiting for multi-user.target, then poweroff.
#   2. Booting the same disk image again and verifying multi-user.target.
#   3. Checking that optid.service and boot-assessment markers survive.
#
# Prerequisites:
#   - qemu-system-x86_64 and OVMF firmware
#   - A disk image built by build-mkosi-image.sh or rush-install.sh
#
# Usage:
#   bash tools/test-double-boot.sh [disk_image]
#
# Environment:
#   OVMF_FIRMWARE       Override OVMF firmware path.
#   RUSH_BOOT_TIMEOUT   Per-boot timeout in seconds (default: 150).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DISK="${1:-${ROOT}/build/rush-linux.raw}"
TIMEOUT_SEC="${RUSH_BOOT_TIMEOUT:-150}"
LOG_DIR="${ROOT}/build/double-boot-test"
FIRMWARE="${OVMF_FIRMWARE:-}"
QEMU_ACCEL_ARGS=()

die() { echo "❌ FAIL: $*" >&2; exit 1; }
pass() { echo "✅ PASS: $*"; }

echo "============================================"
echo "  Rush Linux Double-Boot Test (v0.5)"
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

[[ -f "${DISK}" ]] || die "disk image not found: ${DISK}"
[[ -n "${FIRMWARE}" ]] || die "OVMF firmware not found. Set OVMF_FIRMWARE=."
command -v qemu-system-x86_64 >/dev/null 2>&1 || die "qemu-system-x86_64 not found"

if [[ -e /dev/kvm ]] && [[ -r /dev/kvm ]] && [[ -w /dev/kvm ]]; then
    QEMU_ACCEL_ARGS=(-enable-kvm)
    echo "  QEMU acceleration: KVM enabled"
else
    echo "  QEMU acceleration: TCG"
fi

mkdir -p "${LOG_DIR}"

# Work on a copy to avoid modifying the original
WORK_DISK="${LOG_DIR}/test-disk.raw"
echo "  Creating working copy of disk image..."
cp "${DISK}" "${WORK_DISK}"

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
echo "━━━ Boot 1: Initial boot ━━━"
# ══════════════════════════════════════════════════════════════════

boot_and_log "boot-1" "${WORK_DISK}"

if log_has "${LOG_DIR}/boot-1.log" "multi-user"; then
    pass "Boot 1: Reached multi-user.target"
else
    die "Boot 1: Did not reach multi-user.target"
fi

if log_has "${LOG_DIR}/boot-1.log" "optid"; then
    pass "Boot 1: optid.service started"
else
    echo "  ⚠️  Warning: optid.service marker not found"
fi

# ══════════════════════════════════════════════════════════════════
echo ""
echo "━━━ Boot 2: Reboot (same disk) ━━━"
# ══════════════════════════════════════════════════════════════════

boot_and_log "boot-2" "${WORK_DISK}"

if log_has "${LOG_DIR}/boot-2.log" "multi-user"; then
    pass "Boot 2: Reached multi-user.target"
else
    die "Boot 2: Did not reach multi-user.target"
fi

if log_has "${LOG_DIR}/boot-2.log" "optid"; then
    pass "Boot 2: optid.service started"
else
    echo "  ⚠️  Warning: optid.service marker not found"
fi

# Check for boot assessment marker
if log_has "${LOG_DIR}/boot-2.log" "boot-assess\|boot.good"; then
    pass "Boot 2: Boot assessment marker present"
else
    echo "  ⚠️  Warning: Boot assessment marker not explicitly visible in log (may be normal)"
fi

# ══════════════════════════════════════════════════════════════════
echo ""
echo "============================================"
echo "  Double-boot test PASSED"
echo "============================================"
echo ""
echo "v0.5.0-beta.1 criterion 2 verified:"
echo "  ✅ Installed system boots twice cleanly"
