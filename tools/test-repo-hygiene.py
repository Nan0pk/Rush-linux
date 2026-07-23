#!/usr/bin/env python3
"""Unit tests for the repository-hygiene gate."""

from __future__ import annotations

import importlib.machinery
import importlib.util
import sys
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
loader = importlib.machinery.SourceFileLoader(
    "check_repo_hygiene", str(TOOLS / "check-repo-hygiene.py")
)
spec = importlib.util.spec_from_loader("check_repo_hygiene", loader)
hygiene = importlib.util.module_from_spec(spec)
sys.modules["check_repo_hygiene"] = hygiene
loader.exec_module(hygiene)


def test_source_file_is_allowed(tmp_path):
    (tmp_path / "tools").mkdir()
    (tmp_path / "tools" / "safe.py").write_text("print('safe')\n", encoding="utf-8")
    assert hygiene.violations(["tools/safe.py"], tmp_path) == []


def test_generated_staging_path_is_rejected(tmp_path):
    target = tmp_path / "mkosi" / "mkosi.extra" / "usr" / "bin"
    target.mkdir(parents=True)
    (target / "tool").write_text("generated", encoding="utf-8")
    failures = hygiene.violations(["mkosi/mkosi.extra/usr/bin/tool"], tmp_path)
    assert failures and "generated" in failures[0]


def test_elf_is_rejected(tmp_path):
    target = tmp_path / "tool"
    target.write_bytes(b"\x7fELFcompiled")
    failures = hygiene.violations(["tool"], tmp_path)
    assert failures and "ELF" in failures[0]


def test_windows_executable_is_rejected(tmp_path):
    target = tmp_path / "tool.exe"
    target.write_bytes(b"MZcompiled")
    failures = hygiene.violations(["tool.exe"], tmp_path)
    assert failures and "Windows" in failures[0]
