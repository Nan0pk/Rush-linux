#!/usr/bin/env python3
"""
rush_agent_lib — AI-assisted development library for Rush LiveDev.

Provides:
  - Context bundle builder: collects failing run summary, manifest, plan,
    validator output, logs, command-log excerpts, source files, git diff,
    hardware metadata, and previous attempts — all redacted.
  - Redaction validator: verifies no secrets leak in a context bundle.
  - Mock provider: deterministic diagnosis + patch for tests (no network).
  - Patch validator: rejects forbidden files, oversized patches, invalid
    JSON, and patches that would weaken validators or delete tests.

This library is imported by tools/rush-agent (the CLI) and by
tools/test-rush-agent.py (the tests). It NEVER calls system()/exec()/
subprocess.run() for AI model calls — the mock provider returns canned
text, and real providers (when ratified) are called via HTTP only.

Design constraints (per docs/ai-interface-policy.md):
  - AI interface is CLI/harness-based, not browser-chat-based.
  - Tests must use the mock provider — no real model calls in tests.
  - Provider secrets never logged.
  - AI cannot execute shell commands directly.
  - AI cannot merge PRs or change release truth.
  - AI summaries are commentary, not evidence.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

_TOOLS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(_TOOLS_DIR))
import rush_capture_lib as lib  # noqa: E402


# ─── Constants ───────────────────────────────────────────────────────────────

SCHEMA_VERSION = 1

# Forbidden paths — dev-if-fail must NEVER modify these.
FORBIDDEN_PATHS = [
    "VERSION",
    "Cargo.toml",  # workspace version field
    "RELEASES.md",
    "release/milestones.toml",
    "release/evidence/",  # entire tree
    "release/test-tiers.toml",
    ".github/workflows/ci.yml",
    "docs/decisions/",  # ADR Status/Ratified-by lines
    "mkosi/mkosi.extra/etc/os-release",
]

# Forbidden patterns in patch content — reject patches that contain these.
FORBIDDEN_PATTERNS = [
    (r"verified\s*=\s*true", "marking a milestone criterion verified"),
    (r"skip.*check|disable.*check|remove.*check", "weakening validators"),
    (r"rm\s+-rf\s+.*test|delete.*test|remove.*test", "deleting tests"),
    (r"sudo\s|chmod\s+777|chown\s+root", "privileged shell commands"),
    (r"curl\s|wget\s|http://|https://(?!schema\.json)", "network calls in tests"),
]

# Max patch size (bytes).
MAX_PATCH_SIZE = 64 * 1024  # 64 KiB


# ─── Context bundle ──────────────────────────────────────────────────────────


@dataclass
class ContextBundle:
    """A redacted context bundle for AI-assisted diagnosis."""

    schema_version: int = SCHEMA_VERSION
    context_kind: str = "rush-agent-context"
    run_dir: str = ""
    run_record: dict = field(default_factory=dict)
    manifest: dict = field(default_factory=dict)
    plan: dict = field(default_factory=dict)
    validator_output: dict = field(default_factory=dict)
    log_excerpts: dict = field(default_factory=dict)
    command_log_excerpts: list = field(default_factory=list)
    source_files: dict = field(default_factory=dict)
    git_diff: str = ""
    hardware_metadata: dict = field(default_factory=dict)
    previous_attempts: list = field(default_factory=list)
    redaction_report: dict = field(default_factory=dict)

    def to_dict(self) -> dict:
        return {
            "schema_version": self.schema_version,
            "context_kind": self.context_kind,
            "run_dir": self.run_dir,
            "run_record": self.run_record,
            "manifest": self.manifest,
            "plan": self.plan,
            "validator_output": self.validator_output,
            "log_excerpts": self.log_excerpts,
            "command_log_excerpts": self.command_log_excerpts,
            "source_files": self.source_files,
            "git_diff": self.git_diff,
            "hardware_metadata": self.hardware_metadata,
            "previous_attempts": self.previous_attempts,
            "redaction_report": self.redaction_report,
        }


def build_context(
    run_dir: Path,
    repo_root: Path,
    source_files: list[Path] | None = None,
    max_log_lines: int = 50,
) -> ContextBundle:
    """Build a redacted context bundle from a failed run directory.

    Args:
        run_dir: The capture run directory (from rush-autopilot run).
        repo_root: The repository root.
        source_files: Optional list of source files to include (redacted).
        max_log_lines: Max lines to include from each log file.
    """
    report = lib.RedactionReport()
    bundle = ContextBundle(run_dir=str(run_dir))

    # 1. Run record.
    run_record_path = run_dir / "run-record.json"
    if run_record_path.exists():
        try:
            bundle.run_record = lib.redact_dict(
                json.loads(run_record_path.read_text()), report
            )
        except (OSError, json.JSONDecodeError):
            pass

    # 2. Manifest (from rush-capture start).
    manifest_path = run_dir / "manifest.json"
    if manifest_path.exists():
        try:
            bundle.manifest = lib.redact_dict(
                json.loads(manifest_path.read_text()), report
            )
        except (OSError, json.JSONDecodeError):
            pass

    # 3. Plan (from plan.json in run-dir, if copied there).
    plan_path = run_dir / "plan.json"
    if not plan_path.exists():
        # Try to find the plan from the run record.
        pass
    if plan_path.exists():
        try:
            bundle.plan = lib.redact_dict(
                json.loads(plan_path.read_text()), report
            )
        except (OSError, json.JSONDecodeError):
            pass

    # 4. Validator output.
    # Run the hardware evidence validator on the run-dir.
    validator_path = _TOOLS_DIR / "validate-hwtest-evidence.py"
    if validator_path.exists() and run_dir.exists():
        try:
            r = subprocess.run(
                ["python3", str(validator_path), "--bundle", str(run_dir)],
                capture_output=True,
                text=True,
                timeout=30,
                cwd=str(repo_root),
            )
            bundle.validator_output = {
                "exit_code": r.returncode,
                "stdout": lib.redact(r.stdout[:4096], report),
                "stderr": lib.redact(r.stderr[:4096], report),
            }
        except (OSError, subprocess.TimeoutExpired):
            pass

    # 5. Log excerpts.
    for log_name in ("events.jsonl", "command-log.jsonl", "prompts.log",
                      "decisions.log", "actions.log", "summary.md"):
        log_path = run_dir / log_name
        if log_path.exists():
            try:
                text = log_path.read_text(errors="replace")
                lines = text.splitlines()[:max_log_lines]
                excerpt = "\n".join(lines)
                bundle.log_excerpts[log_name] = lib.redact(excerpt, report)
            except OSError:
                pass

    # 6. Command-log excerpts (first 5 entries).
    cl_path = run_dir / "command-log.jsonl"
    if cl_path.exists():
        try:
            entries = lib.read_jsonl(cl_path)[:5]
            bundle.command_log_excerpts = lib.redact_dict(entries, report)
        except OSError:
            pass

    # 7. Source files (redacted).
    if source_files:
        for sf in source_files:
            sf_path = repo_root / sf if not sf.is_absolute() else sf
            if sf_path.exists() and sf_path.is_file():
                try:
                    text = sf_path.read_text(errors="replace")[:8192]
                    rel = str(sf_path.relative_to(repo_root)) if repo_root in sf_path.parents else str(sf_path)
                    bundle.source_files[rel] = lib.redact(text, report)
                except OSError:
                    pass

    # 8. Git diff (redacted).
    try:
        r = subprocess.run(
            ["git", "-C", str(repo_root), "diff", "--stat"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        if r.returncode == 0:
            bundle.git_diff = lib.redact(r.stdout[:4096], report)
    except (OSError, subprocess.TimeoutExpired):
        pass

    # 9. Hardware metadata (from host.json in run-dir).
    host_path = run_dir / "host.json"
    if host_path.exists():
        try:
            bundle.hardware_metadata = lib.redact_dict(
                json.loads(host_path.read_text()), report
            )
        except (OSError, json.JSONDecodeError):
            pass

    # 10. Previous attempts (from run-dir/ai-attempts/).
    attempts_dir = run_dir / "ai-attempts"
    if attempts_dir.exists():
        for attempt_dir in sorted(attempts_dir.iterdir()):
            if not attempt_dir.is_dir():
                continue
            attempt_record = attempt_dir / "attempt.json"
            if attempt_record.exists():
                try:
                    bundle.previous_attempts.append(
                        lib.redact_dict(json.loads(attempt_record.read_text()), report)
                    )
                except (OSError, json.JSONDecodeError):
                    pass

    # Redaction report.
    bundle.redaction_report = report.to_dict()

    return bundle


# ─── Redaction validation ────────────────────────────────────────────────────


def validate_redaction(context: ContextBundle | dict) -> tuple[bool, list[str]]:
    """Validate that a context bundle has no unredacted secrets.

    Returns (ok, errors).
    """
    errors: list[str] = []
    if isinstance(context, ContextBundle):
        context_dict = context.to_dict()
    else:
        context_dict = context

    # Scan every string in the context for secret patterns.
    def _scan(obj, path="$"):
        if isinstance(obj, str):
            report = lib.RedactionReport()
            redacted = lib.redact(obj, report)
            if redacted != obj:
                errors.append(f"{path}: unredacted secret detected (redactors: {list(report.counts.keys())})")
        elif isinstance(obj, dict):
            for k, v in obj.items():
                _scan(v, f"{path}.{k}")
        elif isinstance(obj, list):
            for i, v in enumerate(obj):
                _scan(v, f"{path}[{i}]")

    _scan(context_dict)
    return (len(errors) == 0, errors)


# ─── Mock provider ───────────────────────────────────────────────────────────


# The mock provider returns deterministic diagnoses and patches.
# It does NOT call any external service — it produces canned text based on
# the context bundle's content. This is the ONLY provider available in tests.

MOCK_DIAGNOSIS = {
    "schema_version": SCHEMA_VERSION,
    "diagnosis_kind": "rush-agent-diagnosis",
    "provider": "mock",
    "model": "mock-v1",
    "diagnosis": "The run failed because the baseline benchmark command exited with a non-zero status. "
                 "The likely cause is that the rushbench binary is not installed in the test environment. "
                 "The fix is to use 'echo' as a stand-in command in the plan's benchmark steps, or to "
                 "ensure rushbench is built before running the plan.",
    "confidence": "medium",
    "suggested_files": [],
    "forbidden_actions": [],
}

MOCK_PATCH = {
    "schema_version": SCHEMA_VERSION,
    "patch_kind": "rush-agent-patch",
    "provider": "mock",
    "model": "mock-v1",
    "files": [
        {
            "path": "tools/rush-autopilot",
            "action": "modify",
            "description": "Replace rushbench with echo in fake mode (no functional change — fake mode already skips real commands).",
            "patch": "@@ -1,5 +1,5 @@\n # rush-autopilot\n-# (no change needed — this is a mock patch)\n+# mock patch: no actual change\n",
        }
    ],
    "validation_commands": [
        ["python3", "-m", "pytest", "tools/test-rush-runner.py", "-q"],
    ],
    "claim_pass": False,  # AI claim alone never marks pass
}


def mock_diagnose(context: ContextBundle | dict) -> dict:
    """Return a deterministic mock diagnosis. No network calls."""
    return dict(MOCK_DIAGNOSIS)


def mock_propose_patch(context: ContextBundle | dict) -> dict:
    """Return a deterministic mock patch. No network calls."""
    return dict(MOCK_PATCH)


# ─── Patch validation ────────────────────────────────────────────────────────


@dataclass
class PatchValidationResult:
    """Result of validating a proposed patch."""

    valid: bool
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    forbidden_files: list[str] = field(default_factory=list)
    forbidden_patterns: list[str] = field(default_factory=list)


def validate_patch(patch: dict, repo_root: Path | None = None) -> PatchValidationResult:
    """Validate a proposed patch.

    Checks:
      1. Patch is valid JSON with the required structure.
      2. No file in the patch is in the FORBIDDEN_PATHS list.
      3. No forbidden patterns in the patch content.
      4. Patch size is within limits.
      5. Validation commands use rush-exec (not direct shell).
    """
    result = PatchValidationResult(valid=True)

    # 1. Structure check.
    if not isinstance(patch, dict):
        result.valid = False
        result.errors.append("patch is not a dict")
        return result
    if patch.get("patch_kind") != "rush-agent-patch":
        result.valid = False
        result.errors.append(f"patch_kind must be 'rush-agent-patch', got {patch.get('patch_kind')!r}")
    files = patch.get("files", [])
    if not isinstance(files, list):
        result.valid = False
        result.errors.append("patch.files must be a list")
        return result

    # 2. Forbidden paths check.
    for f in files:
        if not isinstance(f, dict):
            continue
        path = f.get("path", "")
        for forbidden in FORBIDDEN_PATHS:
            if path == forbidden or path.startswith(forbidden):
                result.forbidden_files.append(path)
                result.valid = False
                result.errors.append(f"forbidden path: {path} (matches {forbidden})")

    # 3. Forbidden patterns check.
    patch_text = json.dumps(patch)
    for pattern, description in FORBIDDEN_PATTERNS:
        if re.search(pattern, patch_text, re.IGNORECASE):
            result.forbidden_patterns.append(description)
            result.valid = False
            result.errors.append(f"forbidden pattern: {description}")

    # 4. Size check.
    patch_bytes = len(patch_text.encode("utf-8"))
    if patch_bytes > MAX_PATCH_SIZE:
        result.valid = False
        result.errors.append(
            f"patch too large: {patch_bytes} bytes > {MAX_PATCH_SIZE} bytes"
        )

    # 5. Validation commands must use rush-exec.
    val_cmds = patch.get("validation_commands", [])
    for cmd in val_cmds:
        if isinstance(cmd, list) and cmd:
            # Commands should go through rush-exec.
            if cmd[0] not in ("rush-exec", "python3", "cargo", "git", "echo"):
                result.warnings.append(
                    f"validation command does not start with a known safe binary: {cmd[0]}"
                )

    # 6. claim_pass must be False (AI claim alone never marks pass).
    if patch.get("claim_pass", False) is True:
        result.valid = False
        result.errors.append("AI claim_pass=True is forbidden — AI claim alone never marks pass")

    return result


# ─── Dev-if-fail loop ────────────────────────────────────────────────────────


def dev_if_fail(
    run_dir: Path,
    repo_root: Path,
    provider: str = "mock",
    max_iterations: int = 3,
    fake: bool = True,
    fake_sys: Path | None = None,
) -> dict:
    """Run the dev-if-fail loop on a failed run directory.

    Steps:
      1. Confirm failure exists (run-record.json has status="aborted").
      2. Build redacted context bundle.
      3. Call rush-agent mock/provider for diagnosis.
      4. Call rush-agent mock/provider for patch.
      5. Validate patch (forbidden files, oversized, forbidden patterns).
      6. Apply patch on a branch/worktree (in fake mode, this is a no-op).
      7. Run validation commands through rush-exec.
      8. Rerun targeted test if feasible.
      9. Stop at pass/fail/limits.
      10. Preserve every attempt.

    Returns a dev-if-fail record dict.
    """
    attempts_dir = run_dir / "ai-attempts"
    attempts_dir.mkdir(parents=True, exist_ok=True)

    # 1. Confirm failure exists.
    run_record_path = run_dir / "run-record.json"
    if not run_record_path.exists():
        return {"status": "no-failure", "reason": "no run-record.json found"}
    try:
        run_record = json.loads(run_record_path.read_text())
    except json.JSONDecodeError:
        return {"status": "error", "error": "run-record.json is not valid JSON"}
    if run_record.get("status") != "aborted":
        return {"status": "no-failure", "reason": f"run status is {run_record.get('status')}, not 'aborted'"}

    # Find existing attempts to determine the iteration number.
    existing_attempts = sorted(attempts_dir.iterdir()) if attempts_dir.exists() else []
    start_iteration = len(existing_attempts)

    record = {
        "schema_version": SCHEMA_VERSION,
        "dev_if_fail_kind": "rush-agent-dev-if-fail",
        "run_dir": str(run_dir),
        "provider": provider,
        "max_iterations": max_iterations,
        "started_at": lib._now_iso(),
        "status": "in-progress",
        "attempts": [],
    }

    for iteration in range(start_iteration, start_iteration + max_iterations):
        attempt_dir = attempts_dir / f"attempt-{iteration:03d}"
        attempt_dir.mkdir(parents=True, exist_ok=True)

        attempt = {
            "iteration": iteration,
            "started_at": lib._now_iso(),
            "status": "in-progress",
        }

        # 2. Build redacted context bundle.
        context = build_context(run_dir, repo_root)
        context_path = attempt_dir / "context.json"
        context_path.write_text(json.dumps(context.to_dict(), indent=2, sort_keys=True) + "\n")
        attempt["context_path"] = str(context_path.relative_to(run_dir))

        # Validate redaction.
        redaction_ok, redaction_errors = validate_redaction(context)
        if not redaction_ok:
            attempt["status"] = "redaction-failed"
            attempt["redaction_errors"] = redaction_errors
            _write_attempt(attempt_dir, attempt)
            record["attempts"].append(attempt)
            record["status"] = "redaction-failed"
            break

        # 3. Diagnose.
        if provider == "mock":
            diagnosis = mock_diagnose(context)
        else:
            # Real providers not implemented in this phase.
            attempt["status"] = "provider-unavailable"
            attempt["error"] = f"provider {provider!r} not implemented (only 'mock' is available)"
            _write_attempt(attempt_dir, attempt)
            record["attempts"].append(attempt)
            record["status"] = "provider-unavailable"
            break

        diagnosis_path = attempt_dir / "diagnosis.json"
        diagnosis_path.write_text(json.dumps(diagnosis, indent=2, sort_keys=True) + "\n")
        attempt["diagnosis"] = diagnosis

        # 4. Propose patch.
        if provider == "mock":
            patch = mock_propose_patch(context)
        else:
            attempt["status"] = "provider-unavailable"
            _write_attempt(attempt_dir, attempt)
            record["attempts"].append(attempt)
            record["status"] = "provider-unavailable"
            break

        patch_path = attempt_dir / "patch.json"
        patch_path.write_text(json.dumps(patch, indent=2, sort_keys=True) + "\n")
        attempt["patch"] = patch

        # 5. Validate patch.
        patch_validation = validate_patch(patch, repo_root)
        attempt["patch_valid"] = patch_validation.valid
        attempt["patch_errors"] = patch_validation.errors
        attempt["patch_warnings"] = patch_validation.warnings
        if not patch_validation.valid:
            attempt["status"] = "patch-rejected"
            _write_attempt(attempt_dir, attempt)
            record["attempts"].append(attempt)
            record["status"] = "patch-rejected"
            break

        # 6. Apply patch (in fake mode, this is a no-op — we don't modify real files).
        if fake:
            attempt["patch_applied"] = False
            attempt["patch_apply_note"] = "fake mode — patch not applied to real files"
        else:
            # Real mode: apply the patch to a worktree.
            # NOT IMPLEMENTED in this phase — would require git worktree creation.
            attempt["patch_applied"] = False
            attempt["patch_apply_note"] = "real patch application not implemented in this phase"

        # 7. Run validation commands through rush-exec.
        val_cmds = patch.get("validation_commands", [])
        val_results = []
        all_passed = True
        for cmd in val_cmds:
            if fake:
                # Fake mode: simulate validation. If the command is "false"
                # (which always exits 1), simulate failure; otherwise pass.
                if cmd and cmd[0] == "false":
                    val_results.append({
                        "command": cmd,
                        "exit_code": 1,
                        "stdout": "",
                        "stderr": "[fake] validation failed (false)\n",
                    })
                    all_passed = False
                else:
                    val_results.append({
                        "command": cmd,
                        "exit_code": 0,
                        "stdout": "[fake] validation passed",
                        "stderr": "",
                    })
            else:
                # Real mode: run through rush-exec.
                rush_exec = str(_TOOLS_DIR / "rush-exec")
                full_cmd = ["python3", rush_exec, "--run-dir", str(run_dir), "--"] + cmd
                try:
                    r = subprocess.run(full_cmd, capture_output=True, text=True, timeout=300)
                    val_results.append({
                        "command": cmd,
                        "exit_code": r.returncode,
                        "stdout": r.stdout[:4096],
                        "stderr": r.stderr[:4096],
                    })
                    if r.returncode != 0:
                        all_passed = False
                except (OSError, subprocess.TimeoutExpired) as e:
                    val_results.append({
                        "command": cmd,
                        "exit_code": -1,
                        "stdout": "",
                        "stderr": str(e),
                    })
                    all_passed = False

        attempt["validation_results"] = val_results
        attempt["all_validation_passed"] = all_passed

        # 8. AI claim alone never marks pass.
        if patch.get("claim_pass", False) and all_passed:
            attempt["status"] = "ai-claim-rejected"
            attempt["note"] = "AI claim_pass=True is forbidden even if validation passed"
            _write_attempt(attempt_dir, attempt)
            record["attempts"].append(attempt)
            record["status"] = "ai-claim-rejected"
            break

        # 9. Stop at pass/fail/limits.
        if all_passed:
            attempt["status"] = "passed"
            _write_attempt(attempt_dir, attempt)
            record["attempts"].append(attempt)
            record["status"] = "passed"
            break
        else:
            attempt["status"] = "failed"
            _write_attempt(attempt_dir, attempt)
            record["attempts"].append(attempt)
            # Continue to next iteration.
            continue

    # Check if we hit the iteration limit.
    if record["status"] == "in-progress":
        record["status"] = "max-iterations-reached"

    record["finished_at"] = lib._now_iso()
    return record


def _write_attempt(attempt_dir: Path, attempt: dict) -> None:
    """Write the attempt record to attempt.json."""
    path = attempt_dir / "attempt.json"
    path.write_text(json.dumps(attempt, indent=2, sort_keys=True) + "\n")


# ─── Cost tracking (stub for future real providers) ──────────────────────────


def check_budget(provider: str, estimated_cost_usd: float) -> tuple[bool, str]:
    """Check if a provider call would exceed the monthly budget.

    Returns (ok, reason). Always returns (True, "") for the mock provider.
    For real providers, reads ~/.config/rush/ai-budget.json.
    """
    if provider == "mock":
        return (True, "")
    # Real providers: check the budget file.
    budget_path = Path.home() / ".config" / "rush" / "ai-budget.json"
    if not budget_path.exists():
        return (True, "")
    try:
        budget = json.loads(budget_path.read_text())
        monthly = budget.get(provider, {}).get("monthly_spend_usd", 0)
        cap = budget.get(provider, {}).get("monthly_cap_usd", 50)
        if monthly + estimated_cost_usd > cap:
            return (False, f"budget exceeded: {monthly + estimated_cost_usd:.2f} > {cap:.2f}")
    except (OSError, json.JSONDecodeError):
        pass
    return (True, "")
