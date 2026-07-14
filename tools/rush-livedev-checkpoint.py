#!/usr/bin/env python3
"""
rush-livedev-checkpoint.py — persistent resumable state for the LiveDev
hardware-evidence workflow.

State is stored OUTSIDE /tmp (which is cleared on reboot) so the workflow
can resume after a reboot. The default location is
${XDG_DATA_HOME:-$HOME/.local/share}/rush-livedev/ which persists across
reboots but is user-scoped and not system-wide.

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


def checkpoint_dir() -> Path:
    """Return the persistent checkpoint directory (survives reboot)."""
    cp_dir = _xdg_data_home() / "rush-livedev"
    cp_dir.mkdir(parents=True, exist_ok=True)
    return cp_dir


def runs_dir() -> Path:
    """Return the persistent runs directory (survives reboot)."""
    rd = checkpoint_dir() / "runs"
    rd.mkdir(parents=True, exist_ok=True)
    return rd


def checkpoint_path() -> Path:
    return checkpoint_dir() / "checkpoint.json"


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
        return json.loads(cp.read_text())
    except (json.JSONDecodeError, OSError):
        return None


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
    bootstrap = repo / "tools" / "livedev-bootstrap.sh"

    # All commands use absolute paths so they work from any CWD.
    # For --submit, use the run_dir from the checkpoint (which is absolute
    # and persistent under XDG_DATA_HOME, not /tmp).
    commands = {
        "preflight":     f"bash {bootstrap} --auto",
        "mock_verified":  f"bash {bootstrap} --auto",
        "plan_ready":     f"bash {bootstrap} --auto",
        "usb_prepared":   f"python3 {livedev_next} --resume",
        "booted":         f"python3 {livedev_next} --resume",
        "collected":      (
            f"python3 {livedev_next} --submit {run_dir} --dry-run"
            if run_dir else f"python3 {livedev_next} --resume"
        ),
        "validated":      (
            f"python3 {livedev_next} --submit {run_dir} --dry-run"
            if run_dir else f"python3 {livedev_next} --resume"
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
            print(f"python3 {livedev_next} --submit {run_dir}")

    print(cmd)


def clear_checkpoint() -> None:
    cp = checkpoint_path()
    if cp.exists():
        cp.unlink()
        print(f"Cleared: {cp}")
    else:
        print("No checkpoint to clear.")


def main():
    parser = argparse.ArgumentParser(
        description="Persistent resumable state for Rush LiveDev"
    )
    sub = parser.add_subparsers(dest="command", required=True)

    p_init = sub.add_parser("init-run", help="Create a persistent run directory")
    p_init.add_argument("--run-id", default="",
                        help="Run ID (auto-generated if empty)")

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
    sub.add_parser("clear", help="Delete the checkpoint")
    sub.add_parser("resume-command", help="Print the exact resume command")
    sub.add_parser("run-dir", help="Print the persistent run directory for the current checkpoint")

    args = parser.parse_args()

    if args.command == "init-run":
        run_id = args.run_id
        if not run_id:
            run_id = datetime.now(timezone.utc).strftime("run-%Y%m%d-%H%M%S")
        rd = run_dir_for(run_id)
        print(str(rd))
        return

    if args.command == "save":
        # Ensure the run directory exists (persistent, outside /tmp)
        rd = run_dir_for(args.run_id)
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
