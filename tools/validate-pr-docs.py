#!/usr/bin/env python3
"""
Rush Linux — PR Documentation Completeness Validator.

Uses docs/docmap.toml to determine which docs cover which code paths.
When code files change, the covering docs must also be updated (or have
their last_verified date bumped in docmap.toml) in the same PR.

This replaces manual PR labeling and doc checklists: if CI is green,
docs are complete.

Usage:
  # Check against main branch (default):
  python3 tools/validate-pr-docs.py

  # Check against a specific base:
  python3 tools/validate-pr-docs.py --base origin/main

  # Dry-run / advisory mode (warns but exits 0):
  python3 tools/validate-pr-docs.py --advisory

Exit code: 0 = docs complete, 1 = missing doc updates.
"""

import argparse
import os
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCMAP_PATH = ROOT / "docs" / "docmap.toml"

# Files that are pure infra and never need doc updates
INFRA_GLOBS = {
    ".github/",
    ".gitignore",
    ".gitattributes",
    "Cargo.lock",
    "deny.toml",
    "rustfmt.toml",
    ".clippy.toml",
    ".claude/",
}


def load_docmap():
    with open(DOCMAP_PATH, "rb") as f:
        data = tomllib.load(f)
    return data.get("docs", {})


def get_changed_files(base):
    """Get files changed relative to the base branch."""
    try:
        subprocess.run(
            ["git", "fetch", "origin", base.replace("origin/", "")],
            cwd=ROOT, capture_output=True, timeout=30,
        )
    except Exception:
        pass

    result = subprocess.run(
        ["git", "diff", "--name-only", base + "...HEAD"],
        cwd=ROOT, capture_output=True, text=True, timeout=30,
    )
    if result.returncode != 0:
        result = subprocess.run(
            ["git", "diff", "--name-only", base],
            cwd=ROOT, capture_output=True, text=True, timeout=30,
        )
    return [f.strip() for f in result.stdout.strip().split("\n") if f.strip()]


def is_infra_file(path):
    for prefix in INFRA_GLOBS:
        if path.startswith(prefix):
            return True
    return False


def is_doc_file(path):
    if path.endswith(".md") or path.endswith(".toml"):
        if path.startswith("docs/") or path in (
            "README.md", "CONTRIBUTING.md", "SECURITY.md",
            "RELEASES.md", "ROADMAP.md", "AUTHORS", "VERSION",
            "CODE_OF_CONDUCT.md", "AGENTS.md", "CLAUDE.md",
        ):
            return True
    if path == "docs/docmap.toml":
        return True
    return False


def build_code_to_docs_map(entries):
    """Invert covers_code: map each code path/glob to the docs that cover it."""
    code_to_docs = {}
    for doc_path, entry in entries.items():
        for code_pattern in entry.get("covers_code", []):
            code_to_docs.setdefault(code_pattern, []).append(doc_path)
    return code_to_docs


def find_covering_docs(changed_file, code_to_docs):
    """Find all docs that cover a given changed file."""
    covering = set()
    for pattern, docs in code_to_docs.items():
        if pattern.endswith("*") or pattern.endswith("/"):
            prefix = pattern.rstrip("*").rstrip("/")
            if changed_file.startswith(prefix):
                covering.update(docs)
        elif changed_file == pattern:
            covering.update(docs)
        elif "/" in pattern and changed_file.startswith(pattern.split("/")[0] + "/"):
            prefix = pattern.rstrip("*").rstrip("/")
            if changed_file.startswith(prefix):
                covering.update(docs)
    return covering


def main():
    parser = argparse.ArgumentParser(
        description="Validate that code changes have corresponding doc updates"
    )
    parser.add_argument(
        "--base", default="origin/main",
        help="Base branch to diff against (default: origin/main)",
    )
    parser.add_argument(
        "--advisory", action="store_true",
        help="Warn but don't fail (exit 0 regardless)",
    )
    args = parser.parse_args()

    print("╔══════════════════════════════════════════════════════════╗")
    print("║   Rush Linux — PR Documentation Completeness Check      ║")
    print("╚══════════════════════════════════════════════════════════╝")

    entries = load_docmap()
    code_to_docs = build_code_to_docs_map(entries)
    changed_files = get_changed_files(args.base)

    if not changed_files:
        print("\n  ✅ No changed files detected.")
        return 0

    changed_set = set(changed_files)
    changed_docs = {f for f in changed_files if is_doc_file(f)}
    changed_code = {f for f in changed_files if not is_doc_file(f) and not is_infra_file(f)}

    # Also count docmap.toml changes as doc updates (bumping last_verified)
    docmap_changed = "docs/docmap.toml" in changed_set

    print(f"\n  Files changed: {len(changed_files)}")
    print(f"  Code files:    {len(changed_code)}")
    print(f"  Doc files:     {len(changed_docs)}")
    print(f"  Infra files:   {len(changed_files) - len(changed_code) - len(changed_docs)}")

    if not changed_code:
        print("\n  ✅ Doc-only or infra-only change — no code-doc sync needed.")
        return 0

    # For each changed code file, find which docs should cover it
    missing = {}
    covered_ok = []
    uncovered_code = []

    for code_file in sorted(changed_code):
        covering_docs = find_covering_docs(code_file, code_to_docs)
        if not covering_docs:
            uncovered_code.append(code_file)
            continue

        # Check if at least one covering doc was updated in this PR
        updated = covering_docs & changed_docs
        if updated or docmap_changed:
            covered_ok.append((code_file, covering_docs))
        else:
            missing[code_file] = covering_docs

    print("\n── Coverage Analysis ──")

    if covered_ok:
        print(f"\n  ✅ {len(covered_ok)} code file(s) have matching doc updates")

    if uncovered_code:
        print(f"\n  ℹ️  {len(uncovered_code)} code file(s) not tracked by any doc (OK):")
        for f in uncovered_code[:10]:
            print(f"     {f}")
        if len(uncovered_code) > 10:
            print(f"     ... and {len(uncovered_code) - 10} more")

    if missing:
        print(f"\n  ❌ {len(missing)} code file(s) changed WITHOUT updating their covering docs:\n")
        for code_file, docs in sorted(missing.items()):
            print(f"     {code_file}")
            print(f"       ↳ needs update in: {', '.join(sorted(docs))}")

        print("\n  To fix: update the listed docs, or bump their last_verified")
        print("  date in docs/docmap.toml if the docs are already accurate.\n")

        if args.advisory:
            print("  ⚠️  Advisory mode — not blocking merge.")
            return 0
        return 1

    print(f"\n  ✅ All documentation is complete for this change.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
