#!/usr/bin/env python3
"""Regression tests for idempotent LiveDev checkpoint retries."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest


REPO_ROOT = Path(__file__).resolve().parent.parent
CHECKPOINT_TOOL = REPO_ROOT / "tools" / "rush-livedev-checkpoint.py"


def _run(
    args: list[str], env: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(CHECKPOINT_TOOL), *args],
        capture_output=True,
        text=True,
        env=env,
        timeout=10,
        check=False,
    )


def _env(tmp_path: Path) -> dict[str, str]:
    env = os.environ.copy()
    env["XDG_DATA_HOME"] = str(tmp_path / "xdg")
    return env


def _run_dir(env: dict[str, str], run_id: str) -> Path:
    run_dir = (
        Path(env["XDG_DATA_HOME"])
        / "rush-livedev"
        / "runs"
        / run_id
    )
    run_dir.mkdir(parents=True, exist_ok=True)
    (run_dir / "results").mkdir(exist_ok=True)
    return run_dir


def _save(
    env: dict[str, str],
    run_id: str,
    phase: str,
    *,
    inventory: Path | None = None,
    plan: Path | None = None,
    pr_url: str = "",
) -> subprocess.CompletedProcess[str]:
    run_dir = _run_dir(env, run_id)
    args = [
        "save",
        "--run-id",
        run_id,
        "--phase",
        phase,
        "--run-dir",
        str(run_dir),
    ]
    if inventory is not None:
        args.extend(["--inventory-path", str(inventory)])
    if plan is not None:
        args.extend(["--plan-path", str(plan)])
    if pr_url:
        args.extend(["--pr-url", pr_url])
    return _run(args, env)


def _load(env: dict[str, str]) -> dict:
    result = _run(["load"], env)
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)


def test_same_run_replayed_preflight_is_idempotent(tmp_path: Path) -> None:
    """The real failure: plan_ready -> replayed preflight must not abort."""
    env = _env(tmp_path)
    run_id = "auto-20260729-112953"
    run_dir = _run_dir(env, run_id)
    inventory = run_dir / "hardware-inventory.json"
    plan = run_dir / "plan.json"
    inventory.write_text("{}\n")
    plan.write_text("{}\n")

    first = _save(env, run_id, "preflight", inventory=inventory)
    assert first.returncode == 0, first.stderr

    planned = _save(
        env,
        run_id,
        "plan_ready",
        inventory=inventory,
        plan=plan,
    )
    assert planned.returncode == 0, planned.stderr
    before = _load(env)

    replay = _save(env, run_id, "preflight", inventory=inventory)
    assert replay.returncode == 0, replay.stderr
    assert "Checkpoint unchanged" in replay.stdout
    assert "ignored repeated earlier phase=preflight" in replay.stdout

    after = _load(env)
    assert after["phase"] == "plan_ready"
    assert after["run_id"] == run_id
    assert after["plan_path"] == str(plan)
    assert after["inventory_path"] == str(inventory)
    assert after["saved_at"] == before["saved_at"]


def test_cross_run_downgrade_remains_blocked(tmp_path: Path) -> None:
    """A different run_id cannot replace later active state as a retry."""
    env = _env(tmp_path)
    old_run = "active-run"
    old_dir = _run_dir(env, old_run)
    inventory = old_dir / "hardware-inventory.json"
    plan = old_dir / "plan.json"
    inventory.write_text("{}\n")
    plan.write_text("{}\n")

    assert _save(
        env,
        old_run,
        "plan_ready",
        inventory=inventory,
        plan=plan,
    ).returncode == 0

    result = _save(env, "different-run", "preflight")
    assert result.returncode != 0
    assert "REFUSED" in result.stderr
    checkpoint = _load(env)
    assert checkpoint["run_id"] == old_run
    assert checkpoint["phase"] == "plan_ready"


def test_submitted_checkpoint_remains_terminal(tmp_path: Path) -> None:
    """Submitted runs never become idempotent retry candidates."""
    env = _env(tmp_path)
    run_id = "submitted-run"
    submitted = _save(
        env,
        run_id,
        "submitted",
        pr_url="https://github.com/Nan0pk/Rush-linux/pull/999",
    )
    assert submitted.returncode == 0, submitted.stderr

    retry = _save(env, run_id, "preflight")
    assert retry.returncode != 0
    assert "submitted" in retry.stderr.lower()
    assert _load(env)["phase"] == "submitted"


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-v", *sys.argv[1:]]))
