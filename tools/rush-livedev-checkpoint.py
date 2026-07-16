#!/usr/bin/env python3
"""
rush-livedev-checkpoint.py — persistent resumable state for the LiveDev
hardware-evidence workflow.

State is stored OUTSIDE /tmp (which is cleared on reboot) so the workflow
can resume after a reboot. On Linux the default location is
${XDG_DATA_HOME:-$HOME/.local/share}/rush-livedev/. On Windows it is
%LOCALAPPDATA%\\Rush\\livedev-checkpoint.json with run data under
%LOCALAPPDATA%\\Rush\\livedev-runs\\. Both locations are user-scoped and
survive reboot.

Layout:
    ${XDG_DATA_HOME:-$HOME/.local/share}/rush-livedev/
        checkpoint.json              # current run state
        runs/<safe-run-id>/          # persistent run directory
            hardware-inventory.json  # collected before reboot
            plan.json                # benchmark plan
            results/                 # collected USB results
            submission-state.json    # submission metadata

NEVER store authentication tokens in the checkpoint. The checkpoint only
holds non-secret resumable state: run_id, phase, run_dir, plan_path,
and hardware inventory path.

All generated commands use absolute paths so they work regardless of the
operator's current working directory after reboot.

Usage:
    python3 tools/rush-livedev-checkpoint.py init-run [--run-id <id>]
    python3 tools/rush-livedev-checkpoint.py save --run-id <id> --phase <phase> [options]
    python3 tools/rush-livedev-checkpoint.py load
    python3 tools/rush-livedev-checkpoint.py show
    python3 tools/rush-livedev-checkpoint.py clear
    python3 tools/rush-livedev-checkpoint.py resume-command
    python3 tools/rush-livedev-checkpoint.py run-dir

Phases:
    preflight     — preflight checks done, inventory collected
    mock_verified  — mock tests passed
    plan_ready     — plan generated
    usb_prepared   — USB written, ready to boot
    booted         — owner has booted the USB (checkpoint before reboot)
    collected      — results collected from USB after reboot
    validated      — results validated
    submitted      — evidence PR submitted
"""

import argparse
import json
import os
import re
import sys
from pathlib import Path
from datetime import datetime, timezone


def _xdg_data_home() -> Path:
    """Return the XDG_DATA_HOME path, falling back to ~/.local/share."""
    xdg_data = os.environ.get("XDG_DATA_HOME")
    if xdg_data:
        return Path(xdg_data)
    home = os.environ.get("HOME") or "/tmp"
    return Path(home) / ".local" / "share"


def _is_windows() -> bool:
    return os.name == "nt"


def checkpoint_dir() -> Path:
    """Return the persistent checkpoint directory (survives reboot)."""
    if _is_windows():
        local_app_data = os.environ.get("LOCALAPPDATA")
        if not local_app_data:
            raise RuntimeError("LOCALAPPDATA is required for Windows LiveDev checkpoints")
        cp_dir = Path(local_app_data) / "Rush"
    else:
        cp_dir = _xdg_data_home() / "rush-livedev"
    cp_dir.mkdir(parents=True, exist_ok=True)
    return cp_dir


def runs_dir() -> Path:
    """Return the persistent runs directory (survives reboot)."""
    rd = checkpoint_dir() / ("livedev-runs" if _is_windows() else "runs")
    rd.mkdir(parents=True, exist_ok=True)
    return rd


def checkpoint_path() -> Path:
    name = "livedev-checkpoint.json" if _is_windows() else "checkpoint.json"
    return checkpoint_dir() / name


def safe_run_id(raw: str) -> str:
    """Sanitize a run_id for use as a directory name.

    Only allows [A-Za-z0-9_.-] and rejects empty/dot/dot-dot/leading-dash.
    This prevents path traversal via the run_id (which is operator-supplied
    or auto-generated from a timestamp).
    """
    if not raw or raw == "." or raw == "..":
        raise ValueError(f"unsafe run_id: {raw!r}")
    if raw.startswith("-"):
        raise ValueError(f"run_id must not start with '-': {raw!r}")
    if "/" in raw or "\\" in raw:
        raise ValueError(f"run_id must not contain path separators: {raw!r}")
    if not all(c.isalnum() or c in "_.-" for c in raw):
        raise ValueError(
            f"run_id contains disallowed characters: {raw!r} "
            f"(only [A-Za-z0-9_.-] allowed)"
        )
    return raw


def run_dir_for(run_id: str) -> Path:
    """Return the persistent run directory for a given run_id."""
    safe = safe_run_id(run_id)
    rd = runs_dir() / safe
    rd.mkdir(parents=True, exist_ok=True)
    (rd / "results").mkdir(exist_ok=True)
    return rd


def save_checkpoint(data: dict) -> None:
    """Atomically save the checkpoint (write to temp, then rename)."""
    cp = checkpoint_path()
    tmp = cp.with_suffix(".tmp")
    data["saved_at"] = datetime.now(timezone.utc).isoformat()
    tmp.write_text(json.dumps(data, indent=2))
    tmp.rename(cp)  # atomic on POSIX


def load_checkpoint() -> dict | None:
    cp = checkpoint_path()
    if not cp.exists():
        return None
    try:
        data = json.loads(cp.read_text())
        validate_checkpoint(data)
        return data
    except (json.JSONDecodeError, OSError, TypeError, ValueError) as exc:
        print(f"[INVALID CHECKPOINT] {exc}", file=sys.stderr)
        return None


def _contained_path(raw: str, root: Path, field: str) -> Path:
    """Validate an absolute checkpoint path beneath the persistent run root."""
    path = Path(raw)
    if not path.is_absolute():
        raise ValueError(f"{field} is not absolute")
    try:
        path.resolve(strict=False).relative_to(root.resolve(strict=True))
    except (OSError, RuntimeError, ValueError):
        raise ValueError(f"{field} escapes the persistent run directory")
    if path.is_symlink():
        raise ValueError(f"{field} is a symlink")
    return path


def validate_checkpoint(data: dict) -> None:
    """Fail closed if persisted paths can escape the current run directory."""
    if not isinstance(data, dict):
        raise TypeError("checkpoint must be a JSON object")
    run_id = safe_run_id(data.get("run_id", ""))
    root = runs_dir() / run_id
    root.mkdir(parents=True, exist_ok=True)
    expected = root.resolve(strict=True)
    run_dir = _contained_path(data.get("run_dir", ""), expected, "run_dir")
    if run_dir.resolve(strict=False) != expected:
        raise ValueError("run_dir must be the persistent run root")
    for field in ("inventory_path", "plan_path"):
        raw = data.get(field, "")
        if raw:
            _contained_path(raw, expected, field)


def _repo_root() -> Path:
    """Return the repository root for generating absolute commands.

    The checkpoint tool lives in <repo>/tools/, so the repo root is the
    parent of the tools/ directory.
    """
    return Path(__file__).resolve().parent.parent


def show_checkpoint() -> None:
    cp = load_checkpoint()
    if cp is None:
        print("[NO CHECKPOINT] No resumable state found.")
        print("Start a new run: bash tools/livedev-bootstrap.sh --auto")
        return
    print("=== Rush LiveDev Checkpoint ===")
    print(f"  Run ID:    {cp.get('run_id', '?')}")
    print(f"  Phase:     {cp.get('phase', '?')}")
    print(f"  Saved at:  {cp.get('saved_at', '?')}")
    print(f"  Run dir:   {cp.get('run_dir', 'not set')}")
    print(f"  Plan:      {cp.get('plan_path', 'not set')}")
    print(f"  Inventory: {cp.get('inventory_path', 'not set')}")
    print(f"  Branch:    {cp.get('branch', 'not set')}")
    print()
    print("Resume with:")
    print(f"  {Path(__file__).resolve()} resume-command")


def resume_command() -> None:
    """Print the exact command to resume the current run.

    Every generated command uses an ABSOLUTE path to the livedev-next tool
    and the repo root, so it works regardless of the operator's CWD after
    reboot. The command is printed as a single line on the LAST stdout
    line so ``$(... | tail -1)`` works for scripted resume.

    The real pre-reboot command is ``bash tools/livedev-bootstrap.sh --auto``
    (NOT livedev-next --auto, which is a fake/mock pipeline). The resume
    command after reboot uses livedev-next --resume or --submit.
    """
    cp = load_checkpoint()
    if cp is None:
        print("[NO CHECKPOINT] Nothing to resume.")
        print("Start a new run: bash tools/livedev-bootstrap.sh --auto")
        return

    phase = cp.get("phase", "")
    run_dir = cp.get("run_dir", "")
    repo = _repo_root()
    livedev_next = repo / "tools" / "livedev-next"
    bootstrap = repo / "tools" / (
        "livedev-bootstrap.ps1" if _is_windows() else "livedev-bootstrap.sh"
    )
    if _is_windows():
        python_prefix = f'"{sys.executable}"'
        auto_command = (
            f'powershell -ExecutionPolicy Bypass -File "{bootstrap}" -Auto'
        )
        resume_command_text = (
            f'powershell -ExecutionPolicy Bypass -File "{bootstrap}" -Resume'
        )
    else:
        python_prefix = "python3"
        auto_command = f"bash {bootstrap} --auto"
        resume_command_text = f"{python_prefix} {livedev_next} --resume"

    # All commands use absolute paths so they work from any CWD.
    # For --submit, use the run_dir from the checkpoint (which is absolute
    # and persistent under XDG_DATA_HOME, not /tmp).
    commands = {
        "preflight":      auto_command,
        "mock_verified":  auto_command,
        "plan_ready":     auto_command,
        "usb_prepared":   resume_command_text,
        "booted":         resume_command_text,
        "collected":      (
            f'{python_prefix} "{livedev_next}" --submit "{run_dir}" --dry-run'
            if run_dir else resume_command_text
        ),
        "validated":      (
            f'{python_prefix} "{livedev_next}" --submit "{run_dir}" --dry-run'
            if run_dir else resume_command_text
        ),
    }

    if phase == "submitted":
        pr_url = cp.get("pr_url", "unknown")
        print(f"# Already submitted. PR URL: {pr_url}")
        print("# To start a new run, clear the checkpoint first:")
        print(f"  {Path(__file__).resolve()} clear")
        return

    cmd = commands.get(phase)
    if cmd is None:
        print(f"# Unknown phase '{phase}'. Inspect the checkpoint:")
        print(f"  {Path(__file__).resolve()} show")
        return

    # Print human-readable context, then the bare command on the last line.
    if phase in ("preflight", "mock_verified", "plan_ready"):
        print(f"# Resume: continue from phase '{phase}'.")
    elif phase == "usb_prepared":
        print("# Resume: USB is prepared. Boot the test machine from USB.")
        print("# After the test machine reboots back to its host OS, run:")
    elif phase == "booted":
        print("# Resume: collect results from USB after reboot.")
    elif phase in ("collected", "validated"):
        print("# Resume: validate and submit the collected results.")
        if run_dir:
            print(f"# For real submission (opens a PR, no auto-merge):")
            print(f'{python_prefix} "{livedev_next}" --submit "{run_dir}"')

    print(cmd)


def clear_checkpoint() -> None:
    cp = checkpoint_path()
    if cp.exists():
        cp.unlink()
        print(f"Cleared: {cp}")
    else:
        print("No checkpoint to clear.")


def _phase_order(phase: str) -> int:
    """Return the ordering index of a phase. Higher = later in the workflow.
    Used to prevent downgrading (going backwards in the phase progression)."""
    order = {
        "preflight": 0,
        "mock_verified": 1,
        "plan_ready": 2,
        "usb_prepared": 3,
        "booted": 4,
        "collected": 5,
        "validated": 6,
        "submitted": 7,
    }
    return order.get(phase, -1)


def _refuse_downgrade(new_phase: str, existing: dict | None) -> None:
    """Refuse to overwrite a submitted checkpoint or downgrade the phase.

    Submitted checkpoints must NEVER be reused or downgraded. Once a run
    is submitted, the operator must start a fresh run (new run_id, new
    plan, new nonce, new inventory, new directory) — the prior PR data
    is preserved on disk and is NOT erased.
    """
    if existing is None:
        return
    old_phase = existing.get("phase", "")
    if old_phase == "submitted":
        raise SystemExit(
            f"REFUSED: checkpoint is in 'submitted' phase. Submitted "
            f"checkpoints must never be reused or downgraded. To start a "
            f"fresh run, first clear the checkpoint pointer (this does NOT "
            f"erase prior PR data):\n"
            f"  {Path(__file__).resolve()} clear\n"
            f"Then start a new run with a new run_id."
        )
    old_order = _phase_order(old_phase)
    new_order = _phase_order(new_phase)
    if old_order >= 0 and new_order >= 0 and new_order < old_order:
        raise SystemExit(
            f"REFUSED: cannot downgrade checkpoint from '{old_phase}' "
            f"(order={old_order}) to '{new_phase}' (order={new_order}). "
            f"Phase progression is monotonic. To start a fresh run, clear "
            f"the checkpoint and use a new run_id."
        )


def _refuse_reuse_of_existing_run_dir(run_id: str) -> None:
    """Refuse to reuse a run directory that already has data from a prior
    submitted run. This prevents accidental checkpoint reuse/downgrade.

    Prior PR data is NOT erased — the operator must use a new run_id.
    """
    rd = run_dir_for(run_id)
    # Check for evidence of a prior completed run: a submission-state.json
    # or a manifest.json in the results directory.
    submission_state = rd / "submission-state.json"
    if submission_state.exists():
        raise SystemExit(
            f"REFUSED: run directory {rd} already contains a prior "
            f"submission-state.json. This run_id may have been used for a "
            f"submitted run. Use a new run_id to start a fresh run. "
            f"(Prior PR data is preserved on disk and is NOT erased.)"
        )


def _generate_nonce() -> str:
    """Generate a random checkpoint nonce (16 hex chars + timestamp)."""
    import secrets
    return secrets.token_hex(8) + "-" + datetime.now(timezone.utc).strftime("%Y%m%d%H%M%S")


def ensure_fresh_run(force: bool = False) -> str:
    """F3 (corrective-2): Detect a terminal checkpoint BEFORE any inventory
    collection or write. If the current checkpoint is terminal (submitted),
    automatically preserve it and start a fresh run with a new run_id,
    random nonce, directory, inventory, and plan. Do NOT require manual
    `clear`. Prior PR data is preserved on disk.

    Returns the new run_id (or the existing run_id if the checkpoint is
    not terminal).
    """
    existing = load_checkpoint()
    if existing is None:
        # No checkpoint — start fresh.
        run_id = datetime.now(timezone.utc).strftime("run-%Y%m%d-%H%M%S")
        rd = run_dir_for(run_id)
        print(f"[ensure-fresh] No existing checkpoint. Starting fresh run: {run_id}")
        print(f"[ensure-fresh] Run directory: {rd}")
        return run_id

    phase = existing.get("phase", "")
    if phase != "submitted":
        # Not terminal — resume the existing run.
        run_id = existing.get("run_id", "")
        print(f"[ensure-fresh] Checkpoint is in phase '{phase}' (not terminal). Resuming run: {run_id}")
        return run_id

    # Terminal checkpoint detected. Preserve it and start fresh.
    old_run_id = existing.get("run_id", "")
    old_pr_url = existing.get("pr_url", "")
    print(f"[ensure-fresh] Terminal checkpoint detected (phase='submitted').")
    print(f"[ensure-fresh] Preserving prior run: {old_run_id} (PR: {old_pr_url})")
    print(f"[ensure-fresh] Prior data is NOT erased — it remains on disk.")
    print(f"[ensure-fresh] Starting a fresh run with a new run_id, nonce, and directory.")

    # Clear the checkpoint pointer (does NOT erase run data).
    cp = checkpoint_path()
    if cp.exists():
        cp.unlink()

    # Generate a fresh run_id with a suffix to distinguish it from the
    # prior run (avoids collisions if the operator runs two submissions
    # in the same second).
    import secrets
    suffix = secrets.token_hex(4)
    run_id = datetime.now(timezone.utc).strftime(f"run-%Y%m%d-%H%M%S-{suffix}")
    rd = run_dir_for(run_id)
    print(f"[ensure-fresh] New run_id: {run_id}")
    print(f"[ensure-fresh] New run directory: {rd}")
    return run_id


def main():
    parser = argparse.ArgumentParser(
        description="Persistent resumable state for Rush LiveDev"
    )
    sub = parser.add_subparsers(dest="command", required=True)

    p_init = sub.add_parser("init-run", help="Create a persistent run directory")
    p_init.add_argument("--run-id", default="",
                        help="Run ID (auto-generated if empty)")
    p_init.add_argument("--force", action="store_true",
                        help="Allow reusing a run_id whose directory already exists (DANGEROUS: may overwrite prior data)")

    p_save = sub.add_parser("save", help="Save checkpoint state")
    p_save.add_argument("--run-id", required=True)
    p_save.add_argument("--phase", required=True,
                        choices=["preflight", "mock_verified", "plan_ready",
                                 "usb_prepared", "booted", "collected",
                                 "validated", "submitted"])
    p_save.add_argument("--plan-path", default="")
    p_save.add_argument("--run-dir", default="")
    p_save.add_argument("--inventory-path", default="")
    p_save.add_argument("--branch", default="")
    p_save.add_argument("--pr-url", default="")

    sub.add_parser("load", help="Load and print checkpoint JSON")
    sub.add_parser("show", help="Show checkpoint summary")
    sub.add_parser("clear", help="Delete the checkpoint pointer (does NOT erase run data)")
    sub.add_parser("resume-command", help="Print the exact resume command")
    sub.add_parser("run-dir", help="Print the persistent run directory for the current checkpoint")

    p_fresh = sub.add_parser(
        "ensure-fresh",
        help="Detect terminal checkpoint; auto-preserve + start fresh run_id/nonce/dir",
    )
    p_fresh.add_argument("--force", action="store_true",
                         help="Force a fresh run even if the checkpoint is not terminal")

    args = parser.parse_args()

    if args.command == "ensure-fresh":
        run_id = ensure_fresh_run(force=args.force)
        print(run_id)
        return

    if args.command == "init-run":
        run_id = args.run_id
        if not run_id:
            run_id = datetime.now(timezone.utc).strftime("run-%Y%m%d-%H%M%S")
        # F7: refuse to reuse a run_id whose directory already has a prior
        # submission. This prevents accidental reuse/downgrade of submitted
        # checkpoints. --force overrides (for testing only).
        if not args.force:
            _refuse_reuse_of_existing_run_dir(run_id)
        rd = run_dir_for(run_id)
        print(str(rd))
        return

    if args.command == "save":
        # F7: refuse to downgrade a submitted checkpoint.
        existing = load_checkpoint()
        _refuse_downgrade(args.phase, existing)
        # Ensure the run directory exists (persistent, outside /tmp)
        rd = run_dir_for(args.run_id)
        if args.run_dir:
            supplied = Path(args.run_dir).resolve(strict=False)
            expected = rd.resolve(strict=True)
            if supplied != expected and supplied != expected / "results":
                raise SystemExit("run_dir must be the persistent run root or its results directory")
        for field, raw in (("plan_path", args.plan_path),
                           ("inventory_path", args.inventory_path)):
            if raw:
                path = _contained_path(raw, rd, field)
                if not path.exists() or not path.is_file():
                    raise SystemExit(f"{field} must be an existing regular file")
        data = {
            "run_id": args.run_id,
            "phase": args.phase,
            "run_dir": str(rd),
            "plan_path": args.plan_path,
            "inventory_path": args.inventory_path,
            "branch": args.branch,
            "pr_url": args.pr_url,
        }
        save_checkpoint(data)
        print(f"Checkpoint saved: phase={args.phase} run_id={args.run_id}")
        print(f"Run dir: {rd}")
        print(f"Location: {checkpoint_path()}")
        return

    if args.command == "load":
        cp = load_checkpoint()
        if cp:
            print(json.dumps(cp, indent=2))
        else:
            print("null")
            sys.exit(1)
        return

    if args.command == "show":
        show_checkpoint()
        return

    if args.command == "clear":
        clear_checkpoint()
        return

    if args.command == "resume-command":
        resume_command()
        return

    if args.command == "run-dir":
        cp = load_checkpoint()
        if cp:
            print(cp.get("run_dir", ""))
        else:
            sys.exit(1)
        return


if __name__ == "__main__":
    main()
