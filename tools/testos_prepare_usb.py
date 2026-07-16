#!/usr/bin/env python3
"""
tools/testos_prepare_usb.py — production generation and USB installation of
run-intent.json and plan.json for a physical testOS run.

This is the HOST-SIDE counterpart to crates/testos/src/run_intent.rs (the
GUEST-SIDE validator). The host generates a cryptographically-bound contract;
the guest validates it on boot; the strict evidence validator re-checks it
on collection.

Usage (called by tools/livedev-bootstrap.sh after image write):
  python3 tools/testos_prepare_usb.py \\
      --repo-root /path/to/Rush-linux \\
      --plan-path /run-dir/plan.json \\
      --image-path /cache/testos-0.7.0-beta.4.raw \\
      --testos-image-commit 0123456789abcdef0123456789abcdef01234567 \\
      --testos-version 0.7.0-beta.4 \\
      --run-id auto-20260715-120000 \\
      --checkpoint-nonce ckpt-20260715-abcd1234 \\
      --device /dev/sdX

Testing (no hardware, no root):
  python3 tools/testos_prepare_usb.py \\
      --repo-root /path/to/Rush-linux \\
      --plan-path /run-dir/plan.json \\
      --image-path /cache/testos-0.7.0-beta.4.raw \\
      --testos-image-commit 0123456789abcdef0123456789abcdef01234567 \\
      --testos-version 0.7.0-beta.4 \\
      --run-id auto-20260715-120000 \\
      --checkpoint-nonce ckpt-20260715-abcd1234 \\
      --source-dir /tmp/mock-esp

Exit codes:
  0 — intent + plan installed and verified
  1 — generation or installation error
"""

from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

_TOOLS_DIR = Path(__file__).resolve().parent
_REPO_ROOT = _TOOLS_DIR.parent
SCHEMA_VERSION = 1
INTENT_KIND = "testos-run-intent"
ESPCONF_NAME = "testos-esp-mount.conf"


def _git_head(repo_root: Path) -> str:
    r = subprocess.run(
        ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
        capture_output=True, text=True, timeout=5,
    )
    if r.returncode != 0:
        raise RuntimeError(f"cannot resolve HEAD in {repo_root}: {r.stderr.strip()}")
    return r.stdout.strip()


def _version(repo_root: Path) -> str:
    vf = repo_root / "VERSION"
    if not vf.exists():
        raise RuntimeError(f"VERSION file not found: {vf}")
    return vf.read_text().strip()


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def _now_iso() -> str:
    return _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def generate_run_intent(
    *,
    repo_root: Path,
    plan_path: Path,
    image_path: Path,
    run_id: str,
    checkpoint_nonce: str,
    testos_image_commit: str,
    testos_version: str,
    campaign_id: str | None = None,
    dry_run: bool = False,
) -> tuple[bytes, str]:
    """Generate a run-intent.json from real values.

    Returns (raw_bytes, intent_sha256_hex). Every digest is computed from
    the actual artifact: the image bytes being written to USB, the plan
    bytes, and the catalog bytes. Nothing is fabricated.

    Raises RuntimeError if any required input is missing or malformed.
    """
    # --- Source identity ---
    source_commit = _git_head(repo_root)
    source_version = _version(repo_root)
    if not re.fullmatch(r"[0-9a-f]{40}", testos_image_commit):
        raise RuntimeError(
            "testos image commit must be exactly 40 lowercase hex characters"
        )
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", testos_version):
        raise RuntimeError("testos version is not valid semver")

    # --- Plan hash (from the actual plan.json file) ---
    if not plan_path.is_file():
        raise RuntimeError(f"plan file not found: {plan_path}")
    plan_sha256 = _sha256_file(plan_path)

    # --- Catalog hash (from the actual bench-list.toml) ---
    catalog_path = repo_root / "testos" / "bench-list.toml"
    if not catalog_path.is_file():
        raise RuntimeError(f"bench-list.toml not found: {catalog_path}")
    catalog_sha256 = _sha256_file(catalog_path)

    # --- Image hash (from the actual .raw being written to USB) ---
    if not image_path.is_file():
        raise RuntimeError(f"image file not found: {image_path}")
    image_sha256 = _sha256_file(image_path)
    image_digest = f"sha256:{image_sha256}"

    # --- Build the intent ---
    intent = {
        "schema_version": SCHEMA_VERSION,
        "intent_kind": INTENT_KIND,
        "run_id": run_id,
        "source_commit": source_commit,
        "source_version": source_version,
        "testos_version": testos_version,
        "testos_image_digest": image_digest,
        "testos_image_commit": testos_image_commit,
        "plan_sha256": plan_sha256,
        "benchmark_catalog_sha256": catalog_sha256,
        "generated_at": _now_iso(),
        "dry_run": dry_run,
        "checkpoint_nonce": checkpoint_nonce,
    }
    if campaign_id:
        intent["campaign_id"] = campaign_id

    # Canonical encoding (sorted keys, indent=2) so the SHA is deterministic.
    raw = json.dumps(intent, indent=2, sort_keys=True).encode("utf-8")
    intent_sha = _sha256_bytes(raw)
    return raw, intent_sha


def _find_esp_partition(device: str) -> str:
    """Find the ESP partition (label RUSHESP or first vfat partition) of a device."""
    # Try by label first.
    try:
        r = subprocess.run(
            ["blkid", "-t", "LABEL=RUSHESP", "-o", "device"],
            capture_output=True, text=True, timeout=10,
        )
        if r.returncode == 0:
            parts = [p.strip() for p in r.stdout.strip().splitlines() if p.strip()]
            if parts:
                return parts[0]
    except (OSError, subprocess.TimeoutExpired):
        pass

    # Fallback: first vfat partition on the device.
    try:
        r = subprocess.run(
            ["blkid", "-t", "TYPE=vfat", "-o", "device"],
            capture_output=True, text=True, timeout=10,
        )
        if r.returncode == 0:
            parts = [
                p.strip() for p in r.stdout.strip().splitlines()
                if p.strip() and device in p
            ]
            if parts:
                return parts[0]
    except (OSError, subprocess.TimeoutExpired):
        pass

    # Last resort: first partition of the device.
    part = f"{device}1"
    if Path(part).exists():
        return part
    # nvme/mmcblk naming
    for candidate in (f"{device}p1", f"{device}1"):
        if Path(candidate).exists():
            return candidate
    raise RuntimeError(f"cannot find ESP partition on {device}")


def install_intent_plan(
    *,
    esp_mount: Path,
    intent_raw: bytes,
    plan_path: Path,
    catalog_path: Path,
) -> None:
    """Install run-intent.json + plan.json into the ESP mount directory.

    Writes atomically (temp file + rename), fsyncs each file, then reads
    them back and verifies the SHA-256 of each against what was written.

    Raises RuntimeError on any write/verify mismatch.
    """
    files: list[tuple[str, bytes]] = [
        ("run-intent.json", intent_raw),
    ]
    if plan_path.is_file():
        files.append(("plan.json", plan_path.read_bytes()))
    # Also copy the catalog so the guest runner can verify it matches.
    if catalog_path.is_file():
        files.append(("bench-list.toml", catalog_path.read_bytes()))

    for name, data in files:
        dest = esp_mount / name
        esp_mount.mkdir(parents=True, exist_ok=True)
        # Atomic write: temp file in same dir, then rename.
        tmp = dest.with_name(f".{name}.tmp")
        with open(tmp, "wb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, dest)
        # On vfat, fsync the directory so the rename is durable.
        try:
            dir_fd = os.open(str(esp_mount), os.O_RDONLY)
            os.fsync(dir_fd)
            os.close(dir_fd)
        except OSError:
            pass  # vfat/exfat don't support dir fsync on all platforms

    # Read back and verify hashes.
    for name, expected_data in files:
        dest = esp_mount / name
        if not dest.is_file():
            raise RuntimeError(f"verification failed: {name} not present after write")
        actual = dest.read_bytes()
        if actual != expected_data:
            raise RuntimeError(
                f"verification failed: {name} content mismatch after readback "
                f"(expected {len(expected_data)} bytes, got {len(actual)})"
            )


def run_prepare(
    *,
    repo_root: Path,
    plan_path: Path,
    image_path: Path,
    run_id: str,
    checkpoint_nonce: str,
    testos_image_commit: str,
    testos_version: str,
    campaign_id: str | None,
    dry_run: bool,
    device: str | None,
    source_dir: Path | None,
) -> int:
    """Generate and install run-intent.json + plan.json to USB ESP or source dir."""
    catalog_path = repo_root / "testos" / "bench-list.toml"

    # Step 1: Generate the intent from real values.
    print(">> Generating run-intent.json from real values...")
    intent_raw, intent_sha = generate_run_intent(
        repo_root=repo_root,
        plan_path=plan_path,
        image_path=image_path,
        run_id=run_id,
        checkpoint_nonce=checkpoint_nonce,
        testos_image_commit=testos_image_commit,
        testos_version=testos_version,
        campaign_id=campaign_id,
        dry_run=dry_run,
    )
    intent = json.loads(intent_raw)
    print(f"   run_id:              {intent['run_id']}")
    print(f"   source_commit:       {intent['source_commit'][:12]}...")
    print(f"   source_version:      {intent['source_version']}")
    print(f"   testos_version:      {intent['testos_version']}")
    print(f"   testos_image_digest: {intent['testos_image_digest'][:20]}...")
    print(f"   testos_image_commit: {intent['testos_image_commit'][:12]}...")
    print(f"   plan_sha256:         {intent['plan_sha256'][:20]}...")
    print(f"   catalog_sha256:      {intent['benchmark_catalog_sha256'][:20]}...")
    print(f"   intent_sha256:       {intent_sha[:20]}...")
    print(f"   dry_run:             {intent['dry_run']}")

    # Step 2: Install to USB or source directory.
    if source_dir is not None:
        print(f"\n>> Installing intent + plan to source dir: {source_dir}")
        source_dir.mkdir(parents=True, exist_ok=True)
        install_intent_plan(
            esp_mount=source_dir,
            intent_raw=intent_raw,
            plan_path=plan_path,
            catalog_path=catalog_path,
        )
        print("   OK: installed and verified.")
        # Print the intent path for the integration test.
        print(f"\n   run-intent.json at: {source_dir / 'run-intent.json'}")
        return 0

    if device is None:
        print("ERROR: must specify --device or --source-dir", file=sys.stderr)
        return 1

    # Mount the ESP partition of the USB.
    print(f"\n>> Mounting ESP partition of {device}...")
    esp_part = _find_esp_partition(device)
    print(f"   ESP partition: {esp_part}")
    mount_point = Path(tempfile.mkdtemp(prefix="testos-esp-"))
    mounted = False
    try:
        r = subprocess.run(
            ["mount", "-t", "vfat", esp_part, str(mount_point)],
            capture_output=True, text=True, timeout=10,
        )
        if r.returncode != 0:
            raise RuntimeError(f"mount {esp_part} failed: {r.stderr.strip()}")
        mounted = True
        print(f"   Mounted at: {mount_point}")

        print("\n>> Installing run-intent.json + plan.json to ESP...")
        install_intent_plan(
            esp_mount=mount_point,
            intent_raw=intent_raw,
            plan_path=plan_path,
            catalog_path=catalog_path,
        )
        print("   OK: installed and verified (readback confirmed).")

        # Sync to ensure the USB is flushed before reboot.
        subprocess.run(["sync"], timeout=10)
        print("   OK: sync complete.")
        return 0
    finally:
        if mounted:
            subprocess.run(["umount", str(mount_point)], timeout=10,
                           capture_output=True)
        try:
            mount_point.rmdir()
        except OSError:
            pass


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="testos_prepare_usb",
        description="Generate and install run-intent.json + plan.json to a USB.",
    )
    parser.add_argument("--repo-root", type=Path, default=_REPO_ROOT)
    parser.add_argument("--plan-path", type=Path, required=True,
                        help="Path to the benchmark plan.json.")
    parser.add_argument("--image-path", type=Path, required=True,
                        help="Path to the .raw image being written to USB.")
    parser.add_argument("--run-id", required=True,
                        help="Stable run identifier (must match checkpoint).")
    parser.add_argument("--checkpoint-nonce", required=True,
                        help="Checkpoint nonce / campaign identity.")
    parser.add_argument("--testos-image-commit", required=True,
                        help="Full 40-char commit SHA embedded in the release image.")
    parser.add_argument("--testos-version", required=True,
                        help="Version embedded in the release image.")
    parser.add_argument("--campaign-id", default=None)
    parser.add_argument("--dry-run", action="store_true",
                        help="Mark the intent as dry_run=true.")
    parser.add_argument("--device", default=None,
                        help="USB block device (e.g. /dev/sdX). Mounts its ESP.")
    parser.add_argument("--source-dir", type=Path, default=None,
                        help="Write to a local dir instead of mounting a USB (testing).")
    ns = parser.parse_args(argv)

    if ns.device is None and ns.source_dir is None:
        print("ERROR: must specify either --device or --source-dir", file=sys.stderr)
        return 1

    try:
        return run_prepare(
            repo_root=ns.repo_root,
            plan_path=ns.plan_path,
            image_path=ns.image_path,
            run_id=ns.run_id,
            checkpoint_nonce=ns.checkpoint_nonce,
            testos_image_commit=ns.testos_image_commit,
            testos_version=ns.testos_version,
            campaign_id=ns.campaign_id,
            dry_run=ns.dry_run,
            device=ns.device,
            source_dir=ns.source_dir,
        )
    except Exception as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
