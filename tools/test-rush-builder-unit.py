#!/usr/bin/env python3
"""Small unit checks for rush-builder helpers that do not require build assets."""

import importlib.util
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
BUILDER_PATH = REPO_ROOT / "tools" / "rush-builder.py"


spec = importlib.util.spec_from_file_location("rush_builder", BUILDER_PATH)
rush_builder = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(rush_builder)


def test_build_vm_kernel_cmdline_adds_vm_boot_args():
    base = "systemd.unified_cgroup_hierarchy=1 cgroup_no_v1=all psi=1 zswap.enabled=1"
    cmdline = rush_builder.build_vm_kernel_cmdline(base)
    tokens = cmdline.split()

    assert "systemd.unified_cgroup_hierarchy=1" in tokens
    assert "root=/dev/vda2" in tokens
    assert "rw" in tokens
    assert "console=ttyS0,115200" in tokens


def test_build_vm_kernel_cmdline_preserves_existing_root_mode_and_console():
    base = "root=/dev/sda3 ro console=tty0 psi=1"
    cmdline = rush_builder.build_vm_kernel_cmdline(base)
    tokens = cmdline.split()

    assert tokens.count("root=/dev/sda3") == 1
    assert "root=/dev/vda2" not in tokens
    assert "ro" in tokens
    assert "rw" not in tokens
    assert tokens.count("console=tty0") == 1
    assert "console=ttyS0,115200" not in tokens


def test_stage_systemd_boot_layout():
    with tempfile.TemporaryDirectory() as tmp:
        rootfs = Path(tmp) / "rootfs"
        rush_builder.stage_systemd_boot_layout(rootfs, "0.3.0-alpha.1")

        loader_conf = rootfs / "boot" / "loader" / "loader.conf"
        entry = rootfs / "boot" / "loader" / "entries" / "rush-linux.conf"

        assert loader_conf.read_text() == "default rush-linux.conf\ntimeout 3\neditor no\n"
        assert "title Rush Linux\n" in entry.read_text()
        assert "version 0.3.0-alpha.1\n" in entry.read_text()
        assert "efi /EFI/Linux/rush-linux.efi\n" in entry.read_text()


if __name__ == "__main__":
    test_build_vm_kernel_cmdline_adds_vm_boot_args()
    test_build_vm_kernel_cmdline_preserves_existing_root_mode_and_console()
    test_stage_systemd_boot_layout()
    print("rush-builder helper unit tests passed")
