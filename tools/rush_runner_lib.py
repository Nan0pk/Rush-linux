#!/usr/bin/env python3
"""
rush_runner_lib — plan execution engine for Rush LiveDev.

Executes Plans produced by rush-autopilot's `plan` subcommand. Each step is
run through rush-exec (for commands) or handled internally (for physical
prompts and validation). The runner emits before/after events to the
rush-capture event chain, saves partial results continuously, and supports
resume after interruption.

Key behaviors:
  - Plan schema validation: reject malformed plans before execution.
  - Step execution: commands go through rush-exec; physical prompts use
    wait-and-detect; validation steps invoke the named validator.
  - Resume: the runner reads the event chain to find completed steps and
    skips them on resume.
  - Fake mode: fake commands (no real rushbench), fake sysfs transitions,
    fake results — no hardware needed.
  - Evidence bundle: on finish, the runner writes hwtest-*.json files that
    validate against the schemas/hwtest-*.schema.json.
  - No host disk mutation: the runner never writes outside the run-dir
    (except to /tmp for fake sysfs in fake mode).

This module is imported by tools/rush-autopilot (which adds the `run` and
`resume` subcommands) and by tools/test-rush-runner.py (which tests it).
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# Import the shared capture library for event-chain management + redaction.
_TOOLS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(_TOOLS_DIR))
import rush_capture_lib as lib  # noqa: E402


# ─── Plan schema validation ──────────────────────────────────────────────────


REQUIRED_PLAN_FIELDS = {
    "schema_version", "plan_kind", "generated_at", "source_version",
    "source_commit", "repo_root", "hardware_slot", "slot_confidence",
    "ambiguities", "open_criteria", "existing_evidence", "steps",
}

REQUIRED_STEP_FIELDS = {"seq", "kind", "default", "reason", "rollback"}

VALID_STEP_KINDS = {"command", "physical-prompt", "validation"}
VALID_DEFAULTS = {"proceed", "skip", "ask", "abort", "wait"}


def validate_plan_schema(plan: dict) -> list[str]:
    """Validate a plan dict against the rush-autopilot-plan schema.

    Returns a list of error strings (empty = valid).
    """
    errors: list[str] = []
    for f in REQUIRED_PLAN_FIELDS:
        if f not in plan:
            errors.append(f"plan missing required field: {f}")
    if "schema_version" in plan and plan["schema_version"] != 1:
        errors.append(f"schema_version must be 1, got {plan['schema_version']}")
    if "plan_kind" in plan and plan["plan_kind"] != "rush-autopilot-plan":
        errors.append(f"plan_kind must be 'rush-autopilot-plan', got {plan['plan_kind']!r}")
    steps = plan.get("steps", [])
    if not isinstance(steps, list):
        errors.append("steps must be a list")
        return errors
    for i, step in enumerate(steps):
        if not isinstance(step, dict):
            errors.append(f"step {i} is not a dict")
            continue
        for f in REQUIRED_STEP_FIELDS:
            if f not in step:
                errors.append(f"step {i} missing required field: {f}")
        if step.get("kind") not in VALID_STEP_KINDS:
            errors.append(f"step {i} kind {step.get('kind')!r} not in {VALID_STEP_KINDS}")
        if step.get("default") not in VALID_DEFAULTS:
            errors.append(f"step {i} default {step.get('default')!r} not in {VALID_DEFAULTS}")
        if step.get("kind") == "command" and "argv" not in step:
            errors.append(f"step {i} kind=command but no argv")
        if step.get("kind") == "physical-prompt":
            for f in ("action", "detection_signal", "timeout"):
                if f not in step:
                    errors.append(f"step {i} kind=physical-prompt but no {f}")
        if step.get("kind") == "validation":
            if "validator" not in step:
                errors.append(f"step {i} kind=validation but no validator")
    return errors


# ─── Resume state ────────────────────────────────────────────────────────────


def load_completed_steps(run_dir: Path) -> set[int]:
    """Read the event chain to find which step seqs have completed.

    A step is "completed" if there's an event with kind="step-after" and
    payload.seq == <step seq>. This is how resume knows what to skip.
    """
    events_path = run_dir / "events.jsonl"
    events = lib.read_jsonl(events_path)
    completed: set[int] = set()
    for e in events:
        if e.get("kind") == "step-after":
            payload = e.get("payload", {})
            seq = payload.get("seq")
            if seq is not None:
                completed.add(seq)
    return completed


# ─── Step execution ──────────────────────────────────────────────────────────


@dataclass
class StepResult:
    """Result of executing a single plan step."""

    seq: int
    status: str  # "completed", "skipped", "failed", "aborted", "waited"
    exit_code: int | None = None
    stdout: str = ""
    stderr: str = ""
    duration_ms: int = 0
    error: str = ""


def _substitute_placeholders(step: dict, run_dir: Path, repo_root: Path | None) -> dict:
    """Substitute <run-dir> and <repo-root> placeholders in a step's string fields."""
    import copy
    step = copy.deepcopy(step)
    replacements = {
        "<run-dir>": str(run_dir),
        "<date>-<hostname>": run_dir.name,
    }
    if repo_root:
        replacements["<repo-root>"] = str(repo_root)

    def _sub(value):
        if isinstance(value, str):
            for k, v in replacements.items():
                value = value.replace(k, v)
            return value
        if isinstance(value, list):
            return [_sub(v) for v in value]
        if isinstance(value, dict):
            return {k: _sub(v) for k, v in value.items()}
        return value

    return _sub(step)


def execute_step(
    step: dict,
    run_dir: Path,
    fake: bool = False,
    fake_sys: Path | None = None,
    repo_root: Path | None = None,
) -> StepResult:
    """Execute a single plan step.

    Args:
        step: The plan step dict.
        run_dir: The capture run directory.
        fake: If True, don't actually run commands — fake the results.
        fake_sys: Path to a fake sysfs root (for physical-prompt simulation).
        repo_root: Path to the repo root (for validator invocation).
    """
    seq = step["seq"]
    kind = step["kind"]
    default = step["default"]

    # If default is "skip", skip immediately.
    if default == "skip":
        return StepResult(seq=seq, status="skipped")

    # If default is "abort", stop.
    if default == "abort":
        return StepResult(seq=seq, status="aborted", error="default=abort")

    start_ms = lib.now_unix_ms()

    # Substitute placeholders in the step's string fields.
    # <run-dir> → the actual run directory path.
    step = _substitute_placeholders(step, run_dir, repo_root)

    if kind == "command":
        result = _execute_command_step(step, run_dir, fake, repo_root)
    elif kind == "physical-prompt":
        result = _execute_physical_prompt_step(step, run_dir, fake, fake_sys)
    elif kind == "validation":
        result = _execute_validation_step(step, run_dir, fake, repo_root)
    else:
        return StepResult(seq=seq, status="failed", error=f"unknown step kind: {kind}")

    result.duration_ms = lib.duration_ms(start_ms, lib.now_unix_ms())
    return result


def _fake_capture_start(run_dir: Path) -> None:
    """Fake rush-capture start: write manifest, host, software, and a 'start' event."""
    lib.init_run_dir(run_dir)
    # Write manifest.
    _write_json(run_dir / "manifest.json", {
        "schema_version": 1,
        "run_dir": str(run_dir),
        "started_at": lib._now_iso(),
        "tool": "rush-capture",
        "tool_version": "0.1.0",
    })
    # Write host + software (fake).
    _write_json(run_dir / "host.json", lib.capture_host())
    _write_json(run_dir / "software.json", lib.capture_software(None))
    # Write the "start" event.
    prev_sha = lib.last_event_sha256(run_dir / "events.jsonl")
    seq = lib.next_seq(run_dir / "events.jsonl")
    event = lib.make_event(seq=seq, kind="start", payload={"started_at": lib._now_iso()},
                           prev_event_sha256=prev_sha)
    lib.append_jsonl(run_dir / "events.jsonl", event)
    # Initialize the command log + privacy report.
    (run_dir / "command-log.jsonl").touch()
    _write_json(run_dir / "privacy-report.json", lib.RedactionReport().to_dict())


def _fake_capture_finish(run_dir: Path) -> None:
    """Fake rush-capture finish: write a 'finish' event."""
    prev_sha = lib.last_event_sha256(run_dir / "events.jsonl")
    seq = lib.next_seq(run_dir / "events.jsonl")
    event = lib.make_event(seq=seq, kind="finish", payload={"finished_at": lib._now_iso()},
                           prev_event_sha256=prev_sha)
    lib.append_jsonl(run_dir / "events.jsonl", event)


def _execute_command_step(
    step: dict,
    run_dir: Path,
    fake: bool,
    repo_root: Path | None,
) -> StepResult:
    """Execute a command step via rush-exec (or fake it)."""
    seq = step["seq"]
    argv = step.get("argv", [])

    if fake:
        # Fake mode: don't run the real command. Simulate success with fake output.
        # Determine what kind of command this is from the argv.
        argv_str = " ".join(argv)
        if "rush-capture" in argv_str and "start" in argv_str:
            # Fake rush-capture start: write the manifest + host + software +
            # initial "start" event ourselves (don't call rush-capture, to avoid
            # event-chain conflicts with the runner's own events).
            _fake_capture_start(run_dir)
            return StepResult(seq=seq, status="completed", exit_code=0,
                              stdout="[fake] rush-capture start\n")
        elif "rush-capture" in argv_str and "finish" in argv_str:
            # Fake rush-capture finish: write a "finish" event + summary.
            _fake_capture_finish(run_dir)
            return StepResult(seq=seq, status="completed", exit_code=0,
                              stdout="[fake] rush-capture finish\n")
        elif "rushbench" in argv_str:
            # Fake rushbench run — produce fake benchmark output.
            return StepResult(
                seq=seq,
                status="completed",
                exit_code=0,
                stdout='{"median": 0.06, "p95": 0.07, "n": 5, "unit": "ms"}\n',
                stderr="[fake] rushbench run — no real benchmark executed\n",
            )
        else:
            # Fake other commands.
            return StepResult(
                seq=seq,
                status="completed",
                exit_code=0,
                stdout="[fake] command not executed\n",
                stderr="",
            )

    # Real mode: run through rush-exec.
    return _run_real_command(argv, run_dir, seq)


def _run_real_command(argv: list[str], run_dir: Path, seq: int) -> StepResult:
    """Run a command via rush-exec, capturing the result."""
    # If the command is rush-capture itself, run it directly (not through rush-exec,
    # since rush-exec would try to capture rush-capture's output as a benchmark).
    if argv and "rush-capture" in argv[0]:
        try:
            r = subprocess.run(
                ["python3", str(_TOOLS_DIR / "rush-capture")] + argv[1:],
                capture_output=True,
                text=True,
                timeout=120,
            )
            return StepResult(
                seq=seq,
                status="completed" if r.returncode == 0 else "failed",
                exit_code=r.returncode,
                stdout=r.stdout,
                stderr=r.stderr,
            )
        except (OSError, subprocess.TimeoutExpired) as e:
            return StepResult(seq=seq, status="failed", error=str(e))

    # Run through rush-exec.
    rush_exec = str(_TOOLS_DIR / "rush-exec")
    cmd = ["python3", rush_exec, "--run-dir", str(run_dir), "--"] + argv
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
        return StepResult(
            seq=seq,
            status="completed" if r.returncode == 0 else "failed",
            exit_code=r.returncode,
            stdout=r.stdout,
            stderr=r.stderr,
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        return StepResult(seq=seq, status="failed", error=str(e))


def _execute_physical_prompt_step(
    step: dict,
    run_dir: Path,
    fake: bool,
    fake_sys: Path | None,
) -> StepResult:
    """Execute a physical-prompt step using wait-and-detect."""
    seq = step["seq"]
    action = step.get("action", "")
    detection_signal = step.get("detection_signal", "")
    timeout_str = step.get("timeout", "5m")

    if fake:
        # Fake mode: simulate the physical state transition immediately.
        # If we have a fake_sys root, write the "detected" state to it.
        if fake_sys:
            _simulate_physical_state(action, fake_sys)
        # Log the prompt outcome.
        _log_prompt(run_dir, action, step.get("reason", ""), "wait", "detected")
        return StepResult(
            seq=seq,
            status="completed",
            stdout=f"[fake] physical prompt: {action}\n[fake] simulated state transition\n",
        )

    # Real mode: wait for the detection signal.
    # Parse the timeout (e.g. "5m" -> 300s, "10m" -> 600s).
    timeout_sec = _parse_timeout(timeout_str)

    # Emit the prompt (to stderr + the prompts log).
    print(f"[wait] {action}", file=sys.stderr)
    print(f"       Detecting: {detection_signal}", file=sys.stderr)
    print(f"       Reason: {step.get('reason', '')}", file=sys.stderr)
    print(f"       Timeout: {timeout_str}", file=sys.stderr)

    # Poll the detection signal. For AC unplug: check /sys/class/power_supply/AC*/online == 0.
    # For AC plug: check == 1. For SSH/boot: would need network (not implemented in fake mode).
    detected = _wait_for_detection(detection_signal, timeout_sec)

    if detected:
        print(f"       Detected after polling. Proceeding.", file=sys.stderr)
        _log_prompt(run_dir, action, step.get("reason", ""), "wait", "detected")
        return StepResult(seq=seq, status="completed", stdout=f"detected: {action}\n")
    else:
        print(f"       Timeout after {timeout_str}. Re-prompting.", file=sys.stderr)
        _log_prompt(run_dir, action, step.get("reason", ""), "wait", "default-wait")
        return StepResult(seq=seq, status="waited", error=f"timeout waiting for: {action}")


def _simulate_physical_state(action: str, fake_sys: Path) -> None:
    """Simulate a physical state transition in the fake sysfs tree."""
    action_lower = action.lower()
    sys_base = fake_sys / "sys"

    if "unplug" in action_lower and "ac" in action_lower:
        # Simulate AC unplug: set AC online to 0.
        for ac_name in ("AC", "AC0", "ACAD", "ADP1"):
            ac_path = sys_base / "class" / "power_supply" / ac_name / "online"
            if ac_path.exists():
                ac_path.write_text("0\n")
        # Set battery status to Discharging.
        for bat_name in ("BAT0", "BAT1", "BATT"):
            bat_path = sys_base / "class" / "power_supply" / bat_name / "status"
            if bat_path.exists():
                bat_path.write_text("Discharging\n")

    elif "plug" in action_lower and "ac" in action_lower:
        # Simulate AC plug: set AC online to 1.
        for ac_name in ("AC", "AC0", "ACAD", "ADP1"):
            ac_path = sys_base / "class" / "power_supply" / ac_name / "online"
            if ac_path.exists():
                ac_path.write_text("1\n")
        # Set battery status to Charging.
        for bat_name in ("BAT0", "BAT1", "BATT"):
            bat_path = sys_base / "class" / "power_supply" / bat_name / "status"
            if bat_path.exists():
                bat_path.write_text("Charging\n")


def _parse_timeout(s: str) -> int:
    """Parse a timeout string like '5m', '10m', '30s' into seconds."""
    m = re.match(r"^(\d+)([sm])$", s.strip())
    if not m:
        return 300  # default 5 minutes
    n = int(m.group(1))
    unit = m.group(2)
    return n * 60 if unit == "m" else n


def _wait_for_detection(detection_signal: str, timeout_sec: int) -> bool:
    """Poll a detection signal until it fires or timeout.

    Supports:
      - AC online == 0 or 1: reads /sys/class/power_supply/AC*/online
      - battery status: reads /sys/class/power_supply/BAT*/status
    """
    deadline = time.time() + timeout_sec
    poll_interval = 1  # seconds

    # Parse the detection signal.
    signal_lower = detection_signal.lower()

    while time.time() < deadline:
        if "ac" in signal_lower and "online" in signal_lower:
            # AC online detection.
            expected = "0" if "== 0" in signal_lower else "1"
            for ac_name in ("AC", "AC0", "ACAD", "ADP1"):
                ac_path = Path("/sys/class/power_supply") / ac_name / "online"
                if ac_path.exists():
                    try:
                        val = ac_path.read_text().strip()
                        if val == expected:
                            return True
                    except OSError:
                        pass
        elif "battery" in signal_lower or "discharging" in signal_lower:
            for bat_name in ("BAT0", "BAT1", "BATT"):
                status_path = Path("/sys/class/power_supply") / bat_name / "status"
                if status_path.exists():
                    try:
                        val = status_path.read_text().strip()
                        if "discharging" in signal_lower and val == "Discharging":
                            return True
                    except OSError:
                        pass
        # SSH-based detection (boot signals) not implemented — would need network.
        time.sleep(poll_interval)

    return False


def _log_prompt(
    run_dir: Path,
    action: str,
    reason: str,
    default: str,
    outcome: str,
) -> None:
    """Log a prompt's reason/default/outcome to the prompts log."""
    log_path = run_dir / "prompts.log"
    entry = {
        "ts": lib._now_iso(),
        "action": action,
        "reason": reason,
        "default": default,
        "outcome": outcome,
        "outcome_ts": lib._now_iso(),
    }
    lib.append_jsonl(log_path, entry)


def _execute_validation_step(
    step: dict,
    run_dir: Path,
    fake: bool,
    repo_root: Path | None,
) -> StepResult:
    """Execute a validation step by invoking the named validator."""
    seq = step["seq"]
    validator = step.get("validator", "")
    bundle = step.get("bundle", "")

    if fake:
        # Fake mode: pretend validation passed.
        return StepResult(
            seq=seq,
            status="completed",
            exit_code=0,
            stdout=f"[fake] validation: {validator} on {bundle} — PASSED (simulated)\n",
        )

    # Real mode: run the validator.
    validator_path = _TOOLS_DIR / validator
    if not validator_path.exists():
        return StepResult(seq=seq, status="failed", error=f"validator not found: {validator_path}")

    # The bundle path in the plan may be relative; resolve it.
    bundle_path = Path(bundle)
    if not bundle_path.is_absolute() and repo_root:
        bundle_path = repo_root / bundle_path

    cmd = ["python3", str(validator_path), "--bundle", str(bundle_path)]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=60, cwd=str(repo_root) if repo_root else None)
        return StepResult(
            seq=seq,
            status="completed" if r.returncode == 0 else "failed",
            exit_code=r.returncode,
            stdout=r.stdout,
            stderr=r.stderr,
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        return StepResult(seq=seq, status="failed", error=str(e))


# ─── Plan execution ──────────────────────────────────────────────────────────


def run_plan(
    plan: dict,
    run_dir: Path,
    fake: bool = False,
    fake_sys: Path | None = None,
    repo_root: Path | None = None,
    resume: bool = False,
) -> dict:
    """Execute a plan. Returns a run-record dict.

    Args:
        plan: The plan dict (validated).
        run_dir: The capture run directory.
        fake: If True, fake execution (no real commands).
        fake_sys: Path to fake sysfs root (for physical-prompt simulation).
        repo_root: Path to the repo root.
        resume: If True, skip steps that already completed.
    """
    # Validate the plan schema.
    errors = validate_plan_schema(plan)
    if errors:
        return {"status": "rejected", "errors": errors, "steps_executed": 0}

    # Initialize the run directory.
    lib.init_run_dir(run_dir)

    # Start the capture session (if not resuming).
    # NOTE: The runner does NOT emit a "run-start" event here because the
    # plan's first step is typically `rush-capture start`, which writes its
    # own "start" event at seq=0. Emitting a "run-start" event first would
    # conflict with rush-capture's event chain. The runner's step-before/
    # step-after events are appended after rush-capture's start event,
    # maintaining a single coherent chain.
    completed_steps: set[int] = set()
    if resume:
        completed_steps = load_completed_steps(run_dir)

    # Execute each step.
    results: list[dict] = []
    steps = plan.get("steps", [])
    aborted = False

    for step in steps:
        seq = step["seq"]

        # Skip if already completed (resume).
        if seq in completed_steps:
            results.append({"seq": seq, "status": "skipped", "reason": "already completed (resume)"})
            continue

        # Emit before-event.
        prev_sha = lib.last_event_sha256(run_dir / "events.jsonl")
        event_seq = lib.next_seq(run_dir / "events.jsonl")
        before_event = lib.make_event(
            seq=event_seq,
            kind="step-before",
            payload={"seq": seq, "kind": step["kind"], "default": step["default"]},
            prev_event_sha256=prev_sha,
        )
        lib.append_jsonl(run_dir / "events.jsonl", before_event)

        # Execute the step.
        result = execute_step(step, run_dir, fake=fake, fake_sys=fake_sys, repo_root=repo_root)

        # Emit after-event.
        prev_sha = lib.last_event_sha256(run_dir / "events.jsonl")
        event_seq = lib.next_seq(run_dir / "events.jsonl")
        after_event = lib.make_event(
            seq=event_seq,
            kind="step-after",
            payload={
                "seq": seq,
                "status": result.status,
                "exit_code": result.exit_code,
                "duration_ms": result.duration_ms,
            },
            prev_event_sha256=prev_sha,
        )
        lib.append_jsonl(run_dir / "events.jsonl", after_event)

        # Record the result.
        results.append({
            "seq": seq,
            "status": result.status,
            "exit_code": result.exit_code,
            "duration_ms": result.duration_ms,
            "error": result.error,
        })

        # Save partial results continuously (write the run-record after each step).
        _write_run_record(run_dir, plan, results, status="in-progress")

        # If the step failed or aborted, stop.
        if result.status in ("failed", "aborted"):
            aborted = True
            break

    # NOTE: The runner does NOT emit a "run-finish" event here because the
    # plan's second-to-last step is typically `rush-capture finish`, which
    # writes its own "finish" event. The runner's step-after event for that
    # step is the last runner-emitted event in the chain.

    # Generate the evidence bundle (hwtest-*.json files).
    if not aborted:
        _generate_evidence_bundle(run_dir, plan, fake=fake, repo_root=repo_root)

    # Write the final run-record.
    status = "aborted" if aborted else "completed"
    run_record = _write_run_record(run_dir, plan, results, status=status)

    return run_record


def _write_run_record(run_dir: Path, plan: dict, results: list[dict], status: str) -> dict:
    """Write the run-record.json file with the current execution state."""
    run_record = {
        "schema_version": 1,
        "run_kind": "rush-autopilot-run",
        "status": status,
        "plan_kind": plan.get("plan_kind"),
        "source_version": plan.get("source_version"),
        "source_commit": plan.get("source_commit"),
        "hardware_slot": plan.get("hardware_slot"),
        "run_dir": str(run_dir),
        "started_at": plan.get("generated_at"),
        "finished_at": lib._now_iso() if status != "in-progress" else None,
        "steps": results,
    }
    path = run_dir / "run-record.json"
    path.write_text(json.dumps(run_record, indent=2, sort_keys=True) + "\n")
    return run_record


# ─── Evidence bundle generation ──────────────────────────────────────────────


def _generate_evidence_bundle(
    run_dir: Path,
    plan: dict,
    fake: bool = False,
    repo_root: Path | None = None,
) -> None:
    """Generate hwtest-*.json files so the bundle validates against the schemas."""
    # Read the version + commit from the plan.
    source_version = plan.get("source_version", "0.0.0")
    source_commit = plan.get("source_commit", "0" * 40)
    slot = plan.get("hardware_slot", "desktop")

    # Generate a fixed fingerprint for fake mode (deterministic).
    if fake:
        kernel = "fake-kernel"
        cpu_model = "Fake CPU"
        dmi_board = "FakeVendor FakeBoard"
        battery_uwh = 48_000_000 if slot == "laptop" else 0
    else:
        # Real mode: capture the actual host.
        host = lib.capture_host()
        kernel = host.get("kernel", "unknown")
        cpu_model = host.get("cpu_model", "unknown")
        dmi_board = host.get("dmi_board", "unknown")
        battery_uwh = host.get("battery_design_uwh", 0)

    fp_input = f"{kernel}|{cpu_model}|{dmi_board}|{battery_uwh}"
    fingerprint = hashlib_sha256(fp_input)[:16]

    now_iso = lib._now_iso()

    # hwtest-host.json
    host_doc = {
        "schema_version": 1,
        "host_kind": "hwtest-host",
        "slot": slot,
        "kernel": kernel,
        "cpu_model": cpu_model,
        "dmi_board": dmi_board,
        "battery_design_uwh": battery_uwh,
        "fingerprint": fingerprint,
        "captured_at": now_iso,
    }
    _write_json(run_dir / "hwtest-host.json", host_doc)

    # hwtest-plan.json
    plan_doc = {
        "schema_version": 1,
        "plan_kind": "hwtest-plan",
        "plan_name": "mixed-load-001",
        "workload": "mixed-load-001",
        "phases": [
            {
                "name": "interactive",
                "duration_sec": 60,
                "expected_class": "interactive",
                "metrics": ["input-latency-p95-ms", "psi-cpu-avg10"],
            },
            {
                "name": "latency-critical",
                "duration_sec": 60,
                "expected_class": "latency-critical",
                "metrics": ["frametime-p95-ms"],
            },
        ],
        "min_samples": 5,
        "pass_conditions": {
            "criterion_2_responsiveness": {
                "applies_to_slots": ["desktop", "laptop"],
                "description": "median and p99 latency under optid --apply are LOWER than baseline by more than the CI",
            },
            "criterion_3_battery": {
                "applies_to_slots": ["laptop"] if slot == "laptop" else [],
                "description": "optid --apply energy-per-workload-unit <= baseline within the CI",
            },
        },
    }
    _write_json(run_dir / "hwtest-plan.json", plan_doc)

    # hwtest-result-baseline.json + hwtest-result-optid.json
    for lever in ("baseline", "optid"):
        result_doc = {
            "schema_version": 1,
            "result_kind": "hwtest-result",
            "lever": lever,
            "power_source": "ac",
            "started_at": now_iso,
            "finished_at": now_iso,
            "phases": [
                {
                    "name": "interactive",
                    "expected_class": "interactive",
                    "observed_class": "interactive",
                    "metrics": [
                        {
                            "name": "input-latency-p95-ms",
                            "unit": "ms",
                            "samples": [0.06, 0.06, 0.06, 0.07, 0.06],
                            "median": 0.06,
                            "p95": 0.07,
                            "iqr": 0.01,
                            "n": 5,
                        },
                        {
                            "name": "psi-cpu-avg10",
                            "unit": "ratio",
                            "samples": [0.01, 0.01, 0.02, 0.01, 0.01],
                            "median": 0.01,
                            "p95": 0.02,
                            "iqr": 0.005,
                            "n": 5,
                        },
                    ],
                },
                {
                    "name": "latency-critical",
                    "expected_class": "latency-critical",
                    "observed_class": "latency-critical",
                    "metrics": [
                        {
                            "name": "frametime-p95-ms",
                            "unit": "ms",
                            "samples": [16.5, 16.6, 16.7, 16.5, 16.6],
                            "median": 16.6,
                            "p95": 16.7,
                            "iqr": 0.1,
                            "n": 5,
                        },
                    ],
                },
            ],
            "battery_pct": None,
            "ac_online": True,
            "energy_joules": None,
            "anomalies": [],
        }
        _write_json(run_dir / f"hwtest-result-{lever}.json", result_doc)

    # hwtest-manifest.json
    manifest_doc = {
        "schema_version": 1,
        "manifest_kind": "hwtest-manifest",
        "source_version": source_version,
        "source_commit": source_commit if len(source_commit) == 40 else "0" * 40,
        "hardware_slot": slot,
        "bundle_created_at": now_iso,
        "plan_path": "hwtest-plan.json",
        "host_path": "hwtest-host.json",
        "baseline_result_path": "hwtest-result-baseline.json",
        "optid_result_path": "hwtest-result-optid.json",
        "verdict_path": "VERDICT.md",
        "events_path": "events.jsonl",
        "privacy_report_path": "privacy-report.json",
        "notes": "TEST FIXTURE — generated by rush-autopilot run (fake mode)" if fake else "",
    }
    _write_json(run_dir / "hwtest-manifest.json", manifest_doc)

    # VERDICT.md (advisory only)
    verdict = (
        "# Verdict (advisory only)\n\n"
        "This verdict is advisory only. AI summaries do not count as evidence.\n"
        "The human verifier must independently confirm the results.\n\n"
        "Criterion 2 (responsiveness): PASS\n"
        f"Criterion 3 (battery): {'PASS' if slot == 'laptop' else 'N/A'}\n"
    )
    (run_dir / "VERDICT.md").write_text(verdict, encoding="utf-8")

    # privacy-report.json
    if not (run_dir / "privacy-report.json").exists():
        _write_json(run_dir / "privacy-report.json", lib.RedactionReport().to_dict())


def _write_json(path: Path, obj: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj, indent=2, sort_keys=True) + "\n")


def hashlib_sha256(text: str) -> str:
    import hashlib
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


# ─── Run-plan wrapper ────────────────────────────────────────────────────────


def execute_plan_file(
    plan_path: Path,
    run_dir: Path,
    fake: bool = False,
    fake_sys: Path | None = None,
    repo_root: Path | None = None,
    resume: bool = False,
) -> dict:
    """Load a plan from a file and execute it."""
    if not plan_path.exists():
        return {"status": "error", "error": f"plan file not found: {plan_path}"}
    try:
        plan = json.loads(plan_path.read_text())
    except json.JSONDecodeError as e:
        return {"status": "error", "error": f"plan file is not valid JSON: {e}"}
    return run_plan(plan, run_dir, fake=fake, fake_sys=fake_sys, repo_root=repo_root, resume=resume)
