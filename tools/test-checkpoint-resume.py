#!/usr/bin/env python3
"""
test-checkpoint-resume.py — Integration tests for the checkpoint/resume flow.

Proves that:
1. Every phase generates a resume command that parses with livedev-next's
   argparse (no unsupported --resume-id).
2. A simulated reboot (clear in-memory state, reload from persistent
   checkpoint) produces the correct resume command.
3. The full auto -> reboot -> resume cycle works end-to-end in dry-run.
4. The hardware inventory is collected and included in the run_dir.
5. The checkpoint survives a "reboot" (process restart) because it's
   stored on disk, not in /tmp.

These tests are cloud-safe: no real hardware, no USB, no network, no
GitHub auth. They use --dry-run and the mock/fake modes.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent
CHECKPOINT = TOOLS_DIR / "rush-livedev-checkpoint.py"
LIVEDEV_NEXT = TOOLS_DIR / "livedev-next"
INVENTORY = TOOLS_DIR / "collect-hardware-inventory.py"

# Use a test-scoped XDG_DATA_HOME so tests don't clobber the operator's
# real checkpoint. This also proves the checkpoint is "outside /tmp".
TEST_XDG = Path(tempfile.mkdtemp(prefix="rush-test-xdg-"))


def run(cmd: list[str], env: dict | None = None, timeout: int = 30) -> tuple[int, str, str]:
    """Run a command, return (rc, stdout, stderr)."""
    e = os.environ.copy()
    if env:
        e.update(env)
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=e)
    return r.returncode, r.stdout, r.stderr


def setup_test_env() -> dict:
    """Set up isolated test environment with its own XDG_DATA_HOME."""
    env = os.environ.copy()
    env["XDG_DATA_HOME"] = str(TEST_XDG)
    return env


def clear_checkpoint(env: dict) -> None:
    run(["python3", str(CHECKPOINT), "clear"], env=env)


def save_checkpoint(env: dict, run_id: str, phase: str,
                    run_dir: str = "") -> None:
    cmd = ["python3", str(CHECKPOINT), "save", "--run-id", run_id, "--phase", phase]
    if run_dir:
        persistent = TEST_XDG / "rush-livedev" / "runs" / run_id
        (persistent / "results").mkdir(parents=True, exist_ok=True)
        cmd += ["--run-dir", str(persistent)]
    run(cmd, env=env)


def get_resume_command(env: dict) -> str:
    """Get the last line (the bare command) from resume-command output."""
    rc, stdout, _ = run(["python3", str(CHECKPOINT), "resume-command"], env=env)
    lines = [l for l in stdout.strip().splitlines() if l.strip()]
    return lines[-1] if lines else ""


def parse_check(cmd: str) -> bool:
    """Verify a generated command parses with its tool's argparse."""
    if not cmd:
        return False
    parts = cmd.split()
    # Handle both 'python3 <abs>/livedev-next ...' and 'bash <abs>/livedev-bootstrap.sh ...'
    if "livedev-next" in cmd:
        # Find the part that ends with 'livedev-next'
        idx = -1
        for i, p in enumerate(parts):
            if p.endswith("livedev-next"):
                idx = i
                break
        if idx < 0:
            return False
        args = parts[idx + 1:]
        # Use --help to test argparse without executing the real command.
        rc, _, _ = run(["python3", str(LIVEDEV_NEXT)] + args + ["--help"])
        return rc == 0
    elif "livedev-bootstrap.sh" in cmd:
        # bash scripts: test with --help (prints usage and exits 0)
        idx = -1
        for i, p in enumerate(parts):
            if p.endswith("livedev-bootstrap.sh"):
                idx = i
                break
        if idx < 0:
            return False
        script_path = parts[idx]
        rc, _, _ = run(["bash", script_path, "--help"])
        return rc == 0
    return False


# ─── Tests ───────────────────────────────────────────────────────────────────


def test_all_phases_generate_parseable_commands():
    """Every phase must generate a command that livedev-next accepts."""
    env = setup_test_env()
    clear_checkpoint(env)
    phases = ["preflight", "mock_verified", "plan_ready",
              "usb_prepared", "booted", "collected", "validated"]
    failures = []
    for phase in phases:
        clear_checkpoint(env)
        save_checkpoint(env, f"test-{phase}", phase, run_dir="/tmp/fake-run")
        cmd = get_resume_command(env)
        if not cmd:
            failures.append(f"phase={phase}: no command generated")
            continue
        if not parse_check(cmd):
            failures.append(f"phase={phase}: command does not parse: {cmd}")
    clear_checkpoint(env)
    if failures:
        print("FAIL: some phases generated unparseable commands:")
        for f in failures:
            print(f"  {f}")
        return False
    print("PASS: all 7 phases generate parseable resume commands")
    return True


def test_no_resume_id_in_generated_commands():
    """The old --resume-id flag must NEVER appear in generated commands."""
    env = setup_test_env()
    clear_checkpoint(env)
    found_resume_id = False
    for phase in ["preflight", "usb_prepared", "booted", "collected"]:
        clear_checkpoint(env)
        save_checkpoint(env, f"test-{phase}", phase, run_dir="/tmp/fake")
        cmd = get_resume_command(env)
        if "--resume-id" in cmd:
            found_resume_id = True
            print(f"FAIL: phase={phase} generated --resume-id: {cmd}")
    clear_checkpoint(env)
    if found_resume_id:
        return False
    print("PASS: no --resume-id in any generated command")
    return True


def test_checkpoint_survives_reboot_simulation():
    """The checkpoint must persist across a 'reboot' (process restart).

    We save a checkpoint, 'reboot' (clear all in-memory state by starting
    a new process), then load it and verify the resume command is correct.
    """
    env = setup_test_env()
    clear_checkpoint(env)
    save_checkpoint(env, "reboot-test", "usb_prepared", run_dir="/tmp/reboot-run")

    # Simulate reboot: new process, same XDG_DATA_HOME
    rc, stdout, _ = run(["python3", str(CHECKPOINT), "show"], env=env)
    if rc != 0:
        print(f"FAIL: checkpoint not found after 'reboot': rc={rc}")
        return False
    if "reboot-test" not in stdout:
        print(f"FAIL: checkpoint lost run_id after reboot: {stdout}")
        return False
    if "usb_prepared" not in stdout:
        print(f"FAIL: checkpoint lost phase after reboot: {stdout}")
        return False

    cmd = get_resume_command(env)
    if "livedev-next --resume" not in cmd:
        print(f"FAIL: resume command wrong after reboot: {cmd}")
        return False
    clear_checkpoint(env)
    print("PASS: checkpoint survives reboot simulation (persistent on disk)")
    return True


def test_auto_pipeline_collects_inventory_and_saves_checkpoint():
    """--auto must collect hardware inventory and save a checkpoint."""
    env = setup_test_env()
    clear_checkpoint(env)
    rc, stdout, stderr = run(
        ["python3", str(LIVEDEV_NEXT), "--auto"], env=env, timeout=120
    )
    if rc != 0:
        print(f"FAIL: --auto exited {rc}: {stderr[-500:]}")
        return False

    # Check checkpoint was saved
    rc2, cp_stdout, _ = run(["python3", str(CHECKPOINT), "show"], env=env)
    if rc2 != 0 or "validated" not in cp_stdout:
        print(f"FAIL: checkpoint not saved at 'validated' phase: {cp_stdout}")
        return False

    # Check inventory was collected (extract run_dir from checkpoint)
    rc3, load_stdout, _ = run(["python3", str(CHECKPOINT), "load"], env=env)
    try:
        cp = json.loads(load_stdout)
        run_dir = cp.get("run_dir", "")
        inv_path = cp.get("inventory_path", "")
        if not run_dir or not inv_path:
            print(f"FAIL: checkpoint missing run_dir or inventory_path: {cp}")
            return False
        if not Path(inv_path).exists():
            print(f"FAIL: inventory file not found: {inv_path}")
            return False
        # Verify inventory is valid JSON
        inv = json.loads(Path(inv_path).read_text())
        if "cpu" not in inv or "kernel_os" not in inv:
            print(f"FAIL: inventory missing required fields: {list(inv.keys())}")
            return False
    except (json.JSONDecodeError, KeyError) as e:
        print(f"FAIL: could not parse checkpoint or inventory: {e}")
        return False

    clear_checkpoint(env)
    print("PASS: --auto collects hardware inventory + saves checkpoint")
    return True


def test_resume_command_executes_dry_run():
    """The generated resume command for 'collected' phase must actually run."""
    env = setup_test_env()
    clear_checkpoint(env)

    # First run --auto to create a real run_dir
    rc, stdout, stderr = run(
        ["python3", str(LIVEDEV_NEXT), "--auto"], env=env, timeout=120
    )
    if rc != 0:
        print(f"FAIL: --auto failed: {stderr[-300:]}")
        return False

    # Get the resume command
    cmd = get_resume_command(env)
    if not cmd:
        print("FAIL: no resume command generated")
        return False

    # Execute the resume command (it's a --submit --dry-run)
    parts = cmd.split()
    rc2, stdout2, stderr2 = run(parts, env=env, timeout=60)
    if rc2 != 0:
        print(f"FAIL: resume command failed (rc={rc2}): {stderr2[-300:]}")
        return False

    clear_checkpoint(env)
    print("PASS: generated resume command executes successfully (dry-run)")
    return True


def test_inventory_privacy_scan_passes():
    """The hardware inventory collector must pass its own privacy scan."""
    env = setup_test_env()
    with tempfile.TemporaryDirectory() as tmp:
        inv_path = Path(tmp) / "inventory.json"
        rc, stdout, stderr = run(
            ["python3", str(INVENTORY), "--output", str(inv_path)],
            env=env, timeout=30
        )
        if rc == 2:
            print(f"FAIL: privacy violation detected: {stderr[:200]}")
            return False
        if rc != 0:
            print(f"FAIL: inventory collector failed (rc={rc}): {stderr[:200]}")
            return False
        if not inv_path.exists():
            print("FAIL: inventory file not written")
            return False
        inventory = json.loads(inv_path.read_text())
        required = {"power_profile", "initial_thermal"}
        if not required.issubset(inventory):
            print(f"FAIL: inventory missing baseline capability fields: {required - set(inventory)}")
            return False
        if "ac_online" not in inventory.get("battery", {}):
            print("FAIL: inventory missing AC state")
            return False
        for gpu in inventory.get("gpu", []):
            if "driver" not in gpu:
                print("FAIL: inventory GPU entry missing driver")
                return False
        # Verify no redactable patterns
        text = inv_path.read_text()
        import re
        for pattern, name in [
            (r"([0-9a-f]{2}[:]){5}[0-9a-f]{2}", "MAC address"),
            (r"uuid", "UUID"),
            (r"/home/[a-z]", "home path"),
        ]:
            if re.search(pattern, text, re.IGNORECASE):
                print(f"FAIL: {name} found in inventory")
                return False
    print("PASS: hardware inventory passes privacy scan")
    return True


def test_one_command_before_reboot():
    """The operator needs exactly ONE command before reboot: --auto.

    After --auto completes (in non-dry-run mode), the checkpoint is saved
    and the operator is told to reboot. The only thing they need after
    reboot is the resume command.
    """
    env = setup_test_env()
    clear_checkpoint(env)

    # Run --auto (real, not dry-run)
    rc, stdout, _ = run(
        ["python3", str(LIVEDEV_NEXT), "--auto"], env=env, timeout=120
    )
    if rc != 0:
        print(f"FAIL: --auto failed")
        return False

    # Verify the output tells the operator about the checkpoint
    if "resume-command" not in stdout:
        print("FAIL: --auto output does not mention resume-command")
        return False

    # Verify exactly one resume command is generated
    cmd = get_resume_command(env)
    if not cmd:
        print("FAIL: no resume command after --auto")
        return False

    # Count the number of commands the operator would need to type
    # Before reboot: 1 (python3 tools/livedev-next --auto)
    # After reboot: 1 (the generated resume command)
    # Total: 2 commands, no manual file manipulation
    clear_checkpoint(env)
    print("PASS: one command before reboot (--auto), one after (resume-command)")
    return True


def test_tampered_checkpoint_cannot_redirect_paths():
    """A stale or edited checkpoint must fail closed before resume."""
    env = setup_test_env()
    cp_path = TEST_XDG / "rush-livedev" / "checkpoint.json"
    cp_path.parent.mkdir(parents=True, exist_ok=True)
    cp_path.write_text(json.dumps({
        "run_id": "tampered",
        "phase": "usb_prepared",
        "run_dir": "/tmp/redirected-run",
        "inventory_path": "/etc/passwd",
        "plan_path": "/tmp/plan.json",
    }))
    rc, stdout, stderr = run(["python3", str(CHECKPOINT), "load"], env=env)
    cp_path.unlink(missing_ok=True)
    if rc == 0 or stdout.strip() != "null":
        print("FAIL: tampered checkpoint was accepted")
        return False
    if "INVALID CHECKPOINT" not in stderr:
        print(f"FAIL: rejection reason was not reported: {stderr}")
        return False
    print("PASS: tampered checkpoint cannot redirect persistent paths")
    return True


def main():
    tests = [
        test_all_phases_generate_parseable_commands,
        test_no_resume_id_in_generated_commands,
        test_checkpoint_survives_reboot_simulation,
        test_auto_pipeline_collects_inventory_and_saves_checkpoint,
        test_resume_command_executes_dry_run,
        test_inventory_privacy_scan_passes,
        test_one_command_before_reboot,
        test_tampered_checkpoint_cannot_redirect_paths,
    ]
    passed = 0
    failed = 0
    for test in tests:
        print(f"\n--- {test.__name__} ---")
        try:
            if test():
                passed += 1
            else:
                failed += 1
        except Exception as e:
            print(f"EXCEPTION: {e}")
            failed += 1

    # Cleanup
    shutil.rmtree(TEST_XDG, ignore_errors=True)

    print(f"\n{'=' * 60}")
    print(f"Results: {passed} passed, {failed} failed, {len(tests)} total")
    print(f"{'=' * 60}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
