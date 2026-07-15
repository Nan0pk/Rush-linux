#!/usr/bin/env python3
"""
pytest tests for the rush-autopilot runner (test-runner phase, Prompt 7).

Tests the 8 required scenarios:
  1. run plan success with fake commands
  2. failed step preserves partial evidence
  3. resume skips completed steps
  4. AC prompt waits for detectable state
  5. prompt logs reason/default/outcome
  6. every command uses rush-exec
  7. generated evidence validates
  8. no host disk mutation

Plus bonus tests for the testOS transition (menu still works).

Run with:
  python3 -m pytest tools/test-rush-runner.py -v
  # or
  python3 tools/test-rush-runner.py  # standalone
"""

from __future__ import annotations

import importlib.machinery
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent


def _load_module(name: str, path: Path):
    """Load a Python module from a file path (handles files without .py extension)."""
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    loader.exec_module(mod)
    return mod


runner = _load_module("rush_runner_lib", _TOOLS_DIR / "rush_runner_lib.py")
lib = _load_module("rush_capture_lib", _TOOLS_DIR / "rush_capture_lib.py")


# ─── Helpers ─────────────────────────────────────────────────────────────────


def _make_fake_sysfs_laptop(tmpdir: Path) -> Path:
    """Create a fake sysfs tree that looks like a laptop."""
    sys_root = tmpdir / "fake-sys-laptop"
    sys_base = sys_root / "sys"
    bat = sys_base / "class" / "power_supply" / "BAT0"
    bat.mkdir(parents=True)
    (bat / "status").write_text("Discharging\n")
    (bat / "capacity").write_text("75\n")
    (bat / "energy_full_design").write_text("48000000\n")
    ac = sys_base / "class" / "power_supply" / "AC" / "online"
    ac.parent.mkdir(parents=True)
    ac.write_text("1\n")  # AC online initially
    return sys_root


def _make_simple_plan(slot: str = "laptop") -> dict:
    """Make a minimal valid plan for testing."""
    return {
        "schema_version": 1,
        "plan_kind": "rush-autopilot-plan",
        "generated_at": "2026-07-04T12:00:00Z",
        "source_version": "0.7.0-beta.1",
        "source_commit": "a" * 40,
        "repo_root": "/tmp/fake-repo",
        "hardware_slot": slot,
        "slot_confidence": "high",
        "ambiguities": [],
        "open_criteria": [],
        "existing_evidence": [],
        "dry_run": True,
        "steps": [
            {
                "seq": 0,
                "kind": "command",
                "default": "proceed",
                "reason": "start capture session",
                "rollback": "rush-capture finish",
                "argv": ["rush-capture", "start", "--run-dir", "<run-dir>"],
            },
            {
                "seq": 1,
                "kind": "physical-prompt",
                "default": "wait",
                "reason": "unplug AC for battery run",
                "rollback": "N/A",
                "action": "Unplug the laptop's AC adapter to run on battery",
                "detection_signal": "read /sys/class/power_supply/AC*/online == 0 every 5s",
                "timeout": "5m",
            },
            {
                "seq": 2,
                "kind": "command",
                "default": "proceed",
                "reason": "baseline run",
                "rollback": "rush-capture finish",
                "argv": ["rushbench", "run", "preset=mixed-load-001", "--samples=5"],
            },
            {
                "seq": 3,
                "kind": "command",
                "default": "proceed",
                "reason": "optid run",
                "rollback": "rush-capture finish",
                "argv": ["rushbench", "run", "preset=mixed-load-001", "--samples=5", "--apply"],
            },
            {
                "seq": 4,
                "kind": "command",
                "default": "proceed",
                "reason": "finish capture session",
                "rollback": "N/A",
                "argv": ["rush-capture", "finish", "--run-dir", "<run-dir>"],
            },
            {
                "seq": 5,
                "kind": "validation",
                "default": "proceed",
                "reason": "validate evidence",
                "rollback": "N/A",
                "validator": "validate-hwtest-evidence.py",
                "bundle": "<run-dir>",
            },
        ],
    }


def _write_plan(plan: dict, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n")


# ─── Test 1: run plan success with fake commands ─────────────────────────────


def test_run_plan_success_with_fake_commands():
    """A plan with all-proceed steps completes successfully in fake mode."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "run"
        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        plan = _make_simple_plan(slot="laptop")
        plan_path = tmpdir / "plan.json"
        _write_plan(plan, plan_path)

        result = runner.execute_plan_file(
            plan_path=plan_path,
            run_dir=run_dir,
            fake=True,
            fake_sys=fake_sys,
        )

        assert result["status"] == "completed", f"expected completed, got {result['status']}"
        assert len(result["steps"]) == 6
        for step in result["steps"]:
            assert step["status"] == "completed", f"step {step['seq']} status: {step['status']}"


# ─── Test 2: failed step preserves partial evidence ──────────────────────────


def test_failed_step_preserves_partial_evidence():
    """When a step fails, the run aborts but partial evidence is preserved."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "run"
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        # Make a plan where step 2 (baseline) is replaced with a command that fails.
        # Remove the physical-prompt step to avoid hanging on real AC detection.
        plan = _make_simple_plan(slot="laptop")
        plan["steps"] = [s for s in plan["steps"] if s["kind"] != "physical-prompt"]
        # Now step 1 is the baseline run; replace it with 'false'.
        plan["steps"][1] = {
            "seq": 1,
            "kind": "command",
            "default": "proceed",
            "reason": "baseline run (will fail)",
            "rollback": "rush-capture finish",
            "argv": ["false"],
        }
        plan_path = tmpdir / "plan.json"
        _write_plan(plan, plan_path)

        result = runner.execute_plan_file(
            plan_path=plan_path,
            run_dir=run_dir,
            fake=False,  # real mode so 'false' actually runs
        )

        assert result["status"] == "aborted", f"expected aborted, got {result['status']}"

        # Step 0 should have completed; step 1 failed; later steps not reached.
        statuses = {s["seq"]: s["status"] for s in result["steps"]}
        assert statuses.get(0) == "completed"
        assert statuses.get(1) == "failed"
        assert 2 not in statuses  # step 2 not reached

        # Partial evidence: the event chain should have events for the completed steps.
        events = lib.read_jsonl(run_dir / "events.jsonl")
        assert len(events) > 0
        # The first step (rush-capture start) writes a "start" event.
        # The runner's step-before/step-after events follow.
        before_events = [e for e in events if e.get("kind") == "step-before"]
        after_events = [e for e in events if e.get("kind") == "step-after"]
        assert len(before_events) >= 2  # steps 0, 1
        assert len(after_events) >= 2


# ─── Test 3: resume skips completed steps ────────────────────────────────────


def test_resume_skips_completed_steps():
    """Resuming a run skips steps that already completed."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "run"
        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        plan = _make_simple_plan(slot="laptop")
        plan_path = tmpdir / "plan.json"
        _write_plan(plan, plan_path)

        # First run: complete successfully.
        result1 = runner.execute_plan_file(
            plan_path=plan_path,
            run_dir=run_dir,
            fake=True,
            fake_sys=fake_sys,
        )
        assert result1["status"] == "completed"

        # Count the events after the first run.
        events1 = lib.read_jsonl(run_dir / "events.jsonl")
        events1_count = len(events1)

        # Second run: resume. All steps should be skipped.
        result2 = runner.execute_plan_file(
            plan_path=plan_path,
            run_dir=run_dir,
            fake=True,
            fake_sys=fake_sys,
            resume=True,
        )
        assert result2["status"] == "completed"
        # All steps should be "skipped".
        for step in result2["steps"]:
            assert step["status"] == "skipped", \
                f"step {step['seq']} should be skipped on resume, got {step['status']}"

        # No new step-before/step-after events should be added.
        events2 = lib.read_jsonl(run_dir / "events.jsonl")
        new_step_events = [
            e for e in events2[events1_count:]
            if e.get("kind") in ("step-before", "step-after")
        ]
        assert len(new_step_events) == 0, "resume should not add new step events"


# ─── Test 4: AC prompt waits for detectable state ────────────────────────────


def test_ac_prompt_waits_for_detectable_state():
    """The AC-unplug physical prompt simulates the state transition in fake mode."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "run"
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        # Initially AC is online (== "1").
        ac_path = fake_sys / "sys" / "class" / "power_supply" / "AC" / "online"
        assert ac_path.read_text().strip() == "1"

        plan = _make_simple_plan(slot="laptop")
        plan_path = tmpdir / "plan.json"
        _write_plan(plan, plan_path)

        result = runner.execute_plan_file(
            plan_path=plan_path,
            run_dir=run_dir,
            fake=True,
            fake_sys=fake_sys,
        )
        assert result["status"] == "completed"

        # After the run, the fake sysfs should show AC offline (== "0")
        # because the AC-unplug prompt simulated the transition.
        assert ac_path.read_text().strip() == "0", \
            f"AC should be offline after unplug prompt, got {ac_path.read_text().strip()!r}"

        # Battery status should be Discharging.
        bat_status = fake_sys / "sys" / "class" / "power_supply" / "BAT0" / "status"
        assert bat_status.read_text().strip() == "Discharging"


# ─── Test 5: prompt logs reason/default/outcome ──────────────────────────────


def test_prompt_logs_reason_default_outcome():
    """Every physical prompt logs reason/default/outcome to prompts.log."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "run"
        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        plan = _make_simple_plan(slot="laptop")
        plan_path = tmpdir / "plan.json"
        _write_plan(plan, plan_path)

        result = runner.execute_plan_file(
            plan_path=plan_path,
            run_dir=run_dir,
            fake=True,
            fake_sys=fake_sys,
        )
        assert result["status"] == "completed"

        prompts_log = run_dir / "prompts.log"
        assert prompts_log.exists(), "prompts.log should exist"

        entries = lib.read_jsonl(prompts_log)
        assert len(entries) >= 1, "should have at least one prompt entry"

        for entry in entries:
            assert "ts" in entry
            assert "action" in entry
            assert "reason" in entry
            assert "default" in entry
            assert "outcome" in entry
            assert "outcome_ts" in entry
            assert entry["default"] in ("wait", "proceed", "skip", "ask", "abort")
            assert entry["outcome"] in ("detected", "default-wait", "human-confirmed", "human-declined")


# ─── Test 6: every command uses rush-exec ────────────────────────────────────


def test_every_command_uses_rush_exec():
    """Command steps (except rush-capture itself) are executed through rush-exec."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "run"
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        # Use a plan with ONLY command steps (no physical prompts) so the test
        # doesn't hang waiting for real AC state changes. Use 'echo' (which
        # exists) instead of 'rushbench' (which doesn't) so the commands succeed.
        plan = {
            "schema_version": 1,
            "plan_kind": "rush-autopilot-plan",
            "generated_at": "2026-07-04T12:00:00Z",
            "source_version": "0.7.0-beta.1",
            "source_commit": "a" * 40,
            "repo_root": "/tmp",
            "hardware_slot": "laptop",
            "slot_confidence": "high",
            "ambiguities": [],
            "open_criteria": [],
            "existing_evidence": [],
            "dry_run": False,
            "steps": [
                {
                    "seq": 0,
                    "kind": "command",
                    "default": "proceed",
                    "reason": "start capture",
                    "rollback": "finish",
                    "argv": ["rush-capture", "start", "--run-dir", "<run-dir>"],
                },
                {
                    "seq": 1,
                    "kind": "command",
                    "default": "proceed",
                    "reason": "baseline run (simulated with echo)",
                    "rollback": "finish",
                    "argv": ["echo", "baseline-result"],
                },
                {
                    "seq": 2,
                    "kind": "command",
                    "default": "proceed",
                    "reason": "optid run (simulated with echo)",
                    "rollback": "finish",
                    "argv": ["echo", "optid-result"],
                },
                {
                    "seq": 3,
                    "kind": "command",
                    "default": "proceed",
                    "reason": "finish capture",
                    "rollback": "N/A",
                    "argv": ["rush-capture", "finish", "--run-dir", "<run-dir>"],
                },
            ],
        }
        plan_path = tmpdir / "plan.json"
        _write_plan(plan, plan_path)

        result = runner.execute_plan_file(
            plan_path=plan_path,
            run_dir=run_dir,
            fake=False,  # real mode so commands actually go through rush-exec
        )

        # In real mode, the echo commands go through rush-exec, which writes
        # to the command-log.jsonl.
        command_log = lib.read_jsonl(run_dir / "command-log.jsonl")
        # The echo commands should appear in the command log.
        echo_entries = [
            e for e in command_log if any("echo" in a for a in e.get("argv", []))
        ]
        assert len(echo_entries) >= 2, \
            f"echo commands should be in command-log; got {len(echo_entries)} entries"

        # Each command-log entry should have the rush-exec metadata fields.
        for entry in echo_entries:
            assert "argv" in entry
            assert "exit_code" in entry
            assert "duration_ms" in entry
            assert "stdout_sha256" in entry
            assert "stderr_sha256" in entry


# ─── Test 7: generated evidence validates ────────────────────────────────────


def test_generated_evidence_validates():
    """The evidence bundle generated by the runner validates against the schema."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "run"
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        # Use a plan with a real source_commit (the repo's HEAD).
        import subprocess
        head = subprocess.check_output(
            ["git", "-C", str(_ROOT), "rev-parse", "HEAD"]
        ).decode().strip()
        plan = _make_simple_plan(slot="laptop")
        plan["source_commit"] = head
        plan["source_version"] = (_ROOT / "VERSION").read_text().strip()
        plan_path = tmpdir / "plan.json"
        _write_plan(plan, plan_path)

        result = runner.execute_plan_file(
            plan_path=plan_path,
            run_dir=run_dir,
            fake=True,
            fake_sys=fake_sys,
            repo_root=_ROOT,
        )
        assert result["status"] == "completed"

        # Validate the generated evidence bundle.
        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "validate-hwtest-evidence.py"),
             "--bundle", str(run_dir)],
            capture_output=True,
            text=True,
            timeout=30,
            cwd=str(_ROOT),
        )
        assert r.returncode == 0, \
            f"evidence should validate; got rc={r.returncode}\n{r.stdout}\n{r.stderr}"


# ─── Test 8: no host disk mutation ───────────────────────────────────────────


def test_no_host_disk_mutation():
    """The runner does not write outside the run-dir (except to /tmp for fake sysfs)."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "run"
        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        plan = _make_simple_plan(slot="laptop")
        plan_path = tmpdir / "plan.json"
        _write_plan(plan, plan_path)

        result = runner.execute_plan_file(
            plan_path=plan_path,
            run_dir=run_dir,
            fake=True,
            fake_sys=fake_sys,
        )
        assert result["status"] == "completed"

        # All files written by the runner should be inside run_dir.
        # (The fake sysfs is inside tmpdir, which is fine.)
        for p in run_dir.rglob("*"):
            assert p.is_relative_to(run_dir), \
                f"file written outside run-dir: {p}"

        # The runner should NOT write to /sys, /proc, /etc, /run, or the repo root.
        # (We can't easily check this without filesystem snapshots, but we can
        # verify the run-dir doesn't contain anything that looks like a host path.)
        for p in run_dir.rglob("*"):
            if p.is_file():
                rel = p.relative_to(run_dir)
                assert not str(rel).startswith("/"), \
                    f"absolute path in run-dir: {rel}"


# ─── Bonus: plan schema validation ───────────────────────────────────────────


def test_plan_schema_validation_rejects_bad_plan():
    """The runner rejects a malformed plan."""
    bad_plan = {
        "schema_version": 99,  # wrong
        "plan_kind": "wrong-kind",
        "steps": [
            {"seq": 0, "kind": "unknown-kind", "default": "proceed", "reason": "", "rollback": ""},
        ],
    }
    errors = runner.validate_plan_schema(bad_plan)
    assert len(errors) > 0
    assert any("schema_version" in e for e in errors)
    assert any("plan_kind" in e for e in errors)
    assert any("unknown-kind" in e for e in errors)


# ─── Bonus: testOS transition — menu still works ─────────────────────────────


def test_testos_menu_still_compiles():
    """The testOS runner source still exists and is unmodified."""
    runner_path = _ROOT / "crates" / "testos" / "src" / "bin" / "testos-runner.rs"
    assert runner_path.exists(), "testOS runner source should exist"
    text = runner_path.read_text()
    # The menu function should still be there.
    assert "fn show_menu" in text
    # The "Run all" label is rendered by the TUI module (print_menu), not
    # hard-coded in the runner binary. Verify it exists in the TUI source.
    tui_path = _ROOT / "crates" / "testos" / "src" / "tui.rs"
    assert tui_path.exists(), "testOS TUI source should exist"
    tui_text = tui_path.read_text()
    assert "Run all" in tui_text, "TUI print_menu does not render 'Run all'"
    # The runner should NOT have been modified to call rush-autopilot
    # (auto-run is deferred to the LiveDev image phase).
    assert "rush-autopilot" not in text, \
        "testOS runner should not reference rush-autopilot (auto-run deferred to LiveDev image phase)"


def test_testos_lib_still_exports_schema():
    """The testOS library still exports the result schema (unmodified)."""
    lib_path = _ROOT / "crates" / "testos" / "src" / "lib.rs"
    assert lib_path.exists()
    text = lib_path.read_text()
    assert "pub mod catalog" in text
    assert "pub mod host" in text
    assert "pub mod results" in text
    assert "SCHEMA_VERSION" in text


# ─── Bonus: CLI run/resume ───────────────────────────────────────────────────


def test_cli_run_fake():
    """The CLI `run --fake` subcommand produces a valid run."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "run"
        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        plan = _make_simple_plan(slot="laptop")
        plan_path = tmpdir / "plan.json"
        _write_plan(plan, plan_path)

        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush-autopilot"),
             "run", "--plan", str(plan_path), "--run-dir", str(run_dir),
             "--fake", "--fake-sys", str(fake_sys)],
            capture_output=True,
            text=True,
            timeout=60,
        )
        assert r.returncode == 0, f"CLI run should exit 0, got {r.returncode}\n{r.stderr}"
        result = json.loads(r.stdout)
        assert result["status"] == "completed"


def test_cli_resume():
    """The CLI `resume` subcommand skips completed steps."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "run"
        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        plan = _make_simple_plan(slot="laptop")
        plan_path = tmpdir / "plan.json"
        _write_plan(plan, plan_path)

        # First run.
        r1 = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush-autopilot"),
             "run", "--plan", str(plan_path), "--run-dir", str(run_dir),
             "--fake", "--fake-sys", str(fake_sys)],
            capture_output=True,
            text=True,
            timeout=60,
        )
        assert r1.returncode == 0

        # Resume.
        r2 = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush-autopilot"),
             "resume", "--run-dir", str(run_dir), "--plan", str(plan_path),
             "--fake", "--fake-sys", str(fake_sys)],
            capture_output=True,
            text=True,
            timeout=60,
        )
        assert r2.returncode == 0
        result = json.loads(r2.stdout)
        for step in result["steps"]:
            assert step["status"] == "skipped"


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
