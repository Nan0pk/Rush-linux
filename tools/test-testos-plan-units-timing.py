#!/usr/bin/env python3
"""
test-testos-plan-units-timing.py — Regression tests for defects 6, 7, 8.

6. Plan provenance: physical runs generate fresh plans with current UTC,
   dry_run=false, actual source_commit. Stale plans are detected.
7. Typed benchmark units: every shell-numeric bench declares and records
   its real unit (Gbit/s, requests/s, percent, us, ms).
8. Foreground-launch measurement fidelity: high-resolution monotonic timing;
   zero/non-finite results fail validation.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent


def run(cmd: list[str], timeout: int = 30) -> tuple[int, str, str]:
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    return r.returncode, r.stdout, r.stderr


def read_bench_list() -> str:
    return (REPO_ROOT / "testos" / "bench-list.toml").read_text()


# ─── Tests ───────────────────────────────────────────────────────────────────


def test_6a_physical_plan_uses_current_utc():
    """Defect 6: physical runs (--auto without --dry-run) must use current UTC."""
    autopilot = REPO_ROOT / "tools" / "rush-autopilot"
    src = autopilot.read_text()

    # The hardcoded timestamp must only be used for mock/test runs
    if 'generated_at = "2026-07-04T12:00:00Z"' not in src:
        print("FAIL: deterministic timestamp missing for mock mode")
        return False

    # Physical runs must use datetime.now(timezone.utc)
    if "datetime.now(timezone.utc)" not in src:
        print("FAIL: physical runs do not use current UTC timestamp")
        return False

    # The condition must be: args.auto and not args.dry_run
    if "args.auto and not args.dry_run" not in src:
        print("FAIL: physical-run condition not found")
        return False

    print("PASS: physical plans use current UTC; mock plans use fixed timestamp")
    return True


def test_6b_physical_plan_dry_run_false():
    """Defect 6: --auto without --dry-run must set dry_run=false."""
    autopilot = REPO_ROOT / "tools" / "rush-autopilot"
    src = autopilot.read_text()

    # The old buggy code `args.dry_run or args.auto is False` must be gone
    # from ACTIVE code (it may appear in comments explaining the fix).
    # Check that it's not in an executable line (not preceded by #).
    lines = src.split('\n')
    old_logic_active = False
    for line in lines:
        stripped = line.lstrip()
        if stripped.startswith('#'):
            continue
        if 'args.dry_run or args.auto is False' in stripped:
            old_logic_active = True
            break

    if old_logic_active:
        print("FAIL: old buggy dry_run logic still in active code")
        return False

    # New logic: if args.auto, dry_run = args.dry_run
    if "if args.auto:" not in src or "dry_run = args.dry_run" not in src:
        print("FAIL: new dry_run logic not found")
        return False

    print("PASS: --auto without --dry-run sets dry_run=false")
    return True


def test_6c_stale_plan_detected_by_validator():
    """Defect 6: validate_run_dir detects stale plan.json."""
    # Create a run_dir with a stale plan.json
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp)
        # Write a valid manifest.json
        manifest = {
            "started_at": "2026-07-15T10:00:00Z",
            "host": {"fingerprint": "test-host"},
            "passed": ["test-bench"],
            "failed": [],
            "skipped": [],
            "mode": "baseline",
        }
        (run_dir / "manifest.json").write_text(json.dumps(manifest))
        # Write a result file (realistic shape: canonical bench_id + status)
        (run_dir / "test-bench.json").write_text(
            '{"bench_id": "test-bench", "status": "pass", "value": 1.0, "unit": "ms"}')

        # Write a stale plan.json (hardcoded timestamp + dry_run=true +
        # a source_commit that does NOT exist in this repo, representing a
        # plan built from a foreign/unknown tree).
        stale_plan = {
            "generated_at": "2026-07-04T12:00:00Z",
            "dry_run": True,
            "source_commit": "f9eb991eac3c1d1d8d01a9e17e5a5892aa051faa",
        }
        (run_dir / "plan.json").write_text(json.dumps(stale_plan))

        # Call validate_run_dir via subprocess (the file has no .py extension)
        script = f"""
import sys, importlib.util, importlib.machinery
spec = importlib.util.spec_from_file_location("rse", "{TOOLS_DIR}/rush-submit-evidence", loader=importlib.machinery.SourceFileLoader("rse", "{TOOLS_DIR}/rush-submit-evidence"))
rse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rse)
from pathlib import Path
import json
ok, errors, manifest = rse.validate_run_dir(Path("{run_dir}"))
print(json.dumps({{"ok": ok, "errors": errors}}))
"""
        rc, stdout, stderr = run(["python3", "-c", script], timeout=10)
        if rc != 0:
            print(f"FAIL: validate_run_dir call failed: {stderr[:300]}")
            return False

        result = json.loads(stdout.strip())
        if result["ok"]:
            print("FAIL: stale plan was not detected")
            return False

        errors_text = " ".join(result["errors"])
        if "stale generated_at" not in errors_text:
            print(f"FAIL: stale generated_at not reported: {result['errors']}")
            return False
        if "dry_run=true" not in errors_text:
            print(f"FAIL: dry_run=true not reported: {result['errors']}")
            return False
        if "stale source_commit" not in errors_text:
            print(f"FAIL: stale source_commit not reported: {result['errors']}")
            return False

    print("PASS: stale plan detected (generated_at, dry_run, source_commit)")
    return True


def test_7a_all_numeric_benches_have_units():
    """Defect 7: every shell-numeric bench must declare a unit."""
    bench_list = read_bench_list()

    # Parse the TOML to find all shell-numeric benches
    try:
        import tomllib
    except ImportError:
        import tomli as tomllib

    data = tomllib.loads(bench_list)
    numeric_benches = [b for b in data["benches"] if b["kind"] == "shell-numeric"]

    missing = []
    for b in numeric_benches:
        if "unit" not in b or not b["unit"]:
            missing.append(b["id"])

    if missing:
        print(f"FAIL: shell-numeric benches missing unit: {missing}")
        return False

    print(f"PASS: all {len(numeric_benches)} shell-numeric benches have units")
    return True


def test_7b_correct_units_declared():
    """Defect 7: verify the exact units match the spec."""
    bench_list = read_bench_list()
    try:
        import tomllib
    except ImportError:
        import tomli as tomllib

    data = tomllib.loads(bench_list)
    expected = {
        "iperf3-tcp": "Gbit/s",
        "nginx-rps": "requests/s",
        "psi-cpu-avg10": "percent",
        "psi-io-avg10": "percent",
        "cyclictest-max": "us",
        "foreground-launch": "ms",
    }

    benches_by_id = {b["id"]: b for b in data["benches"]}
    for bench_id, expected_unit in expected.items():
        if bench_id not in benches_by_id:
            print(f"FAIL: {bench_id} not found in bench-list.toml")
            return False
        actual_unit = benches_by_id[bench_id].get("unit", "")
        if actual_unit != expected_unit:
            print(f"FAIL: {bench_id} has unit {actual_unit!r}, expected {expected_unit!r}")
            return False

    print(f"PASS: all {len(expected)} units match spec")
    return True


def test_7c_iperf_no_floor_truncation():
    """Defect 7: iperf3 must preserve decimal precision (no floor)."""
    bench_list = read_bench_list()
    iperf_section = re.search(r'id = "iperf3-tcp".*?command = """(.*?)"""', bench_list, re.DOTALL)
    if not iperf_section:
        print("FAIL: iperf3-tcp not found")
        return False

    iperf_cmd = iperf_section.group(1)

    # Must NOT use bare `floor` on the final result
    if "| floor" in iperf_cmd and "/ 1e9 | floor" in iperf_cmd:
        print("FAIL: iperf3 still uses floor truncation on final result")
        return False

    # Must preserve 2 decimal places
    if "* 100 | floor) / 100" not in iperf_cmd:
        print("FAIL: iperf3 does not preserve 2 decimal places")
        return False

    print("PASS: iperf3 preserves decimal precision (no floor truncation)")
    return True


def test_7d_runner_uses_declared_unit():
    """Defect 7: the Rust runner must use bench.unit for shell-numeric."""
    runner_src = (REPO_ROOT / "crates" / "testos" / "src" / "bin" / "testos-runner.rs").read_text()

    # Must reference bench.unit
    if "bench.unit" not in runner_src:
        print("FAIL: runner does not reference bench.unit")
        return False

    # Must fall back to "numeric" for legacy
    if '"numeric"' not in runner_src:
        print("FAIL: runner missing legacy 'numeric' fallback")
        return False

    print("PASS: runner uses bench.unit for shell-numeric results")
    return True


def test_7e_runner_rejects_non_finite():
    """Defect 7/8: runner must reject NaN/Inf values."""
    runner_src = (REPO_ROOT / "crates" / "testos" / "src" / "bin" / "testos-runner.rs").read_text()

    if "is_finite" not in runner_src:
        print("FAIL: runner does not check is_finite")
        return False

    print("PASS: runner rejects non-finite (NaN/Inf) values")
    return True


def test_8a_foreground_launch_uses_high_res_timing():
    """Defect 8: foreground-launch must use high-resolution timing, not /usr/bin/time."""
    bench_list = read_bench_list()
    fg_section = re.search(r'id = "foreground-launch".*?command = (?:\"\"\"|\'\'\')(.*?)(?:\"\"\"|\'\'\')', bench_list, re.DOTALL)
    if not fg_section:
        print("FAIL: foreground-launch not found")
        return False

    fg_cmd = fg_section.group(1)

    # Must NOT use /usr/bin/time -f as the actual timing mechanism.
    # Check executable lines only (skip comments).
    exec_lines = [l for l in fg_cmd.split('\n') if l.strip() and not l.strip().startswith('#')]
    exec_text = ' '.join(exec_lines)

    if "/usr/bin/time -f" in exec_text:
        print("FAIL: foreground-launch still uses /usr/bin/time in executable code")
        return False

    # Must use perf_counter_ns or equivalent high-res timer
    if "perf_counter_ns" not in exec_text:
        print("FAIL: foreground-launch does not use perf_counter_ns")
        return False

    print("PASS: foreground-launch uses high-resolution monotonic timing")
    return True


def test_8b_foreground_launch_rejects_zero():
    """Defect 8: foreground-launch must reject zero/non-finite results."""
    bench_list = read_bench_list()
    fg_section = re.search(r'id = "foreground-launch".*?command = (?:\'\'\'|""")(.*?)(?:\'\'\'|""")', bench_list, re.DOTALL)
    if not fg_section:
        print("FAIL: foreground-launch not found")
        return False

    fg_cmd = fg_section.group(1)

    # Must check for zero or non-finite and output ERROR
    if "median_ms == 0" not in fg_cmd or "ERROR" not in fg_cmd:
        print("FAIL: foreground-launch does not reject zero results")
        return False

    print("PASS: foreground-launch rejects zero/non-finite results")
    return True


def test_8c_foreground_launch_produces_nonzero():
    """Defect 8: foreground-launch command actually produces a non-zero result."""
    bench_list = read_bench_list()
    fg_section = re.search(r'id = "foreground-launch".*?command = (?:\'\'\'|""")(.*?)(?:\'\'\'|""")', bench_list, re.DOTALL)
    if not fg_section:
        print("FAIL: foreground-launch not found")
        return False

    fg_cmd = fg_section.group(1).strip()

    # Run the command (it should complete in < 10 seconds)
    rc, stdout, stderr = run(["bash", "-c", fg_cmd], timeout=30)
    if rc != 0:
        print(f"FAIL: foreground-launch command failed: {stderr[:200]}")
        return False

    result = stdout.strip()
    if result == "ERROR":
        print("FAIL: foreground-launch produced ERROR (timing failed)")
        return False

    try:
        val = float(result)
    except ValueError:
        print(f"FAIL: foreground-launch produced non-numeric: {result!r}")
        return False

    if val <= 0:
        print(f"FAIL: foreground-launch produced zero/negative: {val}")
        return False

    print(f"PASS: foreground-launch produces non-zero result ({val:.3f} ms)")
    return True


def main():
    tests = [
        test_6a_physical_plan_uses_current_utc,
        test_6b_physical_plan_dry_run_false,
        test_6c_stale_plan_detected_by_validator,
        test_7a_all_numeric_benches_have_units,
        test_7b_correct_units_declared,
        test_7c_iperf_no_floor_truncation,
        test_7d_runner_uses_declared_unit,
        test_7e_runner_rejects_non_finite,
        test_8a_foreground_launch_uses_high_res_timing,
        test_8b_foreground_launch_rejects_zero,
        test_8c_foreground_launch_produces_nonzero,
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
            import traceback
            traceback.print_exc()
            failed += 1

    print(f"\n{'=' * 60}")
    print(f"Results: {passed} passed, {failed} failed, {len(tests)} total")
    print(f"{'=' * 60}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
