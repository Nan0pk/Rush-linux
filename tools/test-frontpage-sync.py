#!/usr/bin/env python3
"""Tests for the practical README generator and documentation-impact gate."""

from __future__ import annotations

import importlib.machinery
import importlib.util
import re
import sys
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
ROOT = TOOLS.parent


def _load(name: str, path: Path):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    loader.exec_module(module)
    return module


frontpage = _load("render_frontpage", TOOLS / "render-frontpage.py")
docs_impact = _load("check_docs_impact", TOOLS / "check-docs-impact.py")


def test_generated_output_is_deterministic():
    assert frontpage.render_section() == frontpage.render_section()


def test_manifest_commands_point_to_real_targets():
    manifest = frontpage._manifest()
    for item in manifest["command"]:
        assert item["targets"]
        for target in item["targets"]:
            assert (ROOT / target).is_file()
            assert target in item["command"]


def test_generated_status_comes_from_live_sources():
    output = frontpage.render_section()
    ledger, packages = frontpage._ledger()
    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    assert f"`{version}`" in output
    assert f"`{ledger['active_general']}`" in output
    assert f"`{ledger['active_safety']}`" in output
    for package_id, package in packages.items():
        if package["status"] == "merged_incomplete":
            assert f"`{package_id}`" in output


def test_generated_status_lists_every_build_profile():
    output = frontpage.render_section()
    for edition in frontpage._editions():
        assert f"`{edition}`" in output


def test_generated_command_index_has_stable_targets():
    output = frontpage.render_section()
    for item in frontpage._manifest()["command"]:
        target = f"command-{item['id']}"
        assert f"(#{target})" in output
        assert f'<a id="{target}"></a>' in output


def test_readme_command_links_have_targets():
    readme = frontpage.README_PATH.read_text(encoding="utf-8")
    links = set(re.findall(r"\(#(command-[^)]+)\)", readme))
    targets = set(re.findall(r'<a id="(command-[^"]+)"></a>', readme))
    assert links
    assert links <= targets


def test_readme_is_in_sync():
    assert frontpage.check() == 0


def test_check_detects_stale_generated_block(tmp_path, monkeypatch):
    source = frontpage.README_PATH.read_text(encoding="utf-8")
    stale = source.replace(
        frontpage.START_MARKER,
        frontpage.START_MARKER + "\nSTALE",
        1,
    )
    candidate = tmp_path / "README.md"
    candidate.write_text(stale, encoding="utf-8")
    monkeypatch.setattr(frontpage, "README_PATH", candidate)
    assert frontpage.check() == 1


def test_write_repairs_stale_generated_block(tmp_path, monkeypatch):
    source = frontpage.README_PATH.read_text(encoding="utf-8")
    candidate = tmp_path / "README.md"
    candidate.write_text(source.replace(frontpage.END_MARKER, "STALE\n" + frontpage.END_MARKER), encoding="utf-8")
    monkeypatch.setattr(frontpage, "README_PATH", candidate)
    assert frontpage.write() == 0
    assert frontpage.check() == 0


def test_docs_impact_recognizes_user_facing_paths():
    for path in (
        "tools/livedev-next",
        "packaging/systemd/rush-livedev-test.service",
        "mkosi/mkosi.profiles/livedev/mkosi.conf",
        ".github/workflows/ci.yml",
    ):
        assert docs_impact._match_any(path, docs_impact.USER_FACING_PATTERNS)


def test_frontpage_manifest_is_a_docs_update():
    assert docs_impact._match_any(
        "docs/frontpage/project.toml",
        docs_impact.DOCS_SATISFYING_PATTERNS,
    )


def test_docs_impact_rejects_user_change_without_docs(monkeypatch):
    monkeypatch.setattr(
        docs_impact,
        "_changed_files",
        lambda base, head: ["tools/rush-livedev-runner"],
    )
    code, message = docs_impact.check("HEAD", "HEAD")
    assert code == 1
    assert "user-facing" in message.lower()


def test_docs_impact_accepts_user_change_with_docs(monkeypatch):
    monkeypatch.setattr(
        docs_impact,
        "_changed_files",
        lambda base, head: ["tools/rush-livedev-runner", "README.md"],
    )
    assert docs_impact.check("HEAD", "HEAD")[0] == 0
