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


def test_generated_build_output_is_rejected(tmp_path):
    target = tmp_path / ".mkosi-output" / "usr" / "bin"
    target.mkdir(parents=True)
    (target / "tool").write_text("generated", encoding="utf-8")
    failures = hygiene.violations([".mkosi-output/usr/bin/tool"], tmp_path)
    assert failures and "generated" in failures[0]


def test_mkosi_extra_image_source_is_allowed(tmp_path):
    # mkosi/mkosi.extra/ is hand-maintained image source that mkosi copies
    # into the rootfs, not build output. The optid systemd units under it
    # are required by the capability drift test to stay byte-identical to
    # their packaging/systemd/ counterparts, so edits here must be
    # mergeable.
    target = tmp_path / "mkosi" / "mkosi.extra" / "usr" / "lib" / "systemd" / "system"
    target.mkdir(parents=True)
    (target / "optid-apply.service").write_text("[Service]\n", encoding="utf-8")
    assert (
        hygiene.violations(
            ["mkosi/mkosi.extra/usr/lib/systemd/system/optid-apply.service"],
            tmp_path,
        )
        == []
    )


def test_root_report_is_rejected(tmp_path):
    report = tmp_path / "LATEST-AUDIT.md"
    report.write_text("stale narrative", encoding="utf-8")
    failures = hygiene.violations(["LATEST-AUDIT.md"], tmp_path)
    assert failures and "belongs under docs" in failures[0]


def test_allowed_root_instructions_are_accepted(tmp_path):
    instructions = tmp_path / "CLAUDE.md"
    instructions.write_text("See AGENTS.md", encoding="utf-8")
    assert hygiene.violations(["CLAUDE.md"], tmp_path) == []


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


# ── Private-key material regression (post-#337) ──────────────────────


def _marker(prefix: str) -> bytes:
    # Build the marker bytes at runtime so the test file itself contains
    # no PEM private-key header (which the gate scans for in tracked
    # files — embedding the literal header here would self-trip).
    # `prefix` is "" (PKCS#8), "RSA", "OPENSSH", or "EC".
    if prefix:
        return f"-----BEGIN {prefix} PRIVATE KEY-----".encode("ascii")
    return b"-----BEGIN PRIVATE KEY-----"


def test_private_key_marker_is_rejected(tmp_path):
    target = tmp_path / "config" / "keys"
    target.mkdir(parents=True)
    (target / "testing.private.pem").write_bytes(_marker("") + b"\nDUMMY\n")
    failures = hygiene.private_key_violations(
        ["config/keys/testing.private.pem"], tmp_path
    )
    assert failures and "private key material" in failures[0]


def test_rsa_private_key_marker_is_rejected(tmp_path):
    target = tmp_path / "legacy.key"
    target.write_bytes(_marker("RSA") + b"\nDUMMY\n")
    failures = hygiene.private_key_violations(["legacy.key"], tmp_path)
    assert failures and "private key material" in failures[0]


def test_openssh_private_key_marker_is_rejected(tmp_path):
    target = tmp_path / "id_ed25519"
    target.write_bytes(_marker("OPENSSH") + b"\nDUMMY\n")
    failures = hygiene.private_key_violations(["id_ed25519"], tmp_path)
    assert failures and "private key material" in failures[0]


def test_extensionless_private_key_is_rejected(tmp_path):
    # A private key with no `.pem` extension must still be caught — the
    # scan is content-based, not name-based.
    target = tmp_path / "keys.txt"
    target.write_bytes(_marker("") + b"\nDUMMY\n")
    failures = hygiene.private_key_violations(["keys.txt"], tmp_path)
    assert failures and "private key material" in failures[0]


def test_public_key_block_is_not_flagged(tmp_path):
    # PUBLIC KEY blocks (used for signature verification) must not be
    # flagged — only PRIVATE KEY material is rejected.
    target = tmp_path / "config" / "keys"
    target.mkdir(parents=True)
    (target / "testing.public.pem").write_bytes(
        b"-----BEGIN PUBLIC KEY-----\nDUMMY\n-----END PUBLIC KEY-----\n"
    )
    failures = hygiene.private_key_violations(
        ["config/keys/testing.public.pem"], tmp_path
    )
    assert failures == []
