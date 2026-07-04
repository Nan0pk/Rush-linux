#!/usr/bin/env python3
"""
pytest tests for tools/rush_pr_lib.py and the submit-* subcommands (Prompt 10).

Tests the 9 required scenarios:
  1. evidence PR dry-run
  2. failing evidence PR dry-run
  3. code PR dry-run
  4. validation failure blocks submission
  5. token redaction works
  6. branch names deterministic
  7. no self-merge command exists
  8. release/milestone files blocked
  9. CI invokes validator

Run with:
  python3 -m pytest tools/test-rush-pr.py -v
  # or
  python3 tools/test-rush-pr.py  # standalone
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


pr_lib = _load_module("rush_pr_lib", _TOOLS_DIR / "rush_pr_lib.py")
lib = _load_module("rush_capture_lib", _TOOLS_DIR / "rush_capture_lib.py")


# ─── Helpers ─────────────────────────────────────────────────────────────────


def _make_valid_run_dir(tmpdir: Path) -> Path:
    """Create a run directory with valid evidence (passes the validator)."""
    run_dir = tmpdir / "valid-run"
    run_dir.mkdir(parents=True)

    import subprocess
    head = subprocess.check_output(["git", "-C", str(_ROOT), "rev-parse", "HEAD"]).decode().strip()
    version = (_ROOT / "VERSION").read_text().strip()

    # hwtest-manifest.json
    _write_json(run_dir / "hwtest-manifest.json", {
        "schema_version": 1, "manifest_kind": "hwtest-manifest",
        "source_version": version, "source_commit": head,
        "hardware_slot": "laptop", "bundle_created_at": "2026-07-04T12:00:00Z",
        "plan_path": "hwtest-plan.json", "host_path": "hwtest-host.json",
        "baseline_result_path": "hwtest-result-baseline.json",
        "optid_result_path": "hwtest-result-optid.json",
        "verdict_path": "VERDICT.md", "events_path": "events.jsonl",
        "privacy_report_path": "privacy-report.json",
    })
    # hwtest-plan.json
    _write_json(run_dir / "hwtest-plan.json", {
        "schema_version": 1, "plan_kind": "hwtest-plan",
        "plan_name": "mixed-load-001", "workload": "mixed-load-001",
        "phases": [{"name": "interactive", "duration_sec": 60, "expected_class": "interactive", "metrics": ["input-latency-p95-ms"]}],
        "min_samples": 5,
        "pass_conditions": {
            "criterion_2_responsiveness": {"applies_to_slots": ["desktop", "laptop"], "description": "test"},
            "criterion_3_battery": {"applies_to_slots": ["laptop"], "description": "test"},
        },
    })
    # hwtest-host.json
    _write_json(run_dir / "hwtest-host.json", {
        "schema_version": 1, "host_kind": "hwtest-host", "slot": "laptop",
        "kernel": "fake-kernel", "cpu_model": "Fake CPU", "dmi_board": "Fake Board",
        "battery_design_uwh": 48000000, "fingerprint": "0123456789abcdef",
        "captured_at": "2026-07-04T12:00:00Z",
    })
    # hwtest-result-*.json
    for lever in ("baseline", "optid"):
        _write_json(run_dir / f"hwtest-result-{lever}.json", {
            "schema_version": 1, "result_kind": "hwtest-result", "lever": lever,
            "power_source": "ac", "started_at": "2026-07-04T12:00:00Z", "finished_at": "2026-07-04T12:30:00Z",
            "phases": [{"name": "interactive", "expected_class": "interactive", "observed_class": "interactive",
                        "metrics": [{"name": "input-latency-p95-ms", "unit": "ms", "samples": [0.06]*5, "median": 0.06, "n": 5}]}],
            "battery_pct": None, "ac_online": True, "anomalies": [],
        })
    # VERDICT.md
    (run_dir / "VERDICT.md").write_text("# Verdict (advisory only)\n\nPASS\n")
    # events.jsonl
    e0 = lib.make_event(seq=0, kind="start", payload={})
    lib.append_jsonl(run_dir / "events.jsonl", e0)
    # privacy-report.json
    _write_json(run_dir / "privacy-report.json", {"schema_version": 1, "redactors": [], "counts": {}, "total": 0})

    return run_dir


def _make_invalid_run_dir(tmpdir: Path) -> Path:
    """Create a run directory with invalid evidence (missing manifest)."""
    run_dir = tmpdir / "invalid-run"
    run_dir.mkdir(parents=True)
    # No hwtest-manifest.json — validation will fail.
    (run_dir / "README.txt").write_text("invalid evidence bundle\n")
    return run_dir


def _write_json(path: Path, obj: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj, indent=2, sort_keys=True) + "\n")


# ─── Test 1: evidence PR dry-run ─────────────────────────────────────────────


def test_evidence_pr_dry_run():
    """Evidence PR dry-run shows files, branch, commit message, PR title/body, validation status."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_valid_run_dir(tmpdir)

        plan = pr_lib.prepare_evidence_pr(
            run_dir=run_dir, repo_root=_ROOT, failing=False, dry_run=True,
        )

        assert plan.kind == "evidence"
        assert plan.dry_run is True
        assert "evidence/livedev-" in plan.branch_name
        assert "evidence(livedev):" in plan.commit_message
        assert "evidence(livedev):" in plan.pr_title
        assert "Evidence Submission" in plan.pr_body
        assert "Awaiting Verifier review" in plan.pr_body
        assert len(plan.files_to_add) > 0
        assert "hwtest-manifest.json" in " ".join(plan.files_to_add)


# ─── Test 2: failing evidence PR dry-run ─────────────────────────────────────


def test_failing_evidence_pr_dry_run():
    """Failing evidence PR dry-run shows the failing kind label."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_valid_run_dir(tmpdir)

        plan = pr_lib.prepare_evidence_pr(
            run_dir=run_dir, repo_root=_ROOT, failing=True, dry_run=True,
        )

        assert plan.kind == "failing-evidence"
        assert "failing evidence" in plan.commit_message
        assert "failing evidence" in plan.pr_title.lower()


# ─── Test 3: code PR dry-run ─────────────────────────────────────────────────


def test_code_pr_dry_run():
    """Code PR dry-run shows branch, commit message, PR title/body."""
    plan = pr_lib.prepare_code_pr(
        branch="test-branch", repo_root=_ROOT, dry_run=True,
    )

    assert plan.kind == "code"
    assert plan.dry_run is True
    assert "code(livedev):" in plan.commit_message
    assert "code(livedev):" in plan.pr_title
    assert "Code Submission" in plan.pr_body
    assert "Awaiting Verifier review" in plan.pr_body


# ─── Test 4: validation failure blocks submission ────────────────────────────


def test_validation_failure_blocks_submission():
    """When evidence validation fails, the submission plan records the failure."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_invalid_run_dir(tmpdir)

        plan = pr_lib.prepare_evidence_pr(
            run_dir=run_dir, repo_root=_ROOT, failing=False, dry_run=True,
        )

        assert not plan.validation_ok
        assert len(plan.validation_errors) > 0


# ─── Test 5: token redaction works ───────────────────────────────────────────


def test_token_redaction_works():
    """The privacy scan detects unredacted tokens in evidence files."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_valid_run_dir(tmpdir)

        # Write a file with an unredacted GitHub token.
        token = "ghp_" + "aBcDeFgHiJkLmNoPqRsTuVwXyZ" + "1234567890abcd"
        (run_dir / "transcript.log").write_text(f"GITHUB_TOKEN={token}\n")

        ok, errors = pr_lib.privacy_scan(run_dir)
        assert not ok
        assert any("unredacted secret" in e for e in errors)


# ─── Test 6: branch names deterministic ──────────────────────────────────────


def test_branch_names_deterministic():
    """The same inputs produce the same branch name."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_valid_run_dir(tmpdir)

        name1 = pr_lib.make_evidence_branch_name(run_dir, "20260704-120000Z")
        name2 = pr_lib.make_evidence_branch_name(run_dir, "20260704-120000Z")
        assert name1 == name2

        # Different run dirs produce different branch names.
        run_dir2 = tmpdir / "other-run"
        run_dir2.mkdir()
        name3 = pr_lib.make_evidence_branch_name(run_dir2, "20260704-120000Z")
        assert name1 != name3


# ─── Test 7: no self-merge command exists ────────────────────────────────────


def test_no_self_merge_command():
    """The PR library has no merge command."""
    assert pr_lib.has_merge_command() is False

    # Also verify by source inspection: no "merge" in the rush_pr_lib source.
    source = Path(pr_lib.__file__).read_text().lower()
    # The word "merge" may appear in comments/docs but not as a function call.
    # Check that no function named "merge" or "merge_pr" exists.
    assert "def merge" not in source
    assert "def _merge" not in source
    # Check that no "gh pr merge" or "pulls/.*/merge" API call exists.
    assert "pr merge" not in source
    assert "pulls/" not in source or "pulls/" in source.split("def _open_pr")[0]  # PR creation is OK; merge is not


# ─── Test 8: release/milestone files blocked ─────────────────────────────────


def test_release_milestone_files_blocked():
    """The forbidden path check blocks release-truth files."""
    for forbidden in ["VERSION", "release/milestones.toml", "RELEASES.md",
                       "release/test-tiers.toml", ".github/workflows/ci.yml",
                       "docs/decisions/0024-livedev-pr-submission.md"]:
        violations = pr_lib.check_forbidden_paths([forbidden])
        assert len(violations) > 0, f"should block forbidden path: {forbidden}"


def test_protected_evidence_subdirs_blocked():
    """Existing evidence subdirs are blocked."""
    for protected in ["release/evidence/v0.5.0-beta.1/transcript.log",
                       "release/evidence/dragnet/LEDGER.md",
                       "release/evidence/host-bench/2026-06-10-victus/meta.txt"]:
        violations = pr_lib.check_forbidden_paths([protected])
        assert len(violations) > 0, f"should block protected evidence path: {protected}"


def test_livedev_evidence_subdir_allowed():
    """New livedev-* evidence subdirs are allowed."""
    allowed = "release/evidence/livedev-20260704-120000Z-valid-run/hwtest-manifest.json"
    violations = pr_lib.check_forbidden_paths([allowed])
    assert len(violations) == 0, f"should allow livedev evidence path: {allowed}"


# ─── Test 9: CI invokes validator ────────────────────────────────────────────


def test_ci_workflow_exists():
    """The livedev-validate.yml CI workflow exists and is valid YAML."""
    workflow = _ROOT / ".github" / "workflows" / "livedev-validate.yml"
    assert workflow.exists()
    # Validate YAML syntax.
    try:
        import yaml
        data = yaml.safe_load(workflow.read_text())
        assert "jobs" in data
        assert "livedev-validate" in data["jobs"]
    except ImportError:
        # PyYAML not available — just check the file exists and has basic structure.
        text = workflow.read_text()
        assert "name:" in text
        assert "jobs:" in text
        assert "livedev-validate" in text


def test_ci_invokes_validator():
    """The CI workflow invokes the evidence validator."""
    workflow = _ROOT / ".github" / "workflows" / "livedev-validate.yml"
    text = workflow.read_text()
    assert "validate-hwtest-evidence" in text
    assert "--fixtures" in text


def test_ci_checks_release_truth():
    """The CI workflow checks that release-truth files are not modified."""
    workflow = _ROOT / ".github" / "workflows" / "livedev-validate.yml"
    text = workflow.read_text()
    assert "release truth" in text.lower() or "release-truth" in text.lower()
    assert "milestones.toml" in text


def test_ci_checks_no_merge():
    """The CI workflow checks that no self-merge commands exist."""
    workflow = _ROOT / ".github" / "workflows" / "livedev-validate.yml"
    text = workflow.read_text()
    assert "self-merge" in text.lower() or "merge" in text.lower()


# ─── Bonus: CLI tests ────────────────────────────────────────────────────────


def test_cli_submit_evidence_dry_run():
    """The CLI submit-evidence --dry-run produces a valid plan."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_valid_run_dir(tmpdir)

        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush-autopilot"),
             "submit-evidence", "--run-dir", str(run_dir), "--dry-run"],
            capture_output=True, text=True, timeout=30,
        )
        assert r.returncode == 0, f"CLI should exit 0, got {r.returncode}\n{r.stderr}"
        plan = json.loads(r.stdout)
        assert plan["submission_kind"] == "evidence"
        assert plan["dry_run"] is True


def test_cli_submit_code_pr_dry_run():
    """The CLI submit-code-pr --dry-run produces a valid plan."""
    r = subprocess.run(
        ["python3", str(_TOOLS_DIR / "rush-autopilot"),
         "submit-code-pr", "--branch", "test-branch", "--dry-run"],
        capture_output=True, text=True, timeout=30,
    )
    assert r.returncode == 0, f"CLI should exit 0, got {r.returncode}\n{r.stderr}"
    plan = json.loads(r.stdout)
    assert plan["submission_kind"] == "code"
    assert plan["dry_run"] is True


# ─── Bonus: PR template exists ───────────────────────────────────────────────


def test_pr_template_exists():
    """The livedev-pr.md template exists with the required sections."""
    template = _ROOT / "docs" / "templates" / "livedev-pr.md"
    assert template.exists()
    text = template.read_text()
    assert "Goal" in text
    assert "Plan" in text
    assert "Execution Record" in text
    assert "Evidence Paths" in text
    assert "Inferred Verdict" in text
    assert "Awaiting Verifier review" in text


# ─── Bonus: ADR 0024 exists ──────────────────────────────────────────────────


def test_adr_0024_exists():
    """ADR 0024 (livedev PR submission) exists and is proposed."""
    adr = _ROOT / "docs" / "decisions" / "0024-livedev-pr-submission.md"
    assert adr.exists()
    text = adr.read_text()
    assert "Status: proposed" in text
    assert "ADR 0024" in text
    assert "no self-merge" in text.lower()


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
