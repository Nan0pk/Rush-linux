#!/usr/bin/env python3
"""
pytest tests for LiveDev post-merge hardening (STEP 4 A-D).

Tests:
  A. Real-run evidence bundle ordering — validation fails if bundle files missing.
  B. Generated argv does not nest rush-exec.
  C. PR staging does not use git add -A (unrelated files not staged).
  D. Manifest path traversal rejected (absolute, .., escape).

Run with:
  python3 -m pytest tools/test-livedev-hardening.py -v
  python3 tools/test-livedev-hardening.py  # standalone
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
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    loader.exec_module(mod)
    return mod


# ─── STEP 4A: Evidence bundle ordering ───────────────────────────────────────


def test_validation_fails_when_bundle_files_missing():
    """In non-fake mode, validation must fail if hwtest bundle files don't exist."""
    runner = _load_module("rush_runner_lib", _TOOLS_DIR / "rush_runner_lib.py")

    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir(parents=True)
        # Create an empty run_dir — no hwtest-*.json files.

        step = {
            "seq": 0,
            "kind": "validation",
            "default": "proceed",
            "reason": "validate",
            "rollback": "N/A",
            "validator": "validate-hwtest-evidence.py",
            "bundle": str(run_dir),
        }

        result = runner.execute_step(step, run_dir, fake=False, repo_root=_ROOT)
        assert result.status == "failed"
        assert "missing bundle files" in result.error


def test_validation_passes_when_bundle_files_exist():
    """In non-fake mode, validation proceeds when bundle files exist."""
    runner = _load_module("rush_runner_lib", _TOOLS_DIR / "rush_runner_lib.py")

    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir(parents=True)

        # Create minimal required files.
        for f in ["hwtest-manifest.json", "hwtest-plan.json", "hwtest-host.json",
                   "hwtest-result-baseline.json", "hwtest-result-optid.json",
                   "VERDICT.md", "events.jsonl", "privacy-report.json"]:
            (run_dir / f).write_text("{}\n")

        step = {
            "seq": 0,
            "kind": "validation",
            "default": "proceed",
            "reason": "validate",
            "rollback": "N/A",
            "validator": "validate-hwtest-evidence.py",
            "bundle": str(run_dir),
        }

        result = runner.execute_step(step, run_dir, fake=False, repo_root=_ROOT)
        # It should NOT fail with "missing bundle files" — it may fail later
        # from the validator itself (empty JSON), but NOT from the bundle check.
        assert result.status != "failed" or "missing bundle files" not in result.error, \
            f"should not fail on missing files: {result.error}"


# ─── STEP 4B: Generated argv does not nest rush-exec ─────────────────────────


def test_generated_argv_does_not_nest_rush_exec():
    """Generated plan argv must not start with rush-exec (the runner wraps it)."""
    # Read the rush-autopilot source and check that no generated argv starts with rush-exec.
    source = (_TOOLS_DIR / "rush-autopilot").read_text()

    # Find all argv= blocks and check none start with "rush-exec".
    import re
    # Look for argv lists that start with "rush-exec"
    # Pattern: argv=[\n\s]*"rush-exec"
    matches = re.findall(r'argv=\[\s*\n?\s*"rush-exec"', source)
    assert len(matches) == 0, f"Generated argv must not start with rush-exec; found {len(matches)} occurrences"


def test_generated_argv_uses_payload_command_only():
    """Generated argv should be the payload command, not wrapped in rush-exec."""
    ap = _load_module("rush_autopilot", _TOOLS_DIR / "rush-autopilot")

    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        # Create a minimal fake repo.
        repo = tmpdir / "repo"
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

        # Create fake laptop sysfs.
        fake_sys = tmpdir / "fake-sys"
        bat = fake_sys / "sys" / "class" / "power_supply" / "BAT0"
        bat.mkdir(parents=True)
        (bat / "status").write_text("Discharging\n")
        (bat / "capacity").write_text("75\n")
        (bat / "energy_full_design").write_text("48000000\n")
        ac = fake_sys / "sys" / "class" / "power_supply" / "AC" / "online"
        ac.parent.mkdir(parents=True)
        ac.write_text("0\n")

        hw = ap.detect_hardware(fake_sys)
        plan = ap.generate_plan(repo, hw)

        # Check every command step's argv.
        for step in plan.steps:
            if step.kind == "command" and step.argv:
                assert step.argv[0] != "rush-exec", \
                    f"step {step.seq} argv starts with rush-exec: {step.argv}"
                # Also check no argv contains "rush-exec" as the first element
                # after a "--" separator (which would indicate nesting).
                for i, arg in enumerate(step.argv):
                    if arg == "--" and i + 1 < len(step.argv):
                        assert step.argv[i + 1] != "rush-exec", \
                            f"step {step.seq} argv nests rush-exec after --"


# ─── STEP 4C: PR staging does not use git add -A ─────────────────────────────


def test_pr_staging_does_not_use_git_add_A():
    """The PR submission code must not use 'git add -A'."""
    source = (_TOOLS_DIR / "rush_pr_lib.py").read_text()
    # Check that 'add', '-A' or 'add', '--all' is not used.
    assert '"add", "-A"' not in source, "rush_pr_lib.py uses 'git add -A'"
    assert '"add", "--all"' not in source, "rush_pr_lib.py uses 'git add --all'"
    # Verify it uses explicit paths instead.
    assert '"add", plan.evidence_path' in source or '"add", f' in source, \
        "rush_pr_lib.py should stage explicit paths"


def test_unrelated_dirty_file_not_staged():
    """An unrelated dirty file must not be staged by the PR submission."""
    # Verify by source file inspection that execute_submission stages only
    # plan.evidence_path or plan.files_to_add — not -A.
    source = (_TOOLS_DIR / "rush_pr_lib.py").read_text()

    # Find the execute_submission function body.
    idx = source.find("def execute_submission")
    assert idx != -1, "execute_submission not found in rush_pr_lib.py"
    # Get from def to the next def or end of file.
    next_def = source.find("\ndef ", idx + 1)
    if next_def == -1:
        func_body = source[idx:]
    else:
        func_body = source[idx:next_def]

    assert '"add", "-A"' not in func_body, "execute_submission should not use 'add -A'"
    assert '"add", "--all"' not in func_body, "execute_submission should not use 'add --all'"
    assert "plan.evidence_path" in func_body or "plan.files_to_add" in func_body, \
        "execute_submission should stage explicit paths"


# ─── STEP 4D: Manifest path traversal ────────────────────────────────────────


def test_absolute_path_rejected():
    """Manifest path fields with absolute paths are rejected."""
    validator = _load_module("validate_hwtest_evidence", _TOOLS_DIR / "validate-hwtest-evidence.py")

    with tempfile.TemporaryDirectory() as tmp:
        bundle = Path(tmp) / "bundle"
        bundle.mkdir()

        manifest = {
            "schema_version": 1,
            "manifest_kind": "hwtest-manifest",
            "source_version": "0.7.0-beta.1",
            "source_commit": "a" * 40,
            "hardware_slot": "laptop",
            "bundle_created_at": "2026-07-04T12:00:00Z",
            "plan_path": "/etc/passwd",  # absolute path
            "host_path": "hwtest-host.json",
            "baseline_result_path": "hwtest-result-baseline.json",
            "optid_result_path": "hwtest-result-optid.json",
            "verdict_path": "VERDICT.md",
            "events_path": "events.jsonl",
            "privacy_report_path": "privacy-report.json",
        }
        (bundle / "hwtest-manifest.json").write_text(json.dumps(manifest))

        ok, errors, _ = validator.validate_bundle(bundle, _ROOT)
        assert not ok
        assert any("absolute path rejected" in e for e in errors)


def test_dotdot_path_rejected():
    """Manifest path fields with '..' are rejected."""
    validator = _load_module("validate_hwtest_evidence", _TOOLS_DIR / "validate-hwtest-evidence.py")

    with tempfile.TemporaryDirectory() as tmp:
        bundle = Path(tmp) / "bundle"
        bundle.mkdir()

        manifest = {
            "schema_version": 1,
            "manifest_kind": "hwtest-manifest",
            "source_version": "0.7.0-beta.1",
            "source_commit": "a" * 40,
            "hardware_slot": "laptop",
            "bundle_created_at": "2026-07-04T12:00:00Z",
            "plan_path": "../../../etc/passwd",  # traversal
            "host_path": "hwtest-host.json",
            "baseline_result_path": "hwtest-result-baseline.json",
            "optid_result_path": "hwtest-result-optid.json",
            "verdict_path": "VERDICT.md",
            "events_path": "events.jsonl",
            "privacy_report_path": "privacy-report.json",
        }
        (bundle / "hwtest-manifest.json").write_text(json.dumps(manifest))

        ok, errors, _ = validator.validate_bundle(bundle, _ROOT)
        assert not ok
        assert any("path traversal rejected" in e for e in errors)


def test_normal_relative_path_accepted():
    """Normal relative paths in manifest are accepted (no path-traversal error)."""
    validator = _load_module("validate_hwtest_evidence", _TOOLS_DIR / "validate-hwtest-evidence.py")

    with tempfile.TemporaryDirectory() as tmp:
        bundle = Path(tmp) / "bundle"
        bundle.mkdir()

        manifest = {
            "schema_version": 1,
            "manifest_kind": "hwtest-manifest",
            "source_version": "0.7.0-beta.1",
            "source_commit": "a" * 40,
            "hardware_slot": "laptop",
            "bundle_created_at": "2026-07-04T12:00:00Z",
            "plan_path": "hwtest-plan.json",
            "host_path": "hwtest-host.json",
            "baseline_result_path": "hwtest-result-baseline.json",
            "optid_result_path": "hwtest-result-optid.json",
            "verdict_path": "VERDICT.md",
            "events_path": "events.jsonl",
            "privacy_report_path": "privacy-report.json",
        }
        (bundle / "hwtest-manifest.json").write_text(json.dumps(manifest))
        # Create the referenced files so the existence check passes.
        for f in ["hwtest-plan.json", "hwtest-host.json", "hwtest-result-baseline.json",
                   "hwtest-result-optid.json", "VERDICT.md", "events.jsonl", "privacy-report.json"]:
            (bundle / f).write_text("{}\n")

        ok, errors, _ = validator.validate_bundle(bundle, _ROOT)
        # Should NOT have path traversal errors (may have other errors from empty JSON).
        assert not any("absolute path" in e for e in errors), "normal relative path should not be rejected as absolute"
        assert not any("path traversal" in e for e in errors), "normal relative path should not be rejected as traversal"
        assert not any("escapes bundle_dir" in e for e in errors), "normal relative path should not escape"


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
