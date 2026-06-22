#!/usr/bin/env python3
"""
Dragnet — the project's recurring evidence-integrity sweep.

`python3 tools/dragnet.py --observe` is a READ-ONLY audit: it runs the
validator suite, summarises the evidence state of every milestone, and writes a
dated run report under release/evidence/dragnet/. It never edits audited files,
milestone flags, or code; any fix it surfaces goes through the normal PR
lifecycle.

A run is "green" when:
  1. every validator exits 0, AND
  2. no milestone marked status = "complete" has an unverified criterion.

Both conditions gate a milestone close or a VERSION bump.

Usage:
  python3 tools/dragnet.py --observe          # run sweep, write report
  python3 tools/dragnet.py --observe --no-report
  python3 tools/dragnet.py --observe --run 2  # force run number
"""

import argparse
import datetime as dt
import subprocess
import sys
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parent.parent
DRAGNET_DIR = ROOT / "release" / "evidence" / "dragnet"
MILESTONES = ROOT / "release" / "milestones.toml"

VALIDATORS = [
    ("evidence integrity", ["python3", "tools/validate-evidence.py"]),
    ("version consistency", ["python3", "tools/validate-versions.py"]),
    ("documentation sync", ["python3", "tools/validate-doc-sync.py", "--max-age", "90"]),
]


def run_validator(cmd: list[str]) -> tuple[bool, str]:
    proc = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    return proc.returncode == 0, (proc.stdout + proc.stderr).strip()


def milestone_summary() -> tuple[list[str], list[str]]:
    """Return (lines, problems). `problems` is non-empty if a 'complete'
    milestone has any unverified criterion."""
    lines: list[str] = []
    problems: list[str] = []
    if not MILESTONES.exists():
        return ["(milestones.toml missing)"], ["milestones.toml missing"]
    with MILESTONES.open("rb") as f:
        data = tomllib.load(f)
    for ms in data.get("milestone", []):
        crit = ms.get("criteria_status", [])
        if not crit:
            continue
        ver = ms.get("version", "?")
        status = ms.get("status", "(unset)")
        total = len(crit)
        verified = sum(1 for c in crit if c.get("verified"))
        with_tx = sum(1 for c in crit if c.get("transcript"))
        lines.append(
            f"- **{ver}** (status={status}): {verified}/{total} verified, "
            f"{with_tx}/{total} with committed transcript"
        )
        if status == "complete" and verified < total:
            problems.append(
                f"{ver} is status=complete but only {verified}/{total} criteria verified"
            )
    return lines, problems


def next_run_number() -> int:
    if not DRAGNET_DIR.exists():
        return 1
    existing = list(DRAGNET_DIR.glob("DRAGNET-*.md"))
    return len(existing) + 1


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--observe", action="store_true", help="run the read-only sweep")
    ap.add_argument("--no-report", action="store_true", help="do not write a report file")
    ap.add_argument("--run", type=int, default=None, help="force run number")
    args = ap.parse_args()
    if not args.observe:
        ap.print_help()
        sys.exit(2)

    print("=" * 60)
    print("Dragnet — Evidence-Integrity Sweep (observe / read-only)")
    print("=" * 60)

    results = []
    all_pass = True
    for name, cmd in VALIDATORS:
        ok, out = run_validator(cmd)
        all_pass = all_pass and ok
        print(f"\n[{'PASS' if ok else 'FAIL'}] {name}: {' '.join(cmd)}")
        results.append((name, ok, out))

    ms_lines, ms_problems = milestone_summary()
    green = all_pass and not ms_problems

    print("\n" + "-" * 60)
    print("Milestone evidence summary:")
    for ln in ms_lines:
        print("  " + ln.replace("**", ""))
    print("-" * 60)
    print(f"VERDICT: {'GREEN' if green else 'RED'}")
    if ms_problems:
        for p in ms_problems:
            print(f"  ✗ {p}")

    if not args.no_report:
        DRAGNET_DIR.mkdir(parents=True, exist_ok=True)
        run = args.run if args.run is not None else next_run_number()
        date = dt.date.today().isoformat()
        report = DRAGNET_DIR / f"DRAGNET-{run:03d}-{date}.md"
        body = [
            f"# Dragnet-{run:03d} — {date}",
            "",
            "Read-only evidence-integrity sweep (`tools/dragnet.py --observe`).",
            "",
            f"**Verdict: {'GREEN' if green else 'RED'}**",
            "",
            "## Validators",
            "",
        ]
        for name, ok, out in results:
            body.append(f"### {name} — {'PASS' if ok else 'FAIL'}")
            body.append("")
            body.append("```")
            body.append(out or "(no output)")
            body.append("```")
            body.append("")
        body.append("## Milestone evidence summary")
        body.append("")
        body.extend(ms_lines)
        body.append("")
        if ms_problems:
            body.append("## Problems")
            body.append("")
            body.extend(f"- {p}" for p in ms_problems)
            body.append("")
        body.append("See `release/evidence/dragnet/LEDGER.md` for the per-criterion "
                     "debt ledger and `LESSONS.md` for recurrence countermeasures.")
        body.append("")
        report.write_text("\n".join(body), encoding="utf-8")
        print(f"\nReport written: {report.relative_to(ROOT)}")

    sys.exit(0 if green else 1)


if __name__ == "__main__":
    main()
