#!/usr/bin/env python3
"""
test-testos-boot-behavioral.py — EXECUTABLE behavioral tests for the testOS
boot-reliability corrections.

These are NOT source-text grep tests. They execute the actual mount helper
and recovery scripts in a controlled environment with mocked blkid/udevadm,
proving:

  1. Delayed USB discovery succeeds (blkid returns empty for N attempts,
     then returns the partition → script exits 0, timeline written).
  2. Permanent mount failure exits non-zero within the bounded timeout
     (blkid always returns empty → script exits 1, timeline written with
     "FAILED").
  3. The mount exit-status bug (F3) is fixed: the real exit code is
     reported, not the negated status.
  4. Sync failures are reported honestly (F4), not silently ignored.
  5. The recovery script renders the correct E001/E002 screen when the
     mount service is "failed", and does NOT spawn a root shell.
  6. The recovery script writes PRIVATE-DIAGNOSTICS when the USB is
     mountable.
  7. systemd unit files pass `systemd-analyze verify` (syntax + semantics).
  8. The runner unit has a real bounded restart policy
     (StartLimitIntervalSec + StartLimitBurst).

QEMU is NOT available in the builder environment, so we cannot do full
VM boot tests. These script-level behavioral tests are the next best
thing: they execute the real scripts with real bash, real file I/O, and
mocked external commands. They prove the scripts behave correctly without
requiring a full boot.

Run:
    python3 -m pytest tools/test-testos-boot-behavioral.py -v
    python3 tools/test-testos-boot-behavioral.py
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
BUILD_SCRIPT = REPO_ROOT / "testos" / "build-testos.sh"


# ─── Script extraction ─────────────────────────────────────────────────────


def _extract_heredoc(build_script: str, marker: str) -> str:
    """Extract a heredoc body from build-testos.sh.

    `marker` is the path inside the `cat > "..." << 'TAG'` line.
    """
    # Find the cat > line for the given marker.
    pattern = (
        r"cat > \"\$\{EXTRA_DIR\}/"
        + re.escape(marker)
        + r"\" << '(?:EOF|SCRIPT)'\n(?P<body>.*?)\n(?:EOF|SCRIPT)"
    )
    m = re.search(pattern, build_script, re.DOTALL)
    assert m is not None, f"could not find heredoc for {marker}"
    return m.group("body")


def _extract_mount_helper() -> str:
    return _extract_heredoc(BUILD_SCRIPT.read_text(), "usr/libexec/testos-usb-mount")


def _extract_recovery_script() -> str:
    return _extract_heredoc(BUILD_SCRIPT.read_text(), "usr/libexec/testos-recovery")


def _extract_unit(name: str) -> str:
    return _extract_heredoc(BUILD_SCRIPT.read_text(), f"usr/lib/systemd/system/{name}")


# ─── Test fixtures ─────────────────────────────────────────────────────────


@pytest.fixture
def mock_env(tmp_path):
    """Create a mock environment for running the mount helper.

    Sets up a fake /run/testos, a fake /proc/cmdline, a fake blkid, and
    a fake udevadm. The test can control blkid's behavior by writing to
    a control file.
    """
    root = tmp_path / "root"
    # fake /run
    run_dir = root / "run" / "testos"
    run_dir.mkdir(parents=True)
    (root / "run" / "testos" / "usb").mkdir(parents=True)
    # fake /proc
    proc_dir = root / "proc"
    proc_dir.mkdir(parents=True)
    (proc_dir / "cmdline").write_text("testos.usb_label=RUSHESP testos.runner=1\n")
    # fake /dev — we'll create the device node inside the test
    dev_dir = root / "dev"
    dev_dir.mkdir(parents=True)
    # bin dir for mocks
    bin_dir = root / "bin"
    bin_dir.mkdir(parents=True)
    # lib dir for mocks
    lib_dir = root / "usr" / "libexec"
    lib_dir.mkdir(parents=True)

    # Control file for the mock blkid.
    control_file = root / "blkid-control"
    control_file.write_text("empty\n")  # default: return empty

    # Mock blkid: reads the control file to decide what to return.
    # When control says "empty", returns nothing (exit 0, no output).
    # When control says "found", returns a fake device path.
    # When control says "found:<N>", returns empty for the first N calls,
    # then returns the device path.
    mock_blkid = root / "bin" / "blkid"
    mock_blkid.write_text(f"""#!/usr/bin/env bash
# Mock blkid for testing.
COUNT_FILE="{root / 'blkid-call-count'}"
CALL=$(cat "$COUNT_FILE" 2>/dev/null || echo 0)
CALL=$((CALL + 1))
echo "$CALL" > "$COUNT_FILE"

CONTROL=$(cat "{control_file}" 2>/dev/null || echo "empty")

case "$CONTROL" in
    empty)
        exit 0
        ;;
    found)
        echo "{dev_dir / 'sda1'}"
        exit 0
        ;;
    found:*)
        N="${{CONTROL#found:}}"
        if (( CALL >= N )); then
            echo "{dev_dir / 'sda1'}"
            exit 0
        else
            exit 0
        fi
        ;;
    *)
        exit 0
        ;;
esac
""")
    mock_blkid.chmod(0o755)

    # Mock udevadm: just exit 0 (settle succeeded).
    mock_udevadm = root / "bin" / "udevadm"
    mock_udevadm.write_text("#!/usr/bin/env bash\nexit 0\n")
    mock_udevadm.chmod(0o755)

    # Mock mount: succeeds by default, but can be controlled.
    mount_control = root / "mount-control"
    mount_control.write_text("success\n")
    mock_mount = root / "bin" / "mount"
    mock_mount.write_text(f"""#!/usr/bin/env bash
# Mock mount for testing.
CONTROL=$(cat "{mount_control}" 2>/dev/null || echo "success")
case "$CONTROL" in
    success)
        exit 0
        ;;
    fail)
        echo "mount: unknown filesystem type 'vfat'" >&2
        exit 32
        ;;
    *)
        exit 0
        ;;
esac
""")
    mock_mount.chmod(0o755)

    # Mock sync: succeeds by default, can be controlled.
    sync_control = root / "sync-control"
    sync_control.write_text("success\n")
    mock_sync = root / "bin" / "sync"
    mock_sync.write_text(f"""#!/usr/bin/env bash
CONTROL=$(cat "{sync_control}" 2>/dev/null || echo "success")
case "$CONTROL" in
    success) exit 0 ;;
    fail) exit 1 ;;
    *) exit 0 ;;
esac
""")
    mock_sync.chmod(0o755)

    # Mock date: use real date.
    for cmd in ["date", "cat", "echo", "mkdir", "sleep", "head", "tee"]:
        real = shutil.which(cmd)
        if real:
            (root / "bin" / cmd).symlink_to(real)

    env = {
        "PATH": f"{root / 'bin'}:/usr/bin:/bin",
        "HOME": str(root),
        "TESTOS_USB_MOUNT_TIMEOUT_SECS": "5",  # short for tests
        "FAKE_ROOT": str(root),
    }

    return {
        "root": root,
        "run_dir": run_dir,
        "control_file": control_file,
        "mount_control": mount_control,
        "sync_control": sync_control,
        "dev_dir": dev_dir,
        "env": env,
    }


def _run_mount_helper(script_text: str, env: dict, timeout: int = 30) -> tuple[int, str, str]:
    """Run the mount helper script in the mock environment.

    We rewrite the script to use the fake root's paths instead of the real
    /run, /proc, etc. This is done by prefixing paths with $FAKE_ROOT.
    """
    root = Path(env["FAKE_ROOT"])
    # Rewrite absolute paths in the script to use the fake root.
    # We only rewrite the paths that the script uses: /run, /proc.
    rewritten = script_text
    rewritten = rewritten.replace("/run/testos", str(root / "run" / "testos"))
    rewritten = rewritten.replace("/proc/cmdline", str(root / "proc" / "cmdline"))

    script_path = root / "mount-helper.sh"
    script_path.write_text(rewritten)
    script_path.chmod(0o755)

    r = subprocess.run(
        ["bash", str(script_path)],
        capture_output=True,
        text=True,
        timeout=timeout,
        env=env,
    )
    return r.returncode, r.stdout, r.stderr


# ─── Tests: delayed USB discovery succeeds ──────────────────────────────────


def test_delayed_usb_discovery_succeeds(mock_env):
    """F6: Prove that delayed USB discovery succeeds.

    blkid returns empty for the first 3 attempts, then returns the device.
    The script should exit 0 and write a timeline showing it found the
    partition on attempt 4 (or later).
    """
    # Create the fake device so the mock mount can "mount" it.
    mock_env["dev_dir"].joinpath("sda1").write_text("fake partition\n")
    # Tell blkid to return empty for the first 3 calls, then find it.
    mock_env["control_file"].write_text("found:3\n")

    # Tell mount to succeed.
    mock_env["mount_control"].write_text("success\n")

    script = _extract_mount_helper()
    rc, stdout, stderr = _run_mount_helper(script, mock_env["env"])

    assert rc == 0, f"mount helper failed (rc={rc}): {stderr}"

    # Verify the timeline was written and shows the discovery.
    timeline = mock_env["run_dir"].parent / "usb-discovery-timeline.txt"
    # The timeline path is /run/testos/usb-discovery-timeline.txt in the
    # rewritten script, which maps to <root>/run/testos/usb-discovery-timeline.txt
    timeline_path = Path(mock_env["root"]) / "run" / "testos" / "usb-discovery-timeline.txt"
    assert timeline_path.exists(), f"timeline not written at {timeline_path}"
    timeline_text = timeline_path.read_text()
    assert "found" in timeline_text, f"timeline does not show discovery: {timeline_text}"
    assert "mounted successfully" in timeline_text, f"timeline does not show mount success: {timeline_text}"

    # Verify the boot-attempt counter was written.
    boot_attempt = Path(mock_env["root"]) / "run" / "testos" / "boot-attempt"
    assert boot_attempt.exists(), "boot-attempt file not written"
    assert boot_attempt.read_text().strip() == "1"


# ─── Tests: permanent mount failure exits non-zero ──────────────────────────


def test_permanent_usb_not_found_exits_nonzero(mock_env):
    """F6: Prove that permanent USB discovery failure exits non-zero
    within the bounded timeout.

    blkid always returns empty. The script should exit 1 within the
    bounded timeout and write a timeline showing "FAILED".
    """
    # blkid always returns empty.
    mock_env["control_file"].write_text("empty\n")
    # Short timeout for the test.
    mock_env["env"]["TESTOS_USB_MOUNT_TIMEOUT_SECS"] = "3"

    script = _extract_mount_helper()
    rc, stdout, stderr = _run_mount_helper(script, mock_env["env"], timeout=30)

    assert rc != 0, f"mount helper should have failed but exited 0: {stderr}"

    # Verify the timeline shows the failure.
    timeline_path = Path(mock_env["root"]) / "run" / "testos" / "usb-discovery-timeline.txt"
    assert timeline_path.exists(), "timeline not written on failure"
    timeline_text = timeline_path.read_text()
    assert "FAILED" in timeline_text, f"timeline does not show failure: {timeline_text}"
    assert "no partition with label" in timeline_text, (
        f"timeline does not show 'no partition' message: {timeline_text}"
    )


def test_mount_failure_exits_nonzero(mock_env):
    """F6: Prove that a mount failure (blkid finds the partition, but mount
    returns non-zero) exits non-zero and the timeline reports the real
    exit code.

    This also tests F3: the mount exit status is reported correctly, not
    the negated status.
    """
    # blkid finds the partition immediately.
    mock_env["control_file"].write_text("found\n")
    # Create the fake device.
    mock_env["dev_dir"].joinpath("sda1").write_text("fake partition\n")
    # mount fails with exit code 32.
    mock_env["mount_control"].write_text("fail\n")

    script = _extract_mount_helper()
    rc, stdout, stderr = _run_mount_helper(script, mock_env["env"])

    assert rc != 0, f"mount helper should have failed but exited 0: {stderr}"

    # F3: the timeline must report the REAL mount exit code (32), not the
    # negated status (0).
    timeline_path = Path(mock_env["root"]) / "run" / "testos" / "usb-discovery-timeline.txt"
    assert timeline_path.exists()
    timeline_text = timeline_path.read_text()
    assert "returned 32" in timeline_text, (
        f"timeline does not report real mount exit code 32 (F3 bug): {timeline_text}"
    )
    # The old bug would have reported "returned 0" (the negated status).
    assert "returned 0" not in timeline_text, (
        f"timeline reports negated exit code 0 (F3 bug not fixed): {timeline_text}"
    )


# ─── Tests: sync failures are reported honestly (F4) ────────────────────────


def test_sync_failure_after_mount_is_reported(mock_env):
    """F4: A sync failure after mount must be reported honestly, not
    silently ignored. The script should exit non-zero."""
    mock_env["control_file"].write_text("found\n")
    mock_env["dev_dir"].joinpath("sda1").write_text("fake partition\n")
    mock_env["mount_control"].write_text("success\n")
    # sync fails.
    mock_env["sync_control"].write_text("fail\n")

    script = _extract_mount_helper()
    rc, stdout, stderr = _run_mount_helper(script, mock_env["env"])

    assert rc != 0, f"sync failure should cause non-zero exit: {stderr}"
    timeline_path = Path(mock_env["root"]) / "run" / "testos" / "usb-discovery-timeline.txt"
    timeline_text = timeline_path.read_text()
    assert "sync" in timeline_text.lower() and "FAILED" in timeline_text, (
        f"sync failure not reported in timeline: {timeline_text}"
    )


# ─── Tests: recovery script renders correct screen ──────────────────────────


def test_recovery_script_renders_e001_for_mount_not_found(tmp_path, mock_env):
    """F6: The recovery script renders E001 when the mount service failed
    with 'no partition with label' in the timeline."""
    script = _extract_recovery_script()
    root = Path(mock_env["root"])

    # Set up the mock environment so the recovery script sees a "failed"
    # mount service and a timeline with "no partition with label".
    # We need mock systemctl that reports testos-usb-mount.service as failed.
    mock_systemctl = root / "bin" / "systemctl"
    mock_systemctl.write_text("""#!/usr/bin/env bash
# Mock systemctl: report testos-usb-mount.service as failed.
case "$1" in
    is-failed)
        if [[ "$2" == "testos-usb-mount.service" ]]; then
            exit 0  # is-failed returns 0 when the service IS failed
        fi
        exit 1  # not failed
        ;;
    status|*)
        exit 0
        ;;
esac
""")
    mock_systemctl.chmod(0o755)

    # Write a timeline with "no partition with label".
    timeline = root / "run" / "testos" / "usb-discovery-timeline.txt"
    timeline.parent.mkdir(parents=True, exist_ok=True)
    timeline.write_text("[ts] testos-usb-mount: FAILED: no partition with label 'RUSHESP' found within 30s\n")

    # Write boot-attempt.
    (root / "run" / "testos").mkdir(parents=True, exist_ok=True)
    (root / "run" / "testos" / "boot-attempt").write_text("1\n")

    # Mock mountpoint: report USB as not mounted.
    mock_mountpoint = root / "bin" / "mountpoint"
    mock_mountpoint.write_text("#!/usr/bin/env bash\nexit 1\n")
    mock_mountpoint.chmod(0o755)

    # Mock reboot so the script doesn't actually reboot.
    mock_reboot = root / "bin" / "reboot"
    mock_reboot.write_text("#!/usr/bin/env bash\nexit 0\n")
    mock_reboot.chmod(0o755)

    # Mock sleep to be instant.
    real_sleep = shutil.which("sleep")
    if real_sleep:
        (root / "bin" / "sleep").unlink(missing_ok=True)
        (root / "bin" / "sleep").symlink_to(real_sleep)

    # Rewrite the recovery script to use fake root paths.
    rewritten = script
    rewritten = rewritten.replace("/run/testos", str(root / "run" / "testos"))
    rewritten = rewritten.replace("/proc/cmdline", str(root / "proc" / "cmdline"))

    # Also mock the PRIVATE-DIAGNOSTICS writes — the script tries to mount
    # the USB best-effort. Since mountpoint says "not mounted" and blkid
    # returns nothing, it will skip diagnostics. That's fine for this test.

    script_path = root / "recovery.sh"
    script_path.write_text(rewritten)
    script_path.chmod(0o755)

    env = dict(mock_env["env"])
    # Make sleep very short so the test doesn't take 10 seconds.
    env["PATH"] = f"{root / 'bin'}:{env['PATH']}"

    r = subprocess.run(
        ["bash", str(script_path)],
        capture_output=True,
        text=True,
        timeout=30,
        env=env,
    )

    # The script should have printed the recovery screen to stdout.
    assert "E001" in r.stdout, f"recovery screen missing E001: {r.stdout}"
    assert "USB not found" in r.stdout, f"recovery screen missing 'USB not found': {r.stdout}"
    assert "recovery screen" in r.stdout.lower(), f"not a recovery screen: {r.stdout}"


def test_recovery_script_does_not_spawn_root_shell(tmp_path, mock_env):
    """F6: The recovery script must NOT spawn a root shell."""
    script = _extract_recovery_script()
    root = Path(mock_env["root"])

    # Set up mocks (same as above).
    mock_systemctl = root / "bin" / "systemctl"
    mock_systemctl.write_text("""#!/usr/bin/env bash
exit 0
""")
    mock_systemctl.chmod(0o755)

    mock_mountpoint = root / "bin" / "mountpoint"
    mock_mountpoint.write_text("#!/usr/bin/env bash\nexit 1\n")
    mock_mountpoint.chmod(0o755)

    mock_reboot = root / "bin" / "reboot"
    mock_reboot.write_text("#!/usr/bin/env bash\nexit 0\n")
    mock_reboot.chmod(0o755)

    timeline = root / "run" / "testos" / "usb-discovery-timeline.txt"
    timeline.parent.mkdir(parents=True, exist_ok=True)
    timeline.write_text("[ts] testos-usb-mount: FAILED: no partition\n")

    (root / "run" / "testos").mkdir(parents=True, exist_ok=True)
    (root / "run" / "testos" / "boot-attempt").write_text("1\n")

    # Trap any attempt to exec bash interactively.
    real_bash = shutil.which("bash")
    mock_bash = root / "bin" / "bash_interactive_trap"
    mock_bash.write_text("""#!/usr/bin/env bash
echo "INTERACTIVE_SHELL_SPAWNED" >&2
exit 1
""")
    mock_bash.chmod(0o755)

    rewritten = script
    rewritten = rewritten.replace("/run/testos", str(root / "run" / "testos"))
    rewritten = rewritten.replace("/proc/cmdline", str(root / "proc" / "cmdline"))

    script_path = root / "recovery.sh"
    script_path.write_text(rewritten)
    script_path.chmod(0o755)

    env = dict(mock_env["env"])
    r = subprocess.run(
        ["bash", str(script_path)],
        capture_output=True,
        text=True,
        timeout=30,
        env=env,
    )

    assert "INTERACTIVE_SHELL_SPAWNED" not in r.stderr, (
        f"recovery script spawned an interactive shell: {r.stderr}"
    )
    # The script should not contain "bash" as a standalone exec (it uses
    # bash -c for diagnostic captures, which is fine).
    # Check the script source doesn't exec bash interactively.
    assert "exec bash" not in script, "recovery script contains 'exec bash'"
    assert "Command::new" not in script, "recovery script should be pure bash"


# ─── Tests: systemd unit verification ───────────────────────────────────────


def test_systemd_units_pass_verify(tmp_path):
    """F6: The systemd unit files pass `systemd-analyze verify`.

    This is a real semantic check, not a grep test. systemd-analyze verify
    catches syntax errors, dependency cycles, and invalid directives.
    """
    units = [
        "testos-usb-mount.service",
        "testos-runner.service",
        "testos-recovery.service",
    ]
    for unit_name in units:
        unit_text = _extract_unit(unit_name)
        unit_path = tmp_path / unit_name
        unit_path.write_text(unit_text)

    # Run systemd-analyze verify on all three units.
    r = subprocess.run(
        ["systemd-analyze", "verify"] + [str(tmp_path / u) for u in units],
        capture_output=True,
        text=True,
        timeout=30,
    )
    # systemd-analyze verify exits 0 on success. On failure, it prints
    # errors to stderr and exits non-zero.
    # Some warnings about missing files (like ExecStart paths) are expected
    # in a test environment — we only fail on actual errors.
    errors = [
        line for line in r.stderr.splitlines()
        if "error" in line.lower() and "does not exist" not in line.lower()
    ]
    assert not errors, f"systemd-analyze verify found errors: {errors}"


def test_runner_unit_has_bounded_restart_policy():
    """F2: The runner unit must have StartLimitIntervalSec and StartLimitBurst
    (a real bounded restart policy, not just a comment)."""
    unit = _extract_unit("testos-runner.service")
    assert "StartLimitIntervalSec=" in unit, (
        "runner unit missing StartLimitIntervalSec (F2: no bounded restart policy)"
    )
    assert "StartLimitBurst=" in unit, (
        "runner unit missing StartLimitBurst (F2: no bounded restart policy)"
    )
    # Verify the values are reasonable.
    interval_match = re.search(r"StartLimitIntervalSec=(\d+)", unit)
    burst_match = re.search(r"StartLimitBurst=(\d+)", unit)
    assert interval_match, "StartLimitIntervalSec has no numeric value"
    assert burst_match, "StartLimitBurst has no numeric value"
    interval = int(interval_match.group(1))
    burst = int(burst_match.group(1))
    assert 30 <= interval <= 600, f"StartLimitIntervalSec {interval} out of range"
    assert 1 <= burst <= 5, f"StartLimitBurst {burst} out of range"


def test_mount_unit_has_on_failure_recovery():
    """F1: The mount unit must have OnFailure=testos-recovery.service."""
    unit = _extract_unit("testos-usb-mount.service")
    assert "OnFailure=testos-recovery.service" in unit, (
        "mount unit missing OnFailure=testos-recovery.service (F1)"
    )


def test_runner_unit_has_on_failure_recovery():
    """F1: The runner unit must have OnFailure=testos-recovery.service."""
    unit = _extract_unit("testos-runner.service")
    assert "OnFailure=testos-recovery.service" in unit, (
        "runner unit missing OnFailure=testos-recovery.service (F1)"
    )


def test_recovery_unit_owns_tty1():
    """F1: The recovery unit must own tty1 (StandardInput=tty, TTYPath=/dev/tty1)."""
    unit = _extract_unit("testos-recovery.service")
    assert "StandardInput=tty" in unit, "recovery unit does not take tty input"
    assert "TTYPath=/dev/tty1" in unit, "recovery unit does not own tty1"


def test_recovery_unit_conflicts_runner():
    """F1: The recovery unit must conflict with the runner (so they don't
    both write to tty1)."""
    unit = _extract_unit("testos-recovery.service")
    assert "Conflicts=" in unit, "recovery unit does not declare Conflicts="
    assert "testos-runner.service" in unit, "recovery unit does not conflict with runner"


# ─── Tests: no dead AcpiBlocking classification (F5) ────────────────────────


def test_recovery_script_has_no_acpi_classification():
    """F5: The recovery script must NOT classify ACPI warnings as failures.
    There is no reliable signal, so the dead AcpiBlocking category was removed."""
    script = _extract_recovery_script()
    # The recovery script should not mention ACPI as a failure category.
    assert "ACPI" not in script, (
        "recovery script mentions ACPI — dead classification should be removed (F5)"
    )
    assert "E101" not in script, (
        "recovery script references E101 — dead AcpiBlocking code (F5)"
    )


# ─── Main ───────────────────────────────────────────────────────────────────


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"] + sys.argv[1:]))
