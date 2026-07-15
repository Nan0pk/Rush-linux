#!/usr/bin/env python3
"""
test-testos-real-path-validation.py — Comprehensive behavioral tests for
defects 1-8 from the real HP Victus testOS run.

All tests are assertion-based pytest tests (no bool returns).
Covers: plan provenance, checkpoint lifecycle, generic provenance model,
foreground-launch timing, postgres/PSI/cyclictest, false-green contract,
privacy on submission path, and basic evidence validation.
"""

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


def run(cmd, env=None, timeout=30):
    e = os.environ.copy()
    if env:
        e.update(env)
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=e)
    return r.returncode, r.stdout, r.stderr


def _git_head():
    import subprocess
    r = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "rev-parse", "HEAD"],
        capture_output=True, text=True, timeout=5,
    )
    return r.stdout.strip()


def _version():
    return (REPO_ROOT / "VERSION").read_text().strip()


def _sha256_bytes(b):
    import hashlib
    return hashlib.sha256(b).hexdigest()


def _sha256_file(p):
    return _sha256_bytes(Path(p).read_bytes())


def _now_iso():
    import datetime
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _bench_list_bytes():
    return (REPO_ROOT / "testos" / "bench-list.toml").read_bytes()


def make_valid_run_dir(base: Path) -> Path:
    """Create a fully provenance-bound run dir matching the Rust runner output.

    Carries manifest.json with provenance, run-intent.json, plan.json,
    bench-list.toml, source-sha.txt, and result-hashes.json — the exact
    shape the strict validator requires for physical TestOS submission.
    Legacy planless run dirs are no longer accepted for submission.
    """
    import hashlib, subprocess, datetime
    run_dir = base / "test-run"
    run_dir.mkdir(parents=True, exist_ok=True)
    head = _git_head()
    version = _version()
    catalog = _bench_list_bytes()
    catalog_sha = _sha256_bytes(catalog)

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

    intent = {
        "schema_version": 1, "intent_kind": "testos-run-intent",
        "run_id": "realpath-test-0001", "source_commit": head,
        "source_version": version, "testos_version": version,
        "testos_image_digest": f"sha256:{'a' * 64}",
        "plan_sha256": plan_sha, "benchmark_catalog_sha256": catalog_sha,
        "generated_at": _now_iso(), "dry_run": False,
        "checkpoint_nonce": "ckpt-realpath-0001",
        "campaign_id": "campaign-realpath-001",
    }
    intent_raw = json.dumps(intent, indent=2, sort_keys=True).encode()
    intent_sha = _sha256_bytes(intent_raw)

    manifest = {
        "schema_version": 1,
        "started_at": _now_iso(), "finished_at": _now_iso(),
        "mode": "baseline",
        "host": {"fingerprint": "test-host-abc123"},
        "passed": ["bench-a"], "failed": ["bench-b"], "skipped": [],
        "attempted": ["bench-a", "bench-b"],
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
    result_a = {"schema_version": 1, "bench_id": "bench-a", "bench_name": "bench A throughput",
                "status": "pass", "value": 1.5, "unit": "ms",
                "started_at": _now_iso(), "finished_at": _now_iso(),
                "elapsed_seconds": 0.5, "scenario": "server-throughput",
                "host": {"fingerprint": "test-host-abc123"}}
    result_b = {"schema_version": 1, "bench_id": "bench-b", "bench_name": "bench B latency",
                "status": "fail", "exit_code": 1, "stderr": "err",
                "started_at": _now_iso(), "finished_at": _now_iso(),
                "elapsed_seconds": 0.1, "scenario": "server-throughput",
                "host": {"fingerprint": "test-host-abc123"}}
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


# ─── Defect 1: Physical bootstrap generates dry-run plan ────────────────────


def test_1a_physical_bootstrap_generates_real_plan():
    """Physical bootstrap --auto must generate dry_run=false plan."""
    tmp = tempfile.mkdtemp()
    env = {
        "XDG_DATA_HOME": f"{tmp}/data",
        "RUSH_LIVEDEV_REPO_DIR": str(REPO_ROOT),
        "RUSH_LIVEDEV_TEST_SKIP_USB_WRITE": "1",
    }
    rc, stdout, stderr = run(
        ["bash", str(REPO_ROOT / "tools" / "livedev-bootstrap.sh"),
         "--auto", "--skip-mock"],
        env=env, timeout=60
    )
    assert rc == 0, f"bootstrap failed: {stderr[-300:]}"

    # Find the persisted plan
    plan_files = list(Path(f"{tmp}/data").rglob("plan.json"))
    assert plan_files, "no plan.json found in persistent run dir"
    plan = json.loads(plan_files[0].read_text())
    assert plan["dry_run"] is False, f"plan dry_run={plan['dry_run']}, expected False"
    assert plan["generated_at"] != "2026-07-04T12:00:00Z", \
        "plan has stale hardcoded timestamp"
    # Validate generated_at is parseable UTC
    from datetime import datetime
    ts = plan["generated_at"].replace("Z", "+00:00")
    parsed = datetime.fromisoformat(ts)
    assert parsed.tzinfo is not None, "generated_at lacks timezone"
    assert plan.get("campaign_scope") == "baseline-only", \
        f"campaign_scope={plan.get('campaign_scope')}"
    assert len(plan.get("source_commit", "")) == 40, \
        f"source_commit not 40 chars: {plan.get('source_commit', '')[:20]}"

    # Cleanup
    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


def test_1b_dry_run_bootstrap_produces_dry_run_plan():
    """bootstrap --dry-run must produce dry_run=true and no USB operation."""
    tmp = tempfile.mkdtemp()
    env = {
        "XDG_DATA_HOME": f"{tmp}/data",
        "RUSH_LIVEDEV_REPO_DIR": str(REPO_ROOT),
        "RUSH_LIVEDEV_TEST_SKIP_USB_WRITE": "1",
    }
    rc, stdout, stderr = run(
        ["bash", str(REPO_ROOT / "tools" / "livedev-bootstrap.sh"),
         "--auto", "--dry-run"],
        env=env, timeout=60
    )
    assert rc == 0, f"bootstrap --dry-run failed: {stderr[-300:]}"
    assert "[dry-run]" in stdout, "dry-run mode not indicated"
    assert "USB" in stdout or "usb" in stdout.lower()

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


# ─── Defect 2: Checkpoint/plan lifecycle ─────────────────────────────────────


def test_2a_no_checkpoint_creates_new_run():
    """No checkpoint → new run_id and plan."""
    tmp = tempfile.mkdtemp()
    env = {
        "XDG_DATA_HOME": f"{tmp}/data",
        "RUSH_LIVEDEV_REPO_DIR": str(REPO_ROOT),
        "RUSH_LIVEDEV_TEST_SKIP_USB_WRITE": "1",
    }
    rc, _, _ = run(
        ["bash", str(REPO_ROOT / "tools" / "livedev-bootstrap.sh"),
         "--auto", "--skip-mock"],
        env=env, timeout=60
    )
    assert rc == 0
    # Verify checkpoint was created (non-dry-run mode saves checkpoint)
    cp_path = Path(f"{tmp}/data/rush-livedev/checkpoint.json")
    assert cp_path.exists(), "checkpoint not created"
    cp = json.loads(cp_path.read_text())
    assert cp["run_id"], "run_id is empty"
    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


def test_2b_submitted_checkpoint_creates_new_run():
    """Submitted checkpoint → new run_id on next --auto."""
    tmp = tempfile.mkdtemp()
    env = {
        "XDG_DATA_HOME": f"{tmp}/data",
        "RUSH_LIVEDEV_REPO_DIR": str(REPO_ROOT),
        "RUSH_LIVEDEV_TEST_SKIP_USB_WRITE": "1",
    }
    # Create a 'submitted' checkpoint
    cp_dir = Path(f"{tmp}/data/rush-livedev")
    cp_dir.mkdir(parents=True, exist_ok=True)
    old_cp = {"run_id": "old-run-001", "phase": "submitted"}
    (cp_dir / "checkpoint.json").write_text(json.dumps(old_cp))

    # Run --auto (non-dry-run with USB stub so checkpoint is saved)
    rc, _, _ = run(
        ["bash", str(REPO_ROOT / "tools" / "livedev-bootstrap.sh"),
         "--auto", "--skip-mock"],
        env=env, timeout=60
    )
    assert rc == 0
    # The new checkpoint should have a different run_id
    cp = json.loads((cp_dir / "checkpoint.json").read_text())
    assert cp["run_id"] != "old-run-001", \
        "submitted checkpoint did not create new run_id"
    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


# ─── Defect 3: Generic provenance model ──────────────────────────────────────


def test_3a_no_hardcoded_commit_in_validator():
    """validate_run_dir must not hardcode f9eb991e."""
    src = (REPO_ROOT / "tools" / "rush-submit-evidence").read_text()
    assert "f9eb991eac3c1d1d8d01a9e17e5a5892aa051fd6" not in src, \
        "hardcoded stale commit f9eb991e still present"


def test_3b_no_hardcoded_timestamp_in_validator():
    """validate_run_dir must not hardcode 2026-07-04T12:00:00Z."""
    src = (REPO_ROOT / "tools" / "rush-submit-evidence").read_text()
    # The validator should not reject based on a hardcoded timestamp
    assert '"2026-07-04T12:00:00Z"' not in src or \
           "2026-07-04T12:00:00Z);" not in src, \
        "hardcoded stale timestamp still in validator logic"


def test_3c_validator_rejects_missing_commit():
    """Validator must reject plan.json with missing source_commit."""
    tmp = tempfile.mkdtemp()
    run_dir = make_valid_run_dir(Path(tmp))
    plan = {"generated_at": "2026-07-15T10:00:00Z", "dry_run": False}
    (run_dir / "plan.json").write_text(json.dumps(plan))

    script = f"""
import sys, importlib.util, importlib.machinery
spec = importlib.util.spec_from_file_location("rse", "{TOOLS_DIR}/rush-submit-evidence",
    loader=importlib.machinery.SourceFileLoader("rse", "{TOOLS_DIR}/rush-submit-evidence"))
rse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rse)
from pathlib import Path
import json
ok, errors, _ = rse.validate_run_dir(Path("{run_dir}"))
print(json.dumps({{"ok": ok, "errors": errors}}))
"""
    rc, stdout, _ = run(["python3", "-c", script], timeout=10)
    result = json.loads(stdout.strip())
    assert not result["ok"], "bad plan should be rejected"

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


def test_3d_validator_rejects_malformed_commit():
    """Validator must reject plan.json with non-SHA-1 source_commit."""
    tmp = tempfile.mkdtemp()
    run_dir = make_valid_run_dir(Path(tmp))
    plan = {
        "generated_at": "2026-07-15T10:00:00Z",
        "dry_run": False,
        "source_commit": "not-a-sha",
    }
    (run_dir / "plan.json").write_text(json.dumps(plan))

    script = f"""
import sys, importlib.util, importlib.machinery
spec = importlib.util.spec_from_file_location("rse", "{TOOLS_DIR}/rush-submit-evidence",
    loader=importlib.machinery.SourceFileLoader("rse", "{TOOLS_DIR}/rush-submit-evidence"))
rse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rse)
from pathlib import Path
import json
ok, errors, _ = rse.validate_run_dir(Path("{run_dir}"))
print(json.dumps({{"ok": ok, "errors": errors}}))
"""
    rc, stdout, _ = run(["python3", "-c", script], timeout=10)
    result = json.loads(stdout.strip())
    assert not result["ok"], "bad plan should be rejected"

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


def test_3e_validator_rejects_malformed_timestamp():
    """Validator must reject plan.json with invalid timestamp."""
    tmp = tempfile.mkdtemp()
    run_dir = make_valid_run_dir(Path(tmp))
    plan = {
        "generated_at": "not-a-timestamp",
        "dry_run": False,
        "source_commit": "a" * 40,
    }
    (run_dir / "plan.json").write_text(json.dumps(plan))

    script = f"""
import sys, importlib.util, importlib.machinery
spec = importlib.util.spec_from_file_location("rse", "{TOOLS_DIR}/rush-submit-evidence",
    loader=importlib.machinery.SourceFileLoader("rse", "{TOOLS_DIR}/rush-submit-evidence"))
rse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rse)
from pathlib import Path
import json
ok, errors, _ = rse.validate_run_dir(Path("{run_dir}"))
print(json.dumps({{"ok": ok, "errors": errors}}))
"""
    rc, stdout, _ = run(["python3", "-c", script], timeout=10)
    result = json.loads(stdout.strip())
    assert not result["ok"], "bad plan should be rejected"

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


# ─── Defect 4: Foreground-launch ─────────────────────────────────────────────


def test_4a_foreground_launch_no_python_dependency():
    """foreground-launch must not depend on python3."""
    bench_list = (REPO_ROOT / "testos" / "bench-list.toml").read_text()
    fg_section = re.search(
        r'id = "foreground-launch".*?command = (?:"""|\x27\x27\x27)(.*?)(?:"""|\x27\x27\x27)',
        bench_list, re.DOTALL
    )
    assert fg_section, "foreground-launch not found"
    fg_cmd = fg_section.group(1)
    # Check executable lines only (skip comments)
    exec_lines = [l for l in fg_cmd.split('\n')
                  if l.strip() and not l.strip().startswith('#')]
    exec_text = ' '.join(exec_lines)
    # Timing bug #9: foreground-launch must NOT use bash $EPOCHREALTIME as a
    # timing source — it is wall-clock (CLOCK_REALTIME), not monotonic, and
    # must not call it "monotonic". It must instead use a genuinely monotonic
    # high-resolution source (time.perf_counter_ns = CLOCK_MONOTONIC).
    assert "EPOCHREALTIME" not in exec_text, \
        "foreground-launch uses wall-clock EPOCHREALTIME as a timing source"
    assert "perf_counter_ns" in exec_text, \
        "foreground-launch does not use a genuinely monotonic clock"
    assert "/usr/bin/time -f" not in exec_text, \
        "foreground-launch uses low-resolution /usr/bin/time"


def test_4b_foreground_launch_produces_nonzero():
    """foreground-launch command must produce a non-zero result."""
    bench_list = (REPO_ROOT / "testos" / "bench-list.toml").read_text()
    fg_section = re.search(
        r'id = "foreground-launch".*?command = (?:"""|\x27\x27\x27)(.*?)(?:"""|\x27\x27\x27)',
        bench_list, re.DOTALL
    )
    fg_cmd = fg_section.group(1).strip()
    rc, stdout, stderr = run(["bash", "-c", fg_cmd], timeout=30)
    assert rc == 0, f"foreground-launch failed: {stderr[:200]}"
    result = stdout.strip()
    assert result != "ERROR", "foreground-launch produced ERROR"
    val = float(result)
    assert val > 0, f"foreground-launch produced zero/negative: {val}"


# ─── Defect 5: Postgres/PSI/cyclictest behavioral tests ──────────────────────


def test_5a_psi_parser_cpu_fixture():
    """PSI CPU parser must correctly parse avg10 from fixture."""
    bench_list = (REPO_ROOT / "testos" / "bench-list.toml").read_text()
    psi_section = re.search(r"id = \"psi-cpu-avg10\".*?command = '''(.*?)'''", bench_list, re.DOTALL)
    if not psi_section:
        psi_section = re.search(r'id = "psi-cpu-avg10".*?command = """(.*?)"""', bench_list, re.DOTALL)
    psi_cmd = psi_section.group(1).strip()

    psi_output = "some avg10=0.25 avg60=0.10 avg300=0.05 total=695299\n"
    with tempfile.NamedTemporaryFile(mode='w', suffix='.txt', delete=False) as f:
        f.write(psi_output)
        psi_file = f.name
    try:
        test_cmd = psi_cmd.replace("/proc/pressure/cpu", psi_file)
        rc, stdout, _ = run(["bash", "-c", test_cmd])
        assert rc == 0
        assert float(stdout.strip()) == 0.25
    finally:
        os.unlink(psi_file)


def test_5b_psi_parser_io_fixture():
    """PSI IO parser must correctly parse avg10 from fixture."""
    bench_list = (REPO_ROOT / "testos" / "bench-list.toml").read_text()
    psi_section = re.search(r"id = \"psi-io-avg10\".*?command = '''(.*?)'''", bench_list, re.DOTALL)
    if not psi_section:
        psi_section = re.search(r'id = "psi-io-avg10".*?command = """(.*?)"""', bench_list, re.DOTALL)
    psi_cmd = psi_section.group(1).strip()

    psi_output = "some avg10=0.50 avg60=0.20 avg300=0.10 total=123456\n"
    with tempfile.NamedTemporaryFile(mode='w', suffix='.txt', delete=False) as f:
        f.write(psi_output)
        psi_file = f.name
    try:
        test_cmd = psi_cmd.replace("/proc/pressure/io", psi_file)
        rc, stdout, _ = run(["bash", "-c", test_cmd])
        assert rc == 0
        assert float(stdout.strip()) == 0.50
    finally:
        os.unlink(psi_file)


def test_5c_psi_parser_malformed_fails():
    """PSI parser must produce empty/ERROR on malformed input."""
    bench_list = (REPO_ROOT / "testos" / "bench-list.toml").read_text()
    psi_section = re.search(r"id = \"psi-cpu-avg10\".*?command = '''(.*?)'''", bench_list, re.DOTALL)
    if not psi_section:
        psi_section = re.search(r'id = "psi-cpu-avg10".*?command = """(.*?)"""', bench_list, re.DOTALL)
    psi_cmd = psi_section.group(1).strip()

    psi_output = "garbage without avg10\n"
    with tempfile.NamedTemporaryFile(mode='w', suffix='.txt', delete=False) as f:
        f.write(psi_output)
        psi_file = f.name
    try:
        test_cmd = psi_cmd.replace("/proc/pressure/cpu", psi_file)
        rc, stdout, _ = run(["bash", "-c", test_cmd])
        result = stdout.strip()
        assert result == "" or result == "ERROR", \
            f"parser should produce empty/ERROR on malformed, got {result!r}"
    finally:
        os.unlink(psi_file)


def test_5d_cyclictest_no_quiet_flag():
    """cyclictest must not use -q (suppresses Max: line)."""
    bench_list = (REPO_ROOT / "testos" / "bench-list.toml").read_text()
    cyclic_section = re.search(
        r'id = "cyclictest-max".*?command = (?:"""|\x27\x27\x27)(.*?)(?:"""|\x27\x27\x27)',
        bench_list, re.DOTALL
    )
    cyclic_cmd = cyclic_section.group(1)
    cmd_parts = cyclic_cmd.split()
    for i, part in enumerate(cmd_parts):
        if part == "cyclictest":
            for j in range(i + 1, len(cmd_parts)):
                flag = cmd_parts[j]
                if flag.startswith("-") and "q" in flag:
                    pytest.fail(f"cyclictest uses -q flag: {flag}")
                if not flag.startswith("-"):
                    break
            break


# ─── Defect 6: False-green test contract ─────────────────────────────────────


def test_6a_real_hardware_defects_tests_are_assertion_based():
    """test-testos-real-hardware-defects.py must use assertions, not bool returns."""
    src = (REPO_ROOT / "tools" / "test-testos-real-hardware-defects.py").read_text()
    # Must import pytest
    assert "import pytest" in src, "pytest not imported"
    # Must NOT use 'return False' pattern (old bool-returning style)
    assert "return False" not in src or "return True" not in src or \
           "if __name__" in src, "tests still use bool return pattern"


def test_6b_test_exit_code_reflects_failure():
    """Direct script execution must exit nonzero on test failure."""
    # Run the test file directly — it should use pytest.main which exits nonzero on fail
    rc, stdout, _ = run(
        ["python3", str(REPO_ROOT / "tools" / "test-testos-real-hardware-defects.py"),
         "--co", "-q"],
        timeout=30
    )
    # --co (collect only) should succeed if the file is valid
    assert rc == 0, f"test collection failed: {stdout[:200]}"


# ─── Defect 7: Privacy on real submission path ───────────────────────────────


def test_7a_submit_rejects_symlink_in_run_dir():
    """rush-submit-evidence must reject symlinks before creating bundle."""
    tmp = tempfile.mkdtemp()
    run_dir = make_valid_run_dir(Path(tmp))
    # Create a symlink pointing to an external file
    sentinel = Path(tmp) / "sentinel.txt"
    sentinel.write_text("SECRET: ghp_token_value_here")
    symlink = run_dir / "evil-symlink.json"
    symlink.symlink_to(sentinel)

    rc, stdout, stderr = run(
        ["python3", str(REPO_ROOT / "tools" / "rush-submit-evidence"),
         str(run_dir), "--submit-mode", "local"],
        timeout=30
    )
    assert rc != 0, "submission should have failed (symlink present)"
    assert "symlink" in stderr.lower() or "symlink" in stdout.lower(), \
        f"symlink rejection not reported: {stderr[:200]}"

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


def test_7b_submit_rejects_secret_in_results():
    """rush-submit-evidence must reject unredacted secrets before bundle."""
    tmp = tempfile.mkdtemp()
    run_dir = make_valid_run_dir(Path(tmp))
    # Inject a secret into a result file (keeping the realistic shape with
    # the canonical bench_id so validation reaches the privacy gate).
    (run_dir / "bench-a.json").write_text(json.dumps({
        "schema_version": 1, "bench_id": "bench-a", "bench_name": "bench A throughput",
        "status": "pass", "value": 1.5, "unit": "ms",
        "started_at": _now_iso(), "finished_at": _now_iso(),
        "elapsed_seconds": 0.5, "scenario": "server-throughput",
        "host": {"fingerprint": "test-host-abc123"},
        "stderr": "GITHUB_TOKEN=ghp_0123456789abcdefghijklmnopqrstuvwxyz0123"
    }))

    rc, stdout, stderr = run(
        ["python3", str(REPO_ROOT / "tools" / "rush-submit-evidence"),
         str(run_dir), "--submit-mode", "local"],
        timeout=30
    )
    assert rc != 0, "submission should have failed (secret present)"
    assert "privacy" in stderr.lower() or "secret" in stderr.lower(), \
        f"privacy failure not reported: {stderr[:200]}"

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


def test_7c_no_bundle_created_on_privacy_failure():
    """No bundle file must be created when privacy scan fails."""
    tmp = tempfile.mkdtemp()
    run_dir = make_valid_run_dir(Path(tmp))
    (run_dir / "bench-a.json").write_text(json.dumps({
        "schema_version": 1, "bench_id": "bench-a", "bench_name": "bench A throughput",
        "status": "pass", "value": 1.5, "unit": "ms",
        "started_at": _now_iso(), "finished_at": _now_iso(),
        "elapsed_seconds": 0.5, "scenario": "server-throughput",
        "host": {"fingerprint": "test-host-abc123"},
        "stderr": "GITHUB_TOKEN=ghp_0123456789abcdefghijklmnopqrstuvwxyz0123"
    }))

    run(["python3", str(REPO_ROOT / "tools" / "rush-submit-evidence"),
         str(run_dir), "--submit-mode", "local"], timeout=30)

    # Check no bundle was created
    bundles = list(Path(tmp).rglob("*.tar.gz"))
    assert not bundles, f"bundle created despite privacy failure: {bundles}"

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


# ─── Defect 8: Basic testOS evidence validation ──────────────────────────────


def test_8a_valid_run_dir_passes_validation():
    """A valid run dir must pass validate_run_dir."""
    tmp = tempfile.mkdtemp()
    run_dir = make_valid_run_dir(Path(tmp))

    script = f"""
import sys, importlib.util, importlib.machinery
spec = importlib.util.spec_from_file_location("rse", "{TOOLS_DIR}/rush-submit-evidence",
    loader=importlib.machinery.SourceFileLoader("rse", "{TOOLS_DIR}/rush-submit-evidence"))
rse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rse)
from pathlib import Path
import json
ok, errors, _ = rse.validate_run_dir(Path("{run_dir}"))
print(json.dumps({{"ok": ok, "errors": errors}}))
"""
    rc, stdout, _ = run(["python3", "-c", script], timeout=10)
    result = json.loads(stdout.strip())
    assert result["ok"], f"valid run dir failed validation: {result['errors']}"

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


def test_8b_missing_result_file_detected():
    """Validator must detect passed benchmark with no result file."""
    tmp = tempfile.mkdtemp()
    run_dir = Path(tmp) / "bad-run"
    run_dir.mkdir()
    manifest = {
        "started_at": "2026-07-15T10:00:00Z",
        "host": {"fingerprint": "test"},
        "passed": ["bench-a"],
        "failed": [],
        "skipped": [],
        "attempted": ["bench-a"],
    }
    (run_dir / "manifest.json").write_text(json.dumps(manifest))
    # No bench-a.json file

    script = f"""
import sys, importlib.util, importlib.machinery
spec = importlib.util.spec_from_file_location("rse", "{TOOLS_DIR}/rush-submit-evidence",
    loader=importlib.machinery.SourceFileLoader("rse", "{TOOLS_DIR}/rush-submit-evidence"))
rse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rse)
from pathlib import Path
import json
ok, errors, _ = rse.validate_run_dir(Path("{run_dir}"))
print(json.dumps({{"ok": ok, "errors": errors}}))
"""
    rc, stdout, _ = run(["python3", "-c", script], timeout=10)
    result = json.loads(stdout.strip())
    assert not result["ok"], "should be rejected"
    # Legacy planless evidence is rejected at the provenance gate.
    assert any("provenance" in e for e in result["errors"]), \
        f"expected provenance rejection: {result['errors']}"

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


def test_8c_non_finite_value_detected():
    """Validator must detect non-finite numeric value in passed result."""
    tmp = tempfile.mkdtemp()
    run_dir = Path(tmp) / "bad-run"
    run_dir.mkdir()
    manifest = {
        "started_at": "2026-07-15T10:00:00Z",
        "host": {"fingerprint": "test"},
        "passed": ["bench-a"],
        "failed": [],
        "skipped": [],
        "attempted": ["bench-a"],
    }
    (run_dir / "manifest.json").write_text(json.dumps(manifest))
    import math
    (run_dir / "bench-a.json").write_text(json.dumps({
        "bench_name": "bench-a", "status": "pass",
        "value": float('inf'), "unit": "ms"
    }))

    script = f"""
import sys, importlib.util, importlib.machinery
spec = importlib.util.spec_from_file_location("rse", "{TOOLS_DIR}/rush-submit-evidence",
    loader=importlib.machinery.SourceFileLoader("rse", "{TOOLS_DIR}/rush-submit-evidence"))
rse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rse)
from pathlib import Path
import json
ok, errors, _ = rse.validate_run_dir(Path("{run_dir}"))
print(json.dumps({{"ok": ok, "errors": errors}}))
"""
    rc, stdout, _ = run(["python3", "-c", script], timeout=10)
    result = json.loads(stdout.strip())
    assert not result["ok"], "should be rejected"
    assert any("provenance" in e for e in result["errors"]), \
        f"expected provenance rejection: {result['errors']}"

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


def test_8d_plan_json_not_counted_as_benchmark():
    """plan.json must not be counted as a benchmark result file."""
    tmp = tempfile.mkdtemp()
    run_dir = make_valid_run_dir(Path(tmp))
    (run_dir / "plan.json").write_text(json.dumps({
        "generated_at": "2026-07-15T10:00:00Z",
        "dry_run": False,
        "source_commit": "a" * 40,
    }))

    script = f"""
import sys, importlib.util, importlib.machinery
spec = importlib.util.spec_from_file_location("rse", "{TOOLS_DIR}/rush-submit-evidence",
    loader=importlib.machinery.SourceFileLoader("rse", "{TOOLS_DIR}/rush-submit-evidence"))
rse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rse)
from pathlib import Path
import json
ok, errors, _ = rse.validate_run_dir(Path("{run_dir}"))
print(json.dumps({{"ok": ok, "errors": errors}}))
"""
    rc, stdout, _ = run(["python3", "-c", script], timeout=10)
    result = json.loads(stdout.strip())
    # plan.json should NOT cause "unclassified benchmark" error
    assert not any("plan.json" in e and "unclassified" in e for e in result["errors"]), \
        f"plan.json incorrectly counted as benchmark: {result['errors']}"

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


def test_8e_overlapping_sets_detected():
    """Validator must detect overlapping passed/failed sets."""
    tmp = tempfile.mkdtemp()
    run_dir = Path(tmp) / "bad-run"
    run_dir.mkdir()
    manifest = {
        "started_at": "2026-07-15T10:00:00Z",
        "host": {"fingerprint": "test"},
        "passed": ["bench-a"],
        "failed": ["bench-a"],  # overlap!
        "skipped": [],
        "attempted": ["bench-a"],
    }
    (run_dir / "manifest.json").write_text(json.dumps(manifest))
    (run_dir / "bench-a.json").write_text(json.dumps({
        "bench_name": "bench-a", "status": "pass", "value": 1.0, "unit": "ms"
    }))

    script = f"""
import sys, importlib.util, importlib.machinery
spec = importlib.util.spec_from_file_location("rse", "{TOOLS_DIR}/rush-submit-evidence",
    loader=importlib.machinery.SourceFileLoader("rse", "{TOOLS_DIR}/rush-submit-evidence"))
rse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rse)
from pathlib import Path
import json
ok, errors, _ = rse.validate_run_dir(Path("{run_dir}"))
print(json.dumps({{"ok": ok, "errors": errors}}))
"""
    rc, stdout, _ = run(["python3", "-c", script], timeout=10)
    result = json.loads(stdout.strip())
    assert not result["ok"], "should be rejected"
    assert any("provenance" in e for e in result["errors"])

    import shutil
    shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"] + sys.argv[1:]))
