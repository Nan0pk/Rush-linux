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

def get_releases_version() -> str:
    releases = Path("RELEASES.md").read_text()
    match = re.search(r"Current project version:\s*`([^`]+)`", releases)
    if not match:
        print("ERROR: Could not find version in RELEASES.md")
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

def main():
    print("=" * 60)
    print("Rush Linux — Version Consistency Check")
    print("=" * 60)
    
    version_file = get_version_file()
    cargo_version = get_cargo_workspace_version()
    roadmap_version = get_roadmap_version()
    releases_version = get_releases_version()
    milestones_version = get_milestones_version()
    
    print(f"\nVERSION file:              {version_file}")
    print(f"Cargo.toml workspace:      {cargo_version}")
    print(f"ROADMAP.md:                {roadmap_version}")
    print(f"RELEASES.md:               {releases_version}")
    print(f"milestones.toml:           {milestones_version}")
    
    errors = []
    
    if version_file != cargo_version:
        errors.append(f"VERSION ({version_file}) != Cargo.toml ({cargo_version})")
    
    if version_file != roadmap_version:
        errors.append(f"VERSION ({version_file}) != ROADMAP.md ({roadmap_version})")
    
    if version_file != releases_version:
        errors.append(f"VERSION ({version_file}) != RELEASES.md ({releases_version})")
    
    if version_file != milestones_version:
        errors.append(f"VERSION ({version_file}) != milestones.toml ({milestones_version})")
    
    if not check_crate_versions_use_workspace():
        errors.append("Crates do not use workspace version")
    
    print("\n" + "-" * 60)
    if errors:
        print("FAILED: Version mismatch detected")
        for err in errors:
            print(f"  ✗ {err}")
        print("-" * 60)
        sys.exit(1)
    else:
        print("PASSED: All versions consistent")
        print("-" * 60)
        sys.exit(0)

if __name__ == "__main__":
    main()
