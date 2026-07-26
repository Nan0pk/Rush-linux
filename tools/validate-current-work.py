#!/usr/bin/env python3
"""Validate CURRENT_WORK.md against the canonical optid package ledger."""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
LEDGER_PATH = ROOT / "docs" / "plans" / "optid-package-status.toml"
CURRENT_WORK_PATH = ROOT / "CURRENT_WORK.md"
AGENTS_PATH = ROOT / "AGENTS.md"
README_PATH = ROOT / "README.md"

START_MARKER = "<!-- RUSH_CURRENT_WORK:START -->"
END_MARKER = "<!-- RUSH_CURRENT_WORK:END -->"


class CurrentWorkError(ValueError):
    """The human-readable work selector is missing or contradicts the ledger."""


def load_toml(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise CurrentWorkError(f"cannot load {path.relative_to(ROOT)}: {exc}") from exc


def load_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise CurrentWorkError(f"cannot read {path.relative_to(ROOT)}: {exc}") from exc


def parse_selector(markdown: str) -> dict[str, Any]:
    if markdown.count(START_MARKER) != 1 or markdown.count(END_MARKER) != 1:
        raise CurrentWorkError("CURRENT_WORK.md must contain exactly one marker pair")

    start = markdown.index(START_MARKER) + len(START_MARKER)
    end = markdown.index(END_MARKER, start)
    body = markdown[start:end]
    match = re.fullmatch(r"\s*```toml\s*\n(?P<toml>.*?)\n```\s*", body, re.DOTALL)
    if match is None:
        raise CurrentWorkError("current-work marker must contain one TOML code block")

    try:
        return tomllib.loads(match.group("toml"))
    except tomllib.TOMLDecodeError as exc:
        raise CurrentWorkError(f"CURRENT_WORK.md selector TOML is invalid: {exc}") from exc


def package_map(ledger: dict[str, Any]) -> dict[str, dict[str, Any]]:
    packages = ledger.get("package")
    if not isinstance(packages, list) or not packages:
        raise CurrentWorkError("package ledger has no [[package]] entries")

    by_id: dict[str, dict[str, Any]] = {}
    for package in packages:
        package_id = str(package.get("id", ""))
        if not package_id:
            raise CurrentWorkError("package ledger contains an entry without an id")
        if package_id in by_id:
            raise CurrentWorkError(f"package ledger contains duplicate id {package_id}")
        by_id[package_id] = package
    return by_id


def direct_unlocks(
    by_id: dict[str, dict[str, Any]], active_id: str
) -> list[str]:
    completed = {
        package_id
        for package_id, package in by_id.items()
        if package.get("status") == "completed"
    }
    hypothetical = completed | {active_id}
    unlocked: list[str] = []
    for package_id, package in by_id.items():
        if package.get("status") in {"completed", "blocked"}:
            continue
        dependencies = {str(dep) for dep in package.get("depends", [])}
        if active_id in dependencies and dependencies <= hypothetical:
            unlocked.append(package_id)
    return sorted(unlocked)


def expected_selector(
    ledger: dict[str, Any], by_id: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    active_general = str(ledger.get("active_general", ""))
    active_safety = str(ledger.get("active_safety", ""))
    for key, package_id in (
        ("active_general", active_general),
        ("active_safety", active_safety),
    ):
        if package_id not in by_id:
            raise CurrentWorkError(f"ledger {key} names unknown package {package_id!r}")

    ready_parallel = sorted(
        package_id
        for package_id, package in by_id.items()
        if package.get("status") == "ready_parallel"
    )
    other_merged_incomplete = sorted(
        package_id
        for package_id, package in by_id.items()
        if package.get("status") == "merged_incomplete"
        and package_id not in {active_general, active_safety}
    )

    return {
        "active_general": active_general,
        "active_safety": active_safety,
        "ready_parallel": ready_parallel,
        "other_merged_incomplete": other_merged_incomplete,
        "unlocks_after_active_general": direct_unlocks(by_id, active_general),
        "unlocks_after_active_safety": direct_unlocks(by_id, active_safety),
    }


def validate_projection(actual: dict[str, Any], expected: dict[str, Any]) -> None:
    unknown = sorted(set(actual) - set(expected))
    missing = sorted(set(expected) - set(actual))
    if unknown:
        raise CurrentWorkError(f"selector contains unknown keys: {', '.join(unknown)}")
    if missing:
        raise CurrentWorkError(f"selector is missing keys: {', '.join(missing)}")

    differences = [
        f"{key}: expected {expected[key]!r}, found {actual[key]!r}"
        for key in expected
        if actual[key] != expected[key]
    ]
    if differences:
        raise CurrentWorkError("CURRENT_WORK.md is stale:\n  " + "\n  ".join(differences))


def validate_entry_points(current_work: str) -> None:
    agents = load_text(AGENTS_PATH)
    readme = load_text(README_PATH)

    if "CURRENT_WORK.md" not in agents:
        raise CurrentWorkError("AGENTS.md must link to CURRENT_WORK.md")
    if "CURRENT_WORK.md" not in readme:
        raise CurrentWorkError("README.md must link to CURRENT_WORK.md")
    if "remain the active repair targets" in agents:
        raise CurrentWorkError(
            "AGENTS.md hard-codes active package names; read them from the ledger instead"
        )
    if "active_general" not in agents or "active_safety" not in agents:
        raise CurrentWorkError(
            "AGENTS.md must tell agents to read active_general and active_safety"
        )

    historical_tokens = (
        "docs/plans/agent-work-plan-v1.md",
        "docs/plans/work-plan-v2.md",
        "docs/plans/livedev-progress.json",
        "ROADMAP.md",
        "release/milestones.toml",
    )
    missing = [token for token in historical_tokens if token not in current_work]
    if missing:
        raise CurrentWorkError(
            "CURRENT_WORK.md must classify stale or release-only selectors: "
            + ", ".join(missing)
        )


def main() -> int:
    try:
        ledger = load_toml(LEDGER_PATH)
        by_id = package_map(ledger)
        current_work = load_text(CURRENT_WORK_PATH)
        actual = parse_selector(current_work)
        expected = expected_selector(ledger, by_id)
        validate_projection(actual, expected)
        validate_entry_points(current_work)
    except CurrentWorkError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1

    print("OK: CURRENT_WORK.md matches the optid package ledger and agent entry points.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
