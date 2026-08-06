#!/usr/bin/env python3
"""Behavioral tests for the one-command Rush Linux edition image composer."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

TOOL = Path(__file__).with_name("compose-edition-image.py")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def executable(path: Path, content: str) -> Path:
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)
    return path


def fake_sysext_builder(tmp_path: Path) -> Path:
    return executable(
        tmp_path / "build-sysext.py",
        r'''#!/usr/bin/env python3
import hashlib, json, pathlib, sys
args = sys.argv[1:]
def value(name): return args[args.index(name) + 1]
workspace = pathlib.Path(value("--workspace"))
edition = value("--edition")
workspace.mkdir(parents=True, exist_ok=True)
output = workspace / "output"
output.mkdir(exist_ok=True)
extension_id = f"rush-linux-{edition}"
plan = {
  "schema_version": 1,
  "kind": "rush-linux-edition-sysext",
  "edition": {"name": edition},
  "edition_version": "0.7.0-beta.4",
  "extension": {"id": extension_id, "filename": f"{extension_id}.raw"},
  "base_os": {"id": "rush-linux", "version_id": "0.7.0", "architecture": "x86-64"},
}
plan_path = workspace / "edition-plan.json"
plan_path.write_text(json.dumps(plan, sort_keys=True) + "\n")
artifact = output / f"{extension_id}.raw"
artifact.write_bytes(("extension:" + edition).encode())
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
receipt = {
  "schema_version": 1,
  "kind": "rush-linux-edition-sysext-build",
  "extension_id": extension_id,
  "edition": edition,
  "edition_version": "0.7.0-beta.4",
  "artifact": {
    "path": artifact.name,
    "size_bytes": artifact.stat().st_size,
    "sha256": sha(artifact),
  },
  "plan_sha256": sha(plan_path),
  "signed": "--key" in args,
  "certificate_sha256": "b" * 64 if "--key" in args else None,
}
(output / f"{extension_id}.build.json").write_text(json.dumps(receipt) + "\n")
log = pathlib.Path(__import__("os").environ["SYSEXT_BUILD_LOG"])
log.write_text("\n".join(args) + "\n")
''',
    )


def fake_deployer(tmp_path: Path, *, fail: bool = False) -> Path:
    suffix = "sys.exit(7)" if fail else ""
    return executable(
        tmp_path / "deploy.py",
        f'''#!/usr/bin/env python3
import pathlib, shutil, sys
args = sys.argv[1:]
def value(name): return args[args.index(name) + 1]
root = pathlib.Path(value("--root"))
artifact = pathlib.Path(value("--artifact"))
(root / "var/lib/extensions").mkdir(parents=True, exist_ok=True)
shutil.copy2(artifact, root / "var/lib/extensions" / artifact.name)
pathlib.Path(__import__("os").environ["DEPLOY_LOG"]).write_text("\\n".join(args) + "\\n")
{suffix}
''',
    )


def fake_dissect(tmp_path: Path) -> Path:
    return executable(
        tmp_path / "systemd-dissect",
        r'''#!/usr/bin/env python3
import os, pathlib, sys
args = sys.argv[1:]
log = pathlib.Path(os.environ["DISSECT_LOG"])
with log.open("a") as handle: handle.write(" ".join(args) + "\n")
if args[0] == "--mount":
    image = pathlib.Path(args[1])
    root = pathlib.Path(args[2])
    root.mkdir(parents=True, exist_ok=True)
    (root / ".image-path").write_text(str(image))
    (root / "etc").mkdir(exist_ok=True)
    (root / "etc/os-release").write_text("ID=rush-linux\nVERSION_ID=0.7.0\n")
    unit = root / "usr/lib/systemd/system/systemd-sysext.service"
    unit.parent.mkdir(parents=True, exist_ok=True)
    unit.write_text("[Unit]\n")
elif args[0] == "--umount":
    root = pathlib.Path(args[1])
    image = pathlib.Path((root / ".image-path").read_text())
    extension_dir = root / "var/lib/extensions"
    if extension_dir.exists():
        with image.open("ab") as handle:
            for item in sorted(extension_dir.iterdir()): handle.write(item.read_bytes())
else:
    raise SystemExit(9)
''',
    )


def fake_base_builder(tmp_path: Path, build_dir: Path) -> Path:
    return executable(
        tmp_path / "build-base.sh",
        f'''#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" > "$BASE_BUILD_LOG"
mkdir -p {str(build_dir)!r}
printf 'common-base-built' > {str(build_dir / "rush-linux-server.raw")!r}
''',
    )


def env_and_tools(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, deploy_fail: bool = False
) -> dict[str, Path]:
    paths = {
        "sysext": fake_sysext_builder(tmp_path),
        "deploy": fake_deployer(tmp_path, fail=deploy_fail),
        "dissect": fake_dissect(tmp_path),
        "sysext_log": tmp_path / "sysext.log",
        "deploy_log": tmp_path / "deploy.log",
        "dissect_log": tmp_path / "dissect.log",
    }
    monkeypatch.setenv("SYSEXT_BUILD_LOG", str(paths["sysext_log"]))
    monkeypatch.setenv("DEPLOY_LOG", str(paths["deploy_log"]))
    monkeypatch.setenv("DISSECT_LOG", str(paths["dissect_log"]))
    return paths


def run(args: list[str], env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(TOOL), *args],
        text=True,
        capture_output=True,
        check=False,
        timeout=15,
        env=env,
    )


def common_args(tmp_path: Path, paths: dict[str, Path], base: Path, output: Path) -> list[str]:
    return [
        "--edition", "desktop",
        "--output", str(output),
        "--workspace", str(tmp_path / "workspace"),
        "--base-image", str(base),
        "--sysext-builder", str(paths["sysext"]),
        "--deployer", str(paths["deploy"]),
        "--systemd-dissect", str(paths["dissect"]),
        "--mkosi", "fake-mkosi",
        "--unsigned-development",
    ]


def test_one_command_composes_base_and_extension_atomically(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = env_and_tools(tmp_path, monkeypatch)
    base = tmp_path / "common-base.raw"
    base.write_bytes(b"common-base")
    output = tmp_path / "rush-linux-desktop.raw"
    result = run(common_args(tmp_path, paths, base, output))
    assert result.returncode == 0, result.stderr
    assert output.read_bytes() == b"common-baseextension:desktop"
    receipt = json.loads(output.with_suffix(".raw.compose.json").read_text())
    assert receipt["edition"] == "desktop"
    assert receipt["base"]["sha256"] == digest(base)
    assert receipt["output"]["sha256"] == digest(output)
    assert receipt["unsigned_development"] is True
    assert "--mount" in paths["dissect_log"].read_text()
    assert "--umount" in paths["dissect_log"].read_text()


def test_existing_output_is_preserved_without_force(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = env_and_tools(tmp_path, monkeypatch)
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    output = tmp_path / "output.raw"
    output.write_bytes(b"keep-me")
    result = run(common_args(tmp_path, paths, base, output))
    assert result.returncode == 2
    assert output.read_bytes() == b"keep-me"


def test_failed_deployment_preserves_previous_output_and_unmounts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = env_and_tools(tmp_path, monkeypatch, deploy_fail=True)
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    output = tmp_path / "output.raw"
    output.write_bytes(b"previous")
    result = run([*common_args(tmp_path, paths, base, output), "--force"])
    assert result.returncode == 2
    assert output.read_bytes() == b"previous"
    assert "--umount" in paths["dissect_log"].read_text()


def test_signed_release_arguments_are_forwarded(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = env_and_tools(tmp_path, monkeypatch)
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    key = tmp_path / "key.pem"
    certificate = tmp_path / "certificate.pem"
    key.write_text("key")
    certificate.write_text("certificate")
    output = tmp_path / "signed.raw"
    args = common_args(tmp_path, paths, base, output)
    args.remove("--unsigned-development")
    args.extend(["--key", str(key), "--certificate", str(certificate)])
    result = run(args)
    assert result.returncode == 0, result.stderr
    log = paths["sysext_log"].read_text()
    assert "--key" in log and str(key) in log
    assert "--certificate" in log and str(certificate) in log
    receipt = json.loads(output.with_suffix(".raw.compose.json").read_text())
    assert receipt["unsigned_development"] is False


def test_release_mode_requires_signing_credentials(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = env_and_tools(tmp_path, monkeypatch)
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    output = tmp_path / "output.raw"
    args = common_args(tmp_path, paths, base, output)
    args.remove("--unsigned-development")
    result = run(args)
    assert result.returncode == 2
    assert "requires --key and --certificate" in result.stderr
    assert not output.exists()


def test_common_base_builder_is_used_when_base_is_not_supplied(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = env_and_tools(tmp_path, monkeypatch)
    build_dir = tmp_path / "build"
    base_builder = fake_base_builder(tmp_path, build_dir)
    base_log = tmp_path / "base.log"
    monkeypatch.setenv("BASE_BUILD_LOG", str(base_log))
    output = tmp_path / "composed.raw"
    args = [
        "--edition", "laptop",
        "--output", str(output),
        "--workspace", str(tmp_path / "workspace"),
        "--base-builder", str(base_builder),
        "--build-dir", str(build_dir),
        "--sysext-builder", str(paths["sysext"]),
        "--deployer", str(paths["deploy"]),
        "--systemd-dissect", str(paths["dissect"]),
        "--unsigned-development",
        "--clean-base",
    ]
    result = run(args)
    assert result.returncode == 0, result.stderr
    assert "--edition server --clean" in base_log.read_text()
    receipt = json.loads(output.with_suffix(".raw.compose.json").read_text())
    assert receipt["base"]["built_by_composer"] is True
    assert receipt["edition"] == "laptop"


def test_output_cannot_overwrite_common_base(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = env_and_tools(tmp_path, monkeypatch)
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    result = run([*common_args(tmp_path, paths, base, base), "--force"])
    assert result.returncode == 2
    assert "must not overwrite" in result.stderr
    assert base.read_bytes() == b"base"


def test_default_output_and_workspace_make_front_page_command_short(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = env_and_tools(tmp_path, monkeypatch)
    base = tmp_path / "base.raw"
    base.write_bytes(b"base")
    build_dir = tmp_path / "build"
    args = [
        "--edition", "desktop",
        "--base-image", str(base),
        "--build-dir", str(build_dir),
        "--sysext-builder", str(paths["sysext"]),
        "--deployer", str(paths["deploy"]),
        "--systemd-dissect", str(paths["dissect"]),
        "--unsigned-development",
    ]
    result = run(args)
    assert result.returncode == 0, result.stderr
    assert (build_dir / "rush-linux-desktop.raw").is_file()
    assert (build_dir / "edition-desktop/edition-plan.json").is_file()


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-v", *sys.argv[1:]]))
