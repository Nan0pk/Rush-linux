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

# Private key material that must never be committed. The headers cover the
# PEM formats the post-#337 repair removed (`config/keys/testing.private.pem`)
# plus the OpenSSH and RSA forms a future tool might add. The check inspects
# every tracked file's content (not just the diff) so a `git revert` or a
# bad merge cannot reintroduce a key the .gitignore rule alone would miss
# (.gitignore does not retroactively untrack files already in the index).
PRIVATE_KEY_MARKERS = (
    b"-----BEGIN PRIVATE KEY-----",
    b"-----BEGIN RSA PRIVATE KEY-----",
    b"-----BEGIN OPENSSH PRIVATE KEY-----",
    b"-----BEGIN EC PRIVATE KEY-----",
)
# Narrow allow-list: files that legitimately embed a PEM marker string for
# scanning or redaction (the scanner itself, its unit tests, and the
# log-capture redactor that strips private key material from logs). Keep
# this list small and review every addition; the goal is "no private key
# material in tracked files". A real key file must never be added here.
PRIVATE_KEY_FIXTURE_ALLOWLIST: frozenset[str] = frozenset(
    {
        # The scanner that detects the markers; it must contain them.
        "tools/check-repo-hygiene.py",
        # Unit tests that construct synthetic markers at runtime.
        "tools/test-repo-hygiene.py",
        # Log-capture redactor that strips private key material from logs.
        "tools/rush_capture_lib.py",
    }
)


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


def tracked_files(root: Path = ROOT) -> list[str]:
    """All files in the git index (committed or staged). Used for the
    private-key scan so a `git revert` or bad merge cannot reintroduce a
    key the .gitignore rule alone would miss.
    """
    return _git_paths(["ls-files"])


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


def private_key_violations(paths: list[str], root: Path = ROOT) -> list[str]:
    """Reject tracked files containing PEM private-key material.

    The scan is content-based (not name-based) so a `.pem`-renamed or
    extension-less private key is still caught. The narrow fixture
    allow-list is reserved for negative-path tests that intentionally
    embed a marker; the default is empty so no real key material passes.
    """
    failures: list[str] = []
    for relative in paths:
        normalized = relative.replace("\\", "/")
        if normalized in PRIVATE_KEY_FIXTURE_ALLOWLIST:
            continue
        path = root / relative
        if not path.is_file():
            continue
        try:
            data = path.read_bytes()
        except OSError:
            continue
        for marker in PRIVATE_KEY_MARKERS:
            if marker in data:
                failures.append(
                    f"{normalized}: tracked file contains private key material "
                    f"({marker.decode('ascii', errors='replace').strip()})"
                )
                break
    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="origin/main")
    args = parser.parse_args(argv)
    try:
        diff_paths = changed_files(args.base)
        failures = violations(diff_paths)
        # Private-key scan runs over every tracked file, not just the diff,
        # so a stale key already in the repository (or one reintroduced by a
        # bad merge) is still caught. This is the post-#337 regression gate.
        failures.extend(private_key_violations(tracked_files()))
    except RuntimeError as exc:
        print(f"BLOCKED: cannot determine changed files: {exc}", file=sys.stderr)
        return 2
    if failures:
        print("BLOCKED: repository hygiene rejected one or more changed files.")
        print("Risk: generated output, stray reports, or private key material can leak into source control.")
        print("Root: build output is reproducible; maintained prose has an owned docs location; keys are generated material.")
        print("Ways forward: regenerate artifacts outside source control, move prose under docs/, generate keys under build/test-signing/keys/.")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("OK: changed files contain no generated artifacts, misplaced root reports, or private key material")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
