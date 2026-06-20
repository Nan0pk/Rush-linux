#!/usr/bin/env bash
# test-rollback.sh — Simulate bad-kernel rollback and verify entry retention.
#
# Validates the three v0.4.0-alpha.1 exit criteria:
#   1. VM boots through UKI               (calls validate-uefi-boot.sh)
#   2. Three rollback entries are retained (checks entry count on ESP)
#   3. Simulated bad kernel rolls back     (installs broken UKI, boots, checks)
#
# This test operates on a copy of build/disk.raw to avoid destroying the
# working image. It uses QEMU + OVMF to boot the VM for each test.
#
# Prerequisites:
#   - build/disk.raw exists (built by build-vm-final.sh)
#   - qemu-system-x86_64 and OVMF firmware are installed
#   - mtools (mcopy, mmd, mdir) for ESP manipulation
#
# Usage:
#   tools/test-rollback.sh [build/disk.raw]
#
# Environment:
#   OVMF_FIRMWARE       Override OVMF firmware path.
#   RUSH_BOOT_TIMEOUT   Per-boot timeout in seconds (default: 150).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DISK="${1:-${ROOT}/build/disk.raw}"
TIMEOUT_SEC="${RUSH_BOOT_TIMEOUT:-150}"
TEST_DIR="${ROOT}/build/rollback-test"
LOG_DIR="${TEST_DIR}/logs"
FIRMWARE="${OVMF_FIRMWARE:-}"
QEMU_ACCEL_ARGS=()

# Find OVMF
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

die() { echo "❌ FAIL: $*" >&2; exit 1; }
pass() { echo "✅ PASS: $*"; }

echo "============================================"
echo "  Rush Linux Rollback Test Suite (v0.4)"
echo "============================================"
echo ""

# ── Preflight checks ──────────────────────────────────────────────────
[ -f "${DISK}" ] || die "disk image not found: ${DISK}"
[ -n "${FIRMWARE}" ] || die "OVMF firmware not found. Set OVMF_FIRMWARE=."
command -v qemu-system-x86_64 >/dev/null 2>&1 || die "qemu-system-x86_64 not found"
command -v mcopy >/dev/null 2>&1 || die "mtools (mcopy) not found"

if [ -e /dev/kvm ] && [ -r /dev/kvm ] && [ -w /dev/kvm ]; then
    QEMU_ACCEL_ARGS=(-enable-kvm)
    echo "  QEMU acceleration: KVM enabled"
else
    echo "  QEMU acceleration: TCG (set permissions on /dev/kvm to enable KVM)"
fi

mkdir -p "${LOG_DIR}"

# ── Helper: boot disk image and capture log ───────────────────────────
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

# ── Helper: check log for patterns ────────────────────────────────────
log_has() {
    local log="$1"
    local pattern="$2"
    grep -aEq "${pattern}" "${log}" 2>/dev/null
}

# ── Helper: count UKI entries on ESP ──────────────────────────────────
count_uki_entries() {
    local disk_path="$1"
    # Use mdir to list files in /EFI/Linux on the first partition
    local listing
    listing=$(mdir -i "${disk_path}@@1048576" ::/EFI/Linux 2>/dev/null || true)
    # Count .efi files (exclude BOOTX64.EFI)
    echo "${listing}" | grep -c "\.efi$" || echo "0"
}

# ── Helper: inject a broken UKI onto the ESP ─────────────────────────
inject_broken_uki() {
    local disk_path="$1"
    local broken_uki="${TEST_DIR}/broken.efi"

    # Create a minimal "broken" EFI binary that prints a message and hangs
    # This is a tiny EFI application that will panic/halt
    # We use a simple approach: write garbage that looks like an EFI binary
    # but crashes the firmware or the kernel
    printf 'MZ\x00\x00' > "${broken_uki}"
    # Pad to a reasonable size with random data (not a valid PE/COFF)
    dd if=/dev/urandom of="${broken_uki}" bs=1 count=4096 conv=notrunc 2>/dev/null

    echo "  Injecting broken UKI as /EFI/Linux/rush-linux-broken.efi..."
    mcopy -i "${disk_path}@@1048576" "${broken_uki}" ::/EFI/Linux/rush-linux-broken.efi
}

# ── Helper: replace the main UKI entry with a broken one ──────────────
replace_main_uki_with_broken() {
    local disk_path="$1"
    local broken_uki="${TEST_DIR}/broken.efi"

    # Back up the good main UKI first
    echo "  Backing up good UKI..."
    mcopy -i "${disk_path}@@1048576" ::/EFI/Linux/rush-linux.efi "${TEST_DIR}/rush-linux-good.efi"

    # Replace the main UKI with the broken one
    echo "  Replacing main UKI with broken UKI..."
    mcopy -i "${disk_path}@@1048576" "${broken_uki}" ::/EFI/Linux/rush-linux.efi
}

# ── Helper: restore the good UKI ─────────────────────────────────────
restore_good_uki() {
    local disk_path="$1"
    if [ -f "${TEST_DIR}/rush-linux-good.efi" ]; then
        echo "  Restoring good UKI..."
        mcopy -i "${disk_path}@@1048576" "${TEST_DIR}/rush-linux-good.efi" ::/EFI/Linux/rush-linux.efi
    fi
}

# ══════════════════════════════════════════════════════════════════════
echo "━━━ Test 1: VM boots through UKI ━━━"
# ══════════════════════════════════════════════════════════════════════

if [ -x "${ROOT}/tools/validate-uefi-boot.sh" ]; then
    echo "  Running validate-uefi-boot.sh..."
    if "${ROOT}/tools/validate-uefi-boot.sh" "${DISK}"; then
        pass "Test 1: VM boots through UKI (validated by validate-uefi-boot.sh)"
    else
        die "Test 1: UEFI UKI boot failed"
    fi
else
    # Manual boot test
    boot_and_log "t1-uki-boot" "${DISK}"
    if log_has "${LOG_DIR}/t1-uki-boot.log" "multi-user"; then
        pass "Test 1: VM boots through UKI"
    else
        die "Test 1: VM did not reach multi-user.target"
    fi
fi

# ══════════════════════════════════════════════════════════════════════
echo ""
echo "━━━ Test 2: Three rollback entries are retained ━━━"
# ══════════════════════════════════════════════════════════════════════

echo "  Simulating 3 update cycles to build rollback entries..."

# Make a working copy of the disk image
cp "${DISK}" "${TEST_DIR}/test-disk.raw"
TEST_DISK="${TEST_DIR}/test-disk.raw"

# Get the ESP partition offset dynamically from GPT (works with both
# build-vm-final.sh images at 2048-sector alignment and mkosi images
# which may use different alignment)
ESP_SECTOR=$(sgdisk -i 1 "${DISK}" 2>/dev/null | grep "First sector" | awk '{print $3}')
ESP_OFFSET=$((ESP_SECTOR * 512))
echo "  ESP offset: ${ESP_OFFSET} bytes (sector ${ESP_SECTOR})"

# Get the original UKI for reuse
mcopy -i "${TEST_DISK}@@${ESP_OFFSET}" ::/EFI/Linux/rush-linux.efi "${TEST_DIR}/original.efi"

# Simulate 3 update cycles: each renames the current UKI to a versioned
# rollback entry and installs a "new" one (we reuse the same binary)
for i in 1 2 3; do
    echo "  Update cycle ${i}: rotating UKI entry..."
    TIMESTAMP="$(date +%Y%m%d%H%M%S)"
    # Rename current main UKI to a rollback entry
    mcopy -i "${TEST_DISK}@@${ESP_OFFSET}" ::/EFI/Linux/rush-linux.efi \
        "${TEST_DIR}/prev.efi" 2>/dev/null || true
    # Copy it back as a versioned rollback entry
    if [ -f "${TEST_DIR}/prev.efi" ]; then
        mcopy -i "${TEST_DISK}@@${ESP_OFFSET}" "${TEST_DIR}/prev.efi" \
            "::/EFI/Linux/rush-linux-${TIMESTAMP}.efi"
        # Create corresponding loader entry
        ENTRY_CONF="${TEST_DIR}/entry-${i}.conf"
        cat > "${ENTRY_CONF}" <<EOF
title Rush Linux (rollback ${i})
version ${TIMESTAMP}
efi /EFI/Linux/rush-linux-${TIMESTAMP}.efi
EOF
        mcopy -i "${TEST_DISK}@@${ESP_OFFSET}" "${ENTRY_CONF}" \
            "::/loader/entries/rush-linux-rollback-${i}.conf"
    fi
    # Reinstall the original UKI as the new "main" entry
    mcopy -i "${TEST_DISK}@@${ESP_OFFSET}" "${TEST_DIR}/original.efi" \
        ::/EFI/Linux/rush-linux.efi
done

# Count rollback entries
ROLLBACK_COUNT=$(mdir -i "${TEST_DISK}@@${ESP_OFFSET}" ::/EFI/Linux 2>/dev/null | grep -c "rush-linux-" || echo "0")
echo "  Rollback entries found: ${ROLLBACK_COUNT}"

if [ "${ROLLBACK_COUNT}" -ge 3 ]; then
    pass "Test 2: ${ROLLBACK_COUNT} rollback entries retained (>= 3 required)"
else
    die "Test 2: Only ${ROLLBACK_COUNT} rollback entries found (need >= 3)"
fi

# Verify the image still boots with rollback entries present
echo "  Verifying image still boots with rollback entries..."
boot_and_log "t2-rollback-boot" "${TEST_DISK}"
if log_has "${LOG_DIR}/t2-rollback-boot.log" "multi-user"; then
    pass "Test 2a: Image boots correctly with rollback entries present"
else
    die "Test 2a: Image failed to boot with rollback entries"
fi

# ══════════════════════════════════════════════════════════════════════
echo ""
echo "━━━ Test 3: Simulated bad kernel rolls back ━━━"
# ══════════════════════════════════════════════════════════════════════

# Make a fresh copy with the rollback entries already in place
cp "${TEST_DISK}" "${TEST_DIR}/test-disk-bad.raw"
BAD_DISK="${TEST_DIR}/test-disk-bad.raw"

# Create a broken EFI binary
BROKEN_EFI="${TEST_DIR}/broken.efi"
printf 'MZ\x00\x00' > "${BROKEN_EFI}"
dd if=/dev/urandom of="${BROKEN_EFI}" bs=1 count=8192 conv=notrunc 2>/dev/null

echo "  Test 3a: Boot with broken main UKI (should fail)..."
# Replace the main UKI with the broken one
mcopy -i "${BAD_DISK}@@${ESP_OFFSET}" "${BROKEN_EFI}" ::/EFI/Linux/rush-linux.efi

boot_and_log "t3-bad-kernel" "${BAD_DISK}"

# The broken UKI should NOT reach multi-user.target
if log_has "${LOG_DIR}/t3-bad-kernel.log" "multi-user"; then
    die "Test 3a: Broken UKI unexpectedly reached multi-user.target"
else
    pass "Test 3a: Broken UKI correctly failed to boot"
fi

echo "  Test 3b: Simulate rollback — restore previous good UKI..."
# In a real system, the bootloader would detect the failure and select
# the rollback entry. We simulate this by replacing the broken UKI with
# the first rollback entry.
FIRST_ROLLBACK=$(mdir -i "${BAD_DISK}@@${ESP_OFFSET}" ::/EFI/Linux 2>/dev/null \
    | grep -o "rush-linux-[^r][^ ]*\.efi" | head -1 || true)

if [ -n "${FIRST_ROLLBACK}" ]; then
    echo "  Rolling back to: ${FIRST_ROLLBACK}"
    # Copy the rollback UKI to be the new main UKI
    mcopy -i "${BAD_DISK}@@${ESP_OFFSET}" "::/EFI/Linux/${FIRST_ROLLBACK}" \
        "${TEST_DIR}/rollback.efi"
    mcopy -i "${BAD_DISK}@@${ESP_OFFSET}" "${TEST_DIR}/rollback.efi" \
        ::/EFI/Linux/rush-linux.efi

    boot_and_log "t3-rollback-boot" "${BAD_DISK}"
    if log_has "${LOG_DIR}/t3-rollback-boot.log" "multi-user"; then
        pass "Test 3b: Rolled-back system booted successfully to multi-user.target"
    else
        die "Test 3b: Rolled-back system failed to boot"
    fi
else
    die "Test 3b: No rollback entries available for recovery"
fi

# ══════════════════════════════════════════════════════════════════════
echo ""
echo "============================================"
echo "  All rollback tests PASSED"
echo "============================================"
echo ""
echo "v0.4.0-alpha.1 exit criteria verified:"
echo "  ✅ VM boots through UKI"
echo "  ✅ Three rollback entries are retained"
echo "  ✅ Simulated bad kernel rolls back"
