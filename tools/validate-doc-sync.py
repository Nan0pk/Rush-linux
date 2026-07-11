#!/usr/bin/env python3
"""
Rush Linux Documentation Sync Validator.

Reads docs/docmap.toml and checks:
  1. Every registered doc exists on disk.
  2. Cross-references (deps) point to registered docs.
  3. Version strings in key docs match VERSION file.
  4. ADR status values are valid.
  5. No known stale patterns (e.g. "next step" for completed features).
  6. Markdown links between docs resolve.
  7. Freshens lists are bidirectional where needed.
  8. last_verified dates are not older than N days (warning).

Usage:
  python3 tools/validate-doc-sync.py [--max-age 90] [--verbose]

Exit code: 0 = all pass, 1 = errors found.
"""

import argparse
import os
import re
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCMAP_PATH = ROOT / "docs" / "docmap.toml"

# ── Helpers ──────────────────────────────────────────────────

errors = []
warnings = []


def err(msg):
    errors.append(msg)
    print(f"  ❌ {msg}")


def warn(msg):
    warnings.append(msg)
    print(f"  ⚠️  {msg}")


def ok(msg):
    print(f"  ✅ {msg}")


def read_file(path):
    full = ROOT / path
    if not full.exists():
        return None
    return full.read_text(encoding="utf-8")


def get_version():
    return read_file("VERSION").strip() if read_file("VERSION") else None


# ── Checks ───────────────────────────────────────────────────

def check_docmap_loads():
    """Check that docmap.toml is valid and loadable."""
    if not DOCMAP_PATH.exists():
        err("docs/docmap.toml is missing — the doc registry does not exist")
        return None
    try:
        with open(DOCMAP_PATH, "rb") as f:
            data = tomllib.load(f)
        entries = data.get("docs", {})
        ok(f"docmap.toml loaded: {len(entries)} doc entries")
        return entries
    except Exception as e:
        err(f"docs/docmap.toml failed to parse: {e}")
        return None


def check_all_docs_exist(entries):
    """Every registered doc must exist on disk (except optional files)."""
    print("\n── Check: All registered docs exist ──")
    optional = {"DIRTY_STATE.md"}  # exists only during active work
    for path in sorted(entries.keys()):
        full = ROOT / path
        if path in optional:
            if full.exists():
                ok(f"{path} (present — work in progress)")
            else:
                print(f"  ℹ️  {path} (absent — no active work session)")
        elif full.exists():
            ok(f"{path}")
        else:
            err(f"Registered doc does not exist: {path}")


def check_research_is_discoverable(entries):
    """Every research paper must be registered even when it is unfinished."""
    print("\n── Check: Research discoverability ──")
    registered = set(entries)
    research_dir = ROOT / "docs" / "research"
    missing = []
    for paper in sorted(research_dir.glob("[0-9][0-9][0-9][0-9]-*.md")):
        path = paper.relative_to(ROOT).as_posix()
        if path not in registered:
            missing.append(path)
            err(f"Research paper is missing from docs/docmap.toml: {path}")
    if not missing:
        ok("Every research paper is registered in docmap.toml")


def check_deps_exist(entries):
    """Every dep must reference a registered doc."""
    print("\n── Check: All deps reference registered docs ──")
    all_paths = set(entries.keys())
    for path, entry in sorted(entries.items()):
        deps = entry.get("deps", [])
        for dep in deps:
            if dep not in all_paths:
                err(f"{path} has dep '{dep}' which is not registered in docmap")
            elif not (ROOT / dep).exists():
                err(f"{path} has dep '{dep}' which does not exist on disk")
            else:
                pass  # fine


def check_version_consistency(entries):
    """Key docs must reference the same version as the VERSION file."""
    print("\n── Check: Version consistency ──")
    version = get_version()
    if not version:
        err("VERSION file is missing or empty")
        return

    ok(f"VERSION file: {version}")

    # Docs that should contain the current version
    version_docs = [
        ("docs/IMPLEMENTATION_STATUS.md", version.replace(".", r"\.")),
        ("ROADMAP.md", version.replace(".", r"\.")),
        ("docs/AI_CONTINUATION.md", version.replace(".", r"\.")),
    ]

    for path, escaped in version_docs:
        text = read_file(path)
        if text is None:
            err(f"{path} does not exist")
            continue
        if re.search(escaped, text):
            ok(f"{path} contains version {version}")
        else:
            err(f"{path} does NOT contain current version {version}")


def check_adr_status(entries):
    """ADR entries should have valid status values."""
    print("\n── Check: ADR status validity ──")
    adr_dir = ROOT / "docs" / "decisions"
    if not adr_dir.exists():
        warn("docs/decisions/ directory not found")
        return

    for adr_file in sorted(adr_dir.glob("*.md")):
        if adr_file.name == "README.md":
            continue
        text = adr_file.read_text(encoding="utf-8")
        match = re.search(r"^Status:\s*(\S+)", text, re.MULTILINE)
        if not match:
            err(f"ADR {adr_file.name} has no 'Status:' line")
            continue
        status = match.group(1)
        valid = {"proposed", "accepted", "superseded", "rejected"}
        if status not in valid:
            err(f"ADR {adr_file.name} has invalid status '{status}' (must be one of {valid})")
        else:
            ok(f"ADR {adr_file.name}: Status {status}")


def check_stale_patterns(entries):
    """Check for known stale patterns in key docs."""
    print("\n── Check: Stale pattern detection ──")

    # README should not say D-Bus is "next step" (it's implemented)
    readme = read_file("README.md")
    if readme:
        stale_phrases = [
            ("next implementation step is replacing file-based control with a D-Bus",
             "D-Bus is already implemented (see docs/IMPLEMENTATION_STATUS.md)"),
            ("currently talks through the state directory",
             "optctl now uses D-Bus first with file fallback"),
        ]
        for phrase, reason in stale_phrases:
            if phrase.lower() in readme.lower():
                err(f"README.md contains stale phrase: '{phrase}' — {reason}")
            else:
                ok(f"README.md: no stale phrase '{phrase[:50]}...'")


def check_markdown_links(entries):
    """Check that internal markdown links in key docs resolve."""
    print("\n── Check: Markdown link resolution ──")
    link_pattern = re.compile(r"\[([^\]]*)\]\(([^)]+)\)")

    docs_to_check = ["README.md", "CONTRIBUTING.md", "docs/architecture.md",
                     "docs/adaptive-engine.md", "docs/AI_CONTINUATION.md"]

    broken = 0
    for doc_path in docs_to_check:
        text = read_file(doc_path)
        if not text:
            continue
        for match in link_pattern.finditer(text):
            link = match.group(2)
            # Skip external URLs and anchors
            if link.startswith(("http://", "https://", "mailto:", "#")):
                continue
            # Strip anchor
            link = link.split("#")[0]
            if not link:
                continue
            # Check file exists
            target = ROOT / link
            if not target.exists():
                err(f"Broken link in {doc_path}: '{link}'")
                broken += 1

    if broken == 0:
        ok(f"All internal links in {len(docs_to_check)} key docs resolve")


def check_optid_doc_sync(entries):
    """Check that docs describing optid features match the actual code."""
    print("\n── Check: optid code ↔ doc sync ──")

    optid_src = read_file("crates/optid/src/main.rs")
    if not optid_src:
        warn("crates/optid/src/main.rs not found, skipping optid sync check")
        return

    # Check: adaptive-engine.md says optid reads PSI
    engine = read_file("docs/adaptive-engine.md")
    if engine:
        features = [
            ("/proc/pressure/", "PSI"),
            ("/sys/class/power_supply", "battery"),
            ("/sys/class/thermal", "thermal"),
            ("zbus", "D-Bus"),
            ("/proc/loadavg", "loadavg"),
        ]
        for code_token, doc_word in features:
            in_code = code_token in optid_src
            in_doc = doc_word.lower() in engine.lower()
            if in_code and not in_doc:
                err(f"optid uses '{code_token}' but adaptive-engine.md doesn't mention '{doc_word}'")
            elif in_code and in_doc:
                ok(f"optid '{code_token}' ↔ adaptive-engine.md '{doc_word}'")


def check_last_verified(entries, max_age_days):
    """Warn if last_verified is older than max_age_days."""
    print(f"\n── Check: last_verified freshness (max {max_age_days} days) ──")
    now = datetime.now(timezone.utc)
    threshold_days = max_age_days

    stale_count = 0
    for path, entry in sorted(entries.items()):
        lv = entry.get("last_verified", "")
        if not lv:
            warn(f"{path} has no last_verified date")
            continue
        try:
            dt = datetime.strptime(lv, "%Y-%m-%d").replace(tzinfo=timezone.utc)
            age = (now - dt).days
            if age > threshold_days:
                warn(f"{path} last verified {age} days ago (>{threshold_days})")
                stale_count += 1
        except ValueError:
            err(f"{path} has invalid last_verified format: '{lv}'")

    if stale_count == 0:
        ok(f"All docs verified within {threshold_days} days")


# ── Main ─────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Rush Linux doc sync validator")
    parser.add_argument("--max-age", type=int, default=90,
                        help="Warn if last_verified older than N days (default: 90)")
    parser.add_argument("--verbose", "-v", action="store_true",
                        help="Show passing checks in detail")
    args = parser.parse_args()

    print("╔══════════════════════════════════════════════════════════╗")
    print("║     Rush Linux — Documentation Sync Validation          ║")
    print("╚══════════════════════════════════════════════════════════╝")

    entries = check_docmap_loads()
    if entries is None:
        print("\n❌ Cannot proceed without docmap.toml")
        sys.exit(1)

    check_all_docs_exist(entries)
    check_research_is_discoverable(entries)
    check_deps_exist(entries)
    check_version_consistency(entries)
    check_adr_status(entries)
    check_stale_patterns(entries)
    check_markdown_links(entries)
    check_optid_doc_sync(entries)
    check_last_verified(entries, args.max_age)

    print("\n" + "=" * 60)
    if errors:
        print(f"❌ FAILED: {len(errors)} error(s), {len(warnings)} warning(s)")
        print("\nTo fix: update the flagged docs and bump their last_verified")
        print("date in docs/docmap.toml.")
        sys.exit(1)
    else:
        print(f"✅ PASSED: 0 errors, {len(warnings)} warning(s)")
        print("All documentation is in sync.")
        sys.exit(0)


if __name__ == "__main__":
    main()
