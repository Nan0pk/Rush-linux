#!/usr/bin/env python3
"""Behavioral tests for building an edition image set from one common base."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

TOOL = Path(__file__).with_name("build-edition-set.py")


def executable(path: Path, content: str) -> Path:
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)
    return path


def fake_discovery(tmp_path: Path) -> Path:
    return executable(
        tmp_path / "sysext-builder.py",
        '''#!/usr/bin/env python3
import sys
if sys.argv[1:] == ["list"]:
    print("desktop")
    print("laptop")
    print("realtime-audio")
    print("server")
    raise SystemExit(0)
raise SystemExit(9)
''',
    )


def fake_deployer(tmp_path: Path) -> Path:
    return executable(tmp_path / "deployer.py", "#!/usr/bin/env python3\n")


def fake_composer(tmp_path: Path, *, fail_edition: str | None = None) -> Path:
    failure = repr(fail_edition)
    return executable(
        tmp_path / "composer.py",
        f'''#!/usr/bin/env python3
import hashlib, json, os, pathlib, sys
args = sys.argv[1:]
def value(name): return args[args.index(name) + 1]
edition = value("--edition")
with pathlib.Path(os.environ["COMPOSER_LOG"]).open("a") as handle:
    handle.write(edition + "|" + " ".join(args) + "\\n")
if edition == {failure}:
    raise SystemExit(8)
output = pathlib.Path(value("--output"))
output.parent.mkdir(parents=True, exist_ok=True)
output.write_bytes(("base+" + edition).encode())
receipt = {{
  "schema_version": 1,
  "kind": "rush-linux-composed-edition-image",
  "edition": edition,
  "edition_version": "0.7.0-beta.4",
}}
output.with_suffix(output.suffix + ".compose.json").write_text(json.dumps(receipt) + "\\n")
''',
    )


def fake_base_builder(tmp_path: Path, build_dir: Path) -> Path:
    return executable(
        tmp_path / "build-base.sh",
        f'''#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$BASE_LOG"
mkdir -p {str(build_dir)!r}
printf 'one-common-base' > {str(build_dir / "rush-linux-server.raw")!r}
''',
    )


def make_tools(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    fail_edition: str | None = None,
) -> dict[str, Path]:
    tools = {
        "discovery": fake_discovery(tmp_path),
        "deployer": fake_deployer(tmp_path),
        "composer": fake_composer(tmp_path, fail_edition=fail_edition),
        "composer_log": tmp_path / "composer.log",
        "base_log": tmp_path / "base.log",
    }
    monkeypatch.setenv("COMPOSER_LOG", str(tools["composer_log"]))
    monkeypatch.setenv("BASE_LOG", str(tools["base_log"]))
    return tools


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(TOOL), *args],
        text=True,
        capture_output=True,
        check=False,
        timeout=20,
    )


def common_args(
    tmp_path: Path,
    tools: dict[str, Path],
    *,
    output_dir: Path,
    base_image: Path | None = None,
) -> list[str]:
    args = [
        "--output-dir",
        str(output_dir),
        "--workspace-root",
        str(tmp_path / "workspaces"),
        "--composer",
        str(tools["composer"]),
        "--sysext-builder",
        str(tools["discovery"]),
        "--deployer",
        str(tools["deployer"]),
        "--unsigned-development",
    ]
    if base_image is not None:
        args.extend(["--base-image", str(base_image)])
    return args


def test_discovers_all_product_editions_and_publishes_index(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    tools = make_tools(tmp_path, monkeypatch)
    base = tmp_path / "base.raw"
    base.write_bytes(b"common")
    output_dir = tmp_path / "edition-set"
    result = run(common_args(tmp_path, tools, output_dir=output_dir, base_image=base))
    assert result.returncode == 0, result.stderr
    index = json.loads((output_dir / "edition-set.json").read_text())
    assert [item["edition"] for item in index["images"]] == [
        "desktop",
        "laptop",
        "realtime-audio",
        "server",
    ]
    assert index["base"]["built_by_set_builder"] is False
    assert index["base"]["sha256"] == hashlib.sha256(b"common").hexdigest()
    assert all((output_dir / item["filename"]).is_file() for item in index["images"])


def test_common_base_is_built_once_and_reused_for_selected_editions(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    tools = make_tools(tmp_path, monkeypatch)
    build_dir = tmp_path / "build"
    base_builder = fake_base_builder(tmp_path, build_dir)
    output_dir = tmp_path / "set"
    args = common_args(tmp_path, tools, output_dir=output_dir)
    args.extend(
        [
            "--editions",
            "desktop",
            "laptop",
            "--base-builder",
            str(base_builder),
            "--build-dir",
            str(build_dir),
            "--clean-base",
        ]
    )
    result = run(args)
    assert result.returncode == 0, result.stderr
    assert tools["base_log"].read_text().splitlines() == ["--edition server --clean"]
    composer_lines = tools["composer_log"].read_text().splitlines()
    assert len(composer_lines) == 2
    base_path = str((build_dir / "rush-linux-server.raw").resolve())
    assert all(base_path in line for line in composer_lines)


def test_failed_edition_preserves_existing_published_set(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    tools = make_tools(tmp_path, monkeypatch, fail_edition="laptop")
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    output_dir = tmp_path / "set"
    output_dir.mkdir()
    (output_dir / "old.txt").write_text("keep")
    args = common_args(tmp_path, tools, output_dir=output_dir, base_image=base)
    args.extend(["--editions", "desktop", "laptop", "--force"])
    result = run(args)
    assert result.returncode == 2
    assert (output_dir / "old.txt").read_text() == "keep"
    assert not (output_dir / "rush-linux-desktop.raw").exists()


def test_signed_release_arguments_reach_every_composition(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    tools = make_tools(tmp_path, monkeypatch)
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    key = tmp_path / "key.pem"
    certificate = tmp_path / "cert.pem"
    key.write_text("key")
    certificate.write_text("cert")
    args = common_args(tmp_path, tools, output_dir=tmp_path / "set", base_image=base)
    args.remove("--unsigned-development")
    args.extend(
        [
            "--editions",
            "desktop",
            "server",
            "--key",
            str(key),
            "--certificate",
            str(certificate),
        ]
    )
    result = run(args)
    assert result.returncode == 0, result.stderr
    lines = tools["composer_log"].read_text().splitlines()
    assert len(lines) == 2
    assert all("--key" in line and str(key) in line for line in lines)
    assert all("--certificate" in line and str(certificate) in line for line in lines)


def test_release_set_requires_signing_or_explicit_development_mode(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    tools = make_tools(tmp_path, monkeypatch)
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    args = common_args(tmp_path, tools, output_dir=tmp_path / "set", base_image=base)
    args.remove("--unsigned-development")
    args.extend(["--editions", "desktop"])
    result = run(args)
    assert result.returncode == 2
    assert "require --key and --certificate" in result.stderr


def test_duplicate_or_invalid_editions_fail_before_building(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    tools = make_tools(tmp_path, monkeypatch)
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    base_args = common_args(tmp_path, tools, output_dir=tmp_path / "set", base_image=base)
    duplicate = run([*base_args, "--editions", "desktop", "desktop"])
    assert duplicate.returncode == 2
    assert "duplicate edition" in duplicate.stderr
    invalid = run([*base_args, "--editions", "../../escape"])
    assert invalid.returncode == 2
    assert "invalid edition" in invalid.stderr
    assert not tools["composer_log"].exists()


def test_force_replaces_a_complete_set_only_after_success(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    tools = make_tools(tmp_path, monkeypatch)
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    output_dir = tmp_path / "set"
    output_dir.mkdir()
    (output_dir / "old.txt").write_text("old")
    args = common_args(tmp_path, tools, output_dir=output_dir, base_image=base)
    args.extend(["--editions", "server", "--force"])
    result = run(args)
    assert result.returncode == 0, result.stderr
    assert not (output_dir / "old.txt").exists()
    assert (output_dir / "rush-linux-server.raw").is_file()
    assert (output_dir / "edition-set.json").is_file()


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-v", *sys.argv[1:]]))
