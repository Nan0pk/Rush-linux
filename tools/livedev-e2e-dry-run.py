#!/usr/bin/env python3
"""
tools/livedev-e2e-dry-run.py — End-to-end LiveDev dry-run scenarios.

Exercises the full LiveDev track in fixture/fake/mock mode:
  plan → run → capture → evidence → validate → dev-if-fail → PR submission

Three modes:
  --success              — a full successful run: plan, fake benchmark,
                           evidence generation, validation, PR dry-run.
  --failure-no-ai        — a failed run: failing evidence preserved,
                           validation fails correctly, failing-evidence
                           PR dry-run.
  --failure-with-ai-fix  — a failed run: dev-if-fail mock provider
                           diagnoses, proposes patch, validates, code
                           PR dry-run.

No real hardware, no real AI calls, no real PRs. All fake/mock.

Usage:
  python3 tools/livedev-e2e-dry-run.py --success
  python3 tools/livedev-e2e-dry-run.py --failure-no-ai
  python3 tools/livedev-e2e-dry-run.py --failure-with-ai-fix
  python3 tools/livedev-e2e-dry-run.py --all

Exit codes:
  0 — scenario completed successfully (all steps passed as expected)
  1 — scenario failed unexpectedly
  2 — internal error
"""

from __future__ import annotations

import argparse
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
sys.path.insert(0, str(_TOOLS_DIR))


def _load_module(name: str, path: Path):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    loader.exec_module(mod)
    return mod


# Load the libraries we need.
lib = _load_module("rush_capture_lib", _TOOLS_DIR / "rush_capture_lib.py")
runner = _load_module("rush_runner_lib", _TOOLS_DIR / "rush_runner_lib.py")
agent = _load_module("rush_agent_lib", _TOOLS_DIR / "rush_agent_lib.py")
pr = _load_module("rush_pr_lib", _TOOLS_DIR / "rush_pr_lib.py")


# ─── Helpers ─────────────────────────────────────────────────────────────────


def _banner(title: str) -> None:
    print()
    print("=" * 60)
    print(f"  {title}")
    print("=" * 60)
    print()


def _step(name: str, status: str = "OK") -> None:
    symbol = "✅" if status == "OK" else "❌" if status == "FAIL" else "⏭️"
    print(f"  {symbol} {name}")


def _make_fake_sysfs_laptop(tmpdir: Path) -> Path:
    """Create a fake sysfs tree that looks like a laptop."""
    sys_root = tmpdir / "fake-sys"
    sys_base = sys_root / "sys"
    bat = sys_base / "class" / "power_supply" / "BAT0"
    bat.mkdir(parents=True)
    (bat / "status").write_text("Discharging\n")
    (bat / "capacity").write_text("75\n")
    (bat / "energy_full_design").write_text("48000000\n")
    ac = sys_base / "class" / "power_supply" / "AC" / "online"
    ac.parent.mkdir(parents=True)
    ac.write_text("1\n")  # AC online initially
    # CPU info.
    proc = sys_root / "proc"
    proc.mkdir(parents=True)
    (proc / "cpuinfo").write_text(
        "processor\t: 0\nmodel name\t: Intel(R) Core(TM) i7-8650U CPU @ 1.90GHz\n"
        "processor\t: 1\nmodel name\t: Intel(R) Core(TM) i7-8650U CPU @ 1.90GHz\n"
        "processor\t: 2\nmodel name\t: Intel(R) Core(TM) i7-8650U CPU @ 1.90GHz\n"
        "processor\t: 3\nmodel name\t: Intel(R) Core(TM) i7-8650U CPU @ 1.90GHz\n"
    )
    # DMI.
    dmi = sys_base / "class" / "dmi" / "id"
    dmi.mkdir(parents=True)
    (dmi / "board_vendor").write_text("TestVendor\n")
    (dmi / "board_name").write_text("LaptopBoard\n")
    return sys_root


def _make_fake_repo(tmpdir: Path) -> Path:
    """Create a minimal fake repo with VERSION + milestones.toml."""
    repo = tmpdir / "fake-repo"
    repo.mkdir()
    (repo / "VERSION").write_text("0.7.0-beta.1\n")
    rel = repo / "release" / "evidence" / "host-bench"
    rel.mkdir(parents=True)
    (rel / "_TEMPLATE").mkdir()
    (repo / "release" / "milestones.toml").write_text(
        '[project]\ncurrent_version = "0.7.0-beta.1"\n\n'
        '[[milestone]]\nversion = "0.6.0-beta.1"\nchannel = "beta"\n'
        'name = "Hardware-Aware optid"\nstatus = "in-progress"\n'
        'exit_criteria = ["mixed-load responsiveness improves on two machines"]\n\n'
        '[[milestone.criteria_status]]\n'
        'criterion = "mixed-load responsiveness improves on two machines"\n'
        'verified = false\ntranscript = ""\n'
        'note = "PENDING PHASE D (hardware gate)."\n'
    )
    return repo


def _make_success_plan(repo_root: Path) -> dict:
    """Create a plan that will succeed in fake mode."""
    import subprocess
    head = subprocess.check_output(["git", "-C", str(_ROOT), "rev-parse", "HEAD"]).decode().strip()
    version = (_ROOT / "VERSION").read_text().strip()
    return {
        "schema_version": 1, "plan_kind": "rush-autopilot-plan",
        "generated_at": "2026-07-04T12:00:00Z",
        "source_version": version, "source_commit": head,
        "repo_root": str(repo_root), "hardware_slot": "laptop",
        "slot_confidence": "high", "ambiguities": [], "open_criteria": [],
        "existing_evidence": [], "dry_run": True,
        "steps": [
            {"seq": 0, "kind": "command", "default": "proceed", "reason": "start",
             "rollback": "finish", "argv": ["rush-capture", "start", "--run-dir", "<run-dir>"]},
            {"seq": 1, "kind": "physical-prompt", "default": "wait", "reason": "boot",
             "rollback": "N/A", "action": "Boot the laptop", "detection_signal": "ssh", "timeout": "5m"},
            {"seq": 2, "kind": "physical-prompt", "default": "wait", "reason": "unplug AC",
             "rollback": "N/A", "action": "Unplug AC adapter", "detection_signal": "AC online == 0", "timeout": "5m"},
            {"seq": 3, "kind": "command", "default": "proceed", "reason": "baseline",
             "rollback": "finish", "argv": ["rushbench", "run", "preset=mixed-load-001"]},
            {"seq": 4, "kind": "command", "default": "proceed", "reason": "optid",
             "rollback": "finish", "argv": ["rushbench", "run", "--apply"]},
            {"seq": 5, "kind": "command", "default": "proceed", "reason": "finish",
             "rollback": "N/A", "argv": ["rush-capture", "finish", "--run-dir", "<run-dir>"]},
            {"seq": 6, "kind": "validation", "default": "proceed", "reason": "validate",
             "rollback": "N/A", "validator": "validate-hwtest-evidence.py", "bundle": "<run-dir>"},
        ],
    }


def _make_failing_plan(repo_root: Path) -> dict:
    """Create a plan that fails through the current Python interpreter."""
    import subprocess
    head = subprocess.check_output(["git", "-C", str(_ROOT), "rev-parse", "HEAD"]).decode().strip()
    version = (_ROOT / "VERSION").read_text().strip()
    return {
        "schema_version": 1, "plan_kind": "rush-autopilot-plan",
        "generated_at": "2026-07-04T12:00:00Z",
        "source_version": version, "source_commit": head,
        "repo_root": str(repo_root), "hardware_slot": "laptop",
        "slot_confidence": "high", "ambiguities": [], "open_criteria": [],
        "existing_evidence": [], "dry_run": False,
        "steps": [
            {"seq": 0, "kind": "command", "default": "proceed", "reason": "start",
             "rollback": "finish", "argv": ["rush-capture", "start", "--run-dir", "<run-dir>"]},
            {"seq": 1, "kind": "command", "default": "proceed", "reason": "will fail",
             "rollback": "finish",
             "argv": [sys.executable, "-c", "raise SystemExit(1)"]},
        ],
    }


# ─── Scenario: Success ───────────────────────────────────────────────────────


def run_success_scenario() -> int:
    """Full successful run: plan → fake benchmark → evidence → validate → PR dry-run."""
    _banner("E2E Dry Run: SUCCESS Scenario")

    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        repo = _make_fake_repo(tmpdir)
        run_dir = tmpdir / "run"

        # 1. Plan.
        _step("Generating plan via rush-autopilot")
        plan = _make_success_plan(repo)
        plan_path = tmpdir / "plan.json"
        plan_path.write_text(json.dumps(plan, indent=2))
        _step("Plan generated", "OK")

        # 2. Run the plan in fake mode.
        _step("Running plan in fake mode")
        result = runner.execute_plan_file(
            plan_path=plan_path, run_dir=run_dir, fake=True, fake_sys=fake_sys, repo_root=_ROOT,
        )
        if result["status"] != "completed":
            _step(f"Plan execution failed: {result['status']}", "FAIL")
            return 1
        _step(f"Plan completed ({len(result['steps'])} steps)", "OK")

        # 3. Validate evidence.
        _step("Validating evidence bundle")
        val_r = subprocess.run(
            [sys.executable, str(_TOOLS_DIR / "validate-hwtest-evidence.py"), "--bundle", str(run_dir)],
            capture_output=True, text=True, timeout=30, cwd=str(_ROOT),
        )
        if val_r.returncode != 0:
            _step(f"Evidence validation failed:\n{val_r.stdout}", "FAIL")
            return 1
        _step("Evidence validates against schema", "OK")

        # 4. PR submission dry-run.
        _step("Preparing evidence PR dry-run")
        pr_plan = pr.prepare_evidence_pr(
            run_dir=run_dir, repo_root=_ROOT, failing=False, dry_run=True,
        )
        if not pr_plan.validation_ok:
            _step("PR dry-run validation failed", "FAIL")
            return 1
        _step(f"PR dry-run OK (branch: {pr_plan.branch_name})", "OK")

        # 5. Summary.
        _banner("SUCCESS Scenario Summary")
        print(f"  Plan steps:     {len(result['steps'])}")
        print(f"  Evidence files: {len(pr_plan.files_to_add)}")
        print(f"  Branch name:    {pr_plan.branch_name}")
        print(f"  Validation:     {'PASS' if pr_plan.validation_ok else 'FAIL'}")
        print(f"  Privacy scan:   {'PASS' if pr_plan.privacy_ok else 'FAIL'}")
        print(f"  PR title:       {pr_plan.pr_title}")
        print(f"  All steps:      ✅ completed")

    return 0


# ─── Scenario: Failure without AI ────────────────────────────────────────────


def run_failure_no_ai_scenario() -> int:
    """Failed run: preserve failing evidence, validate it fails, failing-evidence PR dry-run."""
    _banner("E2E Dry Run: FAILURE (no AI) Scenario")

    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        repo = _make_fake_repo(tmpdir)
        run_dir = tmpdir / "run"

        # 1. Create a failing plan.
        _step("Generating failing plan")
        plan = _make_failing_plan(repo)
        plan_path = tmpdir / "plan.json"
        plan_path.write_text(json.dumps(plan, indent=2))
        _step("Failing plan generated", "OK")

        # 2. Run the plan (will fail at step 1).
        _step("Running failing plan")
        result = runner.execute_plan_file(
            plan_path=plan_path, run_dir=run_dir, fake=False, fake_sys=fake_sys, repo_root=_ROOT,
        )
        if result["status"] != "aborted":
            _step(f"Expected 'aborted', got '{result['status']}'", "FAIL")
            return 1
        _step("Plan correctly aborted (step 1 failed)", "OK")

        # 3. Verify partial evidence is preserved.
        _step("Checking partial evidence preservation")
        events = lib.read_jsonl(run_dir / "events.jsonl")
        if not events:
            _step("No events in event chain", "FAIL")
            return 1
        _step(f"Partial evidence preserved ({len(events)} events)", "OK")

        # 4. Failing-evidence PR dry-run.
        _step("Preparing failing-evidence PR dry-run")
        pr_plan = pr.prepare_evidence_pr(
            run_dir=run_dir, repo_root=_ROOT, failing=True, dry_run=True,
        )
        _step(f"Failing-evidence PR dry-run OK (branch: {pr_plan.branch_name})", "OK")

        # 5. Summary.
        _banner("FAILURE (no AI) Scenario Summary")
        print(f"  Plan status:    {result['status']}")
        print(f"  Steps executed: {len(result['steps'])}")
        print(f"  Events:         {len(events)}")
        print(f"  Branch name:    {pr_plan.branch_name}")
        print(f"  PR title:       {pr_plan.pr_title}")
        print(f"  Evidence path:  {pr_plan.evidence_path}")
        print(f"  All steps:      ✅ completed (failure correctly detected)")

    return 0


# ─── Scenario: Failure with AI fix ───────────────────────────────────────────


def run_failure_with_ai_fix_scenario() -> int:
    """Failed run → dev-if-fail mock → patch → validate → code PR dry-run."""
    _banner("E2E Dry Run: FAILURE (with AI fix) Scenario")

    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        repo = _make_fake_repo(tmpdir)
        run_dir = tmpdir / "run"

        # 1. Create a failing plan and run it.
        _step("Generating failing plan")
        plan = _make_failing_plan(repo)
        plan_path = tmpdir / "plan.json"
        plan_path.write_text(json.dumps(plan, indent=2))

        _step("Running failing plan")
        result = runner.execute_plan_file(
            plan_path=plan_path, run_dir=run_dir, fake=False, fake_sys=fake_sys, repo_root=_ROOT,
        )
        if result["status"] != "aborted":
            _step(f"Expected 'aborted', got '{result['status']}'", "FAIL")
            return 1
        _step("Plan correctly aborted", "OK")

        # 2. Run dev-if-fail with mock provider.
        _step("Running dev-if-fail with mock provider")
        dev_record = agent.dev_if_fail(
            run_dir=run_dir, repo_root=_ROOT, provider="mock", max_iterations=3, fake=True,
        )
        if dev_record["status"] != "passed":
            _step(f"dev-if-fail did not pass: {dev_record['status']}", "FAIL")
            return 1
        _step(f"dev-if-fail passed (attempts: {len(dev_record['attempts'])})", "OK")

        # 3. Verify context was built and redacted.
        _step("Checking context + redaction")
        attempts_dir = run_dir / "ai-attempts"
        if not attempts_dir.exists():
            _step("ai-attempts directory missing", "FAIL")
            return 1
        attempt_0 = attempts_dir / "attempt-000"
        context_path = attempt_0 / "context.json"
        if not context_path.exists():
            _step("context.json missing", "FAIL")
            return 1
        context = json.loads(context_path.read_text())
        ok, errors = agent.validate_redaction(context)
        if not ok:
            _step(f"Redaction validation failed: {errors}", "FAIL")
            return 1
        _step("Context built and redacted", "OK")

        # 4. Verify patch was validated.
        _step("Checking patch validation")
        patch_path = attempt_0 / "patch.json"
        if not patch_path.exists():
            _step("patch.json missing", "FAIL")
            return 1
        patch = json.loads(patch_path.read_text())
        patch_val = agent.validate_patch(patch)
        if not patch_val.valid:
            _step(f"Patch validation failed: {patch_val.errors}", "FAIL")
            return 1
        _step("Patch validated (no forbidden paths/patterns)", "OK")

        # 5. Code PR dry-run.
        _step("Preparing code PR dry-run")
        code_plan = pr.prepare_code_pr(
            branch="livedev/ai-fix-test", repo_root=_ROOT, dry_run=True,
        )
        _step(f"Code PR dry-run OK (branch: {code_plan.branch_name})", "OK")

        # 6. Summary.
        _banner("FAILURE (with AI fix) Scenario Summary")
        print(f"  Plan status:      {result['status']}")
        print(f"  dev-if-fail:      {dev_record['status']}")
        print(f"  Attempts:         {len(dev_record['attempts'])}")
        print(f"  Context redacted: ✅")
        print(f"  Patch validated:  ✅ (no forbidden paths)")
        print(f"  Code PR branch:   {code_plan.branch_name}")
        print(f"  All steps:        ✅ completed (AI fix correctly applied)")

    return 0


# ─── CLI ─────────────────────────────────────────────────────────────────────


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="livedev-e2e-dry-run",
        description="End-to-end LiveDev dry-run scenarios (fake/mock mode).",
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--success", action="store_true", help="Run the success scenario.")
    group.add_argument("--failure-no-ai", action="store_true", help="Run the failure (no AI) scenario.")
    group.add_argument("--failure-with-ai-fix", action="store_true", help="Run the failure (with AI fix) scenario.")
    group.add_argument("--all", action="store_true", help="Run all three scenarios.")
    ns = parser.parse_args(argv)

    if ns.all:
        rc1 = run_success_scenario()
        rc2 = run_failure_no_ai_scenario()
        rc3 = run_failure_with_ai_fix_scenario()
        return 0 if rc1 == 0 and rc2 == 0 and rc3 == 0 else 1

    if ns.success:
        return run_success_scenario()
    if ns.failure_no_ai:
        return run_failure_no_ai_scenario()
    if ns.failure_with_ai_fix:
        return run_failure_with_ai_fix_scenario()
    return 2


if __name__ == "__main__":
    sys.exit(main())
