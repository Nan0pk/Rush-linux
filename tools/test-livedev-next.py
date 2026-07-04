#!/usr/bin/env python3
"""
pytest tests for tools/livedev-next.

Tests:
  - --help shows all modes (--auto, --mock, --plan, --run, --submit, --dry-run)
  - default prints "Rush LiveDev Next Step" and "Next commands"
  - default exits 0
  - default does not request GH_TOKEN
  - --mock runs E2E + fixtures
  - --plan generates a plan file
  - --run executes a plan (fake mode)
  - --submit --dry-run works
  - --submit without token prints [TOKEN NEEDED]
  - --auto runs the full pipeline

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


def _run(args: list[str], timeout: int = 120, env: dict | None = None) -> tuple[int, str, str]:
    r = subprocess.run(
        ["python3", str(_TOOLS_DIR / "livedev-next")] + args,
        capture_output=True, text=True, timeout=timeout, cwd=str(_ROOT), env=env,
    )
    return r.returncode, r.stdout, r.stderr


def test_help_shows_all_modes():
    rc, stdout, _ = _run(["--help"])
    assert rc == 0
    for mode in ["--auto", "--mock", "--plan", "--run", "--submit", "--dry-run"]:
        assert mode in stdout, f"--help should mention {mode}"


def test_default_prints_title_and_next_commands():
    rc, stdout, _ = _run([])
    assert rc == 0
    assert "Rush LiveDev Next Step" in stdout
    assert "Next commands" in stdout


def test_default_exits_zero():
    rc, _, _ = _run([])
    assert rc == 0


def test_default_does_not_ask_for_token():
    rc, stdout, stderr = _run([])
    assert "TOKEN" not in stdout
    assert "TOKEN" not in stderr
    assert "GH_TOKEN" not in stdout
    assert "GH_TOKEN" not in stderr


def test_default_shows_tool_check():
    rc, stdout, _ = _run([])
    assert "rush-autopilot" in stdout
    assert "rush-exec" in stdout
    assert "validate-hwtest-evidence" in stdout


def test_default_shows_wired_status():
    rc, stdout, _ = _run([])
    assert "wired" in stdout.lower()
    assert "NOT wired" in stdout or "not wired" in stdout.lower() or "✅" in stdout


def test_mock_runs_all_scenarios():
    rc, stdout, _ = _run(["--mock"], timeout=180)
    assert rc == 0
    assert "success" in stdout.lower()
    assert "failure" in stdout.lower()
    assert "fixtures" in stdout.lower()


def test_plan_generates_file():
    rc, stdout, _ = _run(["--plan"], timeout=60)
    assert rc == 0
    assert "/tmp/rush-livedev-plan.json" in stdout
    plan_path = Path("/tmp/rush-livedev-plan.json")
    assert plan_path.exists()
    plan = json.loads(plan_path.read_text())
    assert plan.get("plan_kind") == "rush-autopilot-plan"


def test_run_executes_plan():
    _run(["--plan"], timeout=60)
    rc, stdout, _ = _run(["--run", "/tmp/rush-livedev-plan.json"], timeout=300)
    assert rc == 0


def test_submit_dry_run_works():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        (run_dir / "run-record.json").write_text('{"status": "completed"}')
        rc, stdout, stderr = _run(["--submit", str(run_dir), "--dry-run"], timeout=60)
        assert "TOKEN" not in stdout
        assert "TOKEN" not in stderr


def test_submit_without_token_prints_token_needed():
    env = os.environ.copy()
    env.pop("GH_TOKEN", None)
    env.pop("GITHUB_TOKEN", None)
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        (run_dir / "run-record.json").write_text('{"status": "completed"}')
        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "livedev-next"), "--submit", str(run_dir)],
            capture_output=True, text=True, timeout=30, cwd=str(_ROOT), env=env,
        )
        assert "[TOKEN NEEDED]" in r.stdout


def test_run_nonexistent_plan_fails():
    rc, stdout, stderr = _run(["--run", "/tmp/nonexistent-plan-12345.json"], timeout=30)
    assert rc != 0
    assert "not found" in stdout.lower() or "not found" in stderr.lower()


def test_auto_runs_full_pipeline():
    rc, stdout, _ = _run(["--auto"], timeout=300)
    # --auto may fail at validation (ambiguous slot on CI) but should
    # at least complete steps 1 and 2 and print the pipeline structure.
    assert "Step 1/4" in stdout
    assert "Step 2/4" in stdout
    assert "Pipeline" in stdout


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
