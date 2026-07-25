#!/usr/bin/env python3
"""Reject generated output, compiled files, and misplaced root reports."""

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
)
# `mkosi/mkosi.extra/` is NOT generated output. It is hand-maintained image
# source that mkosi copies into the rootfs, and 27 of its files are tracked
# in git. Two of them (the optid systemd units) are additionally required by
# `systemd_units_do_not_drift_between_packaging_and_mkosi` in
# crates/optid/src/capability.rs to stay byte-identical to their
# packaging/systemd/ counterparts. Treating the prefix as build output made
# that mandatory mirror-edit unmergeable: the drift test demanded the change
# and this check rejected it. Real mkosi build artifacts land in
# `.mkosi-output/` and `.mkosi-cache/`, which are still rejected above.
COMPILED_MAGIC = {
    b"\x7fELF": "ELF executable",
    b"MZ": "Windows executable",
}
ALLOWED_ROOT_MARKDOWN = {
    "AGENTS.md",
    "CLAUDE.md",
    "CODE_OF_CONDUCT.md",
    "CONTRIBUTING.md",
    "DIRTY_STATE.md",
    "FINAL-AUDIT-REPORT.md",
    "OPTID-COMPLETION-PLAN.md",
    "README.md",
    "RELEASES.md",
    "ROADMAP.md",
    "SECURITY.md",
    "SUPPORT.md",
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
        if "/" not in normalized and normalized.endswith(".md") and normalized not in ALLOWED_ROOT_MARKDOWN:
            failures.append(f"{normalized}: report or handoff belongs under docs/")
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
        print("BLOCKED: repository hygiene rejected one or more changed files.")
        print("Risk: generated output or stray reports can diverge from canonical project truth.")
        print("Root: build output is reproducible; maintained prose has an owned docs location.")
        print("Ways forward: regenerate artifacts outside source control or move prose under docs/.")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("OK: changed files contain no generated artifacts or misplaced root reports")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
