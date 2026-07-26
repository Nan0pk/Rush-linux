#!/usr/bin/env python3
"""Render the small, practical README section from live repository truth."""

from __future__ import annotations

import argparse
import difflib
import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
README_PATH = ROOT / "README.md"
MANIFEST_PATH = ROOT / "docs" / "frontpage" / "project.toml"
LEDGER_PATH = ROOT / "docs" / "plans" / "optid-package-status.toml"
VERSION_PATH = ROOT / "VERSION"
PROFILES_PATH = ROOT / "mkosi" / "mkosi.profiles"

START_MARKER = "<!-- RUSH_FRONTPAGE:START -->"
END_MARKER = "<!-- RUSH_FRONTPAGE:END -->"


class FrontpageError(ValueError):
    """The canonical front-page inputs are missing or inconsistent."""


def _load_toml(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise FrontpageError(f"cannot load {path.relative_to(ROOT)}: {exc}") from exc


def _manifest() -> dict[str, Any]:
    data = _load_toml(MANIFEST_PATH)
    if data.get("schema_version") != 2:
        raise FrontpageError("docs/frontpage/project.toml must use schema_version = 2")

    commands = data.get("command")
    if not isinstance(commands, list) or not commands:
        raise FrontpageError("front-page manifest must define at least one [[command]]")

    seen: set[str] = set()
    required = {"id", "title", "platform", "language", "command", "targets", "note"}
    for item in commands:
        missing = sorted(required - set(item))
        if missing:
            raise FrontpageError(f"front-page command is missing: {', '.join(missing)}")
        command_id = str(item["id"])
        if command_id in seen:
            raise FrontpageError(f"duplicate front-page command id: {command_id}")
        seen.add(command_id)
        targets = item["targets"]
        if not isinstance(targets, list) or not targets:
            raise FrontpageError(f"{command_id}: targets must be a non-empty list")
        command_text = str(item["command"])
        for target in targets:
            target_path = ROOT / str(target)
            if not target_path.is_file():
                raise FrontpageError(f"{command_id}: target does not exist: {target}")
            if str(target) not in command_text:
                raise FrontpageError(
                    f"{command_id}: command does not reference declared target: {target}"
                )

    for name, target in data.get("links", {}).items():
        if not (ROOT / str(target)).is_file():
            raise FrontpageError(f"link {name!r} does not exist: {target}")
    return data


def _ledger() -> tuple[dict[str, Any], dict[str, dict[str, Any]]]:
    data = _load_toml(LEDGER_PATH)
    packages = data.get("package", [])
    by_id = {str(item.get("id")): item for item in packages if item.get("id")}
    for key in ("active_general", "active_safety"):
        if str(data.get(key, "")) not in by_id:
            raise FrontpageError(f"ledger {key} does not name a package")
    return data, by_id


def _editions() -> list[str]:
    if not PROFILES_PATH.is_dir():
        raise FrontpageError("mkosi/mkosi.profiles is missing")
    editions = sorted(
        path.parent.name for path in PROFILES_PATH.glob("*/mkosi.conf") if path.is_file()
    )
    if not editions:
        raise FrontpageError("no mkosi edition profiles found")
    return editions


def _status_lines() -> list[str]:
    manifest = _manifest()
    ledger, by_id = _ledger()
    version = VERSION_PATH.read_text(encoding="utf-8").strip()
    if not version:
        raise FrontpageError("VERSION is empty")

    general_id = str(ledger["active_general"])
    safety_id = str(ledger["active_safety"])
    incomplete = sorted(
        package_id
        for package_id, package in by_id.items()
        if package.get("status") == "merged_incomplete"
        and package_id not in {general_id, safety_id}
    )
    stage = str(manifest.get("project", {}).get("stage", "")).strip()

    lines = [
        "## Repository status",
        "",
        "This table is generated from the repository's canonical version, build, "
        "and work-state files.",
        "",
        "| Item | Current state |",
        "| --- | --- |",
        f"| Project stage | {stage} |",
        f"| Version | `{version}` |",
        (
            f"| Active general repair | `{general_id}` — "
            f"{by_id[general_id]['title']} (`{by_id[general_id]['status']}`) |"
        ),
        (
            f"| Active safety repair | `{safety_id}` — "
            f"{by_id[safety_id]['title']} (`{by_id[safety_id]['status']}`) |"
        ),
    ]
    if incomplete:
        lines.append(
            "| Other merged, incomplete packages | "
            + ", ".join(f"`{package_id}`" for package_id in incomplete)
            + " |"
        )
    links = manifest["links"]
    lines.extend(
        [
            (
                "| Build profiles | "
                + ", ".join(f"`{name}`" for name in _editions())
                + " |"
            ),
            (
                f"| Safety architecture | "
                f"[D2 fail-passive]({links['architecture']}) |"
            ),
            (
                f"| Canonical work state | "
                f"[optid package ledger]({links['ledger']}) |"
            ),
        ]
    )
    return lines


def render_section() -> str:
    manifest = _manifest()
    commands = manifest["command"]
    quickstart = next(
        (item for item in commands if item["id"] == "livedev-posix"),
        None,
    )
    if quickstart is None:
        raise FrontpageError("front-page manifest must define livedev-posix")

    lines = [
        f'<a id="command-{quickstart["id"]}"></a>',
        "## Rush LiveDev quick start",
        "",
        f"**Environment:** {quickstart['platform']}",
        "",
        f"```{quickstart['language']}",
        str(quickstart["command"]),
        "```",
        "",
        str(quickstart["note"]),
        "",
    ]
    lines.extend(_status_lines())
    lines.extend(
        [
            "",
            "## Choose a command",
            "",
            "Pick the goal that matches what you want to do. Detailed options stay "
            "in the linked runbooks.",
            "",
            "| Goal | Environment |",
            "| --- | --- |",
        ]
    )
    for item in commands:
        lines.append(
            f"| [{item['title']}](#command-{item['id']}) | {item['platform']} |"
        )
    lines.extend(["", "## Other command details"])
    for item in commands:
        if item["id"] == quickstart["id"]:
            continue
        lines.extend(
            [
                "",
                f'<a id="command-{item["id"]}"></a>',
                f"### {item['title']}",
                "",
                f"**Environment:** {item['platform']}",
                "",
                f"```{item['language']}",
                str(item["command"]),
                "```",
                "",
                str(item["note"]),
            ]
        )
    return "\n".join(lines).rstrip() + "\n"


def _split(readme: str) -> tuple[str, str, str]:
    start = readme.find(START_MARKER)
    end = readme.find(END_MARKER)
    if start < 0 or end < 0 or end < start:
        raise FrontpageError("README.md is missing the generated front-page markers")
    before = readme[:start]
    generated = readme[start + len(START_MARKER) : end]
    after = readme[end + len(END_MARKER) :]
    return before, generated, after


def render_full_readme() -> str:
    current = README_PATH.read_text(encoding="utf-8")
    before, _generated, after = _split(current)
    return f"{before}{START_MARKER}\n{render_section()}\n{END_MARKER}{after}"


def check() -> int:
    current = README_PATH.read_text(encoding="utf-8")
    expected = render_full_readme()
    if current == expected:
        print("OK: README practical guide matches repository truth.")
        return 0
    print("ERROR: README practical guide is stale.", file=sys.stderr)
    print("Run: python3 tools/render-frontpage.py --write", file=sys.stderr)
    diff = difflib.unified_diff(
        current.splitlines(),
        expected.splitlines(),
        fromfile="README.md",
        tofile="README.md (expected)",
        lineterm="",
    )
    print("\n".join(diff), file=sys.stderr)
    return 1


def write() -> int:
    README_PATH.write_text(render_full_readme(), encoding="utf-8")
    print("OK: README practical guide regenerated.")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.check:
            return check()
        if args.write:
            return write()
        print(render_section(), end="")
        return 0
    except FrontpageError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
