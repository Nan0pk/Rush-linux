#!/usr/bin/env python3
import sys
import os
import subprocess
import shutil
from pathlib import Path

def main():
    repo_root = Path(__file__).resolve().parent.parent
    
    # 1. Clean previous build artifacts
    build_dir = repo_root / "build"
    if build_dir.exists():
        shutil.rmtree(build_dir)
        
    print("--- 1. Building optid package ---")
    # Activate Rust environment if present
    env_setup = "source $HOME/.cargo/env; " if Path(os.environ.get("HOME", "") + "/.cargo/env").exists() else ""
    # We'll run via shell to make sure env is evaluated or standard path works
    res = subprocess.run(
        "python3 tools/rush-builder.py build recipes/core/optid.toml",
        shell=True,
        cwd=repo_root
    )
    assert res.returncode == 0, "Build optid package failed"
    
    print("--- 2. Initializing repository ---")
    res = subprocess.run([
        "python3", "tools/rush-builder.py", "repo-init", "build/packages"
    ], cwd=repo_root)
    assert res.returncode == 0, "Repo-init failed"
    
    print("--- 3. Creating rootfs from minimal edition ---")
    res = subprocess.run([
        "python3", "tools/rush-builder.py", "rootfs-create", "build/rootfs", "recipes/server/minimal.toml", "--repo", "build/packages"
    ], cwd=repo_root)
    assert res.returncode == 0, "Rootfs-create failed"
    
    print("--- 3.5. Building UKI and Initrd ---")
    res = subprocess.run([
        "python3", "tools/rush-builder.py", "build-uki", "build/rootfs"
    ], cwd=repo_root)
    assert res.returncode == 0, "Build-uki failed"
    
    print("--- 4. Creating VM image ---")
    res = subprocess.run([
        "python3", "tools/rush-builder.py", "vm-image", "build/rootfs", "build/disk.raw"
    ], cwd=repo_root)
    assert res.returncode == 0, "Vm-image creation failed"
    
    # 5. Assertions on generated files
    assert (build_dir / "packages" / "optid-0.1.0.tar.gz").exists(), "Archive not found"
    assert (build_dir / "packages" / "repodata.json").exists(), "repodata.json not found"
    assert (build_dir / "packages" / "repodata.json.sig").exists(), "repodata.json.sig not found"
    assert (build_dir / "rootfs" / "usr" / "bin" / "optctl").exists(), "optctl binary not found in rootfs"
    assert (build_dir / "rootfs" / "usr" / "libexec" / "optid").exists(), "optid binary not found in rootfs"
    assert (build_dir / "initrd.img").exists(), "initrd.img not found"
    assert (build_dir / "rootfs" / "boot" / "EFI" / "Linux" / "rush-linux.efi").exists(), "rush-linux.efi UKI not found"
    assert (build_dir / "rootfs" / "boot" / "EFI" / "BOOT" / "BOOTX64.EFI").exists(), "BOOTX64.EFI fallback bootloader not found"
    assert (build_dir / "disk.raw").exists(), "disk.raw VM image not found"
    
    print("\nAll integration tests for rush-builder passed successfully!")

if __name__ == "__main__":
    main()
