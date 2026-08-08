#!/usr/bin/env python3
"""Regression tests for the whole-image vs product-edition build boundary."""

from __future__ import annotations

import subprocess
from pathlib import Path

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
