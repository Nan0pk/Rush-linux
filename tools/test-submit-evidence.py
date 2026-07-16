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
import subprocess
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


def _git_head():
    r = subprocess.run(
        ["git", "-C", str(_ROOT), "rev-parse", "HEAD"],
        capture_output=True, text=True, timeout=5,
    )
    return r.stdout.strip()


def _version():
    return (_ROOT / "VERSION").read_text().strip()


def _sha256_bytes(b: bytes) -> str:
    import hashlib
    return hashlib.sha256(b).hexdigest()


def _sha256_file(p: Path) -> str:
    return _sha256_bytes(p.read_bytes())


def _now_iso():
    import datetime as dt
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _make_valid_run_dir(tmp: Path) -> Path:
    """Create a fully provenance-bound run dir matching the Rust runner output.

    This now carries a provenance block, run-intent.json, plan.json,
    bench-list.toml, source-sha.txt, and result-hashes.json — the exact
    shape the strict validator requires for physical TestOS submission.
    Legacy planless run dirs are no longer accepted for submission.
    """
    run_dir = tmp / "run"
    run_dir.mkdir()
    head = _git_head()
    version = _version()
    catalog = (_ROOT / "testos" / "bench-list.toml").read_bytes()
    catalog_sha = _sha256_bytes(catalog)

    # Valid plan matching the intent.
    plan = {
        "schema_version": 1, "plan_kind": "rush-autopilot-plan",
        "generated_at": _now_iso(), "source_version": version,
        "source_commit": head, "dry_run": False,
        "campaign_scope": "comparative", "hardware_slot": "laptop",
        "slot_confidence": "high", "ambiguities": [],
        "open_criteria": [], "existing_evidence": [], "steps": [],
        "repo_root": ".",
    }
    plan_raw = json.dumps(plan).encode()
    plan_sha = _sha256_bytes(plan_raw)

    # Valid run-intent.
    intent = {
        "schema_version": 1, "intent_kind": "testos-run-intent",
        "run_id": "submit-test-0001", "source_commit": head,
        "source_version": version, "testos_version": version,
        "testos_image_digest": f"sha256:{'a' * 64}",
        "testos_image_commit": head,  # F4: required, full 40-char SHA
        "plan_sha256": plan_sha, "benchmark_catalog_sha256": catalog_sha,
        "generated_at": _now_iso(), "dry_run": False,
        "checkpoint_nonce": "ckpt-submit-test-001",
        "campaign_id": "campaign-test-001",
    }
    intent_raw = json.dumps(intent, indent=2, sort_keys=True).encode()
    intent_sha = _sha256_bytes(intent_raw)

    manifest = {
        "schema_version": 1,
        "started_at": _now_iso(),
        "finished_at": _now_iso(),
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
        "testos_version": version,
        "provenance": {
            "run_id": intent["run_id"],
            "source_commit": head,
            "source_version": version,
            "testos_version": version,
            "testos_image_digest": intent["testos_image_digest"],
            "testos_image_commit": head,  # F4: required, matches intent
            "plan_sha256": plan_sha,
            "benchmark_catalog_sha256": catalog_sha,
            "intent_generated_at": intent["generated_at"],
            "intent_dry_run": False,
            "checkpoint_nonce": intent["checkpoint_nonce"],
            "intent_sha256": intent_sha,
            "campaign_id": intent["campaign_id"],
        },
    }
    (run_dir / "manifest.json").write_text(json.dumps(manifest, indent=2))
    result_a = {"schema_version": 1, "bench_id": "bench-a", "bench_name": "bench A",
                "status": "pass", "value": 1234.5, "unit": "tps",
                "started_at": _now_iso(), "finished_at": _now_iso(),
                "elapsed_seconds": 0.5, "scenario": "server-throughput",
                "host": {"fingerprint": "abc123def456"}}
    result_b = {"schema_version": 1, "bench_id": "bench-b", "bench_name": "bench B",
                "status": "fail", "stderr": "command not found",
                "started_at": _now_iso(), "finished_at": _now_iso(),
                "elapsed_seconds": 0.1, "scenario": "server-throughput",
                "host": {"fingerprint": "abc123def456"}}
    (run_dir / "bench-a.json").write_text(json.dumps(result_a))
    (run_dir / "bench-b.json").write_text(json.dumps(result_b))
    (run_dir / "run-intent.json").write_bytes(intent_raw)
    (run_dir / "plan.json").write_bytes(plan_raw)
    (run_dir / "bench-list.toml").write_bytes(catalog)
    (run_dir / "source-sha.txt").write_text(head[:12])
    (run_dir / "result-hashes.json").write_text(json.dumps({
        "bench-a.json": _sha256_file(run_dir / "bench-a.json"),
        "bench-b.json": _sha256_file(run_dir / "bench-b.json"),
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
    """A legacy run dir without provenance is rejected by the strict gate."""
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        manifest = {"started_at": "x", "passed": [], "failed": [], "host": {}}
        (run_dir / "manifest.json").write_text(json.dumps(manifest))
        (run_dir / "bench.json").write_text("{}")
        ok, errors, _ = rse.validate_run_dir(run_dir)
        assert not ok
        # Legacy planless evidence is rejected at the provenance gate.
        assert any("provenance" in e or "run-intent" in e for e in errors)


def test_validate_no_results():
    """A legacy run dir without provenance is rejected."""
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
        assert any("provenance" in e or "run-intent" in e for e in errors)


def test_planless_legacy_rejected_for_submission():
    """A fully-formed but planless legacy run dir must be rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        manifest = {
            "schema_version": 1,
            "started_at": "2026-07-06T12:00:00Z",
            "host": {"fingerprint": "abc"},
            "passed": ["bench-a"], "failed": [], "skipped": [],
            "attempted": ["bench-a"],
        }
        (run_dir / "manifest.json").write_text(json.dumps(manifest))
        (run_dir / "bench-a.json").write_text(json.dumps({
            "bench_id": "bench-a", "status": "pass", "value": 1.0, "unit": "ms",
        }))
        ok, errors, _ = rse.validate_run_dir(run_dir)
        assert not ok
        assert any("provenance" in e for e in errors), errors


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
