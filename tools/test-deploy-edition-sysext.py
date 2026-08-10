#!/usr/bin/env python3
"""Behavioral tests for the Rush Linux edition sysext deployment lifecycle."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

TOOL = Path(__file__).with_name("deploy-edition-sysext.py")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def fixture(tmp_path: Path, *, signed: bool = True) -> dict[str, Path]:
    root = tmp_path / "root"
    (root / "etc").mkdir(parents=True)
    (root / "etc/os-release").write_text(
        'ID=rush-linux\nVERSION_ID="0.7.0"\n', encoding="utf-8"
    )
    unit = root / "usr/lib/systemd/system/systemd-sysext.service"
    unit.parent.mkdir(parents=True)
    unit.write_text("[Unit]\nDescription=System Extension Images\n", encoding="utf-8")

    output = tmp_path / "output"
    output.mkdir()
    artifact = output / "rush-linux-desktop.raw"
    artifact.write_bytes(b"rush-edition-image-v1")
    plan = tmp_path / "edition-plan.json"
    write_json(
        plan,
        {
            "schema_version": 1,
            "kind": "rush-linux-edition-sysext",
            "edition": {"name": "desktop"},
            "extension": {
                "id": "rush-linux-desktop",
                "filename": "rush-linux-desktop.raw",
            },
            "base_os": {
                "id": "rush-linux",
                "version_id": "0.7.0",
                "architecture": "x86-64",
            },
            "edition_version": "0.7.0-beta.4",
        },
    )
    receipt = output / "rush-linux-desktop.build.json"
    write_json(
        receipt,
        {
            "schema_version": 1,
            "kind": "rush-linux-edition-sysext-build",
            "extension_id": "rush-linux-desktop",
            "edition": "desktop",
            "edition_version": "0.7.0-beta.4",
            "artifact": {
                "path": artifact.name,
                "size_bytes": artifact.stat().st_size,
                "sha256": digest(artifact),
            },
            "plan_sha256": digest(plan),
            "signed": signed,
            "certificate_sha256": "a" * 64 if signed else None,
        },
    )
    return {"root": root, "artifact": artifact, "plan": plan, "receipt": receipt}


def run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(TOOL), *args],
        text=True,
        capture_output=True,
        check=False,
        timeout=10,
    )


def install_args(data: dict[str, Path]) -> list[str]:
    return [
        "install",
        "--plan",
        str(data["plan"]),
        "--receipt",
        str(data["receipt"]),
        "--root",
        str(data["root"]),
    ]


def test_signed_install_stages_artifact_state_and_boot_activation(tmp_path: Path) -> None:
    data = fixture(tmp_path)
    result = run(*install_args(data))
    assert result.returncode == 0, result.stderr
    installed = data["root"] / "var/lib/extensions/rush-linux-desktop.raw"
    state = data["root"] / "var/lib/rush-linux/editions/rush-linux-desktop.json"
    link = data["root"] / "etc/systemd/system/sysinit.target.wants/systemd-sysext.service"
    assert installed.read_bytes() == data["artifact"].read_bytes()
    assert json.loads(state.read_text())["signed"] is True
    assert link.is_symlink()
    assert os.readlink(link) == "/usr/lib/systemd/system/systemd-sysext.service"


def test_unsigned_install_requires_explicit_development_override(tmp_path: Path) -> None:
    data = fixture(tmp_path, signed=False)
    refused = run(*install_args(data))
    assert refused.returncode == 2
    assert "refusing unsigned" in refused.stderr
    accepted = run(*install_args(data), "--allow-unsigned-development")
    assert accepted.returncode == 0, accepted.stderr


def test_host_identity_mismatch_fails_before_writes(tmp_path: Path) -> None:
    data = fixture(tmp_path)
    (data["root"] / "etc/os-release").write_text(
        "ID=other-linux\nVERSION_ID=0.7.0\n", encoding="utf-8"
    )
    result = run(*install_args(data))
    assert result.returncode == 2
    assert "does not match" in result.stderr
    assert not (data["root"] / "var/lib/extensions").exists()


def test_artifact_hash_mismatch_fails_before_writes(tmp_path: Path) -> None:
    data = fixture(tmp_path)
    data["artifact"].write_bytes(b"tampered")
    result = run(*install_args(data))
    assert result.returncode == 2
    assert "SHA-256" in result.stderr
    assert not (data["root"] / "var/lib/extensions").exists()


def fake_sysext(tmp_path: Path, *, fail: bool) -> Path:
    script = tmp_path / "systemd-sysext"
    script.write_text(
        "#!/bin/sh\n"
        "printf '%s\\n' \"$*\" >> \"$RUSH_SYSEXT_LOG\"\n"
        + ("echo activation-failed >&2\nexit 9\n" if fail else "exit 0\n"),
        encoding="utf-8",
    )
    script.chmod(0o755)
    return script


def test_live_refresh_failure_rolls_back_existing_install(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    data = fixture(tmp_path)
    first = run(*install_args(data))
    assert first.returncode == 0, first.stderr
    installed = data["root"] / "var/lib/extensions/rush-linux-desktop.raw"
    original = installed.read_bytes()

    data["artifact"].write_bytes(b"rush-edition-image-v2")
    receipt = json.loads(data["receipt"].read_text())
    receipt["artifact"]["size_bytes"] = data["artifact"].stat().st_size
    receipt["artifact"]["sha256"] = digest(data["artifact"])
    write_json(data["receipt"], receipt)

    fake = fake_sysext(tmp_path, fail=True)
    log = tmp_path / "sysext.log"
    monkeypatch.setenv("RUSH_SYSEXT_LOG", str(log))
    result = subprocess.run(
        [
            sys.executable,
            str(TOOL),
            *install_args(data),
            "--force",
            "--activate",
            "--systemd-sysext",
            str(fake),
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 2
    assert "activation-failed" in result.stderr
    assert installed.read_bytes() == original
    assert "refresh" in log.read_text()


def test_remove_with_refresh_removes_artifact_and_state(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    data = fixture(tmp_path)
    assert run(*install_args(data)).returncode == 0
    fake = fake_sysext(tmp_path, fail=False)
    log = tmp_path / "sysext.log"
    monkeypatch.setenv("RUSH_SYSEXT_LOG", str(log))
    result = subprocess.run(
        [
            sys.executable,
            str(TOOL),
            "remove",
            "--extension-id",
            "rush-linux-desktop",
            "--root",
            str(data["root"]),
            "--activate",
            "--systemd-sysext",
            str(fake),
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    assert not (data["root"] / "var/lib/extensions/rush-linux-desktop.raw").exists()
    assert not (
        data["root"] / "var/lib/rush-linux/editions/rush-linux-desktop.json"
    ).exists()
    assert "refresh" in log.read_text()


def test_list_reports_installed_extensions(tmp_path: Path) -> None:
    data = fixture(tmp_path)
    assert run(*install_args(data)).returncode == 0
    result = run("list", "--root", str(data["root"]), "--json")
    assert result.returncode == 0, result.stderr
    listing = json.loads(result.stdout)
    assert [item["extension_id"] for item in listing] == ["rush-linux-desktop"]


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-v", *sys.argv[1:]]))
