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
import gzip
import io
from pathlib import Path

# Highest recipe schema version this builder understands. Recipes declare
# `schema_version` under [package]; see docs/packaging-and-builds.md for the
# versioning and migration policy.
SUPPORTED_SCHEMA_VERSION = 0

KERNEL_VERSION = "6.1.0-49-amd64"
KERNEL_DEB = "linux-image-6.1.0-49-amd64_6.1.174-1_amd64.deb"
BUSYBOX_DEB = "busybox-static_1.35.0-4+deb12u1+b1_amd64.deb"
SYSTEMD_BOOT_DEB = "systemd-boot-efi_252.39-1~deb12u2_amd64.deb"
DEFAULT_VM_ROOT_DEVICE = "/dev/vda2"
DEFAULT_VM_CONSOLE = "ttyS0,115200"
UKI_ESP_PATH = "/EFI/Linux/rush-linux.efi"

# Modules needed for the Debian kernel asset to see the virtio-backed ext4
# root partition used by the v0.3/v0.4 QEMU image. The initrd loader treats
# missing modules as non-fatal so the list can be narrowed later for kernels
# with some drivers built in.
ESSENTIAL_INITRD_MODULES = [
    "kernel/drivers/virtio/virtio.ko",
    "kernel/drivers/virtio/virtio_ring.ko",
    "kernel/drivers/virtio/virtio_pci_legacy_dev.ko",
    "kernel/drivers/virtio/virtio_pci_modern_dev.ko",
    "kernel/drivers/virtio/virtio_pci.ko",
    "kernel/drivers/block/virtio_blk.ko",
    "kernel/crypto/crc32c_generic.ko",
    "kernel/lib/libcrc32c.ko",
    "kernel/lib/crc16.ko",
    "kernel/fs/jbd2/jbd2.ko",
    "kernel/fs/mbcache.ko",
    "kernel/fs/ext4/ext4.ko",
]


def check_schema_version(pkg, recipe_path):
    """Validate the recipe's declared schema_version. Returns the version int."""
    schema_version = pkg.get("schema_version")
    if schema_version is None:
        print(
            f"Warning: {recipe_path} has no [package].schema_version; "
            f"assuming {SUPPORTED_SCHEMA_VERSION}. Add 'schema_version = "
            f"{SUPPORTED_SCHEMA_VERSION}' to make this explicit.",
            file=sys.stderr,
        )
        return SUPPORTED_SCHEMA_VERSION
    if not isinstance(schema_version, int) or schema_version > SUPPORTED_SCHEMA_VERSION:
        print(
            f"Error: {recipe_path} declares schema_version {schema_version!r}, "
            f"but this builder only supports up to {SUPPORTED_SCHEMA_VERSION}.",
            file=sys.stderr,
        )
        sys.exit(1)
    return schema_version

def cmd_build(args):
    recipe_path = Path(args.recipe)
    if not recipe_path.exists():
        print(f"Error: recipe {recipe_path} not found", file=sys.stderr)
        sys.exit(1)
        
    with open(recipe_path, "rb") as f:
        recipe = tomllib.load(f)
        
    pkg = recipe.get("package", {})
    schema_version = check_schema_version(pkg, recipe_path)
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
        "schema_version": schema_version,
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
    
    # Sign repodata.json using the real signing tool if available,
    # otherwise fall back to the mock stub.
    repodata_hash = hashlib.sha256()
    with open(repodata_path, "rb") as f:
        repodata_hash.update(f.read())

    repo_root = Path(__file__).resolve().parent.parent
    sign_tool = repo_root / "tools" / "sign_updates.py"
    # Test signing keys live under build/test-signing/keys/ (generated by
    # tools/sign_updates.py init-keys or tools/test-sign-updates.sh). The
    # historical config/keys/ location is .gitignored for private keys;
    # we still check it as a fallback so existing operator environments
    # that pre-generate keys there continue to work.
    key_dir = repo_root / "build" / "test-signing" / "keys"
    legacy_key_dir = repo_root / "config" / "keys"
    if not (key_dir / "testing.private.pem").exists() and (
        legacy_key_dir / "testing.private.pem"
    ).exists():
        key_dir = legacy_key_dir

    if sign_tool.exists() and (key_dir / "testing.private.pem").exists():
        try:
            from tools.sign_updates import sign_repodata
            # SECURITY (audit finding #7): bind the return value to sig_path
            # before using it. The previous code called sign_repodata() but
            # discarded its return value, then referenced an undefined
            # sig_path variable (NameError). The broad except Exception
            # below caught the NameError and silently fell back to mock
            # signing, so real signing was effectively broken.
            sig_path = sign_repodata(repo_dir, key_dir)
            print(f"Generated repository signature: {sig_path.name}")
        except ImportError:
            # cryptography package not installed — fall back to mock.
            # This is the ONLY acceptable fallback: a missing dependency is
            # a legitimate dev-environment issue. Other exceptions (OSError,
            # ValueError, signing key errors) are real failures and must
            # NOT silently degrade to mock signing.
            _write_mock_signature(repo_dir, repodata_hash.hexdigest())
        except Exception as e:
            # SECURITY (audit finding #7): real signing failures must be
            # loud, not silent. The previous broad except caught NameError
            # (from the sig_path bug) and any other error, then wrote a
            # mock signature as if nothing happened. Now we fail hard.
            print(
                f"ERROR: real signing failed: {e}. "
                f"Refusing to silently fall back to mock signature. "
                f"Fix the signing setup or run with --allow-mock-signing.",
                file=sys.stderr,
            )
            sys.exit(1)
    else:
        _write_mock_signature(repo_dir, repodata_hash.hexdigest())


def _write_mock_signature(repo_dir, hex_digest):
    """Write a mock signature stub when real signing keys are not available."""
    mock_key_id = "RUSH_LINUX_MOCK_KEY_2026"
    signature_data = {
        "key_id": mock_key_id,
        "hash": hex_digest,
        "signature": f"mock_sig_for_{hex_digest}_using_{mock_key_id}",
        "note": "This is a mock signature. Run 'python3 tools/sign_updates.py init-keys' "
                "and 'python3 tools/sign_updates.py sign <repo_dir>' for real Ed25519 signatures.",
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
    check_schema_version(edition_pkg, edition_path)
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

def write_cpio_newc(entries, out_file):
    for entry in entries:
        name = entry['name'].encode('utf-8')
        content = entry['content']
        mode = entry['mode']
        
        namesize = len(name) + 1  # include null byte
        filesize = len(content)
        
        header = f"070701{0:08x}{mode:08x}{0:08x}{0:08x}{1:08x}{0:08x}{filesize:08x}{0:08x}{0:08x}{0:08x}{0:08x}{namesize:08x}{0:08x}"
        out_file.write(header.encode('ascii'))
        out_file.write(name + b'\x00')
        
        header_len = 110 + namesize
        pad_name = (4 - (header_len % 4)) % 4
        if pad_name:
            out_file.write(b'\x00' * pad_name)
            
        out_file.write(content)
        
        pad_content = (4 - (filesize % 4)) % 4
        if pad_content:
            out_file.write(b'\x00' * pad_content)
            
    # Write trailer
    trailer_name = b"TRAILER!!!"
    namesize = len(trailer_name) + 1
    header = f"070701{0:08x}{0:08x}{0:08x}{0:08x}{1:08x}{0:08x}{0:08x}{0:08x}{0:08x}{0:08x}{0:08x}{namesize:08x}{0:08x}"
    out_file.write(header.encode('ascii'))
    out_file.write(trailer_name + b'\x00')
    header_len = 110 + namesize
    pad_name = (4 - (header_len % 4)) % 4
    if pad_name:
        out_file.write(b'\x00' * pad_name)

def iter_deb_data_members(downloads_dir, deb_name):
    deb_path = downloads_dir / deb_name
    if not deb_path.exists():
        raise FileNotFoundError(
            f"Required base package {deb_name} not found in {downloads_dir}. "
            "Run download-assets.py first."
        )

    with open(deb_path, "rb") as f:
        magic = f.read(8)
        if magic != b"!<arch>\n":
            raise ValueError(f"Invalid ar archive {deb_name}")

        while True:
            header = f.read(60)
            if len(header) < 60:
                break

            name = header[0:16].decode("ascii").strip().rstrip("/")
            size = int(header[48:58].strip())
            data = f.read(size)
            if size % 2:
                f.seek(1, 1)

            if name.startswith("data.tar"):
                with tarfile.open(fileobj=io.BytesIO(data), mode="r:*") as tar:
                    for member in tar.getmembers():
                        if member.isfile():
                            extracted = tar.extractfile(member)
                            if extracted is not None:
                                yield member.name.lstrip("./"), extracted.read()
                return


def helper_extract_from_deb(downloads_dir, deb_name, file_path_in_tar):
    target = file_path_in_tar.lstrip("./")
    for member_name, member_bytes in iter_deb_data_members(downloads_dir, deb_name):
        if member_name == target:
            return member_bytes
    raise FileNotFoundError(f"File {file_path_in_tar} not found in {deb_name}")


def helper_extract_many_from_deb(downloads_dir, deb_name, file_paths_in_tar):
    wanted = {path.lstrip("./") for path in file_paths_in_tar}
    found = {}
    for member_name, member_bytes in iter_deb_data_members(downloads_dir, deb_name):
        if member_name in wanted:
            found[member_name] = member_bytes
            if len(found) == len(wanted):
                break
    return found


def build_vm_kernel_cmdline(base_cmdline_text, root_device=DEFAULT_VM_ROOT_DEVICE, console=DEFAULT_VM_CONSOLE):
    tokens = base_cmdline_text.split()
    if not any(token.startswith("root=") for token in tokens):
        tokens.append(f"root={root_device}")
    if "rw" not in tokens and "ro" not in tokens:
        tokens.append("rw")
    if console and not any(token.startswith("console=") for token in tokens):
        tokens.append(f"console={console}")
    return " ".join(tokens)


def stage_systemd_boot_layout(rootfs_dir, version):
    boot_dir = Path(rootfs_dir) / "boot"
    loader_dir = boot_dir / "loader"
    entries_dir = loader_dir / "entries"
    entries_dir.mkdir(parents=True, exist_ok=True)

    (loader_dir / "loader.conf").write_text(
        "default rush-linux.conf\n"
        "timeout 3\n"
        "editor no\n"
    )
    (entries_dir / "rush-linux.conf").write_text(
        "title Rush Linux\n"
        f"version {version}\n"
        f"efi {UKI_ESP_PATH}\n"
    )


def cmd_build_uki(args):
    rootfs_dir = Path(args.rootfs_dir)
    
    repo_root = Path(__file__).resolve().parent.parent
    downloads_dir = repo_root / "build" / "tmp_downloads"
    
    print("Extracting base binaries from cached debian packages...")
    
    busybox_bytes = helper_extract_from_deb(downloads_dir, BUSYBOX_DEB, "bin/busybox")
    
    stub_bytes = helper_extract_from_deb(
        downloads_dir,
        SYSTEMD_BOOT_DEB,
        "usr/lib/systemd/boot/efi/linuxx64.efi.stub"
    )
    
    bootloader_bytes = helper_extract_from_deb(
        downloads_dir,
        SYSTEMD_BOOT_DEB,
        "usr/lib/systemd/boot/efi/systemd-bootx64.efi"
    )
    
    vmlinuz_bytes = helper_extract_from_deb(
        downloads_dir,
        KERNEL_DEB,
        f"boot/vmlinuz-{KERNEL_VERSION}"
    )
    
    module_paths = [f"lib/modules/{KERNEL_VERSION}/{path}" for path in ESSENTIAL_INITRD_MODULES]
    module_bytes = helper_extract_many_from_deb(downloads_dir, KERNEL_DEB, module_paths)
    missing_modules = sorted(set(module_paths) - set(module_bytes))
    if missing_modules:
        print("Warning: some expected initrd modules were not found in the kernel package:", file=sys.stderr)
        for module in missing_modules:
            print(f"  - {module}", file=sys.stderr)
    
    print("Assembling minimal initrd...")
    
    init_script = f"""#!/bin/sh
set -e
PATH=/bin:/sbin
export PATH
echo "== Rush Linux initrd =="
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev
mount -t tmpfs tmpfs /run

M="/lib/modules/{KERNEL_VERSION}"
echo "Loading storage drivers..."
for k in virtio virtio_ring virtio_pci_legacy_dev virtio_pci_modern_dev \
         virtio_pci virtio_blk crc32c_generic libcrc32c crc16 jbd2 mbcache ext4; do
    [ -e "$M/${{k}}.ko" ] && insmod "$M/${{k}}.ko" 2>/dev/null || true
done
sleep 1

ROOT_DEV=""
for arg in $(cat /proc/cmdline); do
    case "$arg" in
        root=*) ROOT_DEV="${{arg#root=}}" ;;
    esac
done

if [ -z "$ROOT_DEV" ]; then
    echo "Error: root= parameter not found in kernel cmdline!"
    ls /dev/vd* /dev/sd* 2>/dev/null || true
    exec /bin/sh
fi

i=0
while [ ! -e "$ROOT_DEV" ] && [ $i -lt 50 ]; do
    sleep 0.2
    i=$((i + 1))
done

if [ ! -e "$ROOT_DEV" ]; then
    echo "Error: root device $ROOT_DEV not found!"
    ls /dev/vd* /dev/sd* 2>/dev/null || true
    exec /bin/sh
fi

echo "Mounting root filesystem $ROOT_DEV..."
mkdir -p /mnt/root
mount -o ro "$ROOT_DEV" /mnt/root

if [ ! -x /mnt/root/sbin/init ]; then
    echo "Error: /sbin/init not found on root device!"
    ls /mnt/root
    exec /bin/sh
fi

echo "Switching root..."
exec switch_root /mnt/root /sbin/init
"""
    
    entries = [
        {'name': 'bin', 'mode': 0o040755, 'content': b''},
        {'name': 'sbin', 'mode': 0o040755, 'content': b''},
        {'name': 'proc', 'mode': 0o040755, 'content': b''},
        {'name': 'sys', 'mode': 0o040755, 'content': b''},
        {'name': 'dev', 'mode': 0o040755, 'content': b''},
        {'name': 'run', 'mode': 0o040755, 'content': b''},
        {'name': 'mnt', 'mode': 0o040755, 'content': b''},
        {'name': 'mnt/root', 'mode': 0o040755, 'content': b''},
        {'name': 'lib', 'mode': 0o040755, 'content': b''},
        {'name': 'lib/modules', 'mode': 0o040755, 'content': b''},
        {'name': f'lib/modules/{KERNEL_VERSION}', 'mode': 0o040755, 'content': b''},
        {'name': 'init', 'mode': 0o100755, 'content': init_script.encode('utf-8')},
        {'name': 'bin/busybox', 'mode': 0o100755, 'content': busybox_bytes},
    ]
    
    for module_path, contents in sorted(module_bytes.items()):
        module_name = Path(module_path).name
        entries.append({
            'name': f'lib/modules/{KERNEL_VERSION}/{module_name}',
            'mode': 0o100644,
            'content': contents,
        })
    
    for applet in ['sh', 'mount', 'cat', 'mkdir', 'echo', 'sleep', 'ls', 'insmod']:
        entries.append({'name': f'bin/{applet}', 'mode': 0o120777, 'content': b'busybox'})
    entries.append({'name': 'sbin/switch_root', 'mode': 0o120777, 'content': b'../bin/busybox'})
    
    initrd_buffer = io.BytesIO()
    write_cpio_newc(entries, initrd_buffer)
    initrd_cpio = initrd_buffer.getvalue()
    initrd_gz = gzip.compress(initrd_cpio)
    
    build_dir = repo_root / "build"
    build_dir.mkdir(exist_ok=True)
    
    initrd_path = build_dir / "initrd.img"
    with open(initrd_path, "wb") as f:
        f.write(initrd_gz)
    print(f"Initrd built: {initrd_path}")
    
    cmdline_path = repo_root / "distro" / "boot" / "cmdline.d" / "adaptive.conf"
    if not cmdline_path.exists():
        raise FileNotFoundError(f"Kernel command line configuration not found at {cmdline_path}")
    cmdline_text = build_vm_kernel_cmdline(cmdline_path.read_text())
    
    temp_cmdline_path = build_dir / "cmdline.txt"
    temp_cmdline_path.write_text(cmdline_text)
    
    temp_stub_path = build_dir / "linuxx64.efi.stub"
    temp_stub_path.write_bytes(stub_bytes)
    
    temp_vmlinuz_path = build_dir / "vmlinuz"
    temp_vmlinuz_path.write_bytes(vmlinuz_bytes)
    
    esp_linux_dir = rootfs_dir / "boot" / "EFI" / "Linux"
    esp_boot_dir = rootfs_dir / "boot" / "EFI" / "BOOT"
    esp_linux_dir.mkdir(parents=True, exist_ok=True)
    esp_boot_dir.mkdir(parents=True, exist_ok=True)
    
    uki_output_path = esp_linux_dir / "rush-linux.efi"
    
    print("Compiling Unified Kernel Image (UKI)...")
    
    ukify_cmd = [
        "ukify", "build",
        f"--stub={temp_stub_path}",
        f"--kernel={temp_vmlinuz_path}",
        f"--cmdline=@{temp_cmdline_path}",
        f"--initrd={initrd_path}",
        f"--output={uki_output_path}"
    ]
    
    objcopy_cmd = [
        "objcopy",
        "--add-section", f".cmdline={temp_cmdline_path}", "--change-section-vma", ".cmdline=0x30000",
        "--add-section", f".linux={temp_vmlinuz_path}", "--change-section-vma", ".linux=0x2000000",
        "--add-section", f".initrd={initrd_path}", "--change-section-vma", ".initrd=0x3000000",
        str(temp_stub_path), str(uki_output_path)
    ]
    
    res = subprocess.run(ukify_cmd, capture_output=True)
    if res.returncode == 0:
        print("UKI compiled successfully using systemd-ukify.")
    else:
        res_obj = subprocess.run(objcopy_cmd, capture_output=True)
        if res_obj.returncode == 0:
            print("UKI compiled successfully using objcopy.")
        else:
            print(f"Error: failed to compile UKI. Both ukify and objcopy failed.", file=sys.stderr)
            print(f"ukify stderr: {res.stderr.decode('utf-8', errors='ignore')}", file=sys.stderr)
            print(f"objcopy stderr: {res_obj.stderr.decode('utf-8', errors='ignore')}", file=sys.stderr)
            sys.exit(1)
            
    bootloader_path = esp_boot_dir / "BOOTX64.EFI"
    bootloader_path.write_bytes(bootloader_bytes)
    print(f"Staged fallback bootloader to {bootloader_path}")

    version_path = repo_root / "VERSION"
    version = version_path.read_text().strip() if version_path.exists() else "unknown"
    stage_systemd_boot_layout(rootfs_dir, version)
    print("Staged systemd-boot loader.conf and Rush Linux UKI entry")
    print("UKI assembly completed successfully!\n")

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
    
    esp_part_def = """
[Partition]
Type=esp
Format=vfat
CopyFiles=/boot
Label=RushLinuxESP
"""
    
    root_part_def = """
[Partition]
Type=root-x86-64
Format=ext4
CopyFiles=/
ExcludeFiles=/boot
Label=RushLinuxRoot
"""
    with open(repart_defs / "35-esp.conf", "w") as f:
        f.write(esp_part_def)
    with open(repart_defs / "50-root.conf", "w") as f:
        f.write(root_part_def)
        
    repart_cmd = [
        "systemd-repart",
        "--empty=create",
        "--size=200M",
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
    
    # build-uki parser
    parser_uki = subparsers.add_parser("build-uki", help="Assemble initrd and compile Unified Kernel Image (UKI)")
    parser_uki.add_argument("rootfs_dir", help="Path to rootfs directory containing staged files")
    
    args = parser.parse_args()
    
    if args.command == "build":
        cmd_build(args)
    elif args.command == "repo-init":
        cmd_repo_init(args)
    elif args.command == "rootfs-create":
        cmd_rootfs_create(args)
    elif args.command == "vm-image":
        cmd_vm_image(args)
    elif args.command == "build-uki":
        cmd_build_uki(args)

if __name__ == "__main__":
    main()
