#!/usr/bin/env python3
"""
pytest tests for the LiveDev image/profile (image-profile phase, Prompt 9).

Tests the 7 required structural scenarios:
  1. profile files exist
  2. required tools referenced
  3. units reference existing paths
  4. RUSH-DATA layout creation works
  5. testOS path not broken
  6. build script accepts --edition livedev
  7. edition descriptor is valid

Run with:
  python3 -m pytest tools/test-livedev-image.py -v
  # or
  python3 tools/test-livedev-image.py  # standalone
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent


# ─── Test 1: profile files exist ─────────────────────────────────────────────


def test_profile_files_exist():
    """The LiveDev mkosi profile directory and config exist."""
    profile_dir = _ROOT / "mkosi" / "mkosi.profiles" / "livedev"
    assert profile_dir.exists(), f"livedev profile dir missing: {profile_dir}"
    conf = profile_dir / "mkosi.conf"
    assert conf.exists(), f"livedev mkosi.conf missing: {conf}"


def test_profile_has_image_id():
    """The profile sets ImageId=rush-linux-livedev."""
    conf = _ROOT / "mkosi" / "mkosi.profiles" / "livedev" / "mkosi.conf"
    text = conf.read_text()
    assert "ImageId=rush-linux-livedev" in text


def test_edition_descriptor_exists():
    """The livedev edition descriptor exists."""
    desc = _ROOT / "distro" / "editions" / "livedev.toml"
    assert desc.exists(), f"livedev edition descriptor missing: {desc}"
    text = desc.read_text()
    assert 'name = "livedev"' in text


# ─── Test 2: required tools referenced ───────────────────────────────────────


def test_required_tools_in_profile():
    """The LiveDev profile references required packages."""
    conf = _ROOT / "mkosi" / "mkosi.profiles" / "livedev" / "mkosi.conf"
    text = conf.read_text()
    required = ["git", "github-cli", "python", "openssh", "curl", "jq"]
    for pkg in required:
        assert pkg in text, f"required package {pkg!r} not in livedev profile"


def test_required_tools_in_build_script():
    """The build script installs the rush-* tools for livedev edition."""
    script = _ROOT / "tools" / "build-mkosi-image.sh"
    text = script.read_text()
    required_tools = [
        "rush-exec", "rush-capture", "rush-autopilot", "rush-agent",
        "rush-livedev-autostart",
    ]
    for tool in required_tools:
        assert tool in text, f"required tool {tool!r} not in build script"


def test_support_libraries_in_build_script():
    """The build script installs the support libraries for livedev edition."""
    script = _ROOT / "tools" / "build-mkosi-image.sh"
    text = script.read_text()
    required_libs = [
        "rush_capture_lib.py", "rush_runner_lib.py", "rush_agent_lib.py",
    ]
    for lib in required_libs:
        assert lib in text, f"support library {lib!r} not in build script"


def test_validator_in_build_script():
    """The build script installs the evidence validator for livedev edition."""
    script = _ROOT / "tools" / "build-mkosi-image.sh"
    text = script.read_text()
    assert "validate-hwtest-evidence" in text


# ─── Test 3: units reference existing paths ──────────────────────────────────


def test_rush_livedev_tmpfiles_exists():
    """The RUSH-DATA tmpfiles config exists and creates the right directories."""
    tmpfiles = _ROOT / "packaging" / "systemd" / "rush-livedev-tmpfiles.conf"
    assert tmpfiles.exists()
    text = tmpfiles.read_text()
    required_dirs = [
        "/RUSH-DATA", "/RUSH-DATA/repo", "/RUSH-DATA/state",
        "/RUSH-DATA/results", "/RUSH-DATA/logs", "/RUSH-DATA/ai",
        "/RUSH-DATA/secrets", "/RUSH-DATA/cache",
    ]
    for d in required_dirs:
        assert d in text, f"RUSH-DATA dir {d!r} not in tmpfiles config"


def test_rush_capture_service_exists():
    """rush-capture.service exists and references valid paths."""
    svc = _ROOT / "packaging" / "systemd" / "rush-capture.service"
    assert svc.exists()
    text = svc.read_text()
    assert "ExecStart=" in text
    assert "rush-capture" in text
    assert "/RUSH-DATA" in text


def test_rush_autopilot_service_exists():
    """rush-autopilot.service exists and references valid paths."""
    svc = _ROOT / "packaging" / "systemd" / "rush-autopilot.service"
    assert svc.exists()
    text = svc.read_text()
    assert "ExecStart=" in text
    assert "rush-autopilot" in text
    assert "/RUSH-DATA" in text


def test_rush_livedev_autostart_service_exists():
    """rush-livedev-autostart.service exists and references the autostart script."""
    svc = _ROOT / "packaging" / "systemd" / "rush-livedev-autostart.service"
    assert svc.exists()
    text = svc.read_text()
    assert "ExecStart=" in text
    assert "rush-livedev-autostart" in text


def test_autostart_script_exists():
    """The rush-livedev-autostart script exists and is executable."""
    script = _ROOT / "tools" / "rush-livedev-autostart"
    assert script.exists()
    assert os.access(script, os.X_OK), "autostart script should be executable"


# ─── Test 4: RUSH-DATA layout creation works ─────────────────────────────────


def test_rush_data_layout_creation():
    """The RUSH-DATA tmpfiles config creates the right directory structure."""
    tmpfiles = _ROOT / "packaging" / "systemd" / "rush-livedev-tmpfiles.conf"
    text = tmpfiles.read_text()
    # Each line should start with 'd' (directory) and contain the path + perms.
    lines = [l.strip() for l in text.splitlines() if l.strip() and not l.startswith("#")]
    for line in lines:
        assert line.startswith("d "), f"tmpfiles line should start with 'd ': {line!r}"
        parts = line.split()
        assert len(parts) >= 4, f"tmpfiles line should have path + perms + owner: {line!r}"
        path = parts[1]
        assert path.startswith("/RUSH-DATA"), f"tmpfiles path should be under /RUSH-DATA: {path!r}"
    # Verify secrets dir has 0700.
    secrets_line = [l for l in lines if "/RUSH-DATA/secrets" in l]
    assert len(secrets_line) == 1
    assert "0700" in secrets_line[0], f"secrets dir should be 0700: {secrets_line[0]!r}"


# ─── Test 5: testOS path not broken ──────────────────────────────────────────


def test_testos_profile_unchanged():
    """The testOS mkosi profile is not modified."""
    profile = _ROOT / "mkosi" / "mkosi.profiles" / "testos" / "mkosi.conf"
    assert profile.exists()
    text = profile.read_text()
    assert "ImageId=rush-linux-testos" in text
    assert "testos.usb_label=RUSHESP" in text
    assert "testos.runner=1" in text


def test_testos_directory_unchanged():
    """The testos/ directory still exists and has its key files."""
    testos_dir = _ROOT / "testos"
    assert testos_dir.exists()
    assert (testos_dir / "README.md").exists()
    assert (testos_dir / "bench-list.toml").exists()
    assert (testos_dir / "build-testos.sh").exists()
    assert (testos_dir / "install.sh").exists()


def test_testos_release_workflow_unchanged():
    """The testOS release CI workflow still exists."""
    workflow = _ROOT / ".github" / "workflows" / "release-testos.yml"
    assert workflow.exists()
    text = workflow.read_text()
    assert "testos" in text.lower()


# ─── Test 6: build script accepts --edition livedev ──────────────────────────


def test_build_script_accepts_livedev():
    """The build script --help mentions livedev."""
    script = _ROOT / "tools" / "build-mkosi-image.sh"
    r = subprocess.run(
        ["bash", str(script), "--help"],
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert r.returncode == 0
    assert "livedev" in r.stdout, f"--help should mention livedev: {r.stdout}"


def test_build_script_has_livedev_staging():
    """The build script has a livedev-specific staging section."""
    script = _ROOT / "tools" / "build-mkosi-image.sh"
    text = script.read_text()
    assert '"livedev"' in text and "Staging LiveDev" in text
    assert "Staging LiveDev tools" in text


# ─── Test 7: edition descriptor is valid ─────────────────────────────────────


def test_edition_descriptor_structure():
    """The livedev edition descriptor has the required sections."""
    desc = _ROOT / "distro" / "editions" / "livedev.toml"
    text = desc.read_text()
    assert "[edition]" in text
    assert "[defaults]" in text
    assert "[packages]" in text
    assert 'desktop = false' in text
    assert "required" in text


# ─── Bonus: ADR exists ───────────────────────────────────────────────────────


def test_adr_0023_exists():
    """ADR 0023 (livedev image) exists and is proposed."""
    adr = _ROOT / "docs" / "decisions" / "0023-livedev-image.md"
    assert adr.exists()
    text = adr.read_text()
    assert "Status: proposed" in text
    assert "ADR 0023" in text


def test_livedev_docs_exist():
    """The LiveDev edition documentation exists."""
    doc = _ROOT / "docs" / "editions" / "livedev.md"
    assert doc.exists()
    text = doc.read_text()
    assert "LiveDev Edition" in text
    assert "RUSH-DATA" in text
    assert "testOS" in text


# ─── Bonus: profile has kernel cmdline params ────────────────────────────────


def test_profile_has_livedev_cmdline_params():
    """The livedev profile has livedev.* kernel cmdline parameters."""
    conf = _ROOT / "mkosi" / "mkosi.profiles" / "livedev" / "mkosi.conf"
    text = conf.read_text()
    assert "livedev.autostart=" in text
    assert "livedev.countdown_sec=" in text
    assert "livedev.mutate_host_disk=" in text


# ─── Bonus: autostart script logic ───────────────────────────────────────────


def test_autostart_has_countdown():
    """The autostart script has a countdown mechanism."""
    script = _ROOT / "tools" / "rush-livedev-autostart"
    text = script.read_text()
    assert "countdown" in text.lower()
    assert "ESC" in text or "\x1b" in text
    assert "shell" in text.lower()


def test_autostart_no_destructive_action():
    """The autostart script does not perform destructive actions."""
    script = _ROOT / "tools" / "rush-livedev-autostart"
    text = text_lower = text = script.read_text().lower()
    # Should not contain destructive commands.
    for pattern in ["rm -rf", "mkfs", "dd if=", "fdisk", "sgdisk", "git push --force"]:
        assert pattern not in text_lower, f"autostart should not contain {pattern!r}"


# ─── Standalone runner ───────────────────────────────────────────────────────


import os


def _run_all_tests() -> int:
    test_funcs = [
        (name, obj)
        for name, obj in sorted(globals().items())
        if name.startswith("test_") and callable(obj)
    ]
    passed = 0
    failed = 0
    for name, func in test_funcs:
        try:
            func()
            print(f"  PASS {name}")
            passed += 1
        except Exception as e:
            print(f"  FAIL {name}: {e}")
            import traceback
            traceback.print_exc()
            failed += 1
    print(f"\n{passed} passed, {failed} failed, {passed + failed} total")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(_run_all_tests())
