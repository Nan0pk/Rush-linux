#!/usr/bin/env python3
"""Behavioral coverage for the Rush Linux edition sysext builder."""

from __future__ import annotations

import importlib.util
import json
import stat
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

TOOL = Path(__file__).with_name("build-edition-sysext.py")
SPEC = importlib.util.spec_from_file_location("edition_sysext", TOOL)
assert SPEC and SPEC.loader
builder = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = builder
SPEC.loader.exec_module(builder)


def write_manifest(
    root: Path,
    name: str,
    *,
    inherits: str | None = None,
    packages: tuple[str, ...] = ("base-requirement",),
    extra: str = "",
) -> None:
    inherit = f'inherits = "{inherits}"\n' if inherits else ""
    values = "\n".join(f'  "{item}",' for item in packages)
    (root / f"{name}.toml").write_text(
        f'''[edition]
name = "{name}"
description = "{name} edition"

[defaults]
{inherit}{extra}
[packages]
required = [
{values}
]
''',
        encoding="utf-8",
    )


def write_profile(root: Path, name: str, packages: tuple[str, ...]) -> None:
    target = root / name
    target.mkdir(parents=True, exist_ok=True)
    values = "\n".join(f"    {item}" for item in packages)
    (target / "mkosi.conf").write_text(
        f'''[Output]
ImageId=rush-linux-{name}

[Content]
Packages=
{values}
''',
        encoding="utf-8",
    )


def make_plan(tmp_path: Path) -> tuple[dict[str, object], Path]:
    manifests = tmp_path / "editions"
    profiles = tmp_path / "profiles"
    manifests.mkdir()
    write_manifest(manifests, "desktop", packages=("adaptive-desktop", "linux-adaptive"))
    write_profile(profiles, "desktop", ("plasma-workspace", "pipewire"))
    edition = builder.resolve_edition("desktop", manifests)
    profile = builder.load_profile("desktop", profiles)
    plan = builder.canonical_plan(
        edition,
        profile,
        manifest_dir=manifests,
        profile_dir=profiles,
        version="0.7.0-beta.4",
        architecture="x86-64",
    )
    base = tmp_path / "base"
    base.mkdir()
    return plan, base


def fake_mkosi(path: Path, require_signing: bool = False) -> None:
    signing = (
        "test -L mkosi.key && test -L mkosi.crt\n"
        if require_signing
        else "test ! -e mkosi.key && test ! -e mkosi.crt\n"
    )
    path.write_text(
        "#!/usr/bin/env bash\nset -euo pipefail\n"
        'test "$1" = "-f"\n'
        + signing
        + "mkdir -p output\n"
        + "artifact=$(sed -n 's/^Output=//p' mkosi.images/*/mkosi.conf)\n"
        + 'printf "fake-sysext\\n" > "output/$artifact"\n',
        encoding="utf-8",
    )
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def test_real_product_editions_resolve_without_fake_kernel_sysext() -> None:
    root = Path(__file__).resolve().parent.parent
    manifests = root / "distro" / "editions"
    profiles = root / "mkosi" / "mkosi.profiles"
    names = builder.edition_names(manifests, profiles)
    assert names == ["desktop", "laptop", "realtime-audio", "server"]
    plans = {
        name: builder.canonical_plan(
            builder.resolve_edition(name, manifests),
            builder.load_profile(name, profiles),
            manifest_dir=manifests,
            profile_dir=profiles,
            version="0.7.0-beta.4",
            architecture="x86-64",
        )
        for name in names
    }
    assert "fwupd" in plans["laptop"]["packages"]["sysext"]
    assert plans["server"]["packages"]["sysext"] == []
    realtime = plans["realtime-audio"]["packages"]
    assert "linux-adaptive-rt" in realtime["edition_requirements"]
    assert "linux-adaptive-rt" not in realtime["sysext"]


def test_inheritance_merges_defaults_but_not_package_payloads(tmp_path: Path) -> None:
    manifests = tmp_path / "editions"
    profiles = tmp_path / "profiles"
    manifests.mkdir()
    write_manifest(manifests, "desktop", packages=("desktop-base",), extra='filesystem = "btrfs"\n')
    write_manifest(
        manifests,
        "laptop",
        inherits="desktop",
        packages=("laptop-complete",),
        extra="runtime_pm = true\n",
    )
    write_profile(profiles, "desktop", ("plasma-workspace",))
    write_profile(profiles, "laptop", ("plasma-workspace", "fwupd"))
    plan = builder.canonical_plan(
        builder.resolve_edition("laptop", manifests),
        builder.load_profile("laptop", profiles),
        manifest_dir=manifests,
        profile_dir=profiles,
        version="0.7.0-beta.4",
        architecture="x86-64",
    )
    assert plan["edition"]["effective_defaults"] == {"filesystem": "btrfs", "runtime_pm": True}
    assert plan["packages"]["edition_requirements"] == ["laptop-complete"]
    assert plan["packages"]["sysext"] == ["fwupd", "plasma-workspace"]


@pytest.mark.parametrize(
    ("setup", "message"),
    [
        (lambda root: write_manifest(root, "bad", packages=("one", "one")), "duplicate required"),
        (lambda root: write_manifest(root, "bad", packages=("one",), extra="unknown = []\n"), "scalar value"),
    ],
)
def test_invalid_manifests_fail_closed(tmp_path: Path, setup, message: str) -> None:
    setup(tmp_path)
    with pytest.raises(builder.EditionError, match=message):
        builder.load_manifest(tmp_path / "bad.toml")


def test_prepare_generates_real_sysext_workspace_and_protects_unmarked_dirs(tmp_path: Path) -> None:
    plan, base = make_plan(tmp_path)
    workspace = tmp_path / "workspace"
    builder.prepare_workspace(plan=plan, workspace=workspace, base_tree=base, force=False)
    image_dir = workspace / "mkosi.images/rush-linux-desktop"
    shared = (workspace / "mkosi.conf").read_text()
    image = (image_dir / "mkosi.conf").read_text()
    release = (
        workspace
        / "tree/usr/lib/extension-release.d/extension-release.rush-linux-desktop"
    ).read_text()
    assert "[Distribution]\nDistribution=arch" in shared
    assert "[Build]\nCacheDirectory=cache" in shared
    assert "CacheDirectory=cache" not in shared.split("[Output]", 1)[1].split("[Build]", 1)[0]
    assert "[Distribution]" not in image
    assert "Distribution=arch" not in image
    assert "Format=sysext" in image and "Overlay=yes" in image
    assert "BaseTrees=../../base" in image
    assert "ExtraTrees=../../tree" in image
    assert (image_dir / "../../base").resolve() == base.resolve()
    assert (image_dir / "../../tree").resolve() == (workspace / "tree").resolve()
    assert "ID=rush-linux" in release and "VERSION_ID=0.7.0" in release
    assert (workspace / "base").resolve() == base.resolve()

    unmarked = tmp_path / "valuable"
    unmarked.mkdir()
    (unmarked / "keep").write_text("yes")
    with pytest.raises(builder.EditionError, match="unmarked directory"):
        builder.prepare_workspace(plan=plan, workspace=unmarked, base_tree=base, force=True)
    assert (unmarked / "keep").read_text() == "yes"


def test_raw_base_is_materialized_read_only_before_mkosi(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    plan, _ = make_plan(tmp_path)
    base_image = tmp_path / "rush-linux.raw"
    base_image.write_bytes(b"raw-image")
    workspace = tmp_path / "workspace"
    calls: list[list[str]] = []

    def fake_run(command: list[str], *, text: bool, check: bool) -> SimpleNamespace:
        calls.append(command)
        assert command[:3] == ["systemd-dissect", "--read-only", "--copy-from"]
        assert Path(command[3]) == base_image
        assert command[4] == "/"
        target = Path(command[5])
        (target / "etc").mkdir(parents=True)
        (target / "etc/os-release").write_text("ID=rush-linux\n", encoding="utf-8")
        return SimpleNamespace(returncode=0)

    monkeypatch.setattr(builder.subprocess, "run", fake_run)
    builder.prepare_workspace(
        plan=plan,
        workspace=workspace,
        base_tree=base_image,
        force=False,
    )

    assert len(calls) == 1
    assert (workspace / "base").is_dir()
    assert not (workspace / "base").is_symlink()
    assert (workspace / "base/etc/os-release").read_text() == "ID=rush-linux\n"
    image = (workspace / "mkosi.images/rush-linux-desktop/mkosi.conf").read_text()
    assert "BaseTrees=../../base" in image


def test_build_requires_signing_and_emits_receipts(tmp_path: Path) -> None:
    plan, base = make_plan(tmp_path)
    workspace = tmp_path / "workspace"
    builder.prepare_workspace(plan=plan, workspace=workspace, base_tree=base, force=False)
    mkosi = tmp_path / "mkosi"
    fake_mkosi(mkosi)

    with pytest.raises(builder.EditionError, match="release builds require"):
        builder.run_build(
            workspace=workspace,
            plan=plan,
            mkosi=str(mkosi),
            key=None,
            certificate=None,
            unsigned_development=False,
        )

    artifact = builder.run_build(
        workspace=workspace,
        plan=plan,
        mkosi=str(mkosi),
        key=None,
        certificate=None,
        unsigned_development=True,
    )
    receipt = json.loads((workspace / "output/rush-linux-desktop.build.json").read_text())
    assert artifact.read_bytes() == b"fake-sysext\n"
    assert receipt["signed"] is False
    assert len(receipt["artifact"]["sha256"]) == 64
    assert (workspace / "output/rush-linux-desktop.raw.sha256").is_file()


def test_signed_build_cleans_temporary_credentials(tmp_path: Path) -> None:
    plan, base = make_plan(tmp_path)
    workspace = tmp_path / "workspace"
    builder.prepare_workspace(plan=plan, workspace=workspace, base_tree=base, force=False)
    mkosi = tmp_path / "mkosi"
    fake_mkosi(mkosi, require_signing=True)
    key = tmp_path / "release.key"
    certificate = tmp_path / "release.crt"
    key.write_text("private")
    certificate.write_text("certificate")
    builder.run_build(
        workspace=workspace,
        plan=plan,
        mkosi=str(mkosi),
        key=key,
        certificate=certificate,
        unsigned_development=False,
    )
    receipt = json.loads((workspace / "output/rush-linux-desktop.build.json").read_text())
    assert receipt["signed"] is True
    assert len(receipt["certificate_sha256"]) == 64
    assert not (workspace / "mkosi.key").exists()
    assert not (workspace / "mkosi.crt").exists()
