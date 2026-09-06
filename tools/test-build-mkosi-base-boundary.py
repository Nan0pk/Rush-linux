#!/usr/bin/env python3
"""Regression tests for the whole-image vs product-edition build boundary."""

from __future__ import annotations

import subprocess
import json
import os
import shutil
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
BUILDER = ROOT / "tools" / "build-mkosi-image.sh"


def run_builder(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(BUILDER), *args],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
        timeout=10,
    )


def test_help_separates_whole_images_from_product_editions() -> None:
    result = run_builder("--help")
    assert result.returncode == 0
    assert "server|livedev" in result.stdout
    assert "build-edition-image.sh" in result.stdout


def test_product_edition_is_rejected_before_compilation() -> None:
    result = run_builder("--edition", "desktop")
    assert result.returncode == 2
    assert "build-edition-image.sh --edition desktop" in result.stderr
    assert "Building optid" not in result.stdout


def test_server_common_base_does_not_apply_server_sysext_profile() -> None:
    text = BUILDER.read_text(encoding="utf-8")
    assert "MKOSI_ARGS=(--force)" in text
    assert 'if [[ "${EDITION}" == "livedev" ]]; then' in text
    assert 'MKOSI_ARGS+=(--profile="${EDITION}")' in text
    assert '--profile="server"' not in text


@pytest.fixture
def isolated_builder(tmp_path: Path) -> Path:
    """Exercise the actual wrapper in a disposable repo, never the real staging tree."""
    repo = tmp_path / "checkout"
    (repo / "tools").mkdir(parents=True)
    shutil.copy2(BUILDER, repo / "tools/build-mkosi-image.sh")
    for directory in ("packaging", "config", "distro"):
        shutil.copytree(ROOT / directory, repo / directory)
    shutil.copy2(ROOT / "tools/optid-boot-assess", repo / "tools/optid-boot-assess")
    (repo / "VERSION").write_text("0.7.0-beta.4\n")
    (repo / "mkosi/mkosi.extra").mkdir(parents=True)
    (repo / "mkosi/mkosi.extra/keep").write_text("existing staging")
    (repo / "build").mkdir()
    (repo / "build/keep").write_text("existing output")
    return repo


def invoke(repo: Path, *args: str, cwd: Path | None = None,
           env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(repo / "tools/build-mkosi-image.sh"), *args],
        cwd=cwd or repo, env=env, text=True, capture_output=True, timeout=15,
    )


@pytest.mark.parametrize("edition", ["server", "livedev"])
def test_plan_with_clean_preserves_staging_and_output(isolated_builder: Path, edition: str) -> None:
    result = invoke(isolated_builder, "--edition", edition, "--clean", "--plan")
    assert result.returncode == 0, result.stderr
    assert "Build plan only" in result.stdout
    assert (isolated_builder / "build/keep").read_text() == "existing output"
    assert (isolated_builder / "mkosi/mkosi.extra/keep").read_text() == "existing staging"
    assert not (isolated_builder / "target").exists()
    assert ("--profile=livedev" in result.stdout) == (edition == "livedev")


@pytest.mark.parametrize("option", ["--edition", "--snapshot", "--package-dir"])
def test_missing_option_value_is_actionable_before_staging(isolated_builder: Path, option: str) -> None:
    result = invoke(isolated_builder, option)
    assert result.returncode == 2
    assert f"Option {option} requires a value" in result.stderr
    assert (isolated_builder / "mkosi/mkosi.extra/keep").exists()


@pytest.mark.parametrize("date", ["2026-09-04", "20260230", "--clean", "garbage"])
def test_invalid_snapshot_is_rejected_before_any_build(isolated_builder: Path, date: str) -> None:
    result = invoke(isolated_builder, "--snapshot", date, "--plan")
    assert result.returncode == 2
    assert (isolated_builder / "build/keep").exists()
    assert not (isolated_builder / "target").exists()


def test_missing_package_directory_is_rejected(isolated_builder: Path) -> None:
    result = invoke(isolated_builder, "--package-dir", "not-present", "--plan")
    assert result.returncode == 2
    assert "Package directory does not exist" in result.stderr


def test_real_build_forwards_inputs_without_shell_interpretation(
    isolated_builder: Path, tmp_path: Path,
) -> None:
    """Stub external tools only; execute argument parsing, staging and mkosi dispatch."""
    repo = isolated_builder
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    trace = tmp_path / "invocation.json"
    # Shell metacharacters are literal directory names, not commands.
    local_packages = tmp_path / "packages with spaces; touch PWNED"
    local_packages.mkdir()
    second_packages = tmp_path / "second-packages"
    second_packages.mkdir()
    cargo = bin_dir / "cargo"
    cargo.write_text(
        "#!/bin/sh\n"
        'if [ "${1:-}" = "--version" ]; then echo "cargo 99-test"; exit 0; fi\n'
        "mkdir -p target/release\n"
        "printf fixture > target/release/optid\n"
        "printf fixture > target/release/optctl\n"
    )
    mkosi = bin_dir / "mkosi"
    mkosi.write_text(
        "#!/usr/bin/env python3\nimport json, os, pathlib, sys\n"
        "trace = pathlib.Path(os.environ['RUSH_TEST_TRACE'])\n"
        "with trace.open('a') as handle: handle.write(json.dumps({'args': sys.argv[1:], 'cwd': os.getcwd()}) + '\\n')\n"
        "if sys.argv[1:] == ['--version']: print('mkosi 99-test')\n"
    )
    cargo.chmod(0o755)
    mkosi.chmod(0o755)
    env = dict(os.environ, PATH=f"{bin_dir}:{os.environ['PATH']}",
               RUSH_TEST_TRACE=str(trace), MKOSI_CACHE=str(tmp_path / "cache with spaces"))
    env.pop("SUDO_USER", None)
    result = invoke(
        repo, "--snapshot", "20260904", "--package-dir", local_packages.name,
        "--package-dir", str(second_packages), cwd=tmp_path, env=env,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    recorded = [json.loads(line) for line in trace.read_text().splitlines()]
    expected_inputs = [
        "--force",
        "--snapshot=20260904",
        f"--package-directory={local_packages}",
        f"--package-directory={second_packages}",
        f"--cache-dir={tmp_path / 'cache with spaces'}",
    ]
    assert recorded == [
        {"args": ["--version"], "cwd": str(tmp_path)},
        {"args": ["summary", *expected_inputs], "cwd": str(repo / "mkosi")},
        {"args": ["build", *expected_inputs], "cwd": str(repo / "mkosi")},
    ]
    assert "cargo: cargo 99-test" in result.stdout
    assert "mkosi: mkosi 99-test" in result.stdout
    assert "Resolved mkosi configuration:" in result.stdout
    assert (repo / "mkosi/mkosi.extra/usr/libexec/optid").read_text() == "fixture"
    assert not (tmp_path / "PWNED").exists()
    assert not (repo / "PWNED").exists()
    assert (repo / "build/keep").exists()
    assert not (tmp_path / "target").exists(), "compilation must run in the repository"
