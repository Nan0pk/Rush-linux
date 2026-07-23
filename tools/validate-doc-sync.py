#!/usr/bin/env python3
"""
Rush Linux Documentation Sync Validator.

Reads docs/docmap.toml and checks:
  1. Every active mapped doc exists on disk.
  2. Cross-references (deps) resolve on disk.
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
        ok(f"docmap.toml loaded: {len(entries)} active doc entries")
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
    """Every numbered paper must be linked from the research index."""
    print("\n── Check: Research discoverability ──")
    research_dir = ROOT / "docs" / "research"
    index_path = research_dir / "README.md"
    if not index_path.exists():
        err("docs/research/README.md is missing")
        return
    index = index_path.read_text(encoding="utf-8")
    missing = []
    for paper in sorted(research_dir.glob("[0-9][0-9][0-9][0-9]-*.md")):
        if paper.name not in index:
            missing.append(paper.name)
            err(f"Research paper is missing from docs/research/README.md: {paper.name}")
    if not missing:
        ok("Every numbered research paper is indexed")


def check_deps_exist(entries):
    """Every declared dependency must resolve; not every dependency is active."""
    print("\n── Check: All active-doc dependencies resolve ──")
    for path, entry in sorted(entries.items()):
        deps = entry.get("deps", [])
        for dep in deps:
            if not (ROOT / dep).exists():
                err(f"{path} has dep '{dep}' which does not exist on disk")


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
            ("code-complete; Phase D hardware-gated",
             "optid capability construction remains active; hardware gates promotion claims"),
            ("restores every knob it touched",
             "current recovery is not yet the persistent verified D2 protocol"),
            ("Nothing is permanently changed on your system",
             "apply mode cannot promise universal crash/power-loss recovery yet"),
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

    docs_to_check = [
        "AGENTS.md",
        "OPTID-COMPLETION-PLAN.md",
        "README.md",
        "CONTRIBUTING.md",
        "docs/IMPLEMENTATION_STATUS.md",
        "docs/SUMMARY.md",
        "docs/research/README.md",
        "docs/adaptive-engine.md",
        "docs/architecture.md",
        "docs/architecture/optid-d2-amendment.md",
        "docs/project-workflow.md",
    ]

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
            target = (ROOT / doc_path).parent / link
            if not target.exists():
                err(f"Broken link in {doc_path}: '{link}'")
                broken += 1

    if broken == 0:
        ok(f"All internal links in {len(docs_to_check)} key docs resolve")


def check_optid_plan_activation(entries):
    """Keep the human-facing plan and machine-readable work selector aligned."""
    print("\n── Check: active optid plan and package ledger ──")

    plan = read_file("OPTID-COMPLETION-PLAN.md")
    amendment = read_file("docs/architecture/optid-d2-amendment.md")
    readme = read_file("README.md")
    constitution = read_file("AGENTS.md")
    ledger_path = ROOT / "docs" / "plans" / "optid-package-status.toml"
    ledger_text = read_file("docs/plans/optid-package-status.toml")

    if not plan or not amendment or not readme or not constitution or not ledger_text or not ledger_path.exists():
        err("active optid plan, D2 amendment, README, or package ledger is missing")
        return

    try:
        with ledger_path.open("rb") as f:
            ledger = tomllib.load(f)
    except Exception as e:
        err(f"optid package ledger failed to parse: {e}")
        return

    provenance = "all other packages carry forward the completion plan merged in PR #322"
    if provenance not in ledger_text:
        err("optid package ledger does not identify PR #322 as the source of non-safety packages")
    else:
        ok("optid package ledger records PR #322 provenance for non-safety packages")

    worker_reading_tokens = (
        "Reading order for safety-lane packets",
        "it is the actionable summary",
        "long-form research only when the packet needs deeper justification",
    )
    if any(token not in plan for token in worker_reading_tokens):
        err("optid worker contract does not require amendment-first, reference-on-demand reading")
    elif "For safety work, read the amendment first." not in constitution:
        err("AGENTS.md does not mirror the amendment-first reading order")
    else:
        ok("worker contract and AGENTS.md require amendment-first, reference-on-demand reading")

    packages = ledger.get("package", [])
    ids = [p.get("id") for p in packages]
    known = set(ids)
    valid_statuses = {
        "next",
        "ready_parallel",
        "planned",
        "candidate",
        "merged_incomplete",
        "completed",
        "blocked",
    }

    if len(packages) != 30:
        err(f"optid package ledger must contain 30 active packages, found {len(packages)}")
    elif len(known) != len(ids):
        err("optid package ledger contains duplicate package IDs")
    else:
        ok("optid package ledger contains 30 unique packages")

    for package in packages:
        package_id = package.get("id", "<missing>")
        status = package.get("status")
        if status not in valid_statuses:
            err(f"optid package {package_id} has invalid status {status!r}")
        for dep in package.get("depends", []):
            if dep not in known:
                err(f"optid package {package_id} depends on unknown package {dep}")
            if dep == package_id:
                err(f"optid package {package_id} depends on itself")

    by_id = {p.get("id"): p for p in packages}
    actionable_statuses = {"next", "ready_parallel", "candidate", "merged_incomplete"}
    for key in ("active_general", "active_safety"):
        actual = ledger.get(key)
        if actual not in by_id:
            err(f"optid ledger {key} names unknown package {actual!r}")
        elif by_id[actual].get("status") not in actionable_statuses:
            err(
                f"optid ledger {key}={actual} is not actionable "
                f"(status {by_id[actual].get('status')!r})"
            )
        else:
            ok(f"{key} is {actual} ({by_id[actual].get('status')})")

    if ledger.get("safety_architecture") != "D2-fail-passive":
        err("optid ledger must record safety_architecture = D2-fail-passive")
    elif "**Status:** Active" not in plan:
        err("OPTID-COMPLETION-PLAN.md is not marked Active")
    elif any(heading in plan for heading in ("### S1 —", "### S2 —", "### S3 —")):
        err("OPTID-COMPLETION-PLAN.md still contains superseded S1-S3 package headings")
    elif "**Status:** Accepted owner direction" not in amendment:
        err("D2 amendment is not marked as accepted owner direction")
    else:
        ok("D2 is accepted and superseded S1-S3 package headings are absent")

    active_general = str(ledger.get("active_general", ""))
    active_safety = str(ledger.get("active_safety", ""))
    merged_incomplete = sorted(
        package_id
        for package_id, package in by_id.items()
        if package.get("status") == "merged_incomplete"
        and package_id not in {active_general, active_safety}
    )
    required_readme = [
        f"**Active general repair:** `{active_general}`",
        f"**Active safety repair:** `{active_safety}`",
        "docs/architecture/optid-d2-amendment.md",
        "docs/plans/optid-package-status.toml",
    ]
    if merged_incomplete:
        required_readme.append(
            "**Other merged but incomplete packages:** "
            + ", ".join(f"`{package_id}`" for package_id in merged_incomplete)
        )
    missing = [token for token in required_readme if token not in readme]
    if missing:
        err(f"README is missing active optid truth: {', '.join(missing)}")
    else:
        ok("README derives active repairs, incomplete packages, D2, and the ledger")


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


def check_adr_citations_resolve(entries) -> None:
    """Any `docs/decisions/NNNN-*.md` citation in any tracked doc or plan
    must resolve to a file on disk. Catches the ADR 0019-0022 gap class
    (plan doc references an ADR that was never written).
    """
    print("\n── Check: ADR citations resolve ──")
    import re as _re
    # Build the set of ADR files that actually exist.
    decisions_dir = ROOT / "docs" / "decisions"
    existing_adrs: set[str] = set()
    if decisions_dir.exists():
        for p in decisions_dir.glob("*.md"):
            if p.name == "README.md":
                continue
            existing_adrs.add(p.name)

    # Regex: matches `docs/decisions/NNNN-slug.md` in free text / lists / code.
    citation_re = _re.compile(r"docs/decisions/(\d{4}-[A-Za-z0-9_\-]+\.md)")

    # Scan every markdown / toml / json file under docs/ + release/. Skip the
    # decisions/ directory itself (an ADR citing another ADR is fine and is
    # resolved by check_deps_exist).
    scan_dirs = [ROOT / "docs", ROOT / "release"]
    scan_files: list[Path] = []
    for d in scan_dirs:
        if d.exists():
            scan_files.extend(p for p in d.rglob("*") if p.is_file() and p.suffix in (".md", ".toml", ".json"))
    bad = 0
    # Phrases that indicate a *forward* reference (ADR not yet written, on
    # purpose). Skip citations on lines containing these phrases.
    forward_phrases = (
        "new adr", "to be written", "optional: write", "optional: docs/decisions",
        "planned adr", "future adr", "(proposed)", "`proposed`",
    )
    for f in sorted(scan_files):
        if "docs/decisions" in str(f):
            continue  # don't recurse into the decisions dir
        try:
            text = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for m in citation_re.finditer(text):
            cited = m.group(1)
            if cited in existing_adrs:
                continue
            # Is this a forward reference? Look at the line containing the match.
            line_start = text.rfind("\n", 0, m.start()) + 1
            line_end = text.find("\n", m.end())
            if line_end < 0:
                line_end = len(text)
            line = text[line_start:line_end].lower()
            if any(p in line for p in forward_phrases):
                continue
            rel = f.relative_to(ROOT) if f.is_relative_to(ROOT) else f
            err(f"{rel}: cites docs/decisions/{cited} which does not exist")
            bad += 1
    if bad == 0:
        ok("All ADR citations in docs/ resolve")


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
    check_optid_plan_activation(entries)
    check_optid_doc_sync(entries)
    check_last_verified(entries, args.max_age)
    check_adr_citations_resolve(entries)

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
