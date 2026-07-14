#!/usr/bin/env python3
"""
pytest tests for tools/rush-submit-evidence — the best submission step.

Covers:
  - run dir validation (valid, missing manifest, missing host.fingerprint, no results)
  - PR body generation (badge, host table, bench table)
  - deterministic branch naming
  - deterministic commit message
  - bundle creation
  - local submission mode
  - dry-run mode

Run with:
  python3 -m pytest tools/test-submit-evidence.py -v
"""

from __future__ import annotations

import importlib.machinery
import importlib.util
import json
import os
import sys
import tempfile
from pathlib import Path
from urllib.parse import urlparse

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent


def _load_module(name: str, path: Path):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    loader.exec_module(mod)
    return mod


rse = _load_module("rush_submit_evidence", _TOOLS_DIR / "rush-submit-evidence")


# ─── Fixtures ───────────────────────────────────────────────────────────────


def _make_valid_run_dir(tmp: Path) -> Path:
    """Create a valid run dir with manifest + 2 results."""
    run_dir = tmp / "run"
    run_dir.mkdir()
    manifest = {
        "schema_version": 1,
        "started_at": "2026-07-06T12:00:00Z",
        "finished_at": "2026-07-06T12:02:00Z",
        "mode": "all",
        "attempted": ["bench-a", "bench-b"],
        "passed": ["bench-a"],
        "failed": ["bench-b"],
        "skipped": [],
        "host": {
            "fingerprint": "abc123def456",
            "kernel": "6.1.0-test",
            "cpu_model": "Test CPU",
            "dmi_board": "TestBoard",
            "battery_design_uwh": 50000000,
        },
        "testos_version": "0.7.0-beta.1",
    }
    (run_dir / "manifest.json").write_text(json.dumps(manifest))
    (run_dir / "bench-a.json").write_text(json.dumps({
        "bench_id": "bench-a", "status": "pass", "value": 1234.5, "unit": "tps",
    }))
    (run_dir / "bench-b.json").write_text(json.dumps({
        "bench_id": "bench-b", "status": "fail", "stderr": "command not found",
    }))
    return run_dir


# ─── Validation ─────────────────────────────────────────────────────────────


def test_validate_valid_run_dir():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _make_valid_run_dir(Path(tmp))
        ok, errors, manifest = rse.validate_run_dir(run_dir)
        assert ok, f"should be valid: {errors}"
        assert errors == []
        assert manifest["host"]["fingerprint"] == "abc123def456"


def test_validate_missing_manifest():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        (run_dir / "bench-a.json").write_text("{}")
        ok, errors, _ = rse.validate_run_dir(run_dir)
        assert not ok
        assert any("manifest.json" in e for e in errors)


def test_validate_missing_host_fingerprint():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        manifest = {"started_at": "x", "passed": [], "failed": [], "host": {}}
        (run_dir / "manifest.json").write_text(json.dumps(manifest))
        (run_dir / "bench.json").write_text("{}")
        ok, errors, _ = rse.validate_run_dir(run_dir)
        assert not ok
        assert any("fingerprint" in e for e in errors)


def test_validate_no_results():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        manifest = {
            "started_at": "x", "passed": [], "failed": [],
            "host": {"fingerprint": "abc"},
        }
        (run_dir / "manifest.json").write_text(json.dumps(manifest))
        ok, errors, _ = rse.validate_run_dir(run_dir)
        assert not ok
        assert any("no benchmark" in e for e in errors)


def test_validate_nonexistent_dir():
    ok, errors, _ = rse.validate_run_dir(Path("/nonexistent/run"))
    assert not ok
    assert any("does not exist" in e for e in errors)


# ─── PR body generation ─────────────────────────────────────────────────────


def test_pr_body_has_pass_fail_badge():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _make_valid_run_dir(Path(tmp))
        _, _, manifest = rse.validate_run_dir(run_dir)
        body = rse.generate_pr_body(run_dir, manifest, "test-branch", None)
        # Extract all URLs from the body using a regex (handles markdown
        # ![alt](url) syntax and bare URLs). A naive split() on whitespace
        # leaves ![alt]( prefix attached to the URL token, so urlparse
        # returns None for the hostname. The regex captures the URL after
        # `](` in markdown image links, or bare http(s) URLs.
        # This resolves CodeQL alert #86 (incomplete URL substring
        # sanitization) by replacing the naive `assert "shields.io" in body`
        # substring check with proper URL parsing.
        import re
        from urllib.parse import urlparse

        url_pattern = re.compile(r'https?://[^\s\)\]]+')
        urls = url_pattern.findall(body)
        hosts = [urlparse(u).hostname for u in urls]
        assert any(host and host.endswith(".shields.io") or host == "shields.io" for host in hosts), (
            f"expected a shields.io badge URL in body, got hosts={hosts}"
        )


def test_pr_body_has_host_table():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _make_valid_run_dir(Path(tmp))
        _, _, manifest = rse.validate_run_dir(run_dir)
        body = rse.generate_pr_body(run_dir, manifest, "test-branch", None)
        assert "abc123def456" in body
        assert "6.1.0-test" in body
        assert "Test CPU" in body


def test_pr_body_has_bench_table():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _make_valid_run_dir(Path(tmp))
        _, _, manifest = rse.validate_run_dir(run_dir)
        body = rse.generate_pr_body(run_dir, manifest, "test-branch", None)
        assert "bench-a" in body
        assert "bench-b" in body
        assert "pass" in body.lower()
        assert "fail" in body.lower()


def test_pr_body_has_summary_counts():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _make_valid_run_dir(Path(tmp))
        _, _, manifest = rse.validate_run_dir(run_dir)
        body = rse.generate_pr_body(run_dir, manifest, "test-branch", None)
        assert "passed" in body.lower()
        assert "1" in body  # 1 passed
        assert "1" in body  # 1 failed


def test_pr_body_has_reviewer_notes():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _make_valid_run_dir(Path(tmp))
        (run_dir / "collection-report.json").write_text(json.dumps({
            "classification": "baseline testOS hardware evidence",
            "privacy": {"excluded_usb_files": ["system-logs/dmesg.txt"]},
        }))
        _, _, manifest = rse.validate_run_dir(run_dir)
        body = rse.generate_pr_body(run_dir, manifest, "test-branch", None)
        assert "ADR 0018" in body
        assert "No auto-merge" in body or "no auto-merge" in body.lower()
        assert "Baseline-only" in body
        assert "system-logs/ (" not in body


def test_pr_body_has_bot_signature():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _make_valid_run_dir(Path(tmp))
        _, _, manifest = rse.validate_run_dir(run_dir)
        body = rse.generate_pr_body(run_dir, manifest, "test-branch", None)
        assert "rush-evidence-bot" in body


# ─── Branch + commit ────────────────────────────────────────────────────────


def test_deterministic_branch_name():
    manifest = {
        "started_at": "2026-07-06T12:00:00Z",
        "host": {"fingerprint": "abc123def456789"},
    }
    branch = rse.deterministic_branch(manifest)
    assert branch == "evidence/2026-07-06/abc123def456"


def test_deterministic_commit_msg():
    manifest = {
        "started_at": "2026-07-06T12:00:00Z",
        "host": {"fingerprint": "abc123def456789"},
        "passed": ["a", "b"],
        "failed": [],
    }
    msg = rse.deterministic_commit_msg(manifest)
    assert "evidence(pass)" in msg
    assert "2026-07-06" in msg
    assert "abc123def456" in msg
    assert "pass=2" in msg


def test_deterministic_commit_msg_mixed():
    manifest = {
        "started_at": "2026-07-06T12:00:00Z",
        "host": {"fingerprint": "abc123def456789"},
        "passed": ["a"],
        "failed": ["b"],
    }
    msg = rse.deterministic_commit_msg(manifest)
    assert "evidence(mixed)" in msg


# ─── Bundle ─────────────────────────────────────────────────────────────────


def test_create_bundle():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _make_valid_run_dir(Path(tmp))
        bundle = rse.create_bundle(run_dir, "test-run")
        assert bundle is not None
        assert bundle.exists()
        assert bundle.name == "rush-evidence-test-run.tar.gz"
        assert bundle.stat().st_size > 0


# ─── Local submission ───────────────────────────────────────────────────────


def test_submit_local_prints_summary():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _make_valid_run_dir(Path(tmp))
        _, _, manifest = rse.validate_run_dir(run_dir)
        status, info = rse.submit_local(run_dir, manifest)
        assert status == "local"
        # info is the bundle path or run_dir path.
        assert len(info) > 0


# ─── Dry-run ────────────────────────────────────────────────────────────────


def test_dry_run_does_not_push():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _make_valid_run_dir(Path(tmp))
        _, _, manifest = rse.validate_run_dir(run_dir)
        # dry-run should not touch the network.
        status, info = rse.submit_to_github(run_dir, manifest, dry_run=True)
        assert status == "dry-run"


def test_submission_clone_populates_worktree():
    source = (_TOOLS_DIR / "rush-submit-evidence").read_text()
    assert "--no-checkout" not in source


# ─── Standalone runner ──────────────────────────────────────────────────────


def _run_all_tests() -> int:
    test_funcs = [
        (name, obj)
        for name, obj in sorted(globals().items())
        if name.startswith("test_") and callable(obj)
    ]
    passed = 0
    failed = 0
    for name, func in test_funcs:
        try:
            func()
            print(f"  PASS {name}")
            passed += 1
        except Exception as e:
            print(f"  FAIL {name}: {e}")
            import traceback
            traceback.print_exc()
            failed += 1
    print(f"\n{passed} passed, {failed} failed, {passed + failed} total")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(_run_all_tests())
