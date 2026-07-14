#!/usr/bin/env python3
"""
Validate version consistency across the repository.

Ensures VERSION file matches:
- Cargo.toml workspace.package.version
- ROADMAP.md "Current project version"
- release/milestones.toml current_version
- crates use version.workspace = true
"""

import re
import sys
from pathlib import Path
import tomllib

def get_version_file() -> str:
    version_path = Path("VERSION")
    if not version_path.exists():
        print("ERROR: VERSION file not found")
        sys.exit(1)
    return version_path.read_text().strip()

def get_cargo_workspace_version() -> str:
    cargo_path = Path("Cargo.toml")
    with cargo_path.open("rb") as f:
        data = tomllib.load(f)
    return data["workspace"]["package"]["version"]

def get_roadmap_version() -> str:
    roadmap = Path("ROADMAP.md").read_text()
    match = re.search(r"Current project version:\s*`([^`]+)`", roadmap)
    if not match:
        print("ERROR: Could not find version in ROADMAP.md")
        sys.exit(1)
    return match.group(1)

def get_milestones_version() -> str:
    milestones_path = Path("release/milestones.toml")
    with milestones_path.open("rb") as f:
        data = tomllib.load(f)
    return data["project"]["current_version"]

def check_crate_versions_use_workspace() -> bool:
    """Ensure crates use version.workspace = true"""
    ok = True
    for crate in ["crates/optid/Cargo.toml", "crates/optctl/Cargo.toml"]:
        path = Path(crate)
        content = path.read_text()
        if 'version = "0.' in content or 'version="0.' in content:
            print(f"ERROR: {crate} has hardcoded version, should use version.workspace = true")
            ok = False
        if "version.workspace = true" not in content:
            print(f"ERROR: {crate} missing version.workspace = true")
            ok = False
    return ok

def get_readme_release_version() -> str | None:
    """Pull the 'Latest release' version out of README.md.

    The README contains a line like:
      > **Latest release: [v0.7.0-beta.1](https://...)**
    We extract the tag name `v0.7.0-beta.1` and strip the leading `v` so it
    is comparable to VERSION. Returns None if the line is not found.
    """
    readme = Path("README.md")
    if not readme.exists():
        return None
    text = readme.read_text(encoding="utf-8", errors="replace")
    import re as _re
    m = _re.search(r"Latest release:\s*\[v([^\]]+)\]", text)
    return m.group(1) if m else None


def get_readme_badge_version() -> str | None:
    """Pull the version out of the README badge URL.

    The README contains a badge like:
      <img src="https://img.shields.io/badge/version-0.7.0--beta.1-blue?..." alt="version">
    (note the double dash before `beta` — shields.io's slug format).
    We extract `0.7.0--beta.1` from the URL and un-slug it back to `0.7.0-beta.1`.
    """
    readme = Path("README.md")
    if not readme.exists():
        return None
    text = readme.read_text(encoding="utf-8", errors="replace")
    import re as _re
    m = _re.search(r"shields\.io/badge/version-([0-9a-zA-Z.\-]+)-blue", text)
    if not m:
        return None
    slug = m.group(1)
    return slug.replace("--", "-")


def check_testos_install_sh_version(expected: str) -> list[str]:
    """The example cache paths in testos/install.sh must use the current version,
    not a stale or unreleased one. Comments only — but stale comments mislead.
    """
    errors: list[str] = []
    p = Path("testos/install.sh")
    if not p.exists():
        return errors
    text = p.read_text(encoding="utf-8", errors="replace")
    import re as _re
    # Find every `0.X.Y-tag.N` literal inside the file.
    found = set(_re.findall(r"\b0\.\d+\.\d+-(?:alpha|beta|rc)\.\d+\b", text))
    for v in found:
        if v != expected:
            errors.append(
                f"testos/install.sh mentions version '{v}' but VERSION is '{expected}'"
            )
    return errors


def main():
    print("=" * 60)
    print("Rush Linux — Version Consistency Check")
    print("=" * 60)
    
    version_file = get_version_file()
    cargo_version = get_cargo_workspace_version()
    roadmap_version = get_roadmap_version()
    milestones_version = get_milestones_version()
    readme_release = get_readme_release_version()
    readme_badge = get_readme_badge_version()
    
    print(f"\nVERSION file:              {version_file}")
    print(f"Cargo.toml workspace:      {cargo_version}")
    print(f"ROADMAP.md:                {roadmap_version}")
    print(f"milestones.toml:           {milestones_version}")
    print(f"README 'Latest release':   {readme_release}")
    print(f"README badge:              {readme_badge}")
    
    errors = []
    
    if version_file != cargo_version:
        errors.append(f"VERSION ({version_file}) != Cargo.toml ({cargo_version})")
    
    if version_file != roadmap_version:
        errors.append(f"VERSION ({version_file}) != ROADMAP.md ({roadmap_version})")
    
    if version_file != milestones_version:
        errors.append(f"VERSION ({version_file}) != milestones.toml ({milestones_version})")

    if readme_release and version_file != readme_release:
        errors.append(
            f"VERSION ({version_file}) != README 'Latest release' ({readme_release})"
        )

    if readme_badge and version_file != readme_badge:
        errors.append(
            f"VERSION ({version_file}) != README badge ({readme_badge})"
        )

    errors.extend(check_testos_install_sh_version(version_file))
    
    if not check_crate_versions_use_workspace():
        errors.append("Crates do not use workspace version")
    
    print("\n" + "-" * 60)
    if errors:
        print("FAILED: Version mismatch detected")
        for err in errors:
            print(f"  X {err}")
        print("-" * 60)
        sys.exit(1)
    else:
        print("PASSED: All versions consistent")
        print("-" * 60)
        sys.exit(0)

if __name__ == "__main__":
    main()
