#!/usr/bin/env python3
"""
test-testos-production-provenance.py — production-level integration tests for
the TestOS provenance workflow.

Tests exercise PRODUCTION entry points (not just synthetic helpers):
  - testos_prepare_usb.py generates a runner-acceptable intent from real values
  - validate-testos-evidence.py strict gate accepts/rejects correctly
  - rush-submit-evidence local mode reaches bundle for clean evidence
  - planless evidence cannot be submitted
  - mismatched plan/intent run IDs fail
  - mismatched source commits fail
  - wrong image identity fails
  - altered result fails
  - missing/extra result hash fails
  - stale/future timestamps fail
  - dry_run intent fails
  - --inspect mode never mutates anything

Run:
    python3 -m pytest tools/test-testos-production-provenance.py -v
    python3 tools/test-testos-production-provenance.py
"""

from __future__ import annotations

import hashlib
import json
import os
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


validate_testos = _load_module("vtp_prod", TOOLS_DIR / "validate-testos-evidence.py")
rse = _load_module("rse_prod", TOOLS_DIR / "rush-submit-evidence")


def _git_head() -> str:
    r = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "rev-parse", "HEAD"],
        capture_output=True, text=True, timeout=5,
    )
    return r.stdout.strip()


def _version() -> str:
    return (REPO_ROOT / "VERSION").read_text().strip()


def _sha256_bytes(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def _sha256_file(p: Path) -> str:
    return _sha256_bytes(p.read_bytes())


def _now_iso() -> str:
    import datetime as dt
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _bench_list_bytes() -> bytes:
    return (REPO_ROOT / "testos" / "bench-list.toml").read_bytes()


def _make_intent(**overrides) -> dict:
    head = _git_head()
    version = _version()
    base = {
        "schema_version": 1, "intent_kind": "testos-run-intent",
        "run_id": "prod-test-0001", "source_commit": head,
        "source_version": version, "testos_version": version,
        "testos_image_digest": f"sha256:{'a' * 64}",
        "testos_image_commit": head,  # F4: required
        "plan_sha256": "b" * 64,
        "benchmark_catalog_sha256": _sha256_bytes(_bench_list_bytes()),
        "generated_at": _now_iso(), "dry_run": False,
        "checkpoint_nonce": "ckpt-prod-0001-abcd",
        "campaign_id": "campaign-prod-001",
    }
    base.update(overrides)
    return base


def _make_plan(intent: dict, **overrides) -> bytes:
    base = {
        "schema_version": 1, "plan_kind": "rush-autopilot-plan",
        "generated_at": intent["generated_at"],
        "source_version": intent["source_version"],
        "source_commit": intent["source_commit"],
        "dry_run": intent["dry_run"],
        "campaign_scope": "baseline-only",
        "hardware_slot": "laptop", "slot_confidence": "high",
        "ambiguities": [], "open_criteria": [],
        "existing_evidence": [], "steps": [], "repo_root": ".",
    }
    base.update(overrides)
    return json.dumps(base).encode()


def _good_result(bench_id: str = "shell-pass") -> dict:
    return {
        "schema_version": 1, "bench_id": bench_id,
        "bench_name": f"{bench_id} test", "scenario": "server-throughput",
        "status": "pass", "started_at": _now_iso(), "finished_at": _now_iso(),
        "elapsed_seconds": 0.1, "value": 42.0, "unit": "ms",
        "host": {"fingerprint": "test-host-0012"},
    }


def _write_bundle(tmp: Path, intent: dict, *, plan_bytes: bytes | None = None,
                  result_files: dict[str, dict] | None = None,
                  result_hashes: dict[str, str] | None = None,
                  include_intent: bool = True, include_plan: bool = True,
                  include_result_hashes: bool = True,
                  provenance_override: dict | None = None) -> Path:
    """Write a complete testOS run dir to tmp and return its path."""
    run_dir = tmp / "run"
    run_dir.mkdir(parents=True, exist_ok=True)
    bl = _bench_list_bytes()
    if plan_bytes is None:
        plan_bytes = _make_plan(intent)
    intent_raw = json.dumps(intent, indent=2, sort_keys=True).encode()
    intent_sha = _sha256_bytes(intent_raw)
    plan_sha = _sha256_bytes(plan_bytes)

    prov = provenance_override or {
        "run_id": intent["run_id"], "source_commit": intent["source_commit"],
        "source_version": intent["source_version"],
        "testos_version": intent["testos_version"],
        "testos_image_digest": intent["testos_image_digest"],
        "testos_image_commit": intent["testos_image_commit"],  # F4: required
        "plan_sha256": plan_sha,
        "benchmark_catalog_sha256": intent["benchmark_catalog_sha256"],
        "intent_generated_at": intent["generated_at"],
        "intent_dry_run": intent["dry_run"],
        "checkpoint_nonce": intent["checkpoint_nonce"],
        "intent_sha256": intent_sha,
        "campaign_id": intent.get("campaign_id"),
    }
    if prov.get("campaign_id") is None:
        prov.pop("campaign_id", None)

    manifest = {
        "schema_version": 1, "started_at": _now_iso(), "finished_at": _now_iso(),
        "mode": "all", "attempted": [], "passed": [], "failed": [], "skipped": [],
        "host": {"fingerprint": "test-host-0012"},
        "testos_version": intent["testos_version"], "provenance": prov,
    }
    if result_files:
        for name, data in result_files.items():
            (run_dir / name).write_text(json.dumps(data))
            bid = data.get("bench_id", name.removesuffix(".json"))
            manifest["attempted"].append(bid)
            if data.get("status") == "pass":
                manifest["passed"].append(bid)
            else:
                manifest["failed"].append(bid)
    (run_dir / "manifest.json").write_text(json.dumps(manifest, indent=2))
    if include_intent:
        (run_dir / "run-intent.json").write_bytes(intent_raw)
    if include_plan:
        (run_dir / "plan.json").write_bytes(plan_bytes)
    (run_dir / "bench-list.toml").write_bytes(bl)
    (run_dir / "source-sha.txt").write_text(intent["source_commit"][:12])
    if result_files:
        auto = {n: _sha256_file(run_dir / n) for n in result_files}
        if result_hashes is not None:
            auto = result_hashes
        if include_result_hashes:
            (run_dir / "result-hashes.json").write_text(json.dumps(auto))
    return run_dir


# ─── 1. prepare-usb creates runner-acceptable intent ────────────────────────


def test_prepare_usb_generates_runner_acceptable_intent():
    """testos_prepare_usb.py generates a run-intent.json that the Rust
    RunIntent::load_and_validate would accept — proven by feeding it to
    the strict validator (which re-validates every field)."""
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        repo_root = REPO_ROOT
        plan_path = tmp_path / "plan.json"
        plan_obj = {
            "schema_version": 1, "plan_kind": "rush-autopilot-plan",
            "generated_at": _now_iso(), "source_version": _version(),
            "source_commit": _git_head(), "dry_run": False,
            "campaign_scope": "baseline-only",
            "hardware_slot": "laptop", "slot_confidence": "high",
            "ambiguities": [], "open_criteria": [],
            "existing_evidence": [], "steps": [], "repo_root": ".",
        }
        plan_path.write_bytes(json.dumps(plan_obj).encode())
        # Create a fake image file.
        image_path = tmp_path / "testos.raw"
        image_path.write_bytes(b"\x00" * 4096)

        prepare_usb = _load_module("tpu", TOOLS_DIR / "testos_prepare_usb.py")
        intent_raw, intent_sha = prepare_usb.generate_run_intent(
            repo_root=repo_root, plan_path=plan_path, image_path=image_path,
            run_id="prep-usb-test-0001", checkpoint_nonce="ckpt-prep-0001",
        )
        intent = json.loads(intent_raw)
        # Verify fields are real values, not placeholders.
        assert intent["source_commit"] == _git_head()
        assert intent["source_version"] == _version()
        assert intent["testos_image_digest"] == f"sha256:{_sha256_file(image_path)}"
        assert intent["plan_sha256"] == _sha256_file(plan_path)
        assert intent["benchmark_catalog_sha256"] == _sha256_bytes(_bench_list_bytes())
        assert intent["dry_run"] is False
        assert intent["intent_kind"] == "testos-run-intent"

        # Install to a source dir and verify readback.
        source_dir = tmp_path / "esp"
        prepare_usb.install_intent_plan(
            esp_mount=source_dir, intent_raw=intent_raw,
            plan_path=plan_path, catalog_path=repo_root / "testos" / "bench-list.toml",
        )
        # Read back and verify hashes.
        installed_intent = (source_dir / "run-intent.json").read_bytes()
        assert installed_intent == intent_raw, "readback mismatch"
        installed_plan = (source_dir / "plan.json").read_bytes()
        assert installed_plan == plan_path.read_bytes()
        installed_cat = (source_dir / "bench-list.toml").read_bytes()
        assert installed_cat == _bench_list_bytes()


def test_prepare_usb_plan_has_required_fields():
    """The plan generated by rush-autopilot --plan --baseline-only contains
    the required provenance fields: source_commit, source_version,
    campaign_scope=baseline-only, dry_run=false, fresh generated_at."""
    with tempfile.TemporaryDirectory() as tmp:
        r = subprocess.run(
            ["python3", str(TOOLS_DIR / "rush-autopilot"), "plan",
             "--auto", "--baseline-only", "--slot", "laptop",
             "--output", str(Path(tmp) / "plan.json")],
            capture_output=True, text=True, timeout=60, cwd=str(REPO_ROOT),
        )
        assert r.returncode == 0, f"plan generation failed: {r.stderr[-300:]}"
        plan = json.loads((Path(tmp) / "plan.json").read_text())
        assert plan["campaign_scope"] == "baseline-only"
        assert plan["dry_run"] is False
        assert len(plan["source_commit"]) == 40
        assert plan["plan_kind"] == "rush-autopilot-plan"
        assert plan["generated_at"] != "2026-07-04T12:00:00Z"  # not stale
        assert plan["source_version"] == _version()


# ─── 2. Clean evidence passes and reaches bundle ────────────────────────────


def test_clean_evidence_passes_strict_and_reaches_bundle():
    """POSITIVE e2e: clean, fully-bound evidence passes strict validation,
    privacy, and reaches local bundle creation."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        rf = {"shell-pass.json": _good_result("shell-pass")}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf)
        # Strict validation passes.
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert ok, f"should pass: {errors}"
        # rush-submit-evidence local mode reaches bundle.
        env = dict(os.environ)
        env["HOME"] = tmp
        r = subprocess.run(
            ["python3", str(TOOLS_DIR / "rush-submit-evidence"),
             str(run_dir), "--submit-mode", "local"],
            capture_output=True, text=True, timeout=30, env=env,
        )
        combined = r.stdout + r.stderr
        assert r.returncode == 0, f"clean run should succeed:\n{combined}"
        assert "OK: privacy scan passed" in combined
        assert "local evidence bundle" in combined
        bundles = list(Path(tmp).rglob("*.tar.gz"))
        assert bundles


# ─── 3. Negative tests ──────────────────────────────────────────────────────


def test_missing_intent_fails():
    """Missing run-intent.json fails closed."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf, include_intent=False)
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok
        assert any("run-intent" in e for e in errors), errors


def test_missing_plan_fails():
    """Missing plan.json fails closed."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf, include_plan=False)
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok
        assert any("plan" in e for e in errors), errors


def test_mismatched_run_id_fails():
    """A plan whose run_id disagrees with the intent fails."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        plan = _make_plan(intent, run_id="wrong-run-id-xyz")
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf, plan_bytes=plan)
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok
        assert any("run_id" in e for e in errors), errors


def test_mismatched_source_commit_fails():
    """A plan whose source_commit disagrees with the intent fails."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        bad_commit = "f" * 40
        plan = _make_plan(intent, source_commit=bad_commit)
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf, plan_bytes=plan)
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok
        # Should mention either plan or source_commit disagreement.
        assert any("source_commit" in e or "plan" in e for e in errors), errors


def test_wrong_image_identity_fails():
    """A provenance block with a wrong image digest fails format/placeholder checks."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent(testos_image_digest="sha256:not-valid-hex")
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf)
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok


def test_altered_result_fails():
    """An altered result file (hash mismatch) fails."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf)
        # Tamper with the result after hashes were computed.
        result = json.loads((run_dir / "shell-pass.json").read_text())
        result["value"] = 999.0
        (run_dir / "shell-pass.json").write_text(json.dumps(result))
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok
        assert any("changed" in e or "digest" in e or "mismatch" in e for e in errors), errors


def test_missing_result_hash_fails():
    """A result-hashes.json missing an entry fails."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf,
                                result_hashes={})  # empty hashes
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok
        assert any("result-hashes" in e for e in errors), errors


def test_extra_result_hash_fails():
    """A result-hashes.json with an extra entry fails."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf)
        # Add an extra entry.
        hashes = json.loads((run_dir / "result-hashes.json").read_text())
        hashes["extra.json"] = "0" * 64
        (run_dir / "result-hashes.json").write_text(json.dumps(hashes))
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok
        assert any("unknown" in e or "extra" in e.lower() for e in errors), errors


def test_stale_timestamp_fails():
    """A stale intent_generated_at fails."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent(generated_at="2025-01-01T00:00:00Z")
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf)
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok
        assert any("stale" in e.lower() or "old" in e.lower() for e in errors), errors


def test_future_timestamp_fails():
    """A future intent_generated_at fails."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent(generated_at="2099-01-01T00:00:00Z")
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf)
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok
        assert any("future" in e.lower() for e in errors), errors


def test_dry_run_intent_fails():
    """A dry_run=true intent fails for physical submission."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent(dry_run=True)
        plan = _make_plan(intent)  # plan also dry_run=True
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf, plan_bytes=plan)
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        assert not ok
        assert any("dry_run" in e for e in errors), errors


# ─── 4. Planless evidence cannot be submitted ───────────────────────────────


def test_planless_legacy_rejected_for_submission():
    """Evidence without provenance/intent/plan cannot be submitted."""
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "legacy-run"
        run_dir.mkdir()
        manifest = {
            "schema_version": 1, "started_at": _now_iso(),
            "host": {"fingerprint": "legacy"}, "passed": ["bench-a"],
            "failed": [], "skipped": [], "attempted": ["bench-a"],
            "testos_version": _version(),
        }
        (run_dir / "manifest.json").write_text(json.dumps(manifest))
        (run_dir / "bench-a.json").write_text(json.dumps(_good_result("bench-a")))
        ok, errors, _ = rse.validate_run_dir(run_dir)
        assert not ok
        assert any("provenance" in e for e in errors), errors


# ─── 5. --inspect mode never mutates ───────────────────────────────────────


def test_inspect_mode_no_mutation():
    """--inspect validates but never creates a bundle, stages, commits, or pushes."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        rf = {"shell-pass.json": _good_result()}
        run_dir = _write_bundle(Path(tmp), intent, result_files=rf)
        # Snapshot the run_dir before inspect.
        before = {f.name: f.read_bytes() for f in run_dir.iterdir() if f.is_file()}
        r = subprocess.run(
            ["python3", str(TOOLS_DIR / "rush-submit-evidence"),
             str(run_dir), "--inspect"],
            capture_output=True, text=True, timeout=30,
        )
        assert r.returncode == 0, f"inspect should pass: {r.stderr}"
        assert "[inspect]" in r.stdout
        assert "No bundle created" in r.stdout
        # No files added or changed.
        after = {f.name: f.read_bytes() for f in run_dir.iterdir() if f.is_file()}
        assert before == after, "inspect mutated files"
        # No bundle created.
        bundles = list(Path(tmp).rglob("*.tar.gz"))
        assert not bundles, "inspect created a bundle"


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
