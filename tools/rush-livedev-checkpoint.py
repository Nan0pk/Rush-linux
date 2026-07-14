#!/usr/bin/env python3
"""
rush-livedev-checkpoint.py — persistent resumable state for the LiveDev
hardware-evidence workflow.

State is stored OUTSIDE /tmp (which is cleared on reboot) so the workflow
can resume after a reboot. The default location is
~/.local/share/rush-livedev/checkpoint.json (XDG_DATA_HOME), which
persists across reboots but is user-scoped and not system-wide.

NEVER store authentication tokens in the checkpoint. The checkpoint only
holds non-secret resumable state: run_id, phase, plan path, run_dir,
and hardware inventory path.

Usage:
    python3 tools/rush-livedev-checkpoint.py save --run-id <id> --phase <phase> [options]
    python3 tools/rush-livedev-checkpoint.py load
    python3 tools/rush-livedev-checkpoint.py show
    python3 tools/rush-livedev-checkpoint.py clear
    python3 tools/rush-livedev-checkpoint.py resume-command

Phases:
    preflight    — preflight checks done
    mock_verified — mock tests passed
    plan_ready   — plan generated
    usb_prepared — USB written, ready to boot
    booted       — owner has booted the USB (checkpoint before reboot)
    collected    — results collected from USB after reboot
    validated    — results validated
    submitted    — evidence PR submitted
"""

import argparse
import json
import os
import sys
from pathlib import Path
from datetime import datetime, timezone


def checkpoint_dir() -> Path:
    """Return the persistent checkpoint directory (survives reboot)."""
    xdg_data = os.environ.get("XDG_DATA_HOME")
    if xdg_data:
        base = Path(xdg_data)
    else:
        home = os.environ.get("HOME", "/tmp")
        base = Path(home) / ".local" / "share"
    cp_dir = base / "rush-livedev"
    cp_dir.mkdir(parents=True, exist_ok=True)
    return cp_dir


def checkpoint_path() -> Path:
    return checkpoint_dir() / "checkpoint.json"


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


def show_checkpoint() -> None:
    cp = load_checkpoint()
    if cp is None:
        print("[NO CHECKPOINT] No resumable state found.")
        print("Start a new run: python3 tools/livedev-next --auto")
        return
    print("=== Rush LiveDev Checkpoint ===")
    print(f"  Run ID:    {cp.get('run_id', '?')}")
    print(f"  Phase:     {cp.get('phase', '?')}")
    print(f"  Saved at:  {cp.get('saved_at', '?')}")
    print(f"  Plan:      {cp.get('plan_path', 'not set')}")
    print(f"  Run dir:   {cp.get('run_dir', 'not set')}")
    print(f"  Inventory: {cp.get('inventory_path', 'not set')}")
    print(f"  Branch:    {cp.get('branch', 'not set')}")
    print()
    print("Resume with:")
    print(f"  python3 tools/rush-livedev-checkpoint.py resume-command")


def resume_command() -> None:
    """Print the exact command to resume the current run."""
    cp = load_checkpoint()
    if cp is None:
        print("[NO CHECKPOINT] Nothing to resume.")
        print("Start a new run: python3 tools/livedev-next --auto")
        return
    phase = cp.get("phase", "")
    run_id = cp.get("run_id", "")
    if phase in ("preflight", "mock_verified", "plan_ready"):
        print(f"# Resume: continue from phase '{phase}'")
        print(f"python3 tools/livedev-next --auto --resume-id {run_id}")
    elif phase == "usb_prepared":
        print("# Resume: USB is prepared. Boot the test machine from USB.")
        print("# After the test machine reboots back to its host OS, run:")
        print(f"python3 tools/livedev-next --resume --resume-id {run_id}")
    elif phase == "booted":
        print("# Resume: collect results from USB after reboot.")
        print(f"python3 tools/livedev-next --resume --resume-id {run_id}")
    elif phase == "collected":
        run_dir = cp.get("run_dir", "")
        print("# Resume: validate and submit the collected results.")
        print(f"python3 tools/livedev-next --submit {run_dir} --dry-run")
        print("# For real submission (opens a PR, no auto-merge):")
        print(f"python3 tools/livedev-next --submit {run_dir}")
    elif phase == "validated":
        run_dir = cp.get("run_dir", "")
        print("# Resume: submit the validated evidence.")
        print(f"python3 tools/livedev-next --submit {run_dir} --dry-run")
        print("# For real submission:")
        print(f"python3 tools/livedev-next --submit {run_dir}")
    elif phase == "submitted":
        print(f"# Already submitted. PR URL: {cp.get('pr_url', 'unknown')}")
        print("# To start a new run, clear the checkpoint first:")
        print("  python3 tools/rush-livedev-checkpoint.py clear")
    else:
        print(f"# Unknown phase '{phase}'. Inspect the checkpoint:")
        print("  python3 tools/rush-livedev-checkpoint.py show")


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

    args = parser.parse_args()

    if args.command == "save":
        data = {
            "run_id": args.run_id,
            "phase": args.phase,
            "plan_path": args.plan_path,
            "run_dir": args.run_dir,
            "inventory_path": args.inventory_path,
            "branch": args.branch,
            "pr_url": args.pr_url,
        }
        save_checkpoint(data)
        print(f"Checkpoint saved: phase={args.phase} run_id={args.run_id}")
        print(f"Location: {checkpoint_path()}")
    elif args.command == "load":
        cp = load_checkpoint()
        if cp:
            print(json.dumps(cp, indent=2))
        else:
            print("null")
            sys.exit(1)
    elif args.command == "show":
        show_checkpoint()
    elif args.command == "clear":
        clear_checkpoint()
    elif args.command == "resume-command":
        resume_command()


if __name__ == "__main__":
    main()
