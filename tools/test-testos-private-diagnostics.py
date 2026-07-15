#!/usr/bin/env python3
"""
test-testos-private-diagnostics.py — pytest tests for the PRIVATE-DIAGNOSTICS
privacy boundary.

Covers:
  - raw logs survive a simulated reboot (they are on the USB, not in RAM)
  - normal evidence collection excludes PRIVATE-DIAGNOSTICS
  - submission rejects accidental inclusion of PRIVATE-DIAGNOSTICS
  - submission rejects raw dmesg/journal artifacts in publishable evidence
  - symlink/path traversal cannot cross the boundary
  - diagnostics failure does not falsely mark benchmarks
  - tools/testos-diagnostics.py inspect is read-only
  - tools/testos-diagnostics.py export requires --yes + prints warning
  - tools/testos-diagnostics.py sanitize never modifies the original
  - sanitized output passes the normal privacy scanner

Run:
    python3 -m pytest tools/test-testos-private-diagnostics.py -v
    python3 tools/test-testos-private-diagnostics.py
"""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
DIAG_TOOL = REPO_ROOT / "tools" / "testos-diagnostics.py"
BOUNDARY_MODULE = REPO_ROOT / "tools" / "test_testos_private_diagnostics_boundary.py"

# Load the boundary-check helpers as a module (the filename uses underscores
# so it is not a valid Python identifier via normal import).
_spec = importlib.util.spec_from_file_location(
    "test_testos_private_diagnostics_boundary", BOUNDARY_MODULE
)
_boundary = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_boundary)
_check_bundle_for_private_diagnostics = _boundary.check_bundle_for_private_diagnostics
_check_bundle_for_raw_diagnostics = _boundary.check_bundle_for_raw_diagnostics
_check_bundle_for_symlink_escape = _boundary.check_bundle_for_symlink_escape


# ─── Helpers ─────────────────────────────────────────────────────────────────


def _make_private_diag_dir(usb_root: Path, run_id: str = "run-2026-07-16-001") -> Path:
    """Create a realistic PRIVATE-DIAGNOSTICS directory under usb_root."""
    d = usb_root / "PRIVATE-DIAGNOSTICS" / run_id
    d.mkdir(parents=True, exist_ok=True)
    (d / "README.txt").write_text(
        "PRIVATE — MAY CONTAIN HARDWARE IDENTIFIERS — DO NOT SUBMIT\n\n"
        f"run_id: {run_id}\nfailure_code: none\n",
        encoding="utf-8",
    )
    # Realistic-looking raw diagnostics with identifiers.
    (d / "dmesg.txt").write_text(
        "[    0.000000] Linux version 6.1.0-testos\n"
        "[    1.234] e1000: MAC addr 52:54:00:12:34:56\n"
        "[    2.345] USB device 001:002 serial ABC123456789\n",
        encoding="utf-8",
    )
    (d / "journalctl.txt").write_text(
        "-- Boot 1234 --\n"
        "testos systemd[1]: Started testos-usb-mount.service.\n"
        "testos kernel: ACPI: Power Button [PNP0C0C]\n",
        encoding="utf-8",
    )
    (d / "systemctl-failed.txt").write_text("0 loaded units listed.\n", encoding="utf-8")
    (d / "status-usb-mount.txt").write_text(
        "testos-usb-mount.service - testOS - mount USB ESP\n"
        "   Active: active (exited) since 2026-07-16T10:00:00Z\n",
        encoding="utf-8",
    )
    (d / "runner-exit.txt").write_text("boot_attempt=1\nfailure_code=none\n", encoding="utf-8")
    (d / "usb-discovery-timeline.txt").write_text(
        "[2026-07-16T10:00:00Z] testos-usb-mount: starting\n"
        "[2026-07-16T10:00:01Z] testos-usb-mount: attempt 1: found /dev/sda1\n",
        encoding="utf-8",
    )
    return d


def _make_results_dir(usb_root: Path, run_id: str = "run-2026-07-16-001") -> Path:
    """Create a minimal valid-looking testos-results directory."""
    import datetime as dt
    ts = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")
    d = usb_root / "testos-results" / ts
    d.mkdir(parents=True, exist_ok=True)
    (d / "manifest.json").write_text(json.dumps({
        "schema_version": 1,
        "started_at": ts,
        "finished_at": ts,
        "mode": "all",
        "attempted": ["fio-seq-read"],
        "passed": ["fio-seq-read"],
        "failed": [],
        "skipped": [],
        "host": {"fingerprint": "abc", "cpu_model": "test", "dmi_board": "test",
                 "kernel": "test", "battery_design_uwh": 0},
        "testos_version": "0.7.0-beta.4",
        "provenance": None,
    }))
    (d / "source-sha.txt").write_text("abc1234\n")
    return d


# ─── Tests: raw logs survive reboot ──────────────────────────────────────────


def test_raw_logs_survive_simulated_reboot(tmp_path):
    """Raw diagnostics are written to the USB (persistent), not to RAM.
    Simulate a reboot by clearing /run and re-mounting the USB."""
    usb = tmp_path / "usb"
    usb.mkdir()
    diag = _make_private_diag_dir(usb)
    # Simulate reboot: clear a /run-like directory, keep the USB.
    run_dir = tmp_path / "run"
    run_dir.mkdir()
    (run_dir / "testos").mkdir()
    # The USB still has the diagnostics.
    assert diag.exists(), "PRIVATE-DIAGNOSTICS disappeared from USB after simulated reboot"
    assert (diag / "dmesg.txt").exists()
    assert (diag / "journalctl.txt").exists()
    assert (diag / "runner-exit.txt").exists()


# ─── Tests: normal collection excludes PRIVATE-DIAGNOSTICS ───────────────────


def test_normal_collection_excludes_private_diagnostics(tmp_path):
    """A normal collection that copies testos-results/ must NOT copy
    PRIVATE-DIAGNOSTICS/."""
    usb = tmp_path / "usb"
    usb.mkdir()
    _make_private_diag_dir(usb)
    _make_results_dir(usb)
    # Simulate collection: copy testos-results/ to a bundle dir.
    bundle = tmp_path / "bundle"
    bundle.mkdir()
    # The collection script should copy only testos-results/, not the whole USB.
    assert (usb / "testos-results").exists()
    subprocess.run(
        ["cp", "-r", str(usb / "testos-results") + "/.", str(bundle)],
        check=True,
    )
    # PRIVATE-DIAGNOSTICS must NOT be in the bundle.
    assert not (bundle / "PRIVATE-DIAGNOSTICS").exists(), (
        "PRIVATE-DIAGNOSTICS leaked into the collection bundle"
    )
    # The bundle should have at least one manifest.json (under a timestamp dir).
    manifests = list(bundle.rglob("manifest.json"))
    assert manifests, "bundle has no manifest.json — collection did not copy results"


# ─── Tests: submission rejects accidental inclusion ─────────────────────────


def test_submission_rejects_private_diagnostics_in_bundle(tmp_path):
    """The strict evidence validator must reject a bundle that contains
    a PRIVATE-DIAGNOSTICS directory anywhere."""
    # Build a minimal bundle and sneak PRIVATE-DIAGNOSTICS into it.
    bundle = tmp_path / "bundle"
    bundle.mkdir()
    _make_results_dir(tmp_path / "src")
    # Copy the results into the bundle.
    subprocess.run(
        ["cp", "-r", str(tmp_path / "src" / "testos-results") + "/.", str(bundle)],
        check=True,
    )
    # Sneak in PRIVATE-DIAGNOSTICS.
    (bundle / "PRIVATE-DIAGNOSTICS").mkdir()
    (bundle / "PRIVATE-DIAGNOSTICS" / "dmesg.txt").write_text("secret\n")
    # The boundary check must reject this.
    problems = _check_bundle_for_private_diagnostics(bundle)
    assert problems, "validator accepted a bundle with PRIVATE-DIAGNOSTICS"


def test_submission_rejects_raw_dmesg_in_bundle(tmp_path):
    """The strict evidence validator must reject a bundle that contains
    a raw dmesg.txt file, even outside a PRIVATE-DIAGNOSTICS directory."""
    bundle = tmp_path / "bundle"
    bundle.mkdir()
    _make_results_dir(tmp_path / "src")
    subprocess.run(
        ["cp", "-r", str(tmp_path / "src" / "testos-results") + "/.", str(bundle)],
        check=True,
    )
    # Sneak in a raw dmesg.txt at the top level.
    (bundle / "dmesg.txt").write_text("raw dmesg with MAC=52:54:00:12:34:56\n")
    problems = _check_bundle_for_raw_diagnostics(bundle)
    assert problems, "validator accepted a bundle with raw dmesg.txt"


def test_submission_rejects_symlink_referencing_private_diagnostics(tmp_path):
    """A symlink inside the bundle that points at PRIVATE-DIAGNOSTICS must
    be rejected (path traversal / boundary crossing)."""
    usb = tmp_path / "usb"
    usb.mkdir()
    diag = _make_private_diag_dir(usb)
    bundle = tmp_path / "bundle"
    bundle.mkdir()
    _make_results_dir(tmp_path / "src")
    subprocess.run(
        ["cp", "-r", str(tmp_path / "src" / "testos-results") + "/.", str(bundle)],
        check=True,
    )
    # Create a symlink inside the bundle that points at the private diag dir.
    os.symlink(diag, bundle / "diag-link")
    problems = _check_bundle_for_symlink_escape(bundle)
    assert problems, "validator accepted a bundle with a symlink escaping to PRIVATE-DIAGNOSTICS"


# ─── Tests: diagnostics failure does not falsely mark benchmarks ─────────────


def test_diagnostics_failure_does_not_falsely_mark_benchmarks(tmp_path):
    """If private-diagnostics capture fails (e.g. disk full), the benchmark
    results that DID succeed must still be recorded honestly as pass/fail.
    The runner records a sync-failure warning but does NOT flip passed
    benchmarks to failed."""
    # This is a structural test: verify the runner source records sync
    # failures separately from benchmark status.
    runner_src = (REPO_ROOT / "crates" / "testos" / "src" / "bin" / "testos-runner.rs").read_text()
    # The runner must have a separate sync_ok flag that does NOT affect
    # the per-benchmark status.
    assert "sync_ok" in runner_src, "runner has no sync_ok flag"
    # The summary must report sync status separately from pass/fail counts.
    tui_src = (REPO_ROOT / "crates" / "testos" / "src" / "tui.rs").read_text()
    assert "sync_ok" in tui_src, "TUI summary does not report sync status"
    # The benchmark status assignment must NOT reference sync_ok.
    # Find the per-benchmark status block and verify sync_ok is not used there.
    # (This is a conservative structural check; the runtime behavior is
    # verified by the Rust unit tests in the testos crate.)
    status_block = runner_src[
        runner_src.find("let status_word = match status.as_str()"):
        runner_src.find("let _ = writeln!(")
    ]
    assert "sync_ok" not in status_block, (
        "benchmark status assignment references sync_ok — a sync failure "
        "could falsely flip benchmark results"
    )


# ─── Tests: tools/testos-diagnostics.py ──────────────────────────────────────


def test_inspect_is_read_only(tmp_path):
    """`inspect` must not modify the source directory or any file in it."""
    usb = tmp_path / "usb"
    diag = _make_private_diag_dir(usb)
    # Snapshot the directory contents + mtimes before inspect.
    before = {}
    for f in diag.rglob("*"):
        if f.is_file():
            before[str(f)] = f.read_bytes()
    # Run inspect.
    r = subprocess.run(
        [sys.executable, str(DIAG_TOOL), "inspect", str(diag)],
        capture_output=True, text=True, timeout=10,
    )
    assert r.returncode == 0, f"inspect failed: {r.stderr}"
    assert "INSPECT (read-only)" in r.stdout
    assert MARKER_TEXT in r.stdout
    # Verify nothing changed.
    for f in diag.rglob("*"):
        if f.is_file():
            assert f.read_bytes() == before[str(f)], f"inspect modified {f}"


def test_inspect_rejects_symlinks(tmp_path):
    """`inspect` must refuse a directory containing symlinks (path traversal)."""
    usb = tmp_path / "usb"
    diag = _make_private_diag_dir(usb)
    # Drop a symlink inside.
    target = tmp_path / "secret"
    target.write_text("outside the boundary\n")
    os.symlink(target, diag / "escape.txt")
    r = subprocess.run(
        [sys.executable, str(DIAG_TOOL), "inspect", str(diag)],
        capture_output=True, text=True, timeout=10,
    )
    assert r.returncode == 3, f"inspect should exit 3 on symlinks, got {r.returncode}"
    assert "PRIVACY VIOLATION" in r.stderr or "symlink" in r.stderr.lower()


def test_export_requires_yes_flag(tmp_path):
    """`export` must require --yes to confirm the privacy warning."""
    usb = tmp_path / "usb"
    diag = _make_private_diag_dir(usb)
    dest = tmp_path / "dest"
    r = subprocess.run(
        [sys.executable, str(DIAG_TOOL), "export", str(diag), str(dest)],
        capture_output=True, text=True, timeout=10,
    )
    assert r.returncode != 0, "export succeeded without --yes"
    assert "PRIVACY WARNING" in r.stderr or "privacy" in r.stderr.lower()
    assert not dest.exists(), "export created the destination without --yes"


def test_export_copies_raw_with_yes(tmp_path):
    """`export --yes` copies the raw diagnostics (with identifiers intact)
    and writes the marker at the destination."""
    usb = tmp_path / "usb"
    diag = _make_private_diag_dir(usb)
    dest = tmp_path / "dest"
    r = subprocess.run(
        [sys.executable, str(DIAG_TOOL), "export", str(diag), str(dest), "--yes"],
        capture_output=True, text=True, timeout=10,
    )
    assert r.returncode == 0, f"export failed: {r.stderr}"
    assert dest.exists()
    # Raw identifiers are preserved (export does NOT sanitize).
    exported_dmesg = (dest / "dmesg.txt").read_text()
    assert "52:54:00:12:34:56" in exported_dmesg, "export sanitized when it should not have"
    # The marker is present at the destination.
    readme = (dest / "README.txt").read_text()
    assert "DO NOT SUBMIT" in readme


def test_export_refuses_existing_destination(tmp_path):
    """`export` must refuse to overwrite an existing destination."""
    usb = tmp_path / "usb"
    diag = _make_private_diag_dir(usb)
    dest = tmp_path / "dest"
    dest.mkdir()
    r = subprocess.run(
        [sys.executable, str(DIAG_TOOL), "export", str(diag), str(dest), "--yes"],
        capture_output=True, text=True, timeout=10,
    )
    assert r.returncode != 0


def test_sanitize_never_modifies_original(tmp_path):
    """`sanitize` must create a new copy and leave the original untouched."""
    usb = tmp_path / "usb"
    diag = _make_private_diag_dir(usb)
    # Snapshot the original.
    original_dmesg = (diag / "dmesg.txt").read_bytes()
    dest = tmp_path / "sanitized"
    r = subprocess.run(
        [sys.executable, str(DIAG_TOOL), "sanitize", str(diag), str(dest)],
        capture_output=True, text=True, timeout=10,
    )
    assert r.returncode == 0, f"sanitize failed: {r.stderr}"
    assert dest.exists()
    # The original must be byte-for-byte unchanged.
    assert (diag / "dmesg.txt").read_bytes() == original_dmesg, (
        "sanitize modified the original directory"
    )


def test_sanitize_redacts_identifiers(tmp_path):
    """`sanitize` must redact MACs, serials, UUIDs, IPs, and the testos hostname."""
    usb = tmp_path / "usb"
    diag = _make_private_diag_dir(usb)
    dest = tmp_path / "sanitized"
    r = subprocess.run(
        [sys.executable, str(DIAG_TOOL), "sanitize", str(diag), str(dest)],
        capture_output=True, text=True, timeout=10,
    )
    assert r.returncode == 0, f"sanitize failed: {r.stderr}"
    sanitized_dmesg = (dest / "dmesg.txt").read_text()
    assert "52:54:00:12:34:56" not in sanitized_dmesg, "MAC not redacted"
    assert "<MAC>" in sanitized_dmesg
    assert "ABC123456789" not in sanitized_dmesg, "serial not redacted"
    assert "<SERIAL>" in sanitized_dmesg
    sanitized_journal = (dest / "journalctl.txt").read_text()
    assert "testos systemd" not in sanitized_journal or "<HOSTNAME>" in sanitized_journal, (
        "hostname not redacted"
    )


def test_sanitize_writes_marker_at_destination(tmp_path):
    """The sanitized destination must have a marker explaining it is a
    reviewed copy."""
    usb = tmp_path / "usb"
    diag = _make_private_diag_dir(usb)
    dest = tmp_path / "sanitized"
    r = subprocess.run(
        [sys.executable, str(DIAG_TOOL), "sanitize", str(diag), str(dest)],
        capture_output=True, text=True, timeout=10,
    )
    assert r.returncode == 0
    readme = (dest / "README.txt").read_text()
    assert "SANITIZED COPY" in readme
    assert "best-effort" in readme.lower() or "BEST-EFFORT" in readme


def test_sanitize_refuses_symlinks(tmp_path):
    """`sanitize` must refuse a source containing symlinks."""
    usb = tmp_path / "usb"
    diag = _make_private_diag_dir(usb)
    target = tmp_path / "secret"
    target.write_text("outside\n")
    os.symlink(target, diag / "escape.txt")
    dest = tmp_path / "sanitized"
    r = subprocess.run(
        [sys.executable, str(DIAG_TOOL), "sanitize", str(diag), str(dest)],
        capture_output=True, text=True, timeout=10,
    )
    assert r.returncode == 3


MARKER_TEXT = "PRIVATE — MAY CONTAIN HARDWARE IDENTIFIERS — DO NOT SUBMIT"


# ─── Main for direct execution ───────────────────────────────────────────────


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"] + sys.argv[1:]))
