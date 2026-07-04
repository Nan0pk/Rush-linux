#!/usr/bin/env python3
"""
pytest tests for tools/livedev-next.

Tests:
  - --help shows all modes
  - default mode prints "Rush LiveDev Next Step" and "Next commands"
  - default mode exits 0
  - default mode does not ask for GH_TOKEN
  - --mock runs E2E + fixtures
  - --plan generates a plan file
  - --run executes a plan (fake mode)
  - --submit --dry-run works
  - --submit without token prints [TOKEN NEEDED]

Run with:
  python3 -m pytest tools/test-livedev-next.py -v
  python3 tools/test-livedev-next.py  # standalone
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent


def _run(args: list[str], timeout: int = 120) -> tuple[int, str, str]:
    """Run livedev-next with given args, return (exit_code, stdout, stderr)."""
    r = subprocess.run(
        ["python3", str(_TOOLS_DIR / "livedev-next")] + args,
        capture_output=True,
        text=True,
        timeout=timeout,
        cwd=str(_ROOT),
    )
    return r.returncode, r.stdout, r.stderr


def test_help_shows_all_modes():
    """--help mentions --mock, --plan, --run, --submit, --dry-run."""
    rc, stdout, stderr = _run(["--help"])
    assert rc == 0
    assert "--mock" in stdout
    assert "--plan" in stdout
    assert "--run" in stdout
    assert "--submit" in stdout
    assert "--dry-run" in stdout


def test_default_prints_title_and_next_commands():
    """Default mode prints 'Rush LiveDev Next Step' and 'Next commands'."""
    rc, stdout, stderr = _run([])
    assert rc == 0
    assert "Rush LiveDev Next Step" in stdout
    assert "Next commands" in stdout


def test_default_exits_zero():
    """Default mode exits 0."""
    rc, _, _ = _run([])
    assert rc == 0


def test_default_does_not_ask_for_token():
    """Default mode does not ask for GH_TOKEN."""
    rc, stdout, stderr = _run([])
    assert "TOKEN" not in stdout
    assert "TOKEN" not in stderr
    assert "GH_TOKEN" not in stdout
    assert "GH_TOKEN" not in stderr


def test_default_shows_tool_check():
    """Default mode checks for required tools."""
    rc, stdout, _ = _run([])
    assert "rush-autopilot" in stdout
    assert "rush-exec" in stdout
    assert "validate-hwtest-evidence" in stdout


def test_mock_runs_all_scenarios():
    """--mock runs E2E + fixtures and exits 0."""
    rc, stdout, stderr = _run(["--mock"], timeout=180)
    assert rc == 0
    assert "success" in stdout.lower()
    assert "failure" in stdout.lower()
    assert "fixtures" in stdout.lower()


def test_plan_generates_file():
    """--plan generates a plan file at /tmp/rush-livedev-plan.json."""
    rc, stdout, stderr = _run(["--plan"], timeout=60)
    assert rc == 0
    assert "/tmp/rush-livedev-plan.json" in stdout
    # The file should exist.
    plan_path = Path("/tmp/rush-livedev-plan.json")
    assert plan_path.exists(), "plan file should exist"
    # Should be valid JSON.
    plan = json.loads(plan_path.read_text())
    assert plan.get("plan_kind") == "rush-autopilot-plan"


def test_run_executes_plan():
    """--run executes a plan in fake mode and exits 0."""
    # First generate a plan.
    _run(["--plan"], timeout=60)
    # Then run it.
    rc, stdout, stderr = _run(["--run", "/tmp/rush-livedev-plan.json"], timeout=300)
    assert rc == 0
    assert "Plan executed" in stdout or "run" in stdout.lower()


def test_submit_dry_run_works():
    """--submit --dry-run runs without requiring a token."""
    # Create a minimal run dir.
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        (run_dir / "run-record.json").write_text('{"status": "completed"}')

        rc, stdout, stderr = _run(["--submit", str(run_dir), "--dry-run"], timeout=60)
        # It may fail validation (no hwtest files) but should not ask for token.
        assert "TOKEN" not in stdout
        assert "TOKEN" not in stderr


def test_submit_without_token_prints_token_needed():
    """--submit without --dry-run and without GH_TOKEN prints [TOKEN NEEDED]."""
    # Ensure no token in environment.
    env = os.environ.copy()
    env.pop("GH_TOKEN", None)
    env.pop("GITHUB_TOKEN", None)

    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        (run_dir / "run-record.json").write_text('{"status": "completed"}')

        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "livedev-next"), "--submit", str(run_dir)],
            capture_output=True,
            text=True,
            timeout=30,
            cwd=str(_ROOT),
            env=env,
        )
        assert "[TOKEN NEEDED]" in r.stdout


def test_run_nonexistent_plan_fails():
    """--run with a nonexistent plan fails clearly."""
    rc, stdout, stderr = _run(["--run", "/tmp/nonexistent-plan-12345.json"], timeout=30)
    assert rc != 0
    assert "not found" in stdout.lower() or "not found" in stderr.lower()


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
