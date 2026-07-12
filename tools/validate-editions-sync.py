#!/usr/bin/env python3
"""Validate that distro/editions/*.toml and mkosi/mkosi.profiles/*/mkosi.conf
agree on which editions exist.

Catches the drift class where:
  - distro/editions/laptop.toml exists but mkosi/mkosi.profiles/laptop/ doesn't
    (or vice versa)
  - the edition name in the toml doesn't match the directory name
  - the mkosi profile's ImageId doesn't follow the rush-linux-<edition> pattern

This validator is invoked by tools/validate-doc-sync.py as an additional
check, but can also be run standalone.
"""
from __future__ import annotations

import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EDITIONS_DIR = ROOT / "distro" / "editions"
PROFILES_DIR = ROOT / "mkosi" / "mkosi.profiles"


def collect_edition_tomls() -> dict[str, dict]:
    """Return {edition_name: parsed_toml} for every distro/editions/*.toml."""
    out: dict[str, dict] = {}
    if not EDITIONS_DIR.exists():
        return out
    for p in sorted(EDITIONS_DIR.glob("*.toml")):
        try:
            with p.open("rb") as f:
                data = tomllib.load(f)
            name = data.get("edition", {}).get("name", p.stem)
            out[name] = data
        except (OSError, tomllib.TOMLDecodeError) as e:
            print(f"  X {p.relative_to(ROOT)}: cannot parse: {e}", file=sys.stderr)
    return out


def collect_mkosi_profiles() -> dict[str, str]:
    """Return {edition_name: image_id} for every mkosi/mkosi.profiles/*/mkosi.conf."""
    out: dict[str, str] = {}
    if not PROFILES_DIR.exists():
        return out
    for d in sorted(PROFILES_DIR.iterdir()):
        if not d.is_dir():
            continue
        conf = d / "mkosi.conf"
        if not conf.exists():
            continue
        text = conf.read_text(encoding="utf-8", errors="replace")
        image_id = None
        for line in text.splitlines():
            line = line.strip()
            if line.startswith("ImageId="):
                image_id = line.split("=", 1)[1].strip()
                break
        out[d.name] = image_id or ""
    return out


def main() -> int:
    print("=" * 60)
    print("Rush Linux — Editions Cross-Check")
    print("=" * 60)

    editions = collect_edition_tomls()
    profiles = collect_mkosi_profiles()

    print(f"\n  distro/editions/*.toml   : {sorted(editions.keys())}")
    print(f"  mkosi/mkosi.profiles/*/  : {sorted(profiles.keys())}")

    errors: list[str] = []

    # 1. Every edition in distro/editions/ should have a mkosi profile.
    for name in sorted(editions):
        if name not in profiles:
            errors.append(
                f"edition '{name}' has distro/editions/{name}.toml but no "
                f"mkosi/mkosi.profiles/{name}/mkosi.conf"
            )

    # 2. Every mkosi profile should have an edition toml (informational —
    #    testos is a build profile, not a user-facing edition, so allow
    #    profiles without an edition toml).
    orphan_profiles = sorted(set(profiles) - set(editions))
    if orphan_profiles:
        print(f"\n  Build-only profiles (no distro/editions/*.toml): {orphan_profiles}")
        print("  (informational; testos is a build profile, not a user-facing edition)")

    # 3. mkosi profile ImageId should follow rush-linux-<edition> pattern.
    for name, image_id in sorted(profiles.items()):
        expected = f"rush-linux-{name}"
        if image_id != expected:
            errors.append(
                f"mkosi/mkosi.profiles/{name}/mkosi.conf: ImageId='{image_id}' "
                f"does not match expected '{expected}'"
            )

    print("\n" + "-" * 60)
    if errors:
        print(f"FAILED: {len(errors)} edition cross-check violation(s)")
        for e in errors:
            print(f"  X {e}")
        return 1
    print("PASSED: distro/editions/*.toml and mkosi/mkosi.profiles/*/ agree")
    return 0


if __name__ == "__main__":
    sys.exit(main())
