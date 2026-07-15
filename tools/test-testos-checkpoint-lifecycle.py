#!/usr/bin/env python3
"""
test-testos-checkpoint-lifecycle.py — Integration tests for the testOS
checkpoint lifecycle (corrective-2 F3).

Proves:
  1. A terminal (submitted) checkpoint is detected BEFORE any write.
  2. ensure-fresh auto-preserves the terminal checkpoint and creates a
     fresh run_id, directory, and (in the host workflow) a fresh nonce,
     inventory, and plan.
  3. Prior PR data is NOT erased — the old run directory survives.
  4. The new run_id is distinct from the old one.
  5. Two consecutive runs work: run 1 submits, run 2 starts fresh without
     manual `clear`.

Run:
    python3 -m pytest tools/test-testos-checkpoint-lifecycle.py -v
    python3 tools/test-testos-checkpoint-lifecycle.py
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
CHECKPOINT_TOOL = REPO_ROOT / "tools" / "rush-livedev-checkpoint.py"


def _run_checkpoint(args: list[str], env: dict | None = None, timeout: int = 10) -> tuple[int, str, str]:
    """Run the checkpoint tool with the given args and XDG_DATA_HOME."""
    e = os.environ.copy()
    if env:
        e.update(env)
    r = subprocess.run(
        [sys.executable, str(CHECKPOINT_TOOL)] + args,
        capture_output=True, text=True, timeout=timeout, env=e,
    )
    return r.returncode, r.stdout, r.stderr


def _setup_xdg(tmp_path: Path) -> dict:
    """Set up an isolated XDG_DATA_HOME for the test."""
    xdg = tmp_path / "xdg"
    xdg.mkdir(parents=True)
    return {"XDG_DATA_HOME": str(xdg)}


def _save_checkpoint(env: dict, run_id: str, phase: str, **extra) -> None:
    """Save a checkpoint with the given phase."""
    args = ["save", "--run-id", run_id, "--phase", phase]
    for k, v in extra.items():
        args.extend([f"--{k}", v])
    rc, out, err = _run_checkpoint(args, env=env)
    assert rc == 0, f"save failed: {err}"


# ─── Tests ──────────────────────────────────────────────────────────────────


def test_ensure_fresh_with_no_checkpoint_starts_fresh(tmp_path):
    """F3: With no existing checkpoint, ensure-fresh starts a fresh run."""
    env = _setup_xdg(tmp_path)
    rc, out, err = _run_checkpoint(["ensure-fresh"], env=env)
    assert rc == 0, f"ensure-fresh failed: {err}"
    # The last line is the new run_id.
    run_id = out.strip().split("\n")[-1]
    assert run_id.startswith("run-"), f"unexpected run_id: {run_id}"
    # The run directory should exist.
    rd = Path(env["XDG_DATA_HOME"]) / "rush-livedev" / "runs" / run_id
    assert rd.exists(), f"run directory not created: {rd}"


def test_ensure_fresh_with_nonterminal_checkpoint_resumes(tmp_path):
    """F3: With a non-terminal checkpoint (e.g. 'booted'), ensure-fresh
    resumes the existing run instead of starting fresh."""
    env = _setup_xdg(tmp_path)
    # Save a non-terminal checkpoint.
    _save_checkpoint(env, "run-test-001", "booted")
    rc, out, err = _run_checkpoint(["ensure-fresh"], env=env)
    assert rc == 0, f"ensure-fresh failed: {err}"
    # The last line should be the existing run_id.
    run_id = out.strip().split("\n")[-1]
    assert run_id == "run-test-001", (
        f"ensure-fresh should resume existing run, got: {run_id}"
    )


def test_ensure_fresh_with_terminal_checkpoint_starts_fresh(tmp_path):
    """F3: With a terminal (submitted) checkpoint, ensure-fresh auto-
    preserves it and starts a fresh run WITHOUT manual `clear`."""
    env = _setup_xdg(tmp_path)
    # Save a terminal checkpoint.
    _save_checkpoint(env, "run-test-001", "submitted", **{"pr-url": "https://github.com/Nan0pk/Rush-linux/pull/999"})
    # Verify the old run directory exists.
    old_rd = Path(env["XDG_DATA_HOME"]) / "rush-livedev" / "runs" / "run-test-001"
    assert old_rd.exists()
    # Write a marker file to prove prior data survives.
    (old_rd / "prior-pr-data.txt").write_text("this must survive\n")

    # Run ensure-fresh.
    rc, out, err = _run_checkpoint(["ensure-fresh"], env=env)
    assert rc == 0, f"ensure-fresh failed: {err}"
    # The output should mention "Terminal checkpoint detected".
    assert "Terminal checkpoint detected" in out, (
        f"ensure-fresh did not detect terminal checkpoint: {out}"
    )
    # The last line should be a NEW run_id, distinct from the old one.
    new_run_id = out.strip().split("\n")[-1]
    assert new_run_id != "run-test-001", (
        f"ensure-fresh did not create a new run_id: {new_run_id}"
    )
    assert new_run_id.startswith("run-"), f"unexpected run_id: {new_run_id}"

    # The old run directory must still exist (prior data preserved).
    assert old_rd.exists(), "old run directory was erased!"
    assert (old_rd / "prior-pr-data.txt").exists(), "prior PR data was erased!"

    # The new run directory should exist.
    new_rd = Path(env["XDG_DATA_HOME"]) / "rush-livedev" / "runs" / new_run_id
    assert new_rd.exists(), f"new run directory not created: {new_rd}"

    # The checkpoint should now point to the new run (or be cleared).
    rc2, out2, err2 = _run_checkpoint(["load"], env=env)
    # After ensure-fresh, the checkpoint pointer is cleared (the new run
    # has not been saved yet). load should return null / exit 1.
    assert rc2 != 0 or "null" in out2, (
        f"checkpoint pointer was not cleared after ensure-fresh: {out2}"
    )


def test_two_run_integration(tmp_path):
    """F3: Literal two-run integration coverage.

    Run 1: start fresh, progress to 'submitted'.
    Run 2: ensure-fresh detects the terminal checkpoint, preserves run 1,
    starts a fresh run 2 with a new run_id and directory.
    """
    env = _setup_xdg(tmp_path)

    # ── Run 1 ──
    rc, out, err = _run_checkpoint(["ensure-fresh"], env=env)
    assert rc == 0, f"run 1 ensure-fresh failed: {err}"
    run1_id = out.strip().split("\n")[-1]
    assert run1_id.startswith("run-")

    # Write inventory + plan for run 1.
    run1_rd = Path(env["XDG_DATA_HOME"]) / "rush-livedev" / "runs" / run1_id
    inventory1 = run1_rd / "hardware-inventory.json"
    inventory1.write_text(json.dumps({"fingerprint": "host-001"}))
    plan1 = run1_rd / "plan.json"
    plan1.write_text(json.dumps({"plan_kind": "rush-autopilot-plan"}))

    # Progress run 1 through phases to 'submitted'.
    _save_checkpoint(env, run1_id, "preflight",
                     **{"inventory-path": str(inventory1), "plan-path": str(plan1)})
    _save_checkpoint(env, run1_id, "plan_ready",
                     **{"inventory-path": str(inventory1), "plan-path": str(plan1)})
    _save_checkpoint(env, run1_id, "submitted",
                     **{"pr-url": "https://github.com/Nan0pk/Rush-linux/pull/1001"})

    # Verify run 1 is terminal.
    rc, out, err = _run_checkpoint(["show"], env=env)
    assert "submitted" in out, f"run 1 not in submitted phase: {out}"

    # ── Run 2 ──
    # ensure-fresh should detect the terminal checkpoint and start fresh.
    rc, out, err = _run_checkpoint(["ensure-fresh"], env=env)
    assert rc == 0, f"run 2 ensure-fresh failed: {err}"
    assert "Terminal checkpoint detected" in out
    run2_id = out.strip().split("\n")[-1]
    assert run2_id != run1_id, (
        f"run 2 reuses run 1's run_id: {run2_id} == {run1_id}"
    )

    # Run 1's data must survive.
    assert run1_rd.exists(), "run 1 directory was erased!"
    assert inventory1.exists(), "run 1 inventory was erased!"
    assert plan1.exists(), "run 1 plan was erased!"

    # Run 2's directory must exist and be distinct.
    run2_rd = Path(env["XDG_DATA_HOME"]) / "rush-livedev" / "runs" / run2_id
    assert run2_rd.exists()
    assert run2_rd != run1_rd

    # Run 2 should be able to save a new checkpoint without downgrade errors.
    inventory2 = run2_rd / "hardware-inventory.json"
    inventory2.write_text(json.dumps({"fingerprint": "host-002"}))
    plan2 = run2_rd / "plan.json"
    plan2.write_text(json.dumps({"plan_kind": "rush-autopilot-plan"}))
    _save_checkpoint(env, run2_id, "preflight",
                     **{"inventory-path": str(inventory2), "plan-path": str(plan2)})

    # Verify run 2 is now the active checkpoint.
    rc, out, err = _run_checkpoint(["show"], env=env)
    assert run2_id in out, f"run 2 not active after save: {out}"
    assert "preflight" in out


def test_submitted_checkpoint_cannot_be_downgraded(tmp_path):
    """F3/F7: A submitted checkpoint cannot be downgraded to an earlier phase."""
    env = _setup_xdg(tmp_path)
    _save_checkpoint(env, "run-test-001", "submitted",
                     **{"pr-url": "https://github.com/Nan0pk/Rush-linux/pull/999"})
    # Attempt to downgrade to 'booted' should fail.
    rc, out, err = _run_checkpoint(
        ["save", "--run-id", "run-test-001", "--phase", "booted"], env=env
    )
    assert rc != 0, "downgrade from submitted should fail"
    assert "REFUSED" in err or "submitted" in err.lower(), (
        f"downgrade did not produce a REFUSED error: {err}"
    )


# ─── Main ───────────────────────────────────────────────────────────────────


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"] + sys.argv[1:]))
