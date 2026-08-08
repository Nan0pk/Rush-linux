#!/usr/bin/env python3
"""Safety coverage for release and development sysext verity modes."""

from __future__ import annotations

import configparser
import importlib.util
import json
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

TOOL = Path(__file__).with_name("build-edition-sysext.py")
SPEC = importlib.util.spec_from_file_location("edition_sysext_verity", TOOL)
assert SPEC is not None and SPEC.loader is not None
BUILDER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = BUILDER
SPEC.loader.exec_module(BUILDER)


def plan() -> dict[str, object]:
    return {
        "schema_version": 1,
        "kind": "rush-linux-edition-sysext",
        "edition": {
            "name": "server",
            "description": "Server edition",
            "inheritance": ["server"],
            "effective_defaults": {},
        },
        "extension": {
            "id": "rush-linux-server",
            "filename": "rush-linux-server.raw",
            "format": "sysext",
            "overlay": True,
            "signing_required_for_release": True,
        },
        "base_os": {
            "id": "rush-linux",
            "version_id": "0.7.0",
            "architecture": "x86-64",
        },
        "edition_version": "0.7.0-beta.4",
        "packages": {
            "profile_mode": "complete-sysext-payload",
            "sysext": [],
            "edition_requirements": ["adaptive-base"],
        },
        "sources": {
            "edition_manifests": [],
            "mkosi_profile": {"path": "server/mkosi.conf", "sha256": "0" * 64},
        },
    }


def prepared_workspace(tmp_path: Path) -> tuple[dict[str, object], Path]:
    payload = plan()
    base = tmp_path / "base"
    base.mkdir()
    workspace = tmp_path / "workspace"
    BUILDER.prepare_workspace(
        plan=payload,
        workspace=workspace,
        base_tree=base,
        force=False,
    )
    return payload, workspace


def parse_config(text: str) -> configparser.ConfigParser:
    parser = configparser.ConfigParser(interpolation=None, strict=True)
    parser.optionxform = str
    parser.read_string(text)
    return parser


def fake_successful_mkosi(
    expected_verity: str,
    *,
    expect_credentials: bool,
):
    def run(
        command: list[str],
        *,
        cwd: Path,
        text: bool,
        check: bool,
    ) -> SimpleNamespace:
        assert command == ["mkosi", "-f"]
        config = parse_config((cwd / "mkosi.conf").read_text(encoding="utf-8"))
        assert config.get("Validation", "Verity") == expected_verity
        assert config.get("Distribution", "Distribution") == "arch"
        assert config.get("Build", "CacheDirectory") == "cache"
        assert not config.has_option("Output", "CacheDirectory")
        assert (cwd / "mkosi.key").is_symlink() is expect_credentials
        assert (cwd / "mkosi.crt").is_symlink() is expect_credentials
        output = cwd / "output"
        output.mkdir(exist_ok=True)
        (output / "rush-linux-server.raw").write_bytes(b"sysext")
        return SimpleNamespace(returncode=0)

    return run


def test_prepared_workspace_is_neutral_until_build_mode_is_selected(
    tmp_path: Path,
) -> None:
    _, workspace = prepared_workspace(tmp_path)
    config = parse_config((workspace / "mkosi.conf").read_text(encoding="utf-8"))
    assert config.get("Validation", "Verity") == "auto"
    assert config.get("Distribution", "Distribution") == "arch"
    assert config.get("Build", "CacheDirectory") == "cache"
    assert not config.has_option("Output", "CacheDirectory")


def test_unsigned_development_explicitly_disables_verity(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    payload, workspace = prepared_workspace(tmp_path)
    monkeypatch.setattr(
        BUILDER.subprocess,
        "run",
        fake_successful_mkosi("no", expect_credentials=False),
    )

    BUILDER.run_build(
        workspace=workspace,
        plan=payload,
        mkosi="mkosi",
        key=None,
        certificate=None,
        unsigned_development=True,
    )

    receipt = json.loads(
        (workspace / "output/rush-linux-server.build.json").read_text(encoding="utf-8")
    )
    assert receipt["signed"] is False
    assert receipt["verity"] == "no"


def test_release_build_requires_signed_verity_and_cleans_credentials(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    payload, workspace = prepared_workspace(tmp_path)
    key = tmp_path / "release.key"
    certificate = tmp_path / "release.crt"
    key.write_text("private", encoding="utf-8")
    certificate.write_text("certificate", encoding="utf-8")
    monkeypatch.setattr(
        BUILDER.subprocess,
        "run",
        fake_successful_mkosi("signed", expect_credentials=True),
    )

    BUILDER.run_build(
        workspace=workspace,
        plan=payload,
        mkosi="mkosi",
        key=key,
        certificate=certificate,
        unsigned_development=False,
    )

    receipt = json.loads(
        (workspace / "output/rush-linux-server.build.json").read_text(encoding="utf-8")
    )
    assert receipt["signed"] is True
    assert receipt["verity"] == "signed"
    assert not (workspace / "mkosi.key").exists()
    assert not (workspace / "mkosi.crt").exists()


def test_unknown_verity_mode_fails_closed() -> None:
    with pytest.raises(BUILDER.EditionError, match="unsupported mkosi verity mode"):
        BUILDER.render_shared_config(verity="maybe")


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-v"]))
