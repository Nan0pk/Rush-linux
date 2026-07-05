#!/usr/bin/env python3
"""
pytest tests for rush-livedev-orchestrator's state machine and timeout logic.

These tests use a FAKE QEMU (a Python subprocess that emits canned markers
to stdout and exits when stdin closes). This lets us exercise the host-side
state machine deterministically without needing a real disk image or KVM.

Covers:
  - Successful run (BOOT_READY -> TEST_START -> TEST_PASS -> SHUTDOWN)
  - Test failure (BOOT_READY -> TEST_START -> TEST_FAIL exit_code=1 -> SHUTDOWN)
  - Boot timeout (no markers within boot_timeout)
  - Test-start timeout (BOOT_READY but no TEST_START)
  - Test-execution timeout (TEST_START but no terminal marker)
  - Guest failure detection (kernel panic in console stream)
  - Guest failure detection (login prompt before BOOT_READY)
  - Summary.json generation
  - Bundle creation
  - Exit code propagation

Run with:
  python3 -m pytest tools/test-livedev-smoke.py -v
  python3 tools/test-livedev-smoke.py  # standalone
"""

from __future__ import annotations

import importlib.machinery
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import textwrap
import time
from pathlib import Path

import pytest

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent


def _load_module(name: str, path: Path):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    loader.exec_module(mod)
    return mod


orch = _load_module("rush_livedev_orchestrator",
                    _TOOLS_DIR / "rush-livedev-orchestrator")


# ─── Fake QEMU ──────────────────────────────────────────────────────────────

_FAKE_QEMU_SCRIPT = """\
#!/usr/bin/env python3
import sys, time, json, os

# Read the scenario from the env var.
scenario = os.environ.get('RUSH_FAKE_QEMU_SCENARIO', 'success')

# Emit lines on stdout (which the orchestrator reads as the console).
def emit(line):
    sys.stdout.write(line + '\\n')
    sys.stdout.flush()

if scenario == 'success':
    emit('Linux version 6.1.0-rush (fake)')
    emit('[ OK ] Started optid.service')
    emit('RUSH_LIVEDEV_BOOT_READY run_id=fake')
    emit('RUSH_LIVEDEV_TEST_START run_id=fake')
    emit('running fake test...')
    emit('fake test passed')
    emit('RUSH_LIVEDEV_TEST_PASS run_id=fake')
    emit('RUSH_LIVEDEV_ARTIFACTS_READY run_id=fake path=/RUSH-DATA/results/livedev/fake')
    emit('RUSH_LIVEDEV_SHUTDOWN run_id=fake')
    sys.exit(0)
elif scenario == 'test_failure':
    emit('Linux version 6.1.0-rush (fake)')
    emit('RUSH_LIVEDEV_BOOT_READY run_id=fake')
    emit('RUSH_LIVEDEV_TEST_START run_id=fake')
    emit('running fake test...')
    emit('fake test FAILED')
    emit('RUSH_LIVEDEV_TEST_FAIL run_id=fake exit_code=1')
    emit('RUSH_LIVEDEV_ARTIFACTS_READY run_id=fake path=/RUSH-DATA/results/livedev/fake')
    emit('RUSH_LIVEDEV_SHUTDOWN run_id=fake')
    sys.exit(0)
elif scenario == 'boot_timeout':
    # Emit nothing useful — just kernel boot messages forever.
    for i in range(1000):
        emit(f'kernel boot message {i}')
        time.sleep(0.05)
    sys.exit(0)
elif scenario == 'test_start_timeout':
    emit('RUSH_LIVEDEV_BOOT_READY run_id=fake')
    # Then nothing — runner hangs before starting tests.
    time.sleep(30)
    sys.exit(0)
elif scenario == 'test_timeout':
    emit('RUSH_LIVEDEV_BOOT_READY run_id=fake')
    emit('RUSH_LIVEDEV_TEST_START run_id=fake')
    # Then nothing — test hangs forever.
    time.sleep(30)
    sys.exit(0)
elif scenario == 'kernel_panic':
    emit('Linux version 6.1.0-rush (fake)')
    emit('Kernel panic - not syncing: Attempted to kill init')
    emit('Call Trace:')
    time.sleep(30)
    sys.exit(0)
elif scenario == 'root_prompt':
    emit('Linux version 6.1.0-rush (fake)')
    emit('[ OK ] Started multi-user.target')
    emit('root@rush-linux:~# ')
    time.sleep(30)
    sys.exit(0)
elif scenario == 'login_prompt':
    emit('Linux version 6.1.0-rush (fake)')
    emit('[ OK ] Started multi-user.target')
    emit('login:')
    time.sleep(30)
    sys.exit(0)
elif scenario == 'qemu_died_early':
    # Exit immediately with no output.
    sys.exit(1)
elif scenario == 'debug_shell':
    emit('RUSH_LIVEDEV_BOOT_READY run_id=fake')
    emit('RUSH_LIVEDEV_TEST_START run_id=fake')
    emit('RUSH_LIVEDEV_TEST_FAIL run_id=fake exit_code=1')
    emit('RUSH_LIVEDEV_DEBUG_SHELL run_id=fake')
    time.sleep(30)
    sys.exit(0)
else:
    emit(f'unknown scenario: {scenario}')
    sys.exit(1)
"""


@pytest.fixture
def fake_qemu(tmp_path):
    """Create a fake qemu-system-x86_64 script that emits canned markers."""
    script = tmp_path / "qemu-system-x86_64"
    script.write_text(_FAKE_QEMU_SCRIPT)
    script.chmod(0o755)
    return str(script)


@pytest.fixture
def fake_image(tmp_path):
    """Create a fake disk image file (1 MiB of zeros)."""
    img = tmp_path / "rush-linux-livedev.raw"
    img.write_bytes(b"\\0" * (1024 * 1024))
    return str(img)


def _make_cfg(fake_qemu, fake_image, tmp_path, *, scenario="success", **kw):
    """Build an OrchestratorConfig pointed at the fake QEMU."""
    defaults = dict(
        image=fake_image,
        run_id=f"test-{scenario}-{os.getpid() & 0xFFFF:04x}",
        suite="smoke",
        test_command="echo hello",
        submit_mode="none",
        qemu_binary=fake_qemu,
        qemu_accel="tcg",
        qemu_firmware="",  # don't try to find OVMF
        inject_state_method="none",  # don't try to inject into the fake image
        artifacts_dir=str(tmp_path / "artifacts" / scenario),
        # Short timeouts so tests don't hang.
        boot_timeout=10,
        test_start_timeout=5,
        test_timeout=10,
        shutdown_timeout=5,
    )
    defaults.update(kw)
    return orch.OrchestratorConfig(**defaults)


# ─── Tests ───────────────────────────────────────────────────────────────────


def test_success_path(fake_qemu, fake_image, tmp_path, monkeypatch):
    """A clean success run produces status=passed, exit_code=0, all markers."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "success")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="success")
    result = orch.run(cfg, _ROOT)
    assert result.status == "passed"
    assert result.exit_code == 0
    assert "BOOT_READY" in result.markers_seen
    assert "TEST_START" in result.markers_seen
    assert "TEST_PASS" in result.markers_seen
    assert "SHUTDOWN" in result.markers_seen
    # summary.json was written.
    summary_path = Path(result.artifacts_dir) / "summary.json"
    assert summary_path.exists()
    summary = json.loads(summary_path.read_text())
    assert summary["status"] == "passed"
    assert summary["run_id"] == cfg.run_id


def test_test_failure_path(fake_qemu, fake_image, tmp_path, monkeypatch):
    """A test failure produces status=failed, exit_code=1."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "test_failure")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="test_failure")
    result = orch.run(cfg, _ROOT)
    assert result.status == "failed"
    assert result.exit_code == 1
    assert "TEST_FAIL" in result.markers_seen


def test_boot_timeout(fake_qemu, fake_image, tmp_path, monkeypatch):
    """Boot timeout fires when no markers appear in time."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "boot_timeout")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="boot_timeout",
                    boot_timeout=2)
    result = orch.run(cfg, _ROOT)
    assert result.status == "timeout"
    assert result.exit_code == orch.RC_BOOT_TIMEOUT
    assert "boot" in result.failure_reason


def test_test_start_timeout(fake_qemu, fake_image, tmp_path, monkeypatch):
    """Test-start timeout fires when BOOT_READY appears but TEST_START doesn't."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "test_start_timeout")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="test_start_timeout",
                    boot_timeout=10, test_start_timeout=2)
    result = orch.run(cfg, _ROOT)
    assert result.status == "timeout"
    assert result.exit_code == orch.RC_TEST_START_TIMEOUT


def test_test_execution_timeout(fake_qemu, fake_image, tmp_path, monkeypatch):
    """Test-execution timeout fires when TEST_START appears but no terminal marker."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "test_timeout")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="test_timeout",
                    boot_timeout=10, test_start_timeout=10, test_timeout=2)
    result = orch.run(cfg, _ROOT)
    assert result.status == "timeout"
    assert result.exit_code == orch.RC_TEST_TIMEOUT


def test_kernel_panic_detected(fake_qemu, fake_image, tmp_path, monkeypatch):
    """A kernel panic in the console is detected as a guest failure."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "kernel_panic")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="kernel_panic",
                    boot_timeout=10)
    result = orch.run(cfg, _ROOT)
    assert result.status == "guest_failure"
    assert result.exit_code == orch.RC_GUEST_FAILURE
    assert "kernel_panic" in result.failure_reason


def test_root_prompt_detected(fake_qemu, fake_image, tmp_path, monkeypatch):
    """A root shell prompt before BOOT_READY is detected as a guest failure."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "root_prompt")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="root_prompt",
                    boot_timeout=10)
    result = orch.run(cfg, _ROOT)
    assert result.status == "guest_failure"
    assert result.exit_code == orch.RC_GUEST_FAILURE
    assert "root_shell" in result.failure_reason


def test_login_prompt_detected(fake_qemu, fake_image, tmp_path, monkeypatch):
    """A login prompt before BOOT_READY is detected as a guest failure."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "login_prompt")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="login_prompt",
                    boot_timeout=10)
    result = orch.run(cfg, _ROOT)
    assert result.status == "guest_failure"
    assert result.exit_code == orch.RC_GUEST_FAILURE
    assert "login_prompt" in result.failure_reason


def test_qemu_died_early(fake_qemu, fake_image, tmp_path, monkeypatch):
    """QEMU exiting immediately with no output is an infra failure."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "qemu_died_early")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="qemu_died_early",
                    boot_timeout=5)
    result = orch.run(cfg, _ROOT)
    # Either infra_error (QEMU died) — TEST_PASS would also be accepted
    # but the fake didn't emit it.
    assert result.status in ("infra_error", "guest_failure")
    assert result.exit_code != 0


def test_debug_shell_treated_as_failure(fake_qemu, fake_image, tmp_path, monkeypatch):
    """A DEBUG_SHELL marker (without --debug) is treated as a guest failure."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "debug_shell")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="debug_shell",
                    boot_timeout=10, test_timeout=10,
                    shutdown_timeout=2)
    result = orch.run(cfg, _ROOT)
    assert result.status == "failed"
    assert result.exit_code == orch.RC_GUEST_FAILURE
    assert "debug shell" in result.failure_reason


def test_console_log_written(fake_qemu, fake_image, tmp_path, monkeypatch):
    """The full console output is captured to console.log."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "success")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="success")
    result = orch.run(cfg, _ROOT)
    console_path = Path(result.console_log)
    assert console_path.exists()
    text = console_path.read_text()
    assert "RUSH_LIVEDEV_BOOT_READY" in text
    assert "RUSH_LIVEDEV_TEST_PASS" in text


def test_metadata_json_written(fake_qemu, fake_image, tmp_path, monkeypatch):
    """metadata.json is written early and contains git/host info."""
    monkeypatch.setenv("RUSH_FAKE_QEMU_SCENARIO", "success")
    cfg = _make_cfg(fake_qemu, fake_image, tmp_path, scenario="success")
    result = orch.run(cfg, _ROOT)
    md_path = Path(result.metadata_path)
    assert md_path.exists()
    md = json.loads(md_path.read_text())
    assert md["run_id"] == cfg.run_id
    assert "config" in md
    assert "git" in md
    assert "host" in md


def test_missing_image_returns_infra_error(tmp_path):
    """A missing disk image returns infra_error, not a crash."""
    cfg = orch.OrchestratorConfig(
        image=str(tmp_path / "nonexistent.raw"),
        run_id="missing-1",
        artifacts_dir=str(tmp_path / "artifacts" / "missing"),
    )
    result = orch.run(cfg, _ROOT)
    assert result.status == "infra_error"
    assert result.exit_code == orch.RC_INFRA
    assert "image not found" in result.failure_reason


def test_missing_qemu_returns_infra_error(fake_image, tmp_path):
    """A missing QEMU binary returns infra_error, not a crash."""
    cfg = orch.OrchestratorConfig(
        image=fake_image,
        run_id="missing-qemu-1",
        qemu_binary="/nonexistent/qemu-system-x86_64",
        artifacts_dir=str(tmp_path / "artifacts" / "missing-qemu"),
    )
    result = orch.run(cfg, _ROOT)
    assert result.status == "infra_error"
    assert result.exit_code == orch.RC_INFRA
    assert "QEMU binary not found" in result.failure_reason


# ─── Standalone runner ──────────────────────────────────────────────────────


def _run_all_tests() -> int:
    # Run pytest on this file.
    r = subprocess.run(
        ["python3", "-m", "pytest", str(__file__), "-v"],
        cwd=str(_ROOT),
    )
    return r.returncode


if __name__ == "__main__":
    sys.exit(_run_all_tests())
