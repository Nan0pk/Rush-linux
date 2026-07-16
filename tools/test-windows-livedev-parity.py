#!/usr/bin/env python3
"""Focused regression tests for the Windows one-command LiveDev path."""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
TOOLS = ROOT / "tools"


def _load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def test_front_page_download_fails_closed_before_execution():
    readme = (ROOT / "README.md").read_text()
    assert "curl.exe -fL -o livedev-bootstrap.ps1" in readme
    assert "as Administrator" in readme
    command = next(line for line in readme.splitlines() if "curl.exe -fL" in line)
    assert "$LASTEXITCODE -ne 0" in command
    assert command.index("$LASTEXITCODE -ne 0") < command.index("powershell -ExecutionPolicy")


def test_windows_bootstrap_uses_shared_strict_pipeline():
    source = (TOOLS / "livedev-bootstrap.ps1").read_text()
    for required in (
        "ensure-fresh",
        "collect-hardware-inventory.py",
        "testos_prepare_usb.py",
        "rush-safe-copy-tree.py",
        "validate-testos-evidence.py",
        "rush-submit-evidence",
        "TESTOS_IMAGE_COMMIT",
    ):
        assert required in source
    assert "Submit-TestosResults" not in source
    assert "x-access-token:" not in source
    assert "rush-livedev-resume-" not in source


def test_release_publishes_checksummed_image_commit_metadata():
    workflow = (ROOT / ".github/workflows/release-testos.yml").read_text()
    assert 'git rev-parse HEAD > "$STAGE/testos-image-commit.txt"' in workflow
    # The metadata is created before the existing wildcard SHA256SUMS command.
    assert workflow.index("testos-image-commit.txt") < workflow.index("sha256sum * > SHA256SUMS")


def test_release_smoke_boot_injects_verified_run_intent_first():
    workflow = (ROOT / ".github/workflows/release-testos.yml").read_text()
    inject = workflow.index("Inject verified smoke-test run intent")
    prepare = workflow.index("tools/testos_prepare_usb.py", inject)
    boot = workflow.index("Boot in QEMU and wait for testOS menu", prepare)
    assert inject < prepare < boot
    assert "--testos-image-commit" in workflow[prepare:boot]
    assert "--testos-version" in workflow[prepare:boot]
    assert "--baseline-only" in workflow[inject:boot]


def test_installers_fail_closed_and_emit_identity_markers():
    for path in (ROOT / "testos/install.sh", ROOT / "testos/install.ps1"):
        source = path.read_text()
        assert "testos-image-commit.txt" in source
        assert "TESTOS_RAW_IMAGE:" in source
        assert "TESTOS_USB_DEVICE:" in source
        assert "TESTOS_IMAGE_COMMIT:" in source
        assert "TESTOS_VERSION:" in source
        assert "Refusing" in source or "refusing" in source

    windows_installer = (ROOT / "testos/install.ps1").read_text()
    assert "Administrator privileges are required. Refusing disk access" in windows_installer
    assert "Continuing anyway" not in windows_installer


def test_prepare_usb_requires_image_commit(tmp_path: Path):
    module = _load("testos_prepare_usb_windows_test", TOOLS / "testos_prepare_usb.py")
    plan = tmp_path / "plan.json"
    plan.write_text(json.dumps({"source_commit": "0" * 40}))
    image = tmp_path / "testos.raw"
    image.write_bytes(b"image")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    raw, _ = module.generate_run_intent(
        repo_root=ROOT,
        plan_path=plan,
        image_path=image,
        run_id="windows-test-0001",
        checkpoint_nonce="ckpt-windows-test-0001",
        testos_image_commit=head,
        testos_version=(ROOT / "VERSION").read_text().strip(),
    )
    assert json.loads(raw)["testos_image_commit"] == head
    with pytest.raises(RuntimeError, match="40 lowercase hex"):
        module.generate_run_intent(
            repo_root=ROOT,
            plan_path=plan,
            image_path=image,
            run_id="windows-test-0001",
            checkpoint_nonce="ckpt-windows-test-0001",
            testos_image_commit="bad",
            testos_version=(ROOT / "VERSION").read_text().strip(),
        )


def test_reparse_attribute_hook(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    sys.path.insert(0, str(TOOLS))
    import rush_path_safety as safety

    monkeypatch.setattr(safety.sys, "platform", "win32")
    monkeypatch.setattr(safety, "_get_windows_file_attributes", lambda _p: 0x0400)
    assert safety._is_windows_reparse_point(tmp_path)
    monkeypatch.setattr(safety, "_get_windows_file_attributes", lambda _p: 0)
    assert not safety._is_windows_reparse_point(tmp_path)


def test_windows_inventory_mapping_contains_no_identifiers():
    module = _load("inventory_windows_test", TOOLS / "collect-hardware-inventory.py")
    inventory = module._windows_inventory_from_payload({
        "computer_system": {"Manufacturer": "HP", "Model": "HP Laptop", "TotalPhysicalMemory": 8 * 1024**3},
        "processor": {"Name": "Intel CPU", "NumberOfCores": 4, "NumberOfLogicalProcessors": 8},
        "video_controllers": [{"Name": "Intel GPU", "AdapterRAM": 1024}],
        "disk_drives": [{"MediaType": "Fixed", "InterfaceType": "SCSI", "Size": 512000000000}],
        "operating_system": {"Caption": "Windows 11", "Version": "10.0", "OSArchitecture": "64-bit"},
        "battery": {"BatteryStatus": 2, "EstimatedChargeRemaining": 100},
        "battery_design_capacity": 41051,
        "battery_full_capacity": 36413,
    })
    encoded = json.dumps(inventory).lower()
    for forbidden in ("serialnumber", "uuid", "macaddress", "hostname", "username", "productkey"):
        assert forbidden not in encoded
    assert inventory["battery"]["health_pct"] == 88.7


@pytest.mark.skipif(os.name != "nt", reason="native Windows junction test")
def test_native_windows_junction_is_rejected(tmp_path: Path):
    sys.path.insert(0, str(TOOLS))
    import rush_path_safety as safety

    target = tmp_path / "target"
    junction = tmp_path / "junction"
    target.mkdir()
    result = subprocess.run(
        ["cmd.exe", "/d", "/c", "mklink", "/J", str(junction), str(target)],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    try:
        assert safety._is_windows_reparse_point(junction)
        assert junction in safety.reject_non_regular(tmp_path)
    finally:
        os.rmdir(junction)
