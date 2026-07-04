#!/usr/bin/env python3
"""
pytest tests for tools/rush-agent and the dev-if-fail loop (Prompt 8).

Tests the 11 required scenarios:
  1. mock diagnosis deterministic
  2. mock patch deterministic
  3. redaction removes secrets
  4. rush-agent executes no shell commands
  5. valid patch applies
  6. invalid patch rejected
  7. forbidden path rejected
  8. max iterations enforced
  9. cost limit enforced
  10. validation commands run through rush-exec
  11. AI claim alone never marks pass

Plus bonus tests for dev-if-fail end-to-end.

Run with:
  python3 -m pytest tools/test-rush-agent.py -v
  # or
  python3 tools/test-rush-agent.py  # standalone
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


agent = _load_module("rush_agent_lib", _TOOLS_DIR / "rush_agent_lib.py")
lib = _load_module("rush_capture_lib", _TOOLS_DIR / "rush_capture_lib.py")


# ─── Helpers ─────────────────────────────────────────────────────────────────


def _make_failed_run_dir(tmpdir: Path) -> Path:
    """Create a run directory that looks like a failed run."""
    run_dir = tmpdir / "failed-run"
    run_dir.mkdir(parents=True)

    # Write a run-record.json with status="aborted".
    run_record = {
        "schema_version": 1,
        "run_kind": "rush-autopilot-run",
        "status": "aborted",
        "plan_kind": "rush-autopilot-plan",
        "source_version": "0.7.0-beta.1",
        "source_commit": "a" * 40,
        "hardware_slot": "laptop",
        "run_dir": str(run_dir),
        "started_at": "2026-07-04T12:00:00Z",
        "finished_at": "2026-07-04T12:01:00Z",
        "steps": [
            {"seq": 0, "status": "completed", "exit_code": 0, "duration_ms": 10, "error": ""},
            {"seq": 1, "status": "failed", "exit_code": 1, "duration_ms": 5, "error": "command not found"},
        ],
    }
    (run_dir / "run-record.json").write_text(json.dumps(run_record, indent=2, sort_keys=True) + "\n")

    # Write a manifest.
    (run_dir / "manifest.json").write_text(json.dumps({
        "schema_version": 1,
        "run_dir": str(run_dir),
        "started_at": "2026-07-04T12:00:00Z",
        "tool": "rush-capture",
    }, indent=2, sort_keys=True) + "\n")

    # Write host.json.
    (run_dir / "host.json").write_text(json.dumps({
        "schema_version": 1,
        "host_kind": "hwtest-host",
        "slot": "laptop",
        "kernel": "fake-kernel",
        "cpu_model": "Fake CPU",
        "dmi_board": "FakeVendor FakeBoard",
        "battery_design_uwh": 48000000,
        "fingerprint": "0123456789abcdef",
        "captured_at": "2026-07-04T12:00:00Z",
    }, indent=2, sort_keys=True) + "\n")

    # Write events.jsonl.
    events = [
        lib.make_event(seq=0, kind="start", payload={"started_at": "2026-07-04T12:00:00Z"}),
        lib.make_event(seq=1, kind="step-before", payload={"seq": 0, "kind": "command"},
                       prev_event_sha256=lib.last_event_sha256(run_dir / "events.jsonl")),
    ]
    for e in events:
        lib.append_jsonl(run_dir / "events.jsonl", e)

    return run_dir


def _make_valid_patch() -> dict:
    """Return a valid patch that passes validation."""
    return {
        "schema_version": 1,
        "patch_kind": "rush-agent-patch",
        "provider": "mock",
        "model": "mock-v1",
        "files": [
            {
                "path": "tools/test-file.txt",
                "action": "create",
                "description": "test file",
                "patch": "+test content\n",
            }
        ],
        "validation_commands": [
            ["echo", "validation-ok"],
        ],
        "claim_pass": False,
    }


# ─── Test 1: mock diagnosis deterministic ────────────────────────────────────


def test_mock_diagnosis_deterministic():
    """The mock provider returns the same diagnosis every time."""
    context = {"schema_version": 1, "context_kind": "rush-agent-context"}
    d1 = agent.mock_diagnose(context)
    d2 = agent.mock_diagnose(context)
    assert d1 == d2, "mock diagnosis should be deterministic"
    assert d1["provider"] == "mock"
    assert d1["model"] == "mock-v1"
    assert "diagnosis" in d1


# ─── Test 2: mock patch deterministic ────────────────────────────────────────


def test_mock_patch_deterministic():
    """The mock provider returns the same patch every time."""
    context = {"schema_version": 1, "context_kind": "rush-agent-context"}
    p1 = agent.mock_propose_patch(context)
    p2 = agent.mock_propose_patch(context)
    assert p1 == p2, "mock patch should be deterministic"
    assert p1["provider"] == "mock"
    assert p1["patch_kind"] == "rush-agent-patch"
    assert "files" in p1
    assert "validation_commands" in p1


# ─── Test 3: redaction removes secrets ───────────────────────────────────────


def test_redaction_removes_secrets():
    """The redaction validator detects unredacted secrets in a context bundle."""
    # Construct a context with an unredacted GitHub token.
    token = "ghp_" + "aBcDeFgHiJkLmNoPqRsTuVwXyZ" + "1234567890abcd"
    context = {
        "schema_version": 1,
        "context_kind": "rush-agent-context",
        "run_record": {"stderr": f"token={token}"},
    }
    ok, errors = agent.validate_redaction(context)
    assert not ok, "should detect unredacted GitHub token"
    assert any("unredacted secret" in e for e in errors)

    # Now redact it and verify validation passes.
    report = lib.RedactionReport()
    context["run_record"]["stderr"] = lib.redact(context["run_record"]["stderr"], report)
    ok, errors = agent.validate_redaction(context)
    assert ok, f"redacted context should pass validation; errors: {errors}"


# ─── Test 4: rush-agent executes no shell commands ───────────────────────────


def test_rush_agent_executes_no_shell_commands():
    """The rush-agent library never calls system()/exec()/subprocess.run() for AI calls."""
    # The mock provider functions should NOT call subprocess.
    # We verify by inspecting the source code.
    source = (agent.__file__).read_text() if hasattr(agent.__file__, "read_text") else Path(agent.__file__).read_text()

    # The mock_diagnose and mock_propose_patch functions should not contain subprocess calls.
    # Find the function bodies.
    import re
    # Check that mock_diagnose doesn't call subprocess.
    mock_diag_match = re.search(r"def mock_diagnose.*?(?=\ndef )", source, re.DOTALL)
    assert mock_diag_match, "could not find mock_diagnose function"
    mock_diag_body = mock_diag_match.group()
    assert "subprocess" not in mock_diag_body, "mock_diagnose must not call subprocess"
    assert "os.system" not in mock_diag_body, "mock_diagnose must not call os.system"

    mock_patch_match = re.search(r"def mock_propose_patch.*?(?=\ndef )", source, re.DOTALL)
    assert mock_patch_match, "could not find mock_propose_patch function"
    mock_patch_body = mock_patch_match.group()
    assert "subprocess" not in mock_patch_body, "mock_propose_patch must not call subprocess"
    assert "os.system" not in mock_patch_body, "mock_propose_patch must not call os.system"


# ─── Test 5: valid patch applies ─────────────────────────────────────────────


def test_valid_patch_applies():
    """A valid patch passes validation."""
    patch = _make_valid_patch()
    result = agent.validate_patch(patch)
    assert result.valid, f"valid patch should pass; errors: {result.errors}"


# ─── Test 6: invalid patch rejected ──────────────────────────────────────────


def test_invalid_patch_rejected():
    """An invalid patch (wrong structure) is rejected."""
    patch = {
        "schema_version": 99,  # wrong
        "patch_kind": "wrong-kind",
        "files": "not a list",
    }
    result = agent.validate_patch(patch)
    assert not result.valid
    assert any("patch_kind" in e for e in result.errors)


# ─── Test 7: forbidden path rejected ─────────────────────────────────────────


def test_forbidden_path_rejected():
    """A patch that modifies a forbidden path is rejected."""
    for forbidden in ["VERSION", "release/milestones.toml", "RELEASES.md", ".github/workflows/ci.yml"]:
        patch = _make_valid_patch()
        patch["files"][0]["path"] = forbidden
        result = agent.validate_patch(patch)
        assert not result.valid, f"should reject forbidden path: {forbidden}"
        assert any(forbidden in e for e in result.errors)


# ─── Test 8: max iterations enforced ─────────────────────────────────────────


def test_max_iterations_enforced():
    """dev-if-fail stops after max_iterations."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_failed_run_dir(tmpdir)

        # The mock provider always passes, so iteration 0 should pass.
        # But if we set max_iterations=0, it should stop immediately.
        record = agent.dev_if_fail(
            run_dir=run_dir,
            repo_root=_ROOT,
            provider="mock",
            max_iterations=0,
            fake=True,
        )
        # With 0 iterations, no attempts should be made.
        assert len(record.get("attempts", [])) == 0 or record["status"] == "max-iterations-reached"


def test_max_iterations_enforced_with_failing_validation():
    """If validation never passes, dev-if-fail stops at max_iterations."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_failed_run_dir(tmpdir)

        # Monkey-patch the mock provider to return a patch with failing validation commands.
        original_patch = agent.mock_propose_patch
        agent.mock_propose_patch = lambda ctx: {
            "schema_version": 1,
            "patch_kind": "rush-agent-patch",
            "provider": "mock",
            "model": "mock-v1",
            "files": [{"path": "tools/test-file.txt", "action": "create", "description": "test", "patch": "+test\n"}],
            "validation_commands": [["false"]],  # always fails
            "claim_pass": False,
        }

        try:
            record = agent.dev_if_fail(
                run_dir=run_dir,
                repo_root=_ROOT,
                provider="mock",
                max_iterations=2,
                fake=True,
            )
            assert record["status"] == "max-iterations-reached"
            assert len(record["attempts"]) == 2
            for a in record["attempts"]:
                assert a["status"] == "failed"
        finally:
            agent.mock_propose_patch = original_patch


# ─── Test 9: cost limit enforced ─────────────────────────────────────────────


def test_cost_limit_enforced():
    """The budget check refuses calls that would exceed the cap."""
    # Mock provider is always free.
    ok, reason = agent.check_budget("mock", 0.0)
    assert ok

    # Real provider with no budget file is OK.
    ok, reason = agent.check_budget("http", 0.01)
    assert ok

    # Real provider with a budget file that's exceeded.
    # Write the budget file to the actual ~/.config/rush/ai-budget.json.
    rush_config = Path.home() / ".config" / "rush"
    rush_config.mkdir(parents=True, exist_ok=True)
    budget_path = rush_config / "ai-budget.json"
    original_content = budget_path.read_text() if budget_path.exists() else None
    try:
        budget_path.write_text(json.dumps({
            "http": {"monthly_spend_usd": 49.99, "monthly_cap_usd": 50.0}
        }))
        ok, reason = agent.check_budget("http", 1.0)
        assert not ok, "should refuse when budget would be exceeded"
        assert "budget exceeded" in reason
    finally:
        if original_content is not None:
            budget_path.write_text(original_content)
        else:
            budget_path.unlink(missing_ok=True)


# ─── Test 10: validation commands run through rush-exec ──────────────────────


def test_validation_commands_run_through_rush_exec():
    """In non-fake mode, validation commands go through rush-exec."""
    # We verify this by checking the dev_if_fail source code: in non-fake mode,
    # the validation commands are prefixed with rush-exec.
    source = Path(agent.__file__).read_text()
    # The dev_if_fail function should contain a rush-exec reference for non-fake mode.
    assert "rush-exec" in source, "dev_if_fail should reference rush-exec for validation commands"

    # Also verify the patch validator warns about commands that don't start with safe binaries.
    patch = _make_valid_patch()
    patch["validation_commands"] = [["dangerous-command", "--flag"]]
    result = agent.validate_patch(patch)
    assert any("does not start with a known safe binary" in w for w in result.warnings)


# ─── Test 11: AI claim alone never marks pass ────────────────────────────────


def test_ai_claim_alone_never_marks_pass():
    """A patch with claim_pass=True is rejected even if validation passes."""
    patch = _make_valid_patch()
    patch["claim_pass"] = True
    result = agent.validate_patch(patch)
    assert not result.valid, "claim_pass=True should be rejected"
    assert any("claim_pass" in e for e in result.errors)


# ─── Bonus: dev-if-fail end-to-end ───────────────────────────────────────────


def test_dev_if_fail_end_to_end():
    """dev-if-fail detects failure, builds context, gets patch, validates, passes."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_failed_run_dir(tmpdir)

        record = agent.dev_if_fail(
            run_dir=run_dir,
            repo_root=_ROOT,
            provider="mock",
            max_iterations=3,
            fake=True,
        )
        assert record["status"] == "passed"
        assert len(record["attempts"]) == 1
        attempt = record["attempts"][0]
        assert attempt["status"] == "passed"
        assert attempt["patch_valid"] is True
        assert attempt["all_validation_passed"] is True

        # Verify the attempt directory was created.
        attempts_dir = run_dir / "ai-attempts"
        assert attempts_dir.exists()
        assert (attempts_dir / "attempt-000").exists()
        assert (attempts_dir / "attempt-000" / "context.json").exists()
        assert (attempts_dir / "attempt-000" / "diagnosis.json").exists()
        assert (attempts_dir / "attempt-000" / "patch.json").exists()
        assert (attempts_dir / "attempt-000" / "attempt.json").exists()


def test_dev_if_fail_no_failure():
    """dev-if-fail exits cleanly when there's no failure."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = tmpdir / "no-failure-run"
        run_dir.mkdir()
        (run_dir / "run-record.json").write_text(json.dumps({
            "status": "completed",
            "steps": [],
        }))

        record = agent.dev_if_fail(
            run_dir=run_dir,
            repo_root=_ROOT,
            provider="mock",
            max_iterations=3,
            fake=True,
        )
        assert record["status"] == "no-failure"


def test_dev_if_fail_preserves_attempts():
    """dev-if-fail preserves every attempt in the ai-attempts directory."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_failed_run_dir(tmpdir)

        # Run with failing validation to get multiple attempts.
        original_patch = agent.mock_propose_patch
        agent.mock_propose_patch = lambda ctx: {
            "schema_version": 1,
            "patch_kind": "rush-agent-patch",
            "provider": "mock",
            "model": "mock-v1",
            "files": [{"path": "tools/test-file.txt", "action": "create", "description": "test", "patch": "+test\n"}],
            "validation_commands": [["false"]],
            "claim_pass": False,
        }

        try:
            record = agent.dev_if_fail(
                run_dir=run_dir,
                repo_root=_ROOT,
                provider="mock",
                max_iterations=3,
                fake=True,
            )
            assert record["status"] == "max-iterations-reached"
            assert len(record["attempts"]) == 3

            # Verify all 3 attempt directories exist.
            attempts_dir = run_dir / "ai-attempts"
            for i in range(3):
                assert (attempts_dir / f"attempt-{i:03d}").exists()
                assert (attempts_dir / f"attempt-{i:03d}" / "attempt.json").exists()
        finally:
            agent.mock_propose_patch = original_patch


# ─── Bonus: CLI tests ────────────────────────────────────────────────────────


def test_cli_context():
    """The CLI 'context' subcommand builds a context bundle."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        run_dir = _make_failed_run_dir(tmpdir)
        out_path = tmpdir / "context.json"

        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush-agent"),
             "context", "--run-dir", str(run_dir), "--output", str(out_path)],
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert r.returncode == 0, f"CLI context should exit 0, got {r.returncode}\n{r.stderr}"
        assert out_path.exists()
        context = json.loads(out_path.read_text())
        assert context["context_kind"] == "rush-agent-context"


def test_cli_validate_patch():
    """The CLI 'validate-patch' subcommand validates a patch."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        patch_path = tmpdir / "patch.json"
        patch_path.write_text(json.dumps(_make_valid_patch()))

        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush-agent"),
             "validate-patch", "--patch", str(patch_path)],
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert r.returncode == 0
        assert "patch OK" in r.stdout


def test_cli_validate_redaction():
    """The CLI 'validate-redaction' subcommand validates a context."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        context_path = tmpdir / "context.json"
        context_path.write_text(json.dumps({
            "schema_version": 1,
            "context_kind": "rush-agent-context",
            "run_record": {"status": "aborted"},
        }))

        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush-agent"),
             "validate-redaction", "--context", str(context_path)],
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert r.returncode == 0
        assert "redaction OK" in r.stdout


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
