#!/usr/bin/env python3
"""
test_testos_private_diagnostics_boundary.py — shared boundary-check helpers
for the PRIVATE-DIAGNOSTICS privacy contract.

These helpers are imported by test-testos-private-diagnostics.py. They are
also the reference implementation for the checks that the strict evidence
validator (tools/validate-testos-evidence.py) applies to every testOS
evidence bundle.

Contract (from the boot-reliability PR — private local diagnostics):
  Evidence submission fails closed if:
    - PRIVATE-DIAGNOSTICS appears inside the proposed bundle
    - any raw journal/dmesg artifact appears inside publishable evidence
    - a symlink tries to reference private diagnostics
"""

from __future__ import annotations

import os
from pathlib import Path

# Filenames that are raw diagnostics and must NEVER appear in publishable
# evidence. These are the files the testOS runner writes into
# PRIVATE-DIAGNOSTICS/<run_id>/ — finding them inside testos-results/ or a
# submission bundle is a privacy-boundary violation.
_RAW_DIAGNOSTIC_FILENAMES = {
    "dmesg.txt",
    "journalctl.txt",
    "systemctl-failed.txt",
    "status-usb-mount.txt",
    "status-runner.txt",
    "critical-chain.txt",
    "blame.txt",
    "kernel-version.txt",
    "image-version.txt",
    "runner-exit.txt",
    "usb-discovery-timeline.txt",
    # Old names from the pre-PR system-logs/ approach — also forbidden.
    "journal.txt",
    "uname.txt",
    "cpuinfo.txt",
    "meminfo.txt",
    "cmdline.txt",
    "lsblk.txt",
    "lspci.txt",
    "lsusb.txt",
}


def check_bundle_for_private_diagnostics(bundle_dir: Path) -> list[str]:
    """Return a list of problems (empty list = bundle is clean).

    A bundle is dirty if it contains a directory named PRIVATE-DIAGNOSTICS
    at any depth.
    """
    problems: list[str] = []
    for p in bundle_dir.rglob("*"):
        if p.is_dir() and p.name == "PRIVATE-DIAGNOSTICS":
            problems.append(
                f"PRIVATE-DIAGNOSTICS directory found inside bundle: "
                f"{p.relative_to(bundle_dir)}"
            )
    return problems


def check_bundle_for_raw_diagnostics(bundle_dir: Path) -> list[str]:
    """Return a list of problems (empty list = bundle is clean).

    A bundle is dirty if it contains any file whose name matches a raw
    diagnostic filename, regardless of where in the bundle it lives.
    """
    problems: list[str] = []
    for p in bundle_dir.rglob("*"):
        if p.is_file() and p.name in _RAW_DIAGNOSTIC_FILENAMES:
            problems.append(
                f"raw diagnostic file {p.name!r} found inside bundle: "
                f"{p.relative_to(bundle_dir)}"
            )
    return problems


def check_bundle_for_symlink_escape(bundle_dir: Path) -> list[str]:
    """Return a list of problems (empty list = bundle is clean).

    A bundle is dirty if it contains ANY symlink (publishable evidence must
    be regular files only), or if any symlink resolves to a path outside
    the bundle, or if any symlink resolves into a PRIVATE-DIAGNOSTICS
    directory anywhere on the filesystem.
    """
    problems: list[str] = []
    for p in bundle_dir.rglob("*"):
        if not p.is_symlink():
            continue
        # Any symlink inside a publishable bundle is forbidden — the
        # bundle must be regular files only.
        problems.append(
            f"symlink found inside bundle: {p.relative_to(bundle_dir)} -> "
            f"{os.readlink(p)}"
        )
        # If the symlink resolves into a PRIVATE-DIAGNOSTICS directory,
        # that's an especially severe violation.
        try:
            resolved = p.resolve(strict=False)
        except (OSError, RuntimeError):
            continue
        if "PRIVATE-DIAGNOSTICS" in resolved.parts:
            problems.append(
                f"symlink {p.relative_to(bundle_dir)} resolves into "
                f"PRIVATE-DIAGNOSTICS: {resolved}"
            )
    return problems


def check_bundle_boundary(bundle_dir: Path) -> list[str]:
    """Run all three boundary checks and return the combined problem list."""
    problems: list[str] = []
    problems.extend(check_bundle_for_private_diagnostics(bundle_dir))
    problems.extend(check_bundle_for_raw_diagnostics(bundle_dir))
    problems.extend(check_bundle_for_symlink_escape(bundle_dir))
    return problems


if __name__ == "__main__":
    import sys

    if len(sys.argv) != 2:
        print("usage: test_testos_private_diagnostics_boundary.py <bundle_dir>",
              file=sys.stderr)
        sys.exit(2)
    bundle = Path(sys.argv[1])
    if not bundle.is_dir():
        print(f"not a directory: {bundle}", file=sys.stderr)
        sys.exit(2)
    problems = check_bundle_boundary(bundle)
    if problems:
        for p in problems:
            print(f"  BOUNDARY VIOLATION: {p}", file=sys.stderr)
        sys.exit(1)
    print(f"bundle clean: {bundle}")
    sys.exit(0)
