#!/usr/bin/env python3
"""
Rush Linux — Dirty State Validator.

Checks the DIRTY_STATE.md file for completeness. Runs in CI as a
non-blocking warning (dirty state is expected during active work),
but catches missing required fields.

Usage:
  python3 tools/validate-dirty-state.py   # exit 0 always, prints warnings
"""

import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DIRTY_FILE = ROOT / "DIRTY_STATE.md"

def main():
    if not DIRTY_FILE.exists():
        print("✅ No DIRTY_STATE.md — repo is clean.")
        return

    print("⚠️  DIRTY_STATE.md exists — work in progress detected.")
    print("")

    text = DIRTY_FILE.read_text()
    required_fields = ["Started:", "Agent/Person:", "Task:", "What's done so far:", "What's left:"]
    filled = 0
    empty = 0
    for field in required_fields:
        for line in text.split("\n"):
            if field in line:
                value = line.split(field, 1)[1].strip()
                if value and value != "(not set)":
                    print(f"  ✅ {field} {value}")
                    filled += 1
                else:
                    print(f"  ❌ {field} NOT SET")
                    empty += 1
                break

    if empty > 0:
        print(f"\n  ⚠️  {empty} field(s) not filled in. The next agent won't know what was happening.")
        print("  Run 'bash tools/start-work.sh' to populate, or edit DIRTY_STATE.md manually.")
    else:
        print(f"\n  ✅ All fields populated. Next agent can resume work.")

if __name__ == "__main__":
    main()
