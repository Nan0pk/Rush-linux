#!/usr/bin/env python3
"""
test-testos-evidence-submission-blockers.py — focused regression tests for
the confirmed TestOS real-hardware evidence-workflow submission blockers.

These tests cover the specific bugs from the task brief and the hardening
that prevents them from regressing. Every test is a real, assertion-based
test (no "any nonzero exit is fine" shortcuts):

  Bug 1 — bench_id vs bench_name identity:
    - validate_run_dir keys on the canonical bench_id (never bench_name).
    - A realistic result with bench_id="iperf3-tcp" and a DIFFERENT
      human-readable bench_name="iperf3 TCP throughput" is accepted.
    - A result missing bench_id is rejected.
    - A result whose bench_id != filename stem is rejected.

  Bug 2 — privacy scanner import failure:
    - The privacy scanner (rush_pr_lib.privacy_scan) loads cleanly (no
      "'NoneType' object has no attribute '__dict__'").
    - rush-submit-evidence reaches the privacy gate and, for a clean run,
      PASSES the scan and REACHES bundle creation (positive end-to-end).
    - The strict validator delegates correctly (no import/execution failure).

  Timing bug #9 — genuinely monotonic foreground-launch timing:
    - foreground-launch does not call wall-clock EPOCHREALTIME "monotonic".
    - foreground-launch uses perf_counter_ns (CLOCK_MONOTONIC).

Run:
    python3 -m pytest tools/test-testos-evidence-submission-blockers.py -v
    python3 tools/test-testos-evidence-submission-blockers.py
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent


def _load_module(name: str, path: Path):
    import importlib.machinery
    import importlib.util

    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    loader.exec_module(mod)
    return mod


rse = _load_module("rush_submit_evidence_blockers", TOOLS_DIR / "rush-submit-evidence")


# ─── Helpers ────────────────────────────────────────────────────────────────


def _git_head() -> str:
    r = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "rev-parse", "HEAD"],
        capture_output=True, text=True, timeout=5,
    )
    return r.stdout.strip()


def _version() -> str:
    return (REPO_ROOT / "VERSION").read_text().strip()


def _now_iso() -> str:
    import datetime as dt
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _bench_list_bytes() -> bytes:
    return (REPO_ROOT / "testos" / "bench-list.toml").read_bytes()


def _sha256_bytes(b: bytes) -> str:
    import hashlib
    return hashlib.sha256(b).hexdigest()


def _sha256_file(p: Path) -> str:
    return _sha256_bytes(p.read_bytes())


# A realistic per-benchmark result in the EXACT shape the Rust TestOS runner
# emits (crates/testos/src/results.rs::BenchResult). bench_id is canonical
# and equals the filename stem; bench_name is human-readable and may differ.
def _rust_runner_result(bench_id: str, bench_name: str, *, status: str = "pass",
                        value=None, unit=None) -> dict:
    r = {
        "schema_version": 1,
        "bench_id": bench_id,
        "bench_name": bench_name,
        "scenario": "server-throughput",
        "status": status,
        "started_at": _now_iso(),
        "finished_at": _now_iso(),
        "elapsed_seconds": 0.5,
        "host": {"fingerprint": "test-host-0012"},
    }
    if value is not None:
        r["value"] = value
    if unit is not None:
        r["unit"] = unit
    return r


def _write_provenance_run_dir(tmp: Path, *, results: dict[str, dict],
                              passed=None, failed=None, mode: str = "all",
                              source_commit_override: str | None = None) -> Path:
    """Write a fully provenance-bound run dir matching the Rust runner output.

    Includes manifest.json with provenance, run-intent.json, plan.json,
    bench-list.toml, source-sha.txt, and result-hashes.json — the exact
    shape the strict validator requires for physical TestOS submission.
    """
    run_dir = tmp / "run"
    run_dir.mkdir(parents=True, exist_ok=True)
    head = source_commit_override or _git_head()
    version = _version()
    catalog = _bench_list_bytes()
    catalog_sha = _sha256_bytes(catalog)
    ids = [name.removesuffix(".json") for name in results]

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
        "run_id": "blockers-test-0001", "source_commit": head,
        "source_version": version, "testos_version": version,
        "testos_image_digest": f"sha256:{'a' * 64}",
        "plan_sha256": plan_sha, "benchmark_catalog_sha256": catalog_sha,
        "generated_at": _now_iso(), "dry_run": False,
        "checkpoint_nonce": "ckpt-blockers-0001",
        "campaign_id": "campaign-blockers-001",
    }
    intent_raw = json.dumps(intent, indent=2, sort_keys=True).encode()
    intent_sha = _sha256_bytes(intent_raw)

    passed_list = passed if passed is not None else [i for i, r in results.items() if r.get("status") == "pass"]
    failed_list = failed if failed is not None else [i for i, r in results.items() if r.get("status") not in ("pass",)]

    manifest = {
        "schema_version": 1,
        "started_at": _now_iso(), "finished_at": _now_iso(),
        "mode": mode,
        "attempted": ids,
        "passed": passed_list, "failed": failed_list, "skipped": [],
        "host": {"fingerprint": "test-host-0012"},
        "testos_version": version,
        "provenance": {
            "run_id": intent["run_id"], "source_commit": head,
            "source_version": version, "testos_version": version,
            "testos_image_digest": intent["testos_image_digest"],
            "plan_sha256": plan_sha, "benchmark_catalog_sha256": catalog_sha,
            "intent_generated_at": intent["generated_at"],
            "intent_dry_run": False,
            "checkpoint_nonce": intent["checkpoint_nonce"],
            "intent_sha256": intent_sha,
            "campaign_id": intent["campaign_id"],
        },
    }
    (run_dir / "manifest.json").write_text(json.dumps(manifest, indent=2))
    for name, data in results.items():
        (run_dir / name).write_text(json.dumps(data))
    (run_dir / "run-intent.json").write_bytes(intent_raw)
    (run_dir / "plan.json").write_bytes(plan_raw)
    (run_dir / "bench-list.toml").write_bytes(catalog)
    (run_dir / "source-sha.txt").write_text(head[:12])
    # Auto-generate result-hashes from the actual result files.
    auto_hashes = {name: _sha256_file(run_dir / name) for name in results}
    (run_dir / "result-hashes.json").write_text(json.dumps(auto_hashes))
    return run_dir


# ─── Bug 1: bench_id vs bench_name identity ─────────────────────────────────


def test_bug1_bench_id_canonical_not_bench_name():
    """A result whose bench_id matches the filename stem but whose
    human-readable bench_name DIFFERS must be accepted. The previous code
    preferred bench_name, rejecting every real run where they differ."""
    with tempfile.TemporaryDirectory() as tmp:
        # Realistic fixture from the task: bench_id != bench_name.
        results = {
            "iperf3-tcp.json": _rust_runner_result(
                "iperf3-tcp", "iperf3 TCP throughput",
                status="pass", value=79.0, unit="Gbit/s",
            ),
        }
        run_dir = _write_provenance_run_dir(Path(tmp), results=results, passed=["iperf3-tcp"])
        ok, errors, _ = rse.validate_run_dir(run_dir)
        assert ok, f"realistic bench_id!=bench_name result rejected: {errors}"


def test_bug1_missing_bench_id_rejected():
    """A result missing the canonical bench_id field must be rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        results = {
            "iperf3-tcp.json": {
                "schema_version": 1,
                "bench_name": "iperf3 TCP throughput",  # no bench_id
                "status": "pass", "value": 79.0, "unit": "Gbit/s",
                "started_at": _now_iso(), "finished_at": _now_iso(),
                "elapsed_seconds": 0.5, "scenario": "server-throughput",
                "host": {"fingerprint": "test-host-0012"},
            },
        }
        run_dir = _write_provenance_run_dir(Path(tmp), results=results, passed=["iperf3-tcp"])
        ok, errors, _ = rse.validate_run_dir(run_dir)
        assert not ok
        assert any("missing required" in e and "bench_id" in e for e in errors), errors


def test_bug1_bench_id_mismatch_rejected():
    """A result whose bench_id != filename stem must be rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        results = {
            "iperf3-tcp.json": _rust_runner_result(
                "wrong-id", "iperf3 TCP throughput",
                status="pass", value=79.0, unit="Gbit/s",
            ),
        }
        run_dir = _write_provenance_run_dir(Path(tmp), results=results, passed=["iperf3-tcp"])
        ok, errors, _ = rse.validate_run_dir(run_dir)
        assert not ok
        assert any("bench_id" in e and "wrong-id" in e for e in errors), errors


def test_bug1_bench_name_alone_never_accepted_as_identity():
    """bench_name alone (no bench_id) must NOT satisfy the identity check
    even if bench_name happens to equal the filename stem."""
    with tempfile.TemporaryDirectory() as tmp:
        results = {
            "iperf3-tcp.json": {
                "schema_version": 1,
                "bench_name": "iperf3-tcp",  # equals stem, but wrong field
                "status": "pass", "value": 79.0, "unit": "Gbit/s",
                "started_at": _now_iso(), "finished_at": _now_iso(),
                "elapsed_seconds": 0.5, "scenario": "server-throughput",
                "host": {"fingerprint": "test-host-0012"},
            },
        }
        run_dir = _write_provenance_run_dir(Path(tmp), results=results, passed=["iperf3-tcp"])
        ok, errors, _ = rse.validate_run_dir(run_dir)
        assert not ok
        assert any("missing required" in e and "bench_id" in e for e in errors), errors


# ─── Bug 2: privacy scanner import + positive end-to-end ────────────────────


def test_bug2_privacy_scanner_loads_without_noneType_error():
    """rush_pr_lib (which uses @dataclass) must import cleanly. The broken
    dynamic import raised "'NoneType' object has no attribute '__dict__'\"."""
    import importlib
    # Force a fresh import to prove it isn't relying on a cached half-loaded module.
    sys.path.insert(0, str(TOOLS_DIR))
    for m in ("rush_pr_lib", "rush_capture_lib"):
        sys.modules.pop(m, None)
    rush_pr_lib = importlib.import_module("rush_pr_lib")
    assert rush_pr_lib.SubmissionPlan.__name__ == "SubmissionPlan"
    with tempfile.TemporaryDirectory() as tmp:
        ok, errs = rush_pr_lib.privacy_scan(Path(tmp))
        assert ok, errs
        assert errs == []


def test_bug2_strict_validator_loads():
    """The strict validator delegation must not report an import/execution failure."""
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        (run_dir / "manifest.json").write_text("{}")
        # A manifest with provenance but no files -> strict gate runs and fails
        # on missing required files, NOT on an import error.
        ok, errors = rse._run_strict_testos_validator(run_dir)
        assert not any("import/execution failed" in e for e in errors), errors


def test_bug2_positive_e2e_clean_run_passes_privacy_and_reaches_bundle():
    """POSITIVE end-to-end: a valid, clean run dir must PASS the privacy scan
    and REACH bundle creation under --submit-mode=local. This replaces the
    insufficient 'any nonzero exit' tests. We assert rc==0 AND a bundle file
    is created AND the privacy scan reported PASS."""
    with tempfile.TemporaryDirectory() as tmp:
        results = {
            "iperf3-tcp.json": _rust_runner_result(
                "iperf3-tcp", "iperf3 TCP throughput",
                status="pass", value=79.0, unit="Gbit/s",
            ),
        }
        run_dir = _write_provenance_run_dir(Path(tmp), results=results, passed=["iperf3-tcp"])
        env = dict(os.environ)
        env["HOME"] = tmp  # isolate any checkpoint lookups
        r = subprocess.run(
            ["python3", str(TOOLS_DIR / "rush-submit-evidence"),
             str(run_dir), "--submit-mode", "local"],
            capture_output=True, text=True, timeout=30, env=env,
        )
        combined = r.stdout + r.stderr
        assert r.returncode == 0, f"clean run should succeed:\n{combined}"
        assert "OK: privacy scan passed" in combined, combined
        # Bundle creation reached.
        assert "local evidence bundle" in combined, combined
        bundles = list(Path(tmp).rglob("*.tar.gz"))
        assert bundles, "no bundle created (did not reach bundle step)"


def test_bug2_secret_in_clean_shape_is_rejected_by_privacy():
    """A realistic-length secret in a realistic-shape result must be caught
    by the now-working privacy scanner (rc!=0, privacy reported)."""
    realistic_token = "ghp_0123456789abcdefghijklmnopqrstuvwxyz0123"
    with tempfile.TemporaryDirectory() as tmp:
        results = {
            "iperf3-tcp.json": _rust_runner_result(
                "iperf3-tcp", "iperf3 TCP throughput",
                status="pass", value=79.0, unit="Gbit/s",
            ),
        }
        results["iperf3-tcp.json"]["stderr"] = f"GITHUB_TOKEN={realistic_token}"
        run_dir = _write_provenance_run_dir(Path(tmp), results=results, passed=["iperf3-tcp"])
        r = subprocess.run(
            ["python3", str(TOOLS_DIR / "rush-submit-evidence"),
             str(run_dir), "--submit-mode", "local"],
            capture_output=True, text=True, timeout=30,
        )
        assert r.returncode != 0, "secret should block submission"
        low = (r.stdout + r.stderr).lower()
        assert "privacy" in low or "unredacted" in low, (r.stdout + r.stderr)


# ─── Windows workflow audit #10 (platform-neutral static checks) ────────────
#
# These verify properties of the Windows PowerShell scripts by reading their
# source. They run on Linux (no pwsh required) but assert Windows-side safety
# invariants that would otherwise only be caught on a Windows host. Anything
# that genuinely requires a Windows runtime is listed in the PR description
# and in docs/livedev/OPERATOR_RUNBOOK.md "Remaining Windows-only work".

_COLLECT_PS1 = (REPO_ROOT / "testos" / "collect-results.ps1").read_text()
_INSTALL_PS1 = (REPO_ROOT / "testos" / "install.ps1").read_text()


def test_win10a_pr_is_draft_only():
    """The Windows collector must open a draft PR (no auto-merge)."""
    # The pulls payload must include draft = $true.
    assert re.search(r"draft\s*=\s*\$true", _COLLECT_PS1), \
        "collect-results.ps1 must open draft PRs (draft = $true)"
    # And must not merge.
    assert not re.search(r"/pulls/\d+/merge", _COLLECT_PS1), \
        "collect-results.ps1 must not call the merge endpoint"


def test_win10b_no_token_in_git_url():
    """The Windows clone must not embed the token in the git URL (executable code only)."""
    # Strip comment lines so a comment documenting the OLD bad behavior does
    # not produce a false positive.
    code = "\n".join(
        l for l in _COLLECT_PS1.splitlines() if not l.strip().startswith("#")
    )
    assert not re.search(r"https?://[^/\s@]+:[^/\s@]+@github\.com", code), \
        "token embedded in a git URL in executable code"
    assert "extraheader" in code, \
        "collect-results.ps1 should authenticate via extraheader, not a token URL"


def test_win10c_reparse_point_rejection_present():
    """The Windows collector must reject reparse points/junctions before copying."""
    assert "ReparsePoint" in _COLLECT_PS1, \
        "collect-results.ps1 must check for reparse points/junctions before copying"


def test_win10d_destructive_clear_after_confirmation():
    """Clear-Disk (destructive) must run AFTER the 'yes' confirmation, not before.
    We match the EXECUTABLE Clear-Disk call (with -Number), which never appears
    in comment lines."""
    confirm_pos = _INSTALL_PS1.find("Confirmation was not 'yes'")
    # The executable call, not comments mentioning it.
    clear_pos = _INSTALL_PS1.find("Clear-Disk -Number")
    assert confirm_pos != -1 and clear_pos != -1, \
        "confirmation prompt or executable Clear-Disk not found"
    assert clear_pos > confirm_pos, \
        "Clear-Disk must run after the 'yes' confirmation (currently it would destroy data before consent)"


def test_win10e_token_argv_guard_exists_in_submit_tool():
    """The cross-platform submit tool must reject tokens on the argv."""
    submit_src = (TOOLS_DIR / "rush-submit-evidence").read_text()
    assert "assert_no_token_argv" in submit_src, \
        "rush-submit-evidence must guard against token-bearing argv"


def test_win10f_windows_junction_safety_not_falsely_claimed():
    """No code path may claim Windows junction safety until verified."""
    import importlib
    sys.path.insert(0, str(TOOLS_DIR))
    rps = importlib.import_module("rush_path_safety")
    assert rps.windows_reparse_point_safety_verified() is False, \
        "Windows junction safety must NOT be claimed until a real Windows test exists"


# ─── Timing bug #9: genuinely monotonic foreground-launch ───────────────────


def _foreground_command() -> str:
    bench_list = (REPO_ROOT / "testos" / "bench-list.toml").read_text()
    m = re.search(
        r'id = "foreground-launch".*?command = (?:\'\'\'|""")(.*?)(?:\'\'\'|""")',
        bench_list, re.DOTALL,
    )
    assert m, "foreground-launch benchmark not found"
    return m.group(1)


def test_bug9_no_epochrealtime_mislabelled_monotonic():
    cmd = _foreground_command()
    exec_lines = [l for l in cmd.split("\n") if l.strip() and not l.strip().startswith("#")]
    exec_text = " ".join(exec_lines)
    assert "EPOCHREALTIME" not in exec_text, \
        "foreground-launch must not use wall-clock EPOCHREALTIME as a timing source"
    assert "perf_counter_ns" in exec_text, \
        "foreground-launch must use a genuinely monotonic clock (perf_counter_ns)"


def test_bug9_command_produces_finite_nonzero():
    cmd = _foreground_command().strip()
    r = subprocess.run(["bash", "-c", cmd], capture_output=True, text=True, timeout=30)
    assert r.returncode == 0, f"command failed: {r.stderr}"
    val = float(r.stdout.strip())
    assert val > 0 and val == val, f"non-positive or non-finite: {val}"  # NaN != NaN


# ─── Standalone runner ──────────────────────────────────────────────────────


def _run_all_tests() -> int:
    test_funcs = [
        (n, o) for n, o in sorted(globals().items())
        if n.startswith("test_") and callable(o)
    ]
    passed = failed = 0
    for name, func in test_funcs:
        try:
            func()
            print(f"  PASS {name}")
            passed += 1
        except Exception as e:
            import traceback
            print(f"  FAIL {name}: {e}")
            traceback.print_exc()
            failed += 1
    print(f"\n{passed} passed, {failed} failed, {passed + failed} total")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(_run_all_tests())
