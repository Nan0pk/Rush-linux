#!/usr/bin/env python3
"""
pytest tests for rush-livedev-state.py — persistent test-intent state.

Covers:
  - State file creation/parsing (round-trip)
  - Atomic state update (write-temp + rename)
  - Validation rules (mode, status, run_id, attempt_count)
  - Schema version mismatch
  - Missing file handling
  - Invalid JSON handling
  - Concurrent-safe read-modify-write (logical, not real concurrency)

Run with:
  python3 -m pytest tools/test-livedev-state.py -v
  python3 tools/test-livedev-state.py  # standalone
"""

from __future__ import annotations

import importlib.machinery
import importlib.util
import json
import os
import sys
import tempfile
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


st = _load_module("rush_livedev_state", _TOOLS_DIR / "rush_livedev_state.py")


# ─── State creation/parsing ──────────────────────────────────────────────────


def test_state_creation_round_trip():
    """LiveDevState.new() + to_dict + from_dict round-trips losslessly."""
    s = st.LiveDevState.new(
        run_id="test-run-001",
        test_command="echo hello",
        suite="smoke",
        artifacts_host_path="/tmp/artifacts/test-run-001",
        submit_mode="local",
        debug=False,
        ci=True,
    )
    d = s.to_dict()
    s2 = st.LiveDevState.from_dict(d)
    assert s2.run_id == s.run_id
    assert s2.test_command == s.test_command
    assert s2.suite == s.suite
    assert s2.mode == "livedev-test"
    assert s2.status == "pending"
    assert s2.ci is True
    assert s2.schema_version == st.STATE_SCHEMA_VERSION


def test_state_default_artifacts_guest_path():
    """If artifacts_guest_path is empty, a sensible default is used."""
    s = st.LiveDevState.new(run_id="r1", test_command="true")
    assert s.artifacts_guest_path == "/RUSH-DATA/results/livedev/r1"


def test_state_default_artifacts_guest_path_with_run_id_substitution():
    """The default path includes <run_id> which the runner substitutes."""
    s = st.LiveDevState.new(run_id="r2", test_command="true")
    # The runner replaces <run_id> with state.run_id at runtime.
    assert "<run_id>" not in s.artifacts_guest_path  # already substituted
    assert "r2" in s.artifacts_guest_path


# ─── Atomic write ────────────────────────────────────────────────────────────


def test_atomic_write_creates_file():
    """StateStore.write() creates the file atomically."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "state.json"
        store = st.StateStore(path)
        s = st.LiveDevState.new(run_id="atomic-1", test_command="true")
        store.write(s)
        assert path.exists()
        # File should be valid JSON.
        d = json.loads(path.read_text())
        assert d["run_id"] == "atomic-1"


def test_atomic_write_no_temp_file_left_behind():
    """After write, no .tmp files remain in the parent directory."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "state.json"
        store = st.StateStore(path)
        s = st.LiveDevState.new(run_id="atomic-2", test_command="true")
        store.write(s)
        tmps = list(Path(tmp).glob(".*.tmp"))
        assert tmps == [], f"left-behind temp files: {tmps}"


def test_atomic_update_read_modify_write():
    """StateStore.update() does read-modify-write atomically."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "state.json"
        store = st.StateStore(path)
        s = st.LiveDevState.new(run_id="upd-1", test_command="true")
        store.write(s)

        def bump_attempt(state):
            state.attempt_count += 1
            state.status = "running"
            return state

        s2 = store.update(bump_attempt)
        assert s2.attempt_count == 1
        assert s2.status == "running"
        # Re-read to confirm it hit disk.
        s3 = store.read()
        assert s3.attempt_count == 1
        assert s3.status == "running"


def test_atomic_update_updated_at_changes():
    """Each write updates the updated_at timestamp."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "state.json"
        store = st.StateStore(path)
        s = st.LiveDevState.new(run_id="upd-2", test_command="true")
        store.write(s)
        first = s.updated_at
        # Sleep is unnecessary — _now_iso has 1-second resolution.
        import time
        time.sleep(1.1)
        store.update(lambda x: (setattr(x, "status", "running"), x)[1])
        s2 = store.read()
        assert s2.updated_at > first


# ─── Validation ──────────────────────────────────────────────────────────────


def test_validate_for_run_passes_on_fresh_state():
    """A fresh pending state has no validation errors."""
    s = st.LiveDevState.new(run_id="valid-1", test_command="true")
    assert s.validate_for_run() == []


def test_validate_for_run_fails_when_attempt_count_exhausted():
    """Refuses to run again when attempt_count >= max_attempts."""
    s = st.LiveDevState.new(
        run_id="exhausted-1", test_command="true", max_attempts=1,
    )
    s.attempt_count = 1
    errs = s.validate_for_run()
    assert any("attempt_count" in e for e in errs)


def test_validate_for_run_fails_when_already_terminal():
    """Refuses to re-run a state that already has a terminal status."""
    s = st.LiveDevState.new(run_id="done-1", test_command="true")
    s.status = "passed"
    errs = s.validate_for_run()
    assert any("already-terminal" in e or "status" in e for e in errs)


def test_validate_for_run_fails_when_mode_idle():
    """Refuses to run when mode is not 'livedev-test'."""
    s = st.LiveDevState.new(run_id="idle-1", test_command="true")
    s.mode = "idle"
    errs = s.validate_for_run()
    assert any("mode" in e for e in errs)


def test_invalid_run_id_rejected():
    """run_id must match the safe-character regex."""
    try:
        st.LiveDevState.new(run_id="bad run id!", test_command="true")
        assert False, "should have raised"
    except st.StateError as e:
        assert "run_id" in str(e)


def test_empty_test_command_rejected():
    """test_command must be non-empty."""
    try:
        st.LiveDevState.new(run_id="r1", test_command="")
        assert False, "should have raised"
    except st.StateError as e:
        assert "test_command" in str(e)


def test_invalid_submit_mode_rejected():
    """submit_mode must be one of the allowed values."""
    try:
        st.LiveDevState.new(
            run_id="r1", test_command="true", submit_mode="totally-invalid",
        )
        assert False, "should have raised"
    except st.StateError as e:
        assert "submit_mode" in str(e)


# ─── Schema version ─────────────────────────────────────────────────────────


def test_schema_version_mismatch_rejected():
    """A state file with the wrong schema_version is rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "state.json"
        path.write_text(json.dumps({
            "schema_version": 999,
            "mode": "livedev-test",
            "run_id": "r1",
            "test_command": "true",
            "status": "pending",
        }))
        try:
            st.StateStore(path).read()
            assert False, "should have raised"
        except st.StateError as e:
            assert "schema_version" in str(e)


# ─── Missing / corrupt file ─────────────────────────────────────────────────


def test_missing_file_raises_state_error():
    """read() raises StateError when the file does not exist."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "nope.json"
        try:
            st.StateStore(path).read()
            assert False, "should have raised"
        except st.StateError as e:
            assert "does not exist" in str(e)


def test_read_or_none_returns_none_for_missing():
    """read_or_none() returns None for a missing file."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "nope.json"
        assert st.StateStore(path).read_or_none() is None


def test_corrupt_json_raises_state_error():
    """Corrupt JSON is rejected loudly."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "state.json"
        path.write_text("{ this is not json")
        try:
            st.StateStore(path).read()
            assert False, "should have raised"
        except st.StateError as e:
            assert "not valid JSON" in str(e)


def test_delete_file_idempotent():
    """delete() does not raise if the file is already gone."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "state.json"
        store = st.StateStore(path)
        store.delete()  # should not raise
        store.delete()  # still should not raise


# ─── is_terminal ────────────────────────────────────────────────────────────


def test_is_terminal_recognizes_all_terminal_states():
    """All terminal statuses return True; non-terminal return False."""
    s = st.LiveDevState.new(run_id="r1", test_command="true")
    for status in ("passed", "failed", "timeout", "skipped"):
        s.status = status
        assert s.is_terminal(), f"{status} should be terminal"
    for status in ("pending", "running"):
        s.status = status
        assert not s.is_terminal(), f"{status} should not be terminal"


# ─── CLI ─────────────────────────────────────────────────────────────────────


def test_cli_new_and_show():
    """The `new` and `show` CLI subcommands work end-to-end."""
    import subprocess
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "state.json"
        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush_livedev_state.py"),
             "--path", str(path), "new",
             "--run-id", "cli-1", "--test-command", "echo hi",
             "--suite", "smoke", "--submit", "local"],
            capture_output=True, text=True, timeout=10,
        )
        assert r.returncode == 0, r.stderr
        assert path.exists()
        r2 = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush_livedev_state.py"),
             "--path", str(path), "show"],
            capture_output=True, text=True, timeout=10,
        )
        assert r2.returncode == 0
        d = json.loads(r2.stdout)
        assert d["run_id"] == "cli-1"
        assert d["test_command"] == "echo hi"


def test_cli_validate():
    """The `validate` CLI subcommand exits 0 on a valid state."""
    import subprocess
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "state.json"
        subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush_livedev_state.py"),
             "--path", str(path), "new",
             "--run-id", "cli-2", "--test-command", "true"],
            check=True, capture_output=True, timeout=10,
        )
        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush_livedev_state.py"),
             "--path", str(path), "validate"],
            capture_output=True, text=True, timeout=10,
        )
        assert r.returncode == 0, r.stderr


# ─── Standalone runner ───────────────────────────────────────────────────────


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
