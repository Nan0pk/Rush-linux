#!/usr/bin/env python3
"""Regression checks for phase-d capture ownership and restoration boundaries.

These checks intentionally prove the shell wrapper's static mutation contract.
They do not claim physical-hardware restoration or benchmark validity.
"""

from pathlib import Path
import subprocess

REPO = Path(__file__).resolve().parents[1]
SCRIPT = REPO / "tools" / "phase-d-capture.sh"


def source() -> str:
    return SCRIPT.read_text(encoding="utf-8")


def test_phase_d_capture_has_valid_bash_syntax() -> None:
    subprocess.run(["bash", "-n", str(SCRIPT)], check=True)


def test_capture_never_uses_global_process_kill_or_wipes_optid_runtime() -> None:
    text = source()
    assert "pkill" not in text
    assert "rm -rf /run/optid" not in text
    assert "chmod -R a+rX /run/optid" not in text


def test_capture_refuses_preexisting_optid_instead_of_adopting_it() -> None:
    text = source()
    assert "pgrep -x optid" in text
    assert "this capture will not kill or adopt a process it did not start" in text


def test_baseline_must_already_be_the_expected_native_state() -> None:
    text = source()
    assert "systemctl start tuned || true" not in text
    assert "tuned-adm profile balanced" not in text
    assert "refusing to start it just to manufacture the baseline" in text
    assert "refusing to change it" in text


def test_cleanup_tracks_only_run_owned_processes_and_tuned_changes() -> None:
    text = source()
    assert 'OPTID_PID=""' in text
    assert 'RUSHBENCH_PID=""' in text
    assert "terminate_owned_tree" in text
    assert "pgrep -P \"$pid\"" in text
    assert "TUNED_STOPPED_BY_RUN=0" in text
    assert "if (( TUNED_STOPPED_BY_RUN == 1 ))" in text
    assert 'tuned-adm profile "$TUNED_PROFILE_BEFORE"' in text


def test_capture_does_not_accept_stale_optid_status_as_readiness() -> None:
    text = source()
    assert "status_before" in text
    assert "status_after" in text
    assert "did not become fresh after optid launch" in text


def test_capture_uses_a_run_unique_work_directory() -> None:
    text = source()
    assert 'RUN_WORK_DIR="/tmp/rushbench-mixed-load-001-capture-$$"' in text
    assert 'RUSHBENCH_WORK_DIR="$RUN_WORK_DIR"' in text
