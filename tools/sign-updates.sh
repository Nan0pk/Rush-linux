#!/usr/bin/env bash
# sign-updates.sh — Generate test signing keys and sign update metadata.
#
# Implements the v0.4 "test update metadata is signed" exit criterion.
# Generates an Ed25519 test key pair, signs repodata.json, and produces
# a detached signature that the update client can verify.
#
# Usage:
#   tools/sign-updates.sh init-keys     Generate test key pair (once)
#   tools/sign-updates.sh sign <dir>    Sign repodata.json in <dir>
#   tools/sign-updates.sh verify <dir>  Verify repodata.json signature in <dir>
#
# Key storage (generated — never committed):
#   build/test-signing/keys/testing.private.pem — Ed25519 private key
#   build/test-signing/keys/testing.public.pem  — Ed25519 public key
# The historical config/keys/ location is rejected by .gitignore for
# private keys; new test runs must use build/test-signing/keys/.
#
# Environment:
#   RUSH_KEY_DIR   Override key directory (default: build/test-signing/keys)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KEY_DIR="${RUSH_KEY_DIR:-${ROOT}/build/test-signing/keys}"
PRIVATE_KEY="${KEY_DIR}/testing.private.pem"
PUBLIC_KEY="${KEY_DIR}/testing.public.pem"

cmd_init_keys() {
    echo "=== Generating test update signing keys ==="
    mkdir -p "${KEY_DIR}"

    if [ -f "${PRIVATE_KEY}" ]; then
        echo "Warning: ${PRIVATE_KEY} already exists. Skipping key generation."
        echo "Delete it and re-run to regenerate."
        return 0
    fi

    # Generate Ed25519 key pair for signing
    openssl genpkey -algorithm Ed25519 -out "${PRIVATE_KEY}" 2>/dev/null
    openssl pkey -in "${PRIVATE_KEY}" -pubout -out "${PUBLIC_KEY}" 2>/dev/null

    # Set restrictive permissions on the private key
    chmod 600 "${PRIVATE_KEY}"
    chmod 644 "${PUBLIC_KEY}"

    echo "Private key: ${PRIVATE_KEY}"
    echo "Public key:  ${PUBLIC_KEY}"
    echo ""
    echo "⚠  These are TEST keys only. Never use in production."
    echo "   Add ${PRIVATE_KEY} to .gitignore before pushing."
    echo ""
    echo "✅ Test signing keys generated"
}

cmd_sign() {
    local repo_dir="${1:?Usage: sign-updates.sh sign <repo_dir>}"
    local repodata="${repo_dir}/repodata.json"

    [ -f "${repodata}" ] || { echo "Error: ${repodata} not found"; exit 1; }
    [ -f "${PRIVATE_KEY}" ] || { echo "Error: no signing key. Run 'sign-updates.sh init-keys' first."; exit 1; }

    echo "=== Signing update metadata ==="
    echo "  File: ${repodata}"

    # Compute SHA-256 digest
    local digest
    digest=$(sha256sum "${repodata}" | awk '{print $1}')
    echo "  SHA-256: ${digest}"

    # Create detached Ed25519 signature
    openssl pkeyutl -sign -inkey "${PRIVATE_KEY}" \
        -pkeyopt digest:SHA256 \
        -in <(echo -n "${digest}") \
        -out "${repo_dir}.sig.bin" 2>/dev/null

    # Also create a human-readable signature manifest (JSON)
    local sig_b64
    sig_b64=$(base64 -w0 "${repo_dir}.sig.bin")

    # Record the public key path relative to the repo root so verifiers
    # can locate it regardless of whether the operator kept the legacy
    # config/keys/ location or migrated to build/test-signing/keys/.
    pub_rel="${PUBLIC_KEY#${ROOT}/}"
    cat > "${repo_dir}.sig" <<EOF
{
  "algorithm": "Ed25519",
  "digest": "SHA-256:${digest}",
  "signature": "${sig_b64}",
  "public_key_file": "${pub_rel}",
  "signed_at": "$(date -Iseconds)"
}
EOF

    echo "  Signature: ${repo_dir}.sig"
    echo "  Binary sig: ${repo_dir}.sig.bin"
    echo ""
    echo "✅ Update metadata signed"
}

cmd_verify() {
    local repo_dir="${1:?Usage: sign-updates.sh verify <repo_dir>}"
    local repodata="${repo_dir}/repodata.json"

    [ -f "${repodata}" ] || { echo "Error: ${repodata} not found"; exit 1; }
    [ -f "${repo_dir}.sig" ] || { echo "Error: ${repo_dir}.sig not found"; exit 1; }
    [ -f "${repo_dir}.sig.bin" ] || { echo "Error: ${repo_dir}.sig.bin not found"; exit 1; }
    [ -f "${PUBLIC_KEY}" ] || { echo "Error: public key not found at ${PUBLIC_KEY}"; exit 1; }

    echo "=== Verifying update metadata signature ==="

    # Recompute digest
    local digest
    digest=$(sha256sum "${repodata}" | awk '{print $1}')

    # Verify with Ed25519 public key
    if openssl pkeyutl -verify -pubin -inkey "${PUBLIC_KEY}" \
        -pkeyopt digest:SHA256 \
        -in <(echo -n "${digest}") \
        -sigfile "${repo_dir}.sig.bin" 2>/dev/null; then
        echo "✅ Signature VERIFIED: ${repodata}"
        return 0
    else
        echo "❌ Signature INVALID: ${repodata}" >&2
        return 1
    fi
}

case "${1:-help}" in
    init-keys)
        cmd_init_keys
        ;;
    sign)
        cmd_sign "${2:?Usage: sign-updates.sh sign <repo_dir>}"
        ;;
    verify)
        cmd_verify "${2:?Usage: sign-updates.sh verify <repo_dir>}"
        ;;
    help|--help|-h)
        echo "Usage: sign-updates.sh {init-keys|sign <dir>|verify <dir>}"
        echo ""
        echo "Generate test signing keys and sign/verify update metadata."
        echo "Uses Ed25519 keys stored in build/test-signing/keys/."
        ;;
    *)
        echo "Unknown command: ${1}" >&2
        exit 1
        ;;
esac
