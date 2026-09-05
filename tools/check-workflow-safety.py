#!/usr/bin/env python3
"""Keep unattended builders/collectors from merging their own submissions.

Delegated coordinators use the protected GitHub interface after actual separate
agent review (docs/agent-protocol.md). This lexical scan checks repository
scripts; it neither authenticates reviewers nor certifies a merge.
"""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
SEARCH_ROOTS = [ROOT / ".github" / "workflows", ROOT / "tools", ROOT / "testos"]
SKIP_NAMES = {"check-workflow-safety.py"}
PATTERNS = {
    "GitHub CLI merge": re.compile(r"\bgh\s+pr\s+merge\b"),
    "GitHub merge API": re.compile(r"/pulls/(?:\$\{?[^/}]+\}?|\{[^}]+\})/merge"),
    "auto-merge action": re.compile(r"\benable[_ -]?auto[_ -]?merge\b", re.I),
}


def main() -> int:
    failures: list[str] = []
    for root in SEARCH_ROOTS:
        if not root.exists():
            continue
        for path in root.rglob("*"):
            if not path.is_file() or path.name in SKIP_NAMES or path.name.startswith("test-"):
                continue
            try:
                lines = path.read_text(encoding="utf-8").splitlines()
            except UnicodeDecodeError:
                continue
            for number, line in enumerate(lines, 1):
                # Safety tools legitimately keep forbidden commands as quoted
                # data so they can reject them. A standalone Python string is
                # not executable automation.
                if path.suffix == ".py" or path.name in {"rush-autopilot", "rush-agent"}:
                    if re.match(r"^\s*['\"].*['\"],?\s*(?:#.*)?$", line):
                        continue
                for label, pattern in PATTERNS.items():
                    if pattern.search(line):
                        failures.append(f"{path.relative_to(ROOT)}:{number}: {label}")

    if failures:
        print("BLOCKED: a repository script can merge without the coordinating review process.")
        print("Risk: unreviewed work can enter main and alter release truth.")
        print("Root: AGENTS.md section 13 and the LiveDev/testOS safety incidents.")
        print("Ways forward: submit a PR; the coordinator obtains independent review and uses the protected GitHub interface.")
        for failure in failures:
            print(f"  - {failure}")
        return 1

    print("OK: no merge command in unattended repository automation; delegated review is governed by docs/agent-protocol.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
