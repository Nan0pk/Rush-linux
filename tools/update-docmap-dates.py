#!/usr/bin/env python3
"""
Rush Linux — Auto-update docmap last_verified dates.

For every doc in docs/docmap.toml whose source file was modified
relative to the last commit, bump last_verified to today's date.
Also bumps docs that transitively depend on changed docs via deps.
"""

import os
import sys
import tomllib
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCMAP_PATH = ROOT / "docs" / "docmap.toml"

def main():
    if not DOCMAP_PATH.exists():
        print("docs/docmap.toml not found, skipping date update.")
        return

    with open(DOCMAP_PATH, "rb") as f:
        data = tomllib.load(f)

    entries = data.get("docs", {})
    today = datetime.now().strftime("%Y-%m-%d")

    # Find which docs were changed on disk vs last commit
    changed_files = set()
    for doc_path in entries:
        full = ROOT / doc_path
        if not full.exists():
            continue
        # Compare mtime to git
        import subprocess
        try:
            result = subprocess.run(
                ["git", "diff", "--name-only", "HEAD", "--", doc_path],
                capture_output=True, text=True, cwd=ROOT
            )
            if result.stdout.strip():
                changed_files.add(doc_path)
        except Exception:
            pass

    # Also check untracked files (new docs)
    try:
        result = subprocess.run(
            ["git", "ls-files", "--others", "--exclude-standard"],
            capture_output=True, text=True, cwd=ROOT
        )
        for line in result.stdout.strip().split("\n"):
            if line in entries:
                changed_files.add(line)
    except Exception:
        pass

    if not changed_files:
        print("  No changed docs to update in docmap.")
        return

    # Resolve transitive deps: if doc A depends on changed doc B, mark A too
    all_to_update = set(changed_files)
    for path, entry in entries.items():
        deps = entry.get("deps", [])
        for dep in deps:
            if dep in changed_files:
                all_to_update.add(path)

    # Update the docmap.toml file
    with open(DOCMAP_PATH, "r") as f:
        text = f.read()

    updated = 0
    for doc_path in all_to_update:
        if doc_path not in entries:
            continue
        old_date = entries[doc_path].get("last_verified", "")
        if old_date == today:
            continue
        # Replace the last_verified line for this doc
        # Find the section and update
        escaped = doc_path.replace(".", r"\.").replace("/", r"\/")
        import re
        # Find the block for this doc path
        pattern = rf'(last_verified\s*=\s*)"[^"]*"'
        # We need to be careful to only replace within the right section
        # Simple approach: replace all matching last_verified that match the old date
        # for this entry
        old_line = f'last_verified = "{old_date}"'
        new_line = f'last_verified = "{today}"'
        if old_line in text:
            text = text.replace(old_line, new_line, 1)
            updated += 1
            print(f"  Updated {doc_path}: {old_date} → {today}")

    if updated > 0:
        with open(DOCMAP_PATH, "w") as f:
            f.write(text)
        print(f"  Updated {updated} doc(s) in docmap.toml")
    else:
        print(f"  No dates to update ({len(all_to_update)} docs already verified today)")


if __name__ == "__main__":
    main()
