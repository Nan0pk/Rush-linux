#!/usr/bin/env python3
import argparse
import sys
import os
import tomllib
import json
import subprocess
import shutil
import hashlib
import tarfile
from pathlib import Path

def cmd_build(args):
    recipe_path = Path(args.recipe)
    if not recipe_path.exists():
        print(f"Error: recipe {recipe_path} not found", file=sys.stderr)
        sys.exit(1)
        
    with open(recipe_path, "rb") as f:
        recipe = tomllib.load(f)
        
    pkg = recipe.get("package", {})
    name = pkg.get("name")
    version = pkg.get("version", "0.1.0")
    kind = pkg.get("kind")
    
    if not name:
        print("Error: package name is missing in recipe", file=sys.stderr)
        sys.exit(1)
        
    print(f"Building package: {name} (version {version}, kind {kind})")
    
    repo_root = Path(__file__).resolve().parent.parent
    build_tmp = repo_root / "build" / "tmp" / name
    if build_tmp.exists():
        shutil.rmtree(build_tmp)
    
    pkg_rootfs = build_tmp / "rootfs"
    pkg_rootfs.mkdir(parents=True)
    
    build_cfg = recipe.get("build", {})
    build_cmd = build_cfg.get("command")
    if build_cmd:
        print(f"Running build command: {build_cmd}")
        src_cfg = recipe.get("source", {})
        cwd_path = repo_root
        if "path" in src_cfg:
            cwd_path = repo_root / src_cfg["path"]
            
        res = subprocess.run(build_cmd, shell=True, cwd=cwd_path)
        if res.returncode != 0:
            print("Error: build command failed", file=sys.stderr)
            sys.exit(1)
            
    install_cfg = recipe.get("install", {})
    
    def copy_file(src_rel, dest_abs):
        src_path = repo_root / src_rel
        dest_rel = dest_abs.lstrip("/")
        dest_path = pkg_rootfs / dest_rel
        dest_path.parent.mkdir(parents=True, exist_ok=True)
        if src_path.is_dir():
            shutil.copytree(src_path, dest_path, dirs_exist_ok=True)
        else:
            shutil.copy2(src_path, dest_path)
        print(f"  Staged: {src_rel} -> /{dest_rel}")
        
    for item in install_cfg.get("bins", []):
        copy_file(item[0], item[1])
        
    for item in install_cfg.get("systemd_units", []):
        unit_name = Path(item).name
        copy_file(item, f"/usr/lib/systemd/system/{unit_name}")
        
    for item in install_cfg.get("config", []):
        copy_file(item[0], item[1])
        
    packages_dir = repo_root / "build" / "packages"
    packages_dir.mkdir(parents=True, exist_ok=True)
    
    archive_name = f"{name}-{version}.tar.gz"
    archive_path = packages_dir / archive_name
    print(f"Creating archive: {archive_path.name}")
    
    with tarfile.open(archive_path, "w:gz") as tar:
        for root, _, files in os.walk(pkg_rootfs):
            for file in files:
                file_path = Path(root) / file
                rel_path = file_path.relative_to(pkg_rootfs)
                tar.add(file_path, arcname=str(rel_path).replace("\\", "/"))
                
    file_list = []
    for root, _, files in os.walk(pkg_rootfs):
        for file in files:
            file_path = Path(root) / file
            rel_path = file_path.relative_to(pkg_rootfs)
            file_list.append("/" + str(rel_path).replace("\\", "/"))
            
    sha256_hash = hashlib.sha256()
    with open(archive_path, "rb") as f:
        for byte_block in iter(lambda: f.read(4096), b""):
            sha256_hash.update(byte_block)
    checksum = sha256_hash.hexdigest()
    
    depends_cfg = recipe.get("depends", {})
    required_deps = depends_cfg.get("required", [])
    
    metadata = {
        "name": name,
        "version": version,
        "kind": kind,
        "checksum": checksum,
        "files": file_list,
        "depends": {
            "required": required_deps
        }
    }
    
    metadata_path = packages_dir / f"{name}-{version}.json"
    with open(metadata_path, "w") as f:
        json.dump(metadata, f, indent=2)
    print(f"Created package metadata: {metadata_path.name}")
    
    shutil.rmtree(build_tmp)
    print(f"Package {name} built successfully!\n")

def cmd_repo_init(args):
    repo_dir = Path(args.repo_dir)
    if not repo_dir.exists():
        print(f"Error: repo directory {repo_dir} not found", file=sys.stderr)
        sys.exit(1)
        
    print(f"Initializing repository database in {repo_dir}...")
    packages = []
    for file in repo_dir.glob("*.json"):
        if file.name == "repodata.json":
            continue
        try:
            with open(file, "r") as f:
                metadata = json.load(f)
                packages.append(metadata)
        except Exception as e:
            print(f"Warning: failed to read metadata file {file.name}: {e}", file=sys.stderr)
            
    repodata = {
        "packages": packages
    }
    
    repodata_path = repo_dir / "repodata.json"
    with open(repodata_path, "w") as f:
        json.dump(repodata, f, indent=2)
    print(f"Generated repository metadata: {repodata_path.name}")
    
    repodata_hash = hashlib.sha256()
    with open(repodata_path, "rb") as f:
        repodata_hash.update(f.read())
    
    mock_key_id = "RUSH_LINUX_MOCK_KEY_2026"
    signature_data = {
        "key_id": mock_key_id,
        "hash": repodata_hash.hexdigest(),
        "signature": f"mock_sig_for_{repodata_hash.hexdigest()}_using_{mock_key_id}"
    }
    
    sig_path = repo_dir / "repodata.json.sig"
    with open(sig_path, "w") as f:
        json.dump(signature_data, f, indent=2)
    print(f"Generated repository signature stub: {sig_path.name}")
    print("Repository initialized successfully!\n")

def cmd_rootfs_create(args):
    rootfs_dir = Path(args.rootfs_dir)
    edition_path = Path(args.edition)
    repo_dir = Path(args.repo)
    
    if not edition_path.exists():
        print(f"Error: edition recipe {edition_path} not found", file=sys.stderr)
        sys.exit(1)
    if not repo_dir.exists():
        print(f"Error: repository {repo_dir} not found", file=sys.stderr)
        sys.exit(1)
        
    repodata_path = repo_dir / "repodata.json"
    if not repodata_path.exists():
        print(f"Error: repodata.json not found in repository. Run repo-init first.", file=sys.stderr)
        sys.exit(1)
        
    with open(repodata_path, "r") as f:
        repodata = json.load(f)
        
    with open(edition_path, "rb") as f:
        edition_recipe = tomllib.load(f)
        
    edition_pkg = edition_recipe.get("package", {})
    edition_name = edition_pkg.get("name")
    print(f"Creating rootfs for edition: {edition_name}")
    
    repo_pkgs = {pkg["name"]: pkg for pkg in repodata.get("packages", [])}
    
    edition_depends = edition_recipe.get("depends", {})
    initial_deps = edition_depends.get("required", [])
    
    resolved_packages = []
    visited = set()
    
    def resolve(pkg_name):
        if pkg_name in visited:
            return
        visited.add(pkg_name)
        
        if pkg_name in repo_pkgs:
            pkg_meta = repo_pkgs[pkg_name]
            for dep in pkg_meta.get("depends", {}).get("required", []):
                resolve(dep)
            resolved_packages.append(pkg_name)
        else:
            print(f"Warning: package {pkg_name} is a base/system dependency and not present in the local repository (will be assumed as provided by the base system).")
            
    for dep in initial_deps:
        resolve(dep)
        
    print(f"Resolved dependency sequence: {resolved_packages}")
    
    if rootfs_dir.exists():
        shutil.rmtree(rootfs_dir)
    rootfs_dir.mkdir(parents=True)
    
    for subdir in ["etc", "boot", "usr/bin", "usr/lib", "usr/libexec", "usr/sbin", "var/lib"]:
        (rootfs_dir / subdir).mkdir(parents=True, exist_ok=True)
        
    for pkg_name in resolved_packages:
        pkg_meta = repo_pkgs[pkg_name]
        version = pkg_meta["version"]
        archive_name = f"{pkg_name}-{version}.tar.gz"
        archive_path = repo_dir / archive_name
        
        if not archive_path.exists():
            print(f"Error: package archive {archive_path.name} not found in repository", file=sys.stderr)
            sys.exit(1)
            
        print(f"Installing package: {pkg_name} ({version})")
        with tarfile.open(archive_path, "r:gz") as tar:
            tar.extractall(path=rootfs_dir)
            
    print(f"Rootfs populated successfully at {rootfs_dir}!\n")

def cmd_vm_image(args):
    rootfs_dir = Path(args.rootfs_dir)
    output_raw = Path(args.output)
    
    if not rootfs_dir.exists():
        print(f"Error: rootfs directory {rootfs_dir} not found", file=sys.stderr)
        sys.exit(1)
        
    print(f"Generating bootable raw VM disk image: {output_raw}")
    
    repo_root = Path(__file__).resolve().parent.parent
    repart_defs = repo_root / "build" / "repart.d"
    if repart_defs.exists():
        shutil.rmtree(repart_defs)
    repart_defs.mkdir(parents=True)
    
    root_part_def = """
[Partition]
Type=root-x86-64
Format=ext4
CopyFiles=/
Label=RushLinuxRoot
"""
    with open(repart_defs / "50-root.conf", "w") as f:
        f.write(root_part_def)
        
    repart_cmd = [
        "systemd-repart",
        "--empty=create",
        "--size=100M",
        "--dry-run=no",
        f"--definitions={repart_defs}",
        f"--root={rootfs_dir}",
        str(output_raw)
    ]
    
    print(f"Running command: {' '.join(repart_cmd)}")
    res = subprocess.run(repart_cmd)
    
    shutil.rmtree(repart_defs)
    
    if res.returncode != 0:
        print("Error: systemd-repart failed to create VM disk image", file=sys.stderr)
        sys.exit(1)
        
    print(f"Bootable raw VM image created successfully at {output_raw}!\n")

def main():
    parser = argparse.ArgumentParser(description="Rush Linux Package & Rootfs Builder")
    subparsers = parser.add_subparsers(dest="command", required=True)
    
    # build parser
    parser_build = subparsers.add_parser("build", help="Build a package from a recipe")
    parser_build.add_argument("recipe", help="Path to the recipe TOML file")
    
    # repo-init parser
    parser_repo = subparsers.add_parser("repo-init", help="Initialize repository database metadata and signatures")
    parser_repo.add_argument("repo_dir", help="Path to repository packages directory")
    
    # rootfs-create parser
    parser_rootfs = subparsers.add_parser("rootfs-create", help="Create rootfs directory from edition recipe and repository")
    parser_rootfs.add_argument("rootfs_dir", help="Path to output rootfs directory")
    parser_rootfs.add_argument("edition", help="Path to the edition recipe TOML file")
    parser_rootfs.add_argument("--repo", required=True, help="Path to repository packages directory")
    
    # vm-image parser
    parser_vm = subparsers.add_parser("vm-image", help="Compile rootfs directory into a bootable VM raw disk image using systemd-repart")
    parser_vm.add_argument("rootfs_dir", help="Path to input rootfs directory")
    parser_vm.add_argument("output", help="Path to output raw disk image file")
    
    args = parser.parse_parse_args = parser.parse_args()
    
    if args.command == "build":
        cmd_build(args)
    elif args.command == "repo-init":
        cmd_repo_init(args)
    elif args.command == "rootfs-create":
        cmd_rootfs_create(args)
    elif args.command == "vm-image":
        cmd_vm_image(args)

if __name__ == "__main__":
    main()
