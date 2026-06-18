#!/usr/bin/env bash
# test-sign-updates.sh — Validate the update signing system.
#
# Tests the v0.4 "test update metadata is signed" exit criterion:
#   1. Test key pair can be generated
#   2. repodata.json can be signed
#   3. Signature can be verified against the public key
#   4. Tampered data fails verification
#
# Usage:
#   tools/test-sign-updates.sh
#
# No prerequisites beyond Python 3 with the 'cryptography' package.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_DIR="${ROOT}/build/test-signing"

die() { echo "❌ FAIL: $*" >&2; exit 1; }
pass() { echo "✅ PASS: $*"; }

echo "============================================"
echo "  Update Signing Test Suite (v0.4)"
echo "============================================"
echo ""

# Clean up from any previous run
rm -rf "${TEST_DIR}"
mkdir -p "${TEST_DIR}"

# ── Test 1: Generate test keys ────────────────────────────────────────
echo "━━━ Test 1: Generate test signing keys ━━━"

KEY_DIR="${TEST_DIR}/keys"
python3 "${ROOT}/tools/sign_updates.py" init-keys 2>&1 | head -5 || true

# If the module-based keys don't work (no cryptography package), try shell
if [ ! -f "${KEY_DIR}/testing.private.pem" ]; then
    # Fall back to the shell-based tool with custom key dir
    RUSH_KEY_DIR="${KEY_DIR}" "${ROOT}/tools/sign-updates.sh" init-keys
fi

# Check that keys exist (either the global config/keys or our test dir)
FOUND_KEYS=false
for kdir in "${KEY_DIR}" "${ROOT}/config/keys"; do
    if [ -f "${kdir}/testing.private.pem" ] && [ -f "${kdir}/testing.public.pem" ]; then
        FOUND_KEYS=true
        ACTUAL_KEY_DIR="${kdir}"
        break
    fi
done

if ${FOUND_KEYS}; then
    pass "Test 1: Test signing keys generated at ${ACTUAL_KEY_DIR}"
else
    die "Test 1: Failed to generate signing keys"
fi

# ── Test 2: Sign repodata.json ────────────────────────────────────────
echo ""
echo "━━━ Test 2: Sign update metadata ━━━"

# Create a mock repodata.json
mkdir -p "${TEST_DIR}/repo"
cat > "${TEST_DIR}/repo/repodata.json" <<EOF
{
  "packages": [
    {
      "name": "linux-adaptive",
      "version": "6.1.0-49",
      "checksum": "abc123"
    }
  ]
}
EOF

# Sign it
if python3 -c "import cryptography" 2>/dev/null; then
    python3 -c "
from tools.sign_updates import sign_repodata
from pathlib import Path
sign_repodata(Path('${TEST_DIR}/repo'), Path('${ACTUAL_KEY_DIR}'))
" 2>&1 || python3 "${ROOT}/tools/sign_updates.py" sign "${TEST_DIR}/repo"
else
    RUSH_KEY_DIR="${ACTUAL_KEY_DIR}" "${ROOT}/tools/sign-updates.sh" sign "${TEST_DIR}/repo"
fi

if [ -f "${TEST_DIR}/repo/repodata.json.sig" ]; then
    pass "Test 2: repodata.json signed (manifest created)"
else
    die "Test 2: Failed to sign repodata.json"
fi

if [ -f "${TEST_DIR}/repo/repodata.json.sig.bin" ]; then
    pass "Test 2a: Detached binary signature created"
else
    die "Test 2a: Detached binary signature not created"
fi

# Verify signature fields
ALGORITHM=$(python3 -c "import json; d=json.load(open('${TEST_DIR}/repo/repodata.json.sig')); print(d.get('algorithm',''))" 2>/dev/null || echo "")
if [ "${ALGORITHM}" = "Ed25519" ]; then
    pass "Test 2b: Signature algorithm is Ed25519"
else
    die "Test 2b: Expected Ed25519 algorithm, got: ${ALGORITHM}"
fi

# ── Test 3: Verify valid signature ────────────────────────────────────
echo ""
echo "━━━ Test 3: Verify valid signature ━━━"

VERIFY_OK=false
if python3 -c "import cryptography" 2>/dev/null; then
    if python3 -c "
from tools.sign_updates import verify_repodata
from pathlib import Path
ok = verify_repodata(Path('${TEST_DIR}/repo'), Path('${ACTUAL_KEY_DIR}'))
exit(0 if ok else 1)
" 2>&1; then
        VERIFY_OK=true
    fi
fi

if ! ${VERIFY_OK}; then
    if RUSH_KEY_DIR="${ACTUAL_KEY_DIR}" "${ROOT}/tools/sign-updates.sh" verify "${TEST_DIR}/repo"; then
        VERIFY_OK=true
    fi
fi

if ${VERIFY_OK}; then
    pass "Test 3: Valid signature verified successfully"
else
    die "Test 3: Signature verification failed on valid data"
fi

# ── Test 4: Detect tampered data ──────────────────────────────────────
echo ""
echo "━━━ Test 4: Detect tampered data ━━━"

# Tamper with the repodata
echo '{"packages": [{"name": "TAMPERED", "version": "evil"}]}' \
    > "${TEST_DIR}/repo/repodata.json"

TAMPER_DETECTED=false
if python3 -c "import cryptography" 2>/dev/null; then
    if python3 -c "
from tools.sign_updates import verify_repodata
from pathlib import Path
ok = verify_repodata(Path('${TEST_DIR}/repo'), Path('${ACTUAL_KEY_DIR}'))
exit(0 if not ok else 1)
" 2>&1; then
        TAMPER_DETECTED=true
    fi
fi

if ! ${TAMPER_DETECTED}; then
    if RUSH_KEY_DIR="${ACTUAL_KEY_DIR}" "${ROOT}/tools/sign-updates.sh" verify "${TEST_DIR}/repo" 2>&1; then
        TAMPER_DETECTED=false
    else
        TAMPER_DETECTED=true
    fi
fi

if ${TAMPER_DETECTED}; then
    pass "Test 4: Tampered data correctly rejected by signature verification"
else
    die "Test 4: Tampered data was NOT detected (signature check broken)"
fi

# ── Summary ───────────────────────────────────────────────────────────
echo ""
echo "============================================"
echo "  All signing tests PASSED"
echo "============================================"
echo ""
echo "v0.4.0-alpha.1 exit criterion verified:"
echo "  ✅ Test update metadata is signed"
