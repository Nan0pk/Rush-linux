#!/usr/bin/env python3
"""Reject newly tracked build output, generated staging files, and executables."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GENERATED_PREFIXES = (
    "build/",
    "target/",
    ".mkosi-cache/",
    ".mkosi-output/",
    "mkosi/mkosi.extra/",
)
COMPILED_MAGIC = {
    b"\x7fELF": "ELF executable",
    b"MZ": "Windows executable",
}


def _git_paths(args: list[str]) -> list[str]:
    result = subprocess.run(
        ["git", *args],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or "git command failed")
    return [line for line in result.stdout.splitlines() if line]


def changed_files(base: str) -> list[str]:
    paths: set[str] = set()
    commands = (
        ["diff", "--diff-filter=AMCR", "--name-only", f"{base}...HEAD"],
        ["diff", "--diff-filter=AMCR", "--name-only"],
        ["diff", "--cached", "--diff-filter=AMCR", "--name-only"],
        ["ls-files", "--others", "--exclude-standard"],
    )
    for command in commands:
        paths.update(_git_paths(command))
    return sorted(paths)


def violations(paths: list[str], root: Path = ROOT) -> list[str]:
    failures: list[str] = []
    for relative in paths:
        normalized = relative.replace("\\", "/")
        if normalized.startswith(GENERATED_PREFIXES):
            failures.append(f"{normalized}: generated build/staging path")
            continue
        path = root / relative
        if not path.is_file():
            continue
        header = path.read_bytes()[:4]
        for magic, label in COMPILED_MAGIC.items():
            if header.startswith(magic):
                failures.append(f"{normalized}: compiled {label}")
                break
    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="origin/main")
    args = parser.parse_args(argv)
    try:
        failures = violations(changed_files(args.base))
    except RuntimeError as exc:
        print(f"BLOCKED: cannot determine changed files: {exc}", file=sys.stderr)
        return 2
    if failures:
        print("BLOCKED: generated or compiled artifacts are being added to source control.")
        print("Risk: builds become non-reproducible and the checkout dirties itself.")
        print("Root: build outputs must be recreated from canonical source inputs.")
        print("Ways forward: delete the artifact or generate it in an ignored build directory.")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("OK: changed files contain no generated staging or compiled artifacts")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
