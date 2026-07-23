#!/usr/bin/env python3
"""
tools/check-docs-impact.py — fail if user-facing changes lack docs updates.

Compares base vs head (default: origin/main...HEAD) and checks:
  1. Did any user-facing file change?
  2. If yes, did at least one docs/frontpage file change too?

User-facing paths/patterns:
  README.md
  tools/**
  packaging/systemd/**
  mkosi/**
  .github/workflows/**
  docs/editions/**
  docs/livedev/**
  scripts/**
  install/**
  pyproject.toml
  Makefile
  justfile

Docs-satisfying paths:
  README.md
  docs/**
  CHANGELOG.md
  docs/frontpage/**

Bypass:
  --allow-docs-not-needed (simulates the `docs-not-needed` PR label)
  In CI, the workflow passes this flag when the PR has the label.

Exit codes:
  0 — docs impact satisfied (or no user-facing changes, or bypass active)
  1 — user-facing changes detected and no docs update present
  2 — infrastructure error (git failed, base missing, etc.)
"""

from __future__ import annotations

import argparse
import fnmatch
import subprocess
import sys
from pathlib import Path

_HERE = Path(__file__).resolve().parent
_ROOT = _HERE.parent

# User-facing path patterns. A change in any of these triggers the check.
USER_FACING_PATTERNS = [
    "README.md",
    "tools/*",
    "tools/**/*",
    "packaging/systemd/*",
    "packaging/systemd/**/*",
    "mkosi/*",
    "mkosi/**/*",
    ".github/workflows/*",
    ".github/workflows/**/*",
    "docs/editions/*",
    "docs/editions/**/*",
    "docs/livedev/*",
    "docs/livedev/**/*",
    "scripts/*",
    "scripts/**/*",
    "install/*",
    "install/**/*",
    "pyproject.toml",
    "Makefile",
    "justfile",
]

# Docs-satisfying patterns. If at least one of these changed, the check passes.
DOCS_SATISFYING_PATTERNS = [
    "README.md",
    "docs/*",
    "docs/**/*",
    "CHANGELOG.md",
    "docs/frontpage/*",
    "docs/frontpage/**/*",
]


def _git(args: list[str], cwd: Path = _ROOT) -> tuple[int, str, str]:
    try:
        r = subprocess.run(
            ["git"] + args,
            capture_output=True, text=True, cwd=str(cwd), timeout=30,
        )
        return r.returncode, r.stdout, r.stderr
    except (subprocess.SubprocessError, FileNotFoundError) as e:
        return 1, "", str(e)


def _changed_files(base: str, head: str) -> list[str]:
    """Return committed plus local changed paths.

    CI normally has only the committed base-to-head diff. Including working,
    staged, and untracked files makes the same check useful before a local
    commit instead of silently reporting "no changes".
    """
    commands = (
        ["diff", "--name-only", f"{base}...{head}"],
        ["diff", "--name-only"],
        ["diff", "--cached", "--name-only"],
        ["ls-files", "--others", "--exclude-standard"],
    )
    changed: set[str] = set()
    for command in commands:
        rc, out, err = _git(command)
        if rc != 0:
            raise RuntimeError(f"git {' '.join(command)} failed: {err.strip()}")
        changed.update(line.strip() for line in out.splitlines() if line.strip())
    return sorted(changed)


def _match_any(path: str, patterns: list[str]) -> bool:
    """Return True if `path` matches any of the glob patterns.

    We support `**` to mean "any depth" by converting to a recursive check.
    """
    for pat in patterns:
        if _match_one(path, pat):
            return True
    return False


def _match_one(path: str, pattern: str) -> bool:
    """Match a single path against a single glob pattern with `**` support."""
    # Normalize: leading ./
    if path.startswith("./"):
        path = path[2:]
    # Convert `**` to a recursive match.
    # Simple approach: split pattern on `/`, split path on `/`, match segment
    # by segment with `**` matching zero-or-more segments.
    pat_parts = pattern.split("/")
    path_parts = path.split("/")
    return _match_segments(path_parts, pat_parts)


def _match_segments(path_parts: list[str], pat_parts: list[str]) -> bool:
    """Recursive segment matcher. `**` matches zero or more segments."""
    if not pat_parts:
        return not path_parts
    if pat_parts[0] == "**":
        # `**` matches zero or more segments.
        # Try consuming 0, 1, 2, ... segments.
        for i in range(len(path_parts) + 1):
            if _match_segments(path_parts[i:], pat_parts[1:]):
                return True
        return False
    if not path_parts:
        return False
    if fnmatch.fnmatch(path_parts[0], pat_parts[0]):
        return _match_segments(path_parts[1:], pat_parts[1:])
    return False


def _user_facing_changed(files: list[str]) -> list[str]:
    return [f for f in files if _match_any(f, USER_FACING_PATTERNS)]


def _docs_satisfying_changed(files: list[str]) -> list[str]:
    return [f for f in files if _match_any(f, DOCS_SATISFYING_PATTERNS)]


def check(base: str, head: str, allow_bypass: bool = False) -> tuple[int, str]:
    """Run the docs-impact check. Returns (exit_code, message)."""
    try:
        files = _changed_files(base, head)
    except RuntimeError as e:
        return 2, str(e)

    if not files:
        return 0, "No files changed."

    user_facing = _user_facing_changed(files)
    if not user_facing:
        return 0, "No user-facing files changed."

    if allow_bypass:
        return 0, (
            f"Bypass active (--allow-docs-not-needed). "
            f"{len(user_facing)} user-facing file(s) changed but docs-not-needed is set."
        )

    docs = _docs_satisfying_changed(files)
    if docs:
        return 0, (
            f"OK: {len(user_facing)} user-facing file(s) changed; "
            f"{len(docs)} docs file(s) updated: {', '.join(docs[:3])}"
        )

    return 1, (
        f"FAIL: {len(user_facing)} user-facing file(s) changed but no docs updated.\n"
        f"User-facing changes:\n"
        + "\n".join(f"  - {f}" for f in user_facing[:20])
        + (f"\n  ... and {len(user_facing) - 20} more" if len(user_facing) > 20 else "")
        + "\n\nUpdate at least one of: README.md, docs/**, CHANGELOG.md, "
        "docs/frontpage/**, or add the `docs-not-needed` label to the PR."
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="check-docs-impact",
        description="Fail if user-facing changes lack docs updates.",
    )
    parser.add_argument("--base", default="origin/main",
                        help="base ref (default: origin/main)")
    parser.add_argument("--head", default="HEAD",
                        help="head ref (default: HEAD)")
    parser.add_argument("--allow-docs-not-needed", action="store_true",
                        help="bypass (simulates the `docs-not-needed` PR label)")
    ns = parser.parse_args(argv)

    code, msg = check(ns.base, ns.head, ns.allow_docs_not_needed)
    if code == 0:
        print(f"OK: {msg}")
    else:
        print(msg, file=sys.stderr)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
