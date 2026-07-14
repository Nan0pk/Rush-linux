#!/usr/bin/env python3
"""
pytest tests for rush-livedev-markers.py and the orchestrator's failure
detection logic.

Covers:
  - Marker parsing (all kinds, with/without args)
  - Marker emission (round-trip emit -> parse)
  - Failure pattern detection:
      * kernel panic
      * emergency mode
      * maintenance prompt
      * login prompt BEFORE boot ready
      * root shell prompt
      * systemd failed unit
  - Round-trip: marker text -> Marker -> to_line() -> parse -> same Marker

Run with:
  python3 -m pytest tools/test-livedev-orchestrator.py -v
  python3 tools/test-livedev-orchestrator.py  # standalone
"""

from __future__ import annotations

import importlib.machinery
import importlib.util
import sys
from pathlib import Path

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent


def _load_module(name: str, path: Path):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    loader.exec_module(mod)
    return mod


mk = _load_module("rush_livedev_markers", _TOOLS_DIR / "rush_livedev_markers.py")


# ─── Marker parsing ─────────────────────────────────────────────────────────


def test_parse_boot_ready():
    m = mk.parse_marker("RUSH_LIVEDEV_BOOT_READY run_id=r-001")
    assert m is not None
    assert m.kind == "BOOT_READY"
    assert m.run_id == "r-001"


def test_parse_test_start():
    m = mk.parse_marker("RUSH_LIVEDEV_TEST_START run_id=r-002")
    assert m is not None
    assert m.kind == "TEST_START"
    assert m.run_id == "r-002"


def test_parse_test_pass():
    m = mk.parse_marker("RUSH_LIVEDEV_TEST_PASS run_id=r-003")
    assert m is not None
    assert m.kind == "TEST_PASS"
    assert m.is_pass
    assert m.is_terminal


def test_parse_test_fail_with_exit_code():
    m = mk.parse_marker("RUSH_LIVEDEV_TEST_FAIL run_id=r-004 exit_code=42")
    assert m is not None
    assert m.kind == "TEST_FAIL"
    assert m.exit_code == 42
    assert m.is_fail
    assert m.is_terminal


def test_parse_artifacts_ready_with_path():
    m = mk.parse_marker(
        "RUSH_LIVEDEV_ARTIFACTS_READY run_id=r-005 path=/RUSH-DATA/results/livedev/r-005"
    )
    assert m is not None
    assert m.kind == "ARTIFACTS_READY"
    assert m.path == "/RUSH-DATA/results/livedev/r-005"


def test_parse_shutdown():
    m = mk.parse_marker("RUSH_LIVEDEV_SHUTDOWN run_id=r-006")
    assert m is not None
    assert m.kind == "SHUTDOWN"
    assert m.is_terminal


def test_parse_debug_shell():
    m = mk.parse_marker("RUSH_LIVEDEV_DEBUG_SHELL run_id=r-007")
    assert m is not None
    assert m.kind == "DEBUG_SHELL"
    assert m.is_fail
    assert m.is_terminal


def test_parse_non_marker_line_returns_none():
    """Lines without the marker prefix return None."""
    assert mk.parse_marker("regular kernel log line") is None
    assert mk.parse_marker("") is None
    assert mk.parse_marker("RUSH_LIVEDEV_BOGUS_KIND run_id=x") is None


def test_parse_marker_with_no_args():
    """A marker with no key=value args still parses."""
    m = mk.parse_marker("RUSH_LIVEDEV_SHUTDOWN")
    assert m is not None
    assert m.kind == "SHUTDOWN"
    assert m.run_id == ""


def test_parse_marker_ignores_garbage_suffix():
    """A line that starts with a marker but has trailing garbage is rejected."""
    # The regex requires the args section to be key=value pairs.
    m = mk.parse_marker("RUSH_LIVEDEV_TEST_PASS run_id=r trailing-garbage")
    assert m is None


# ─── Marker emission round-trip ─────────────────────────────────────────────


def test_emit_round_trip_test_pass():
    """emit() -> parse() round-trips for TEST_PASS."""
    line = mk.emit("TEST_PASS", run_id="rt-1")
    m = mk.parse_marker(line)
    assert m is not None
    assert m.kind == "TEST_PASS"
    assert m.run_id == "rt-1"


def test_emit_round_trip_test_fail_with_exit_code():
    """emit() -> parse() round-trips for TEST_FAIL with exit_code."""
    line = mk.emit("TEST_FAIL", run_id="rt-2", exit_code=7)
    m = mk.parse_marker(line)
    assert m is not None
    assert m.kind == "TEST_FAIL"
    assert m.exit_code == 7


def test_emit_round_trip_artifacts_ready_with_path():
    """emit() -> parse() round-trips for ARTIFACTS_READY with path."""
    line = mk.emit("ARTIFACTS_READY", run_id="rt-3",
                   path="/RUSH-DATA/results/livedev/rt-3")
    m = mk.parse_marker(line)
    assert m is not None
    assert m.kind == "ARTIFACTS_READY"
    assert m.path == "/RUSH-DATA/results/livedev/rt-3"


def test_emit_rejects_unknown_kind():
    """emit() raises on an unknown marker kind."""
    try:
        mk.emit("NOT_A_REAL_KIND", run_id="x")
        assert False, "should have raised"
    except ValueError:
        pass


def test_to_line_round_trip():
    """Marker.to_line() -> parse_marker() round-trips."""
    original = mk.Marker(
        kind="TEST_FAIL", run_id="rtl-1", exit_code=13,
        path="/RUSH-DATA/results/livedev/rtl-1",
    )
    line = original.to_line()
    parsed = mk.parse_marker(line)
    assert parsed is not None
    assert parsed.kind == original.kind
    assert parsed.run_id == original.run_id
    assert parsed.exit_code == original.exit_code
    assert parsed.path == original.path


# ─── Failure pattern detection ──────────────────────────────────────────────


def test_detect_kernel_panic():
    """A kernel panic line is detected as a failure."""
    assert mk.detect_failure("Kernel panic - not syncing: Attempted to kill init") == "kernel_panic"
    assert mk.detect_failure("BUG: kernel NULL pointer dereference") == "kernel_panic"
    assert mk.detect_failure("Call Trace:") == "kernel_panic"


def test_detect_emergency_mode():
    """Emergency/rescue mode is detected."""
    assert mk.detect_failure("You are in emergency mode.") == "emergency_mode"
    assert mk.detect_failure("Welcome to rescue mode") == "emergency_mode"


def test_detect_maintenance_prompt():
    """The 'Give root password for maintenance' prompt is detected."""
    assert mk.detect_failure("Give root password for maintenance") == "maintenance_prompt"
    assert mk.detect_failure("(or press Control-D to continue)") is None  # not failure alone


def test_detect_login_prompt():
    """A bare 'login:' line is detected as a failure (pre-test)."""
    assert mk.detect_failure("login:") == "login_prompt_before_test"
    assert mk.detect_failure("login: ") == "login_prompt_before_test"


def test_detect_root_shell():
    """A root shell prompt is detected."""
    assert mk.detect_failure("root@host:~# ") == "root_shell"
    assert mk.detect_failure("bash-5.1# ") == "root_shell"
    assert mk.detect_failure("~# ") == "root_shell"


def test_detect_systemd_failed_unit():
    """A systemd failed-unit message is detected."""
    assert mk.detect_failure("Job for optid.service failed") == "systemd_failed_unit"
    assert mk.detect_failure("FAILED optid.service") == "systemd_failed_unit"


def test_detect_failure_returns_none_for_normal_lines():
    """Normal kernel/boot lines are not failures."""
    assert mk.detect_failure("[ OK ] Started Network Manager.") is None
    assert mk.detect_failure("Linux version 6.1.0-rush") is None
    assert mk.detect_failure("RUSH_LIVEDEV_TEST_PASS run_id=r1") is None
    assert mk.detect_failure("some test output: login form submitted") is None


# ─── End-to-end marker stream parsing ───────────────────────────────────────


def test_parse_marker_stream():
    """A multi-line console stream is parsed correctly line-by-line."""
    import io
    stream = io.StringIO(
        "Linux version 6.1.0\n"
        "[ OK ] Started optid.service\n"
        "RUSH_LIVEDEV_BOOT_READY run_id=e2e-1\n"
        "RUSH_LIVEDEV_TEST_START run_id=e2e-1\n"
        "running tests...\n"
        "all tests passed\n"
        "RUSH_LIVEDEV_TEST_PASS run_id=e2e-1\n"
        "RUSH_LIVEDEV_ARTIFACTS_READY run_id=e2e-1 path=/RUSH-DATA/results/livedev/e2e-1\n"
        "RUSH_LIVEDEV_SHUTDOWN run_id=e2e-1\n"
    )
    markers = []
    failures = []
    for line in stream:
        m = mk.parse_marker(line)
        if m:
            markers.append(m)
            continue
        f = mk.detect_failure(line)
        if f:
            failures.append(f)
    assert len(markers) == 5
    assert markers[0].kind == "BOOT_READY"
    assert markers[1].kind == "TEST_START"
    assert markers[2].kind == "TEST_PASS"
    assert markers[3].kind == "ARTIFACTS_READY"
    assert markers[4].kind == "SHUTDOWN"
    assert failures == []


def test_parse_marker_stream_with_failure():
    """A stream with a panic is detected as a failure."""
    import io
    stream = io.StringIO(
        "Linux version 6.1.0\n"
        "Kernel panic - not syncing: Attempted to kill init\n"
        "RUSH_LIVEDEV_TEST_PASS run_id=e2e-2\n"  # too late, panic already happened
    )
    markers = []
    failures = []
    for line in stream:
        m = mk.parse_marker(line)
        if m:
            markers.append(m)
            continue
        f = mk.detect_failure(line)
        if f:
            failures.append(f)
    # The panic should be detected.
    assert "kernel_panic" in failures
    # The marker is still parsed (the host decides whether to act on it).
    assert len(markers) == 1


# ─── Standalone runner ──────────────────────────────────────────────────────


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
