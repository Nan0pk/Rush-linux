#!/usr/bin/env python3
"""Tests for the Rush Linux edition system-extension builder."""

from __future__ import annotations

import json
import os
import stat
import subprocess
import sys
from pathlib import Path

import pytest


TOOL = Path(__file__).with_name("build-edition-sysext.py")


def run_tool(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, "-S", str(TOOL), *args],
        cwd=cwd,
        capture_output=True,
        text=True,
        timeout=20,
        check=False,
    )


def write_manifest(
    root: Path,
    name: str,
    *,
    inherits: str | None = None,
    packages: tuple[str, ...] = ("base-package",),
    extra_defaults: str = "",
    edition_name: str | None = None,
) -> None:
    inherited = f'inherits = "{inherits}"\n' if inherits else ""
    package_lines = "\n".join(f'  "{package}",' for package in packages)
    (root / f"{name}.toml").write_text(
        f"""[edition]
name = "{edition_name or name}"
description = "{name} edition"

[defaults]
{inherited}{extra_defaults}
[packages]
required = [
{package_lines}
]
""",
        encoding="utf-8",
    )


@pytest.fixture
def manifest_dir(tmp_path: Path) -> Path:
    root = tmp_path / "editions"
    root.mkdir()
    write_manifest(
        root,
        "desktop",
        packages=("desktop-shell", "linux-adaptive", "optid"),
        extra_defaults='filesystem = "btrfs"\noptimizer_mode = "auto"\n',
    )
    write_manifest(
        root,
        "laptop",
        inherits="desktop",
        packages=("desktop-shell", "linux-adaptive", "optid", "fwupd"),
        extra_defaults="runtime_pm = true\n",
    )
    write_manifest(
        root,
        "server",
        packages=("server-minimal", "linux-adaptive", "optid"),
        extra_defaults="desktop = false\n",
    )
    return root


def write_profile(root: Path, name: str, packages: tuple[str, ...]) -> None:
    profile = root / name
    profile.mkdir(parents=True)
    package_lines = "\n".join(f"    {package}" for package in packages)
    (profile / "mkosi.conf").write_text(
        f"""[Output]
ImageId=rush-linux-{name}

[Content]
Packages=
{package_lines}
""",
        encoding="utf-8",
    )


@pytest.fixture
def profile_dir(tmp_path: Path) -> Path:
    root = tmp_path / "profiles"
    write_profile(
        root,
        "desktop",
        ("plasma-workspace", "pipewire", "wireplumber"),
    )
    write_profile(
        root,
        "laptop",
        ("plasma-workspace", "pipewire", "wireplumber", "fwupd"),
    )
    write_profile(
        root,
        "server",
        (),
    )
    return root


@pytest.fixture
def version_file(tmp_path: Path) -> Path:
    path = tmp_path / "VERSION"
    path.write_text("0.7.0-beta.4\n", encoding="utf-8")
    return path


def plan_args(
    manifest_dir: Path,
    profile_dir: Path,
    version_file: Path,
    edition: str,
) -> list[str]:
    return [
        "plan",
        "--edition",
        edition,
        "--manifest-dir",
        str(manifest_dir),
        "--profile-dir",
        str(profile_dir),
        "--version-file",
        str(version_file),
    ]


def test_list_returns_sorted_valid_editions(
    manifest_dir: Path, profile_dir: Path
) -> None:
    result = run_tool(
        "list",
        "--manifest-dir",
        str(manifest_dir),
        "--profile-dir",
        str(profile_dir),
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.splitlines() == ["desktop", "laptop", "server"]


def test_plan_resolves_defaults_and_uses_explicit_child_package_payload(
    manifest_dir: Path, profile_dir: Path, version_file: Path
) -> None:
    result = run_tool(*plan_args(manifest_dir, profile_dir, version_file, "laptop"))
    assert result.returncode == 0, result.stderr
    plan = json.loads(result.stdout)
    assert plan["edition"]["inheritance"] == ["desktop", "laptop"]
    assert plan["edition"]["effective_defaults"] == {
        "filesystem": "btrfs",
        "optimizer_mode": "auto",
        "runtime_pm": True,
    }
    assert plan["packages"]["profile_mode"] == "complete-sysext-payload"
    assert plan["packages"]["edition_requirements"] == [
        "desktop-shell",
        "fwupd",
        "linux-adaptive",
        "optid",
    ]
    assert plan["packages"]["sysext"] == [
        "fwupd",
        "pipewire",
        "plasma-workspace",
        "wireplumber",
    ]
    assert plan["extension"]["id"] == "rush-linux-laptop"
    assert plan["extension"]["filename"] == "rush-linux-laptop.raw"
    assert plan["base_os"]["version_id"] == "0.7.0"


def test_profile_is_the_complete_sysext_package_payload(
    manifest_dir: Path, profile_dir: Path, version_file: Path
) -> None:
    write_manifest(
        manifest_dir,
        "realtime-audio",
        inherits="desktop",
        packages=("desktop-shell", "linux-adaptive-rt", "optid"),
    )
    write_profile(
        profile_dir,
        "realtime-audio",
        ("plasma-workspace", "pipewire", "realtime-privileges", "rtkit"),
    )
    result = run_tool(
        *plan_args(manifest_dir, profile_dir, version_file, "realtime-audio")
    )
    assert result.returncode == 0, result.stderr
    plan = json.loads(result.stdout)
    assert "linux-adaptive-rt" in plan["packages"]["edition_requirements"]
    assert "linux-adaptive-rt" not in plan["packages"]["sysext"]
    assert plan["packages"]["sysext"] == [
        "pipewire",
        "plasma-workspace",
        "realtime-privileges",
        "rtkit",
    ]


def test_server_plan_has_no_desktop_inheritance(
    manifest_dir: Path, profile_dir: Path, version_file: Path
) -> None:
    result = run_tool(*plan_args(manifest_dir, profile_dir, version_file, "server"))
    assert result.returncode == 0, result.stderr
    plan = json.loads(result.stdout)
    assert plan["edition"]["inheritance"] == ["server"]
    assert plan["packages"]["sysext"] == []
    assert "desktop-shell" not in plan["packages"]["edition_requirements"]
    assert plan["edition"]["effective_defaults"]["desktop"] is False


def test_filename_and_declared_name_must_match(
    manifest_dir: Path, profile_dir: Path, version_file: Path
) -> None:
    write_manifest(manifest_dir, "broken", edition_name="different")
    result = run_tool(*plan_args(manifest_dir, profile_dir, version_file, "broken"))
    assert result.returncode == 2
    assert "filename must match" in result.stderr


def test_duplicate_packages_are_rejected(
    manifest_dir: Path, profile_dir: Path, version_file: Path
) -> None:
    write_manifest(manifest_dir, "duplicate", packages=("one", "one"))
    result = run_tool(*plan_args(manifest_dir, profile_dir, version_file, "duplicate"))
    assert result.returncode == 2
    assert "duplicate required packages" in result.stderr


def test_inheritance_cycles_are_rejected(
    manifest_dir: Path, profile_dir: Path, version_file: Path
) -> None:
    write_manifest(manifest_dir, "first", inherits="second")
    write_manifest(manifest_dir, "second", inherits="first")
    result = run_tool(*plan_args(manifest_dir, profile_dir, version_file, "first"))
    assert result.returncode == 2
    assert "inheritance cycle" in result.stderr


def test_unknown_manifest_keys_are_rejected(
    manifest_dir: Path, profile_dir: Path, version_file: Path
) -> None:
    path = manifest_dir / "desktop.toml"
    path.write_text(path.read_text(encoding="utf-8") + "\n[magic]\nenabled = true\n")
    result = run_tool(*plan_args(manifest_dir, profile_dir, version_file, "desktop"))
    assert result.returncode == 2
    assert "unknown top-level keys" in result.stderr


def test_prepare_generates_real_mkosi_sysext_workspace(
    manifest_dir: Path, profile_dir: Path, version_file: Path, tmp_path: Path
) -> None:
    base = tmp_path / "base"
    base.mkdir()
    workspace = tmp_path / "workspace"
    result = run_tool(
        "prepare",
        "--edition",
        "laptop",
        "--manifest-dir",
        str(manifest_dir),
        "--profile-dir",
        str(profile_dir),
        "--version-file",
        str(version_file),
        "--base-tree",
        str(base),
        "--workspace",
        str(workspace),
    )
    assert result.returncode == 0, result.stderr

    shared = (workspace / "mkosi.conf").read_text()
    image = (workspace / "mkosi.images/rush-linux-laptop/mkosi.conf").read_text()
    release = (
        workspace
        / "tree/usr/lib/extension-release.d/extension-release.rush-linux-laptop"
    ).read_text()
    payload = json.loads(
        (workspace / "tree/usr/lib/rush-linux/editions/laptop.json").read_text()
    )

    assert "Format=none" in shared
    assert "Format=sysext" in image
    assert "Overlay=yes" in image
    assert "ImageId=rush-linux-laptop" in image
    assert "BaseTrees=base" in image
    assert "fwupd" in image
    assert "ID=rush-linux" in release
    assert "VERSION_ID=0.7.0" in release
    assert "ARCHITECTURE=x86-64" in release
    assert "SYSEXT_ID=rush-linux-laptop" in release
    assert payload["edition"]["inheritance"] == ["desktop", "laptop"]
    assert (workspace / "base").resolve() == base.resolve()


def test_force_never_replaces_unmarked_directory(
    manifest_dir: Path, profile_dir: Path, version_file: Path, tmp_path: Path
) -> None:
    base = tmp_path / "base"
    base.mkdiŠ
BˆÛÜšÜÜXÙHH\Ü]ÈÛÜšÜÜXÙH‚ˆÛÜšÜÜXÙK›ZÙ\Š
Bˆ
ÛÜšÜÜXÙHÈ˜[XX›KŠKÜš]WÝ^
šÙY\YHŠBˆ™\Ý[H[—ÝÛÛ
ˆœ™\\™H‹ˆ‹KYY][Ûˆ‹ˆ™\ÚÝÜ‹ˆ‹K[X[šY™\ÝY\ˆ‹ˆÝŠX[šY™\ÝÙ\ŠKˆ‹K\›Ùš[KY\ˆ‹ˆÝŠ›Ùš[WÙ\ŠKˆ‹K]™\œÚ[Û‹Yš[H‹ˆÝŠ™\œÚ[Û—Ùš[JKˆ‹KX˜\ÙK]™YH‹ˆÝŠ˜\ÙJKˆ‹K]ÛÜšÜÜXÙH‹ˆÝŠÛÜšÜÜXÙJKˆ‹KY›Ü˜ÙH‹ˆ
Bˆ\ÜÙ\™\Ý[œ™]\›˜ÛÙHOH‚ˆ\ÜÙ\œ™Y\Ú[™ÈÈ™\XÙH[›X\šÙY\™XÝÜžHˆ[ˆ™\Ý[œÝ\œ‚ˆ\ÜÙ\
ÛÜšÜÜXÙHÈ˜[XX›KŠKœ™XYÝ^

HOHšÙY\YH‚‚‚™YˆXZÙWÙ˜ZÙWÛZÛÜÚJ]ˆ]
‹™\]Z\™WÜÚYÛš[™Îˆ›ÛÛ
HOˆ›Û™N‚ˆÚYÛš[™×ØÚXÚÈH
ˆ	Ý\ÝSZÛÜÚKšÙ^H	‰ˆ\ÝSZÛÜÚK˜Ü‰ÂˆYˆ™\]Z\™WÜÚYÛš[™Âˆ[ÙH	Ý\ÝHYHZÛÜÚKšÙ^H	‰ˆ\ÝHYHZÛÜÚK˜Ü‰Âˆ
Bˆ]Üš]WÝ^
ˆˆÈKÝ\Ü‹Øš[‹Ù[ˆ˜\Úˆ‚ˆœÙ]Y][È\Y˜Z[ˆ‚ˆ	Ý\Ý‰HˆH‹Yˆ—‰Âˆ
ÈÚYÛš[™×ØÚXÚÂˆ
È›ZÙ\ˆ\Ý]]ˆ‚ˆ
È™Y][ÛI
ÙY[ˆ	ÜË×“Ý]]KËÜ	ÈZÛÜÚKš[XYÙ\ËÊ‹ÛZÛÜÚK˜ÛÛ™ŠWˆ‚ˆ
È	Üš[ˆ™˜ZÙK\Þ\Ù^ˆˆˆ›Ý]]ÉY][Ûˆ—‰Ëˆ[˜ÛÙ[™ÏH]‹N‹ˆ
Bˆ]˜Ú[Ù
]œÝ]

KœÝÛ[ÙHÝ]”×ÒVTÔŠB‚‚™YˆZ[Ø\™ÜÊˆX[šY™\ÝÙ\Žˆ]ˆ›Ùš[WÙ\Žˆ]ˆ™\œÚ[Û—Ùš[Nˆ]ˆ˜\ÙNˆ]ˆÛÜšÜÜXÙNˆ]ˆ˜ZÙWÛZÛÜÚNˆ]ŠHOˆ\ÝÜÝ—N‚ˆ™]\›ˆÂˆ˜Z[‹ˆ‹KYY][Ûˆ‹ˆ™\ÚÝÜ‹ˆ‹K[X[šY™\ÝY\ˆ‹ˆÝŠX[šY™\ÝÙ\ŠKˆ‹K\›Ùš[KY\ˆ‹ˆÝŠ›Ùš[WÙ\ŠKˆ‹K]™\œÚ[Û‹Yš[H‹ˆÝŠ™\œÚ[Û—Ùš[JKˆ‹KX˜\ÙK]™YH‹ˆÝŠ˜\ÙJKˆ‹K]ÛÜšÜÜXÙH‹ˆÝŠÛÜšÜÜXÙJKˆ‹K[ZÛÜÚH‹ˆÝŠ˜ZÙWÛZÛÜÚJKˆB‚‚™Yˆ\ÝØZ[Ü™\]Z\™\×ÜÚYÛš[™×Ý[›\Ü×Ù^XÚ]WÙ]™[ÜY[
ˆX[šY™\ÝÙ\Žˆ]›Ùš[WÙ\Žˆ]™\œÚ[Û—Ùš[Nˆ]\Ü]ˆ]ŠHOˆ›Û™N‚ˆ˜\ÙHH\Ü]È˜˜\ÙH‚ˆ˜\ÙK›ZÙ\Š
Bˆ˜ZÙWÛZÛÜÚHH\Ü]È›ZÛÜÚH‚ˆXZÙWÙ˜ZÙWÛZÛÜÚJ˜ZÙWÛZÛÜÚK™\]Z\™WÜÚYÛš[™ÏQ˜[ÙJBˆ™\Ý[H[—ÝÛÛ
ˆ
˜Z[Ø\™ÜÊˆX[šY™\ÝÙ\‹ˆ›Ùš[WÙ\‹ˆ™\œÚ[Û—Ùš[Kˆ˜\ÙKˆ\Ü]ÈÛÜšÜÜXÙH‹ˆ˜ZÙWÛZÛÜÚKˆ
Bˆ
Bˆ\ÜÙ\™\Ý[œ™]\›˜ÛÙHOH‚ˆ\ÜÙ\œ™[X\ÙHZ[È™\]Z\™HKZÙ^H[™KXÙ\YšXØ]Hˆ[ˆ™\Ý[œÝ\œ‚‚‚™Yˆ\ÝÝ[œÚYÛ™YÙ]™[ÜY[ØZ[Ù[Z]×Ú\ÚÜ™XÙZ\
ˆX[šY™\ÝÙ\Žˆ]›Ùš[WÙ\Žˆ]™\œÚ[Û—Ùš[Nˆ]\Ü]ˆ]ŠHOˆ›Û™N‚ˆ˜\ÙHH\Ü]È˜˜\ÙH‚ˆ˜\ÙK›ZÙ\Š
BˆÛÜšÜÜXÙHH\Ü]ÈÛÜšÜÜXÙH‚ˆ˜ZÙWÛZÛÜÚHH\Ü]È›ZÛÜÚH‚ˆXZÙWÙ˜ZÙWÛZÛÜÚJ˜ZÙWÛZÛÜÚK™\]Z\™WÜÚYÛš[™ÏQ˜[ÙJBˆ™\Ý[H[—ÝÛÛ
ˆ
˜Z[Ø\™ÜÊX[šY™\ÝÙ\‹›Ùš[WÙ\‹™\œÚ[Û—Ùš[K˜\ÙKÛÜšÜÜXÙK˜ZÙWÛZÛÜÚJKˆ‹K][œÚYÛ™YY]™[ÜY[‹ˆ
Bˆ\ÜÙ\™\Ý[œ™]\›˜ÛÙHOH™\Ý[œÝ\œ‚ˆ\Y˜XÝHÛÜšÜÜXÙHÈ›Ý]]Ü\Ú[[^Y\ÚÝÜœ˜]È‚ˆ™XÙZ\HœÛÛ‹›ØYÊˆ
ÛÜšÜÜXÙHÈ›Ý]]Ü\Ú[[^Y\ÚÝÜ˜Z[šœÛÛˆŠKœ™XYÝ^

Bˆ
Bˆ\ÜÙ\\Y˜XÝœ™XYØž]\Ê
HOHˆ™˜ZÙK\Þ\Ù^ˆ‚ˆ\ÜÙ\™XÙZ\ÈœÚYÛ™Y—H\È˜[ÙBˆ\ÜÙ\™XÙZ\È˜Ù\YšXØ]WÜÚLMˆ—H\È›Û™Bˆ\ÜÙ\™XÙZ\È˜\Y˜XÝ—VÈœÚ^™WØž]\È—HOH[Šˆ™˜ZÙK\Þ\Ù^ˆŠBˆ\ÜÙ\[Š™XÙZ\È˜\Y˜XÝ—VÈœÚLMˆ—JHOHˆ\ÜÙ\
ÛÜšÜÜXÙHÈ›Ý]]Ü\Ú[[^Y\ÚÝÜœ˜]ËœÚLMˆŠKš\×Ùš[J
B‚‚™Yˆ\ÝÜÚYÛ™YØZ[Ù^ÜÙ\×ÚÙ^\×ÛÛ›WÙ\š[™×ÛZÛÜÚWÚ[›ØØ][ÛŠˆX[šY™\ÝÙ\Žˆ]›Ùš[WÙ\Žˆ]™\œÚ[Û—Ùš[Nˆ]\Ü]ˆ]ŠHOˆ›Û™N‚ˆ˜\ÙHH\Ü]È˜˜\ÙH‚ˆ˜\ÙK›ZÙ\Š
BˆÛÜšÜÜXÙHH\Ü]ÈÛÜšÜÜXÙH‚ˆ˜ZÙWÛZÛÜÚHH\Ü]È›ZÛÜÚH‚ˆXZÙWÙ˜ZÙWÛZÛÜÚJ˜ZÙWÛZÛÜÚK™\]Z\™WÜÚYÛš[™ÏUYJBˆÙ^HH\Ü]Èœ™[X\ÙKšÙ^H‚ˆÙ\YšXØ]HH\Ü]Èœ™[X\ÙK˜Ü‚ˆÙ^KÜš]WÝ^
œš]˜]HŠBˆÙ\YšXØ]KÜš]WÝ^
˜Ù\YšXØ]HŠB‚ˆ™\Ý[H[—ÝÛÛ
ˆ
˜Z[Ø\™ÜÊX[šY™\ÝÙ\‹›Ùš[WÙ\‹™\œÚ[Û—Ùš[K˜\ÙKÛÜšÜÜXÙK˜ZÙWÛZÛÜÚJKˆ‹KZÙ^H‹ˆÝŠÙ^JKˆ‹KXÙ\YšXØ]H‹ˆÝŠÙ\YšXØ]JKˆ
Bˆ\ÜÙ\™\Ý[œ™]\›˜ÛÙHOH™\Ý[œÝ\œ‚ˆ™XÙZ\HœÛÛ‹›ØYÊˆ
ÛÜšÜÜXÙHÈ›Ý]]Ü\Ú[[^Y\ÚÝÜ˜Z[šœÛÛˆŠKœ™XYÝ^

Bˆ
Bˆ\ÜÙ\™XÙZ\ÈœÚYÛ™Y—H\ÈYBˆ\ÜÙ\[Š™XÙZ\È˜Ù\YšXØ]WÜÚLMˆ—JHOHˆ\ÜÙ\›Ý
ÛÜšÜÜXÙHÈ›ZÛÜÚKšÙ^HŠK™^\ÝÊ
Bˆ\ÜÙ\›Ý
ÛÜšÜÜXÙHÈ›ZÛÜÚK˜ÜŠK™^\ÝÊ
Bˆ\ÜÙ\›Ý
ÛÜšÜÜXÙHÈ›ZÛÜÚKšÙ^HŠKš\×ÜÞ[[[šÊ
Bˆ\ÜÙ\›Ý
ÛÜšÜÜXÙHÈ›ZÛÜÚK˜ÜŠKš\×ÜÞ[[[šÊ
B‚‚™Yˆ\ÝÜ™X[Ü™\ÜÚ]ÜžWÛX[šY™\Ý×Ø[Ü™\ÛÛ™J
HOˆ›Û™N‚ˆ™\×Ü›ÛÝH]
×Ùš[W×ÊKœ™\ÛÛ™J
Kœ\™[œ\™[ˆX[šY™\ÝÜ›ÛÝH™\×Ü›ÛÝÈ™\Ý›ÈˆÈ™Y][ÛœÈ‚ˆ›Ùš[WÜ›ÛÝH™\×Ü›ÛÝÈ›ZÛÜÚHˆÈ›ZÛÜÚKœ›Ùš[\È‚ˆ™\œÚ[ÛˆH™\×Ü›ÛÝÈ•‘T”ÒSÓˆ‚ˆ™\Ý[H[—ÝÛÛ
ˆ›\Ý‹ˆ‹K[X[šY™\ÝY\ˆ‹ˆÝŠX[šY™\ÝÜ›ÛÝ
Kˆ‹K\›Ùš[KY\ˆ‹ˆÝŠ›Ùš[WÜ›ÛÝ
Kˆ
Bˆ\ÜÙ\™\Ý[œ™]\›˜ÛÙHOH™\Ý[œÝ\œ‚ˆ˜[Y\ÈH™\Ý[œÝÝ]œÜ][™\Ê
Bˆ\ÜÙ\˜[Y\ÈOHÈ™\ÚÝÜ‹›\Ü‹œ™X[[YKX]Y[È‹œÙ\™\ˆ—Bˆ›Üˆ˜[YH[ˆ˜[Y\Î‚ˆ[›™YH[—ÝÛÛ

œ[—Ø\™ÜÊX[šY™\ÝÜ›ÛÝ›Ùš[WÜ›ÛÝ™\œÚ[Û‹˜[YJJBˆ\ÜÙ\[›™Yœ™]\›˜ÛÙHOH[›™YœÝ\œ‚ˆ^[ØYHœÛÛ‹›ØYÊ[›™YœÝÝ]
Bˆ\ÜÙ\^[ØYÈ™Y][Ûˆ—VÈ›˜[YH—HOH˜[YBˆ\ÜÙ\œÞ\Ù^ˆ[ˆ^[ØYÈœXÚØYÙ\È—BˆYˆ˜[YHOHœÙ\™\ˆŽ‚ˆ\ÜÙ\^[ØYÈœXÚØYÙ\È—VÈœÞ\Ù^—HOH×BˆYˆ˜[YHOHœ™X[[YKX]Y[ÈŽ‚ˆ\ÜÙ\›[^XY\]™K\ˆ[ˆ^[ØYÈœXÚØYÙ\È—VÈ™Y][Û—Ü™\]Z\™[Y[È—Bˆ\ÜÙ\›[^XY\]™K\ˆ›Ý[ˆ^[ØYÈœXÚØYÙ\È—VÈœÞ\Ù^—B‚‚šYˆ×Û˜[YW×ÈOH—×ÛXZ[—×ÈŽ‚ˆ˜Z\ÙHÞ\Ý[Q^]
]\Ý›XZ[Š××Ùš[W×Ë‹]ˆ‹
œÞ\Ë˜\™Ý–ÌN—WJJB