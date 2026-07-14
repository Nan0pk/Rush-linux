#!/usr/bin/env python3
"""sign_updates.py — Generate test signing keys and sign/verify update metadata.

This module provides programmatic signing and verification for the Rush Linux
update system. It replaces the mock signature stub in rush-builder.py repo-init
with real Ed25519 signatures.

It can be used standalone or imported as a library by the builder.

Usage:
    python3 tools/sign_updates.py init-keys
    python3 tools/sign_updates.py sign   <repo_dir>
    python3 tools/sign_updates.py verify <repo_dir>

Requires: cryptography (pip install cryptography)
"""

import argparse
import base64
import hashlib
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

KEY_DIR = Path(__file__).resolve().parent.parent / "config" / "keys"
PRIVATE_KEY_PATH = KEY_DIR / "testing.private.pem"
PUBLIC_KEY_PATH = KEY_DIR / "testing.public.pem"


def _get_cryptography():
    """Lazy import of the cryptography library with a helpful error."""
    try:
        from cryptography.hazmat.primitives import serialization, hashes
        from cryptography.hazmat.primitives.asymmetric import ed25519
        return serialization, hashes, ed25519
    except ImportError:
        print(
            "Error: 'cryptography' package is required for update signing.\n"
            "Install it with: pip install cryptography",
            file=sys.stderr,
        )
        sys.exit(1)


def init_keys(key_dir: Path | None = None) -> tuple[Path, Path]:
    """Generate an Ed25519 test key pair for signing update metadata.

    Returns (private_key_path, public_key_path).
    """
    serialization, hashes, ed25519 = _get_cryptography()

    if key_dir is None:
        key_dir = KEY_DIR

    key_dir.mkdir(parents=True, exist_ok=True)
    priv_path = key_dir / "testing.private.pem"
    pub_path = key_dir / "testing.public.pem"

    if priv_path.exists():
        print(f"Warning: {priv_path} already exists. Skipping key generation.")
        return priv_path, pub_path

    # Generate Ed25519 key pair
    private_key = ed25519.Ed25519PrivateKey.generate()
    public_key = private_key.public_key()

    # Serialize and save
    priv_pem = private_key.private_bytes(
        encoding=serialization.Encoding.PEM,
        format=serialization.PrivateFormat.PKCS8,
        encryption_algorithm=serialization.NoEncryption(),
    )
    pub_pem = public_key.public_bytes(
        encoding=serialization.Encoding.PEM,
        format=serialization.PublicFormat.SubjectPublicKeyInfo,
    )

    priv_path.write_bytes(priv_pem)
    pub_path.write_bytes(pub_pem)

    os.chmod(priv_path, 0o600)
    os.chmod(pub_path, 0o644)

    print(f"Private key: {priv_path}")
    print(f"Public key:  {pub_path}")
    print("")
    print("⚠  These are TEST keys only. Never use in production.")

    return priv_path, pub_path


def sign_repodata(
    repo_dir: Path,
    key_dir: Path | None = None,
) -> Path:
    """Sign repodata.json in repo_dir with the test private key.

    Creates:
        repodata.json.sig     — JSON manifest with base64 signature
        repodata.json.sig.bin — Raw Ed25519 detached signature

    Returns the path to the .sig manifest file.
    """
    serialization, hashes, ed25519 = _get_cryptography()

    if key_dir is None:
        key_dir = KEY_DIR

    repodata_path = repo_dir / "repodata.json"
    if not repodata_path.exists():
        print(f"Error: {repodata_path} not found", file=sys.stderr)
        sys.exit(1)

    priv_path = key_dir / "testing.private.pem"
    if not priv_path.exists():
        print(
            f"Error: no signing key at {priv_path}. Run 'init-keys' first.",
            file=sys.stderr,
        )
        sys.exit(1)

    # Load private key
    private_key = serialization.load_pem_private_key(
        priv_path.read_bytes(),
        password=None,
    )

    # Compute digest
    data = repodata_path.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    print(f"  SHA-256: {digest}")

    # Sign the digest
    signature = private_key.sign(data)
    sig_b64 = base64.b64encode(signature).decode("ascii")

    # Write raw binary signature
    sig_bin_path = repo_dir / "repodata.json.sig.bin"
    sig_bin_path.write_bytes(signature)

    # Write JSON signature manifest
    sig_manifest = {
        "algorithm": "Ed25519",
        "digest": f"SHA-256:{digest}",
        "signature": sig_b64,
        "public_key_file": "config/keys/testing.public.pem",
        "signed_at": datetime.now(timezone.utc).isoformat(),
    }
    sig_path = repo_dir / "repodata.json.sig"
    sig_path.write_text(json.dumps(sig_manifest, indent=2) + "\n")

    print(f"  Signature: {sig_path}")
    print("✅ Update metadata signed")

    return sig_path


def verify_repodata(
    repo_dir: Path,
    key_dir: Path | None = None,
) -> bool:
    """Verify the signature on repodata.json.

    Returns True if signature is valid, False otherwise.
    """
    serialization, hashes, ed25519 = _get_cryptography()

    if key_dir is None:
        key_dir = KEY_DIR

    repodata_path = repo_dir / "repodata.json"
    sig_bin_path = repo_dir / "repodata.json.sig.bin"
    pub_path = key_dir / "testing.public.pem"

    for p in (repodata_path, sig_bin_path, pub_path):
        if not p.exists():
            print(f"Error: {p} not found", file=sys.stderr)
            return False

    # Load public key
    public_key = serialization.load_pem_public_key(pub_path.read_bytes())

    # Read data and signature
    data = repodata_path.read_bytes()
    signature = sig_bin_path.read_bytes()

    # Verify
    try:
        public_key.verify(signature, data)
        print(f"✅ Signature VERIFIED: {repodata_path}")
        return True
    except Exception as e:
        print(f"❌ Signature INVALID: {repodata_path}: {e}", file=sys.stderr)
        return False


def main():
    parser = argparse.ArgumentParser(
        description="Sign and verify Rush Linux update metadata"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("init-keys", help="Generate test Ed25519 signing key pair")

    sign_parser = subparsers.add_parser("sign", help="Sign repodata.json")
    sign_parser.add_argument("repo_dir", help="Repository directory containing repodata.json")

    verify_parser = subparsers.add_parser("verify", help="Verify repodata.json signature")
    verify_parser.add_argument("repo_dir", help="Repository directory")

    args = parser.parse_args()

    if args.command == "init-keys":
        init_keys()
    elif args.command == "sign":
        sign_repodata(Path(args.repo_dir))
    elif args.command == "verify":
        ok = verify_repodata(Path(args.repo_dir))
        sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
