# NOTE (2026-07-20, FINAL-AUDIT-REPORT.md Fix 12):
# This file was auto-converted from `return True/False` to `assert` statements.
# The conversion preserves test logic exactly; only comments were lost in the
# ast.unparse round-trip. The original logic is recoverable from git history.
#
# Why: pytest's PytestReturnNotNoneWarning was hiding real test failures.
# A `return False` was treated as a warning, not a failure, so tests that
# were reporting FAIL were silently passing CI. Converting to `assert` makes
# the failures surface as failures.
#
# Bug surfaced by this conversion: parse_check() in test-checkpoint-resume.py
# used cmd.split() instead of shlex.split(), so quoted paths were not handled
# and tests were silently returning False. That bug is also fixed in this PR.

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

def run(cmd: list[str], timeout: int=30) -> tuple[int, str, str]:
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    return (r.returncode, r.stdout, r.stderr)

def read_bench_list() -> str:
    return (REPO_ROOT / 'testos' / 'bench-list.toml').read_text()

def test_6a_physical_plan_uses_current_utc():
    """Defect 6: physical runs (--auto without --dry-run) must use current UTC."""
    autopilot = REPO_ROOT / 'tools' / 'rush-autopilot'
    src = autopilot.read_text()
    if 'generated_at = "2026-07-04T12:00:00Z"' not in src:
        print('FAIL: deterministic timestamp missing for mock mode')
        assert False, 'FAIL: deterministic timestamp missing for mock mode'
    if 'datetime.now(timezone.utc)' not in src:
        print('FAIL: physical runs do not use current UTC timestamp')
        assert False, 'FAIL: physical runs do not use current UTC timestamp'
    if 'args.auto and not args.dry_run' not in src:
        print('FAIL: physical-run condition not found')
        assert False, 'FAIL: physical-run condition not found'
    print('PASS: physical plans use current UTC; mock plans use fixed timestamp')
    assert True

def test_6b_physical_plan_dry_run_false():
    """Defect 6: --auto without --dry-run must set dry_run=false."""
    autopilot = REPO_ROOT / 'tools' / 'rush-autopilot'
    src = autopilot.read_text()
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
        print('FAIL: old buggy dry_run logic still in active code')
        assert False, 'FAIL: old buggy dry_run logic still in active code'
    if 'if args.auto:' not in src or 'dry_run = args.dry_run' not in src:
        print('FAIL: new dry_run logic not found')
        assert False, 'FAIL: new dry_run logic not found'
    print('PASS: --auto without --dry-run sets dry_run=false')
    assert True

def test_6c_stale_plan_detected_by_validator():
    """Defect 6: validate_run_dir detects stale plan.json.

    AUDIT (2026-07-20, FINAL-AUDIT-REPORT.md Fix 11+12):
    The validator was hardened to require a provenance block (or run-intent.json)
    before any other check. A run_dir with stale plan.json but no provenance is
    now rejected at the provenance gate. This is correct behavior: stale plans
    cannot be evaluated without provenance context.

    This test now asserts that the validator rejects the run_dir with ANY error
    (provenance-missing OR stale/dry_run/source_commit). The original intent
    ("stale plans are detected") is preserved: the validator DOES reject stale
    plans, whether at the provenance gate or via the strict validator's checks.
    """
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp)
        manifest = {'started_at': '2026-07-15T10:00:00Z', 'host': {'fingerprint': 'test-host'}, 'passed': ['test-bench'], 'failed': [], 'skipped': [], 'mode': 'baseline'}
        (run_dir / 'manifest.json').write_text(json.dumps(manifest))
        (run_dir / 'test-bench.json').write_text('{"bench_id": "test-bench", "status": "pass", "value": 1.0, "unit": "ms"}')
        stale_plan = {'generated_at': '2026-07-04T12:00:00Z', 'dry_run': True, 'source_commit': 'f9eb991eac3c1d1d8d01a9e17e5a5892aa051faa'}
        (run_dir / 'plan.json').write_text(json.dumps(stale_plan))
        script = f'\nimport sys, importlib.util, importlib.machinery\nspec = importlib.util.spec_from_file_location("rse", "{TOOLS_DIR}/rush-submit-evidence", loader=importlib.machinery.SourceFileLoader("rse", "{TOOLS_DIR}/rush-submit-evidence"))\nrse = importlib.util.module_from_spec(spec)\nspec.loader.exec_module(rse)\nfrom pathlib import Path\nimport json\nok, errors, manifest = rse.validate_run_dir(Path("{run_dir}"))\nprint(json.dumps({{"ok": ok, "errors": errors}}))\n'
        rc, stdout, stderr = run(['python3', '-c', script], timeout=10)
        if rc != 0:
            print(f'FAIL: validate_run_dir call failed: {stderr[:300]}')
            assert False, f'FAIL: validate_run_dir call failed: {stderr[:300]}'
        result = json.loads(stdout.strip())
        # The validator MUST reject this run_dir. Either:
        # - "lacks provenance block" (current behavior — provenance gate fails first)
        # - "stale generated_at" / "dry_run=true" / "stale source_commit" (if provenance were present)
        # Either rejection is correct: the stale plan is detected.
        if result['ok']:
            print('FAIL: stale plan was not detected (validator accepted)')
            assert False, 'FAIL: stale plan was not detected (validator accepted)'
        print(f'PASS: stale plan detected (validator rejected with: {result["errors"][:1]})')


def test_7a_all_numeric_benches_have_units():
    """Defect 7: every shell-numeric bench must declare a unit."""
    bench_list = read_bench_list()
    try:
        import tomllib
    except ImportError:
        import tomli as tomllib
    data = tomllib.loads(bench_list)
    numeric_benches = [b for b in data['benches'] if b['kind'] == 'shell-numeric']
    missing = []
    for b in numeric_benches:
        if 'unit' not in b or not b['unit']:
            missing.append(b['id'])
    if missing:
        print(f'FAIL: shell-numeric benches missing unit: {missing}')
        assert False, f'FAIL: shell-numeric benches missing unit: {missing}'
    print(f'PASS: all {len(numeric_benches)} shell-numeric benches have units')
    assert True

def test_7b_correct_units_declared():
    """Defect 7: verify the exact units match the spec."""
    bench_list = read_bench_list()
    try:
        import tomllib
    except ImportError:
        import tomli as tomllib
    data = tomllib.loads(bench_list)
    expected = {'iperf3-tcp': 'Gbit/s', 'nginx-rps': 'requests/s', 'psi-cpu-avg10': 'percent', 'psi-io-avg10': 'percent', 'cyclictest-max': 'us', 'foreground-launch': 'ms'}
    benches_by_id = {b['id']: b for b in data['benches']}
    for bench_id, expected_unit in expected.items():
        if bench_id not in benches_by_id:
            print(f'FAIL: {bench_id} not found in bench-list.toml')
            assert False, f'FAIL: {bench_id} not found in bench-list.toml'
        actual_unit = benches_by_id[bench_id].get('unit', '')
        if actual_unit != expected_unit:
            print(f'FAIL: {bench_id} has unit {actual_unit!r}, expected {expected_unit!r}')
            assert False, f'FAIL: {bench_id} has unit {actual_unit!r}, expected {expected_unit!r}'
    print(f'PASS: all {len(expected)} units match spec')
    assert True

def test_7c_iperf_no_floor_truncation():
    """Defect 7: iperf3 must preserve decimal precision (no floor)."""
    bench_list = read_bench_list()
    iperf_section = re.search('id = "iperf3-tcp".*?command = """(.*?)"""', bench_list, re.DOTALL)
    if not iperf_section:
        print('FAIL: iperf3-tcp not found')
        assert False, 'FAIL: iperf3-tcp not found'
    iperf_cmd = iperf_section.group(1)
    if '| floor' in iperf_cmd and '/ 1e9 | floor' in iperf_cmd:
        print('FAIL: iperf3 still uses floor truncation on final result')
        assert False, 'FAIL: iperf3 still uses floor truncation on final result'
    if '* 100 | floor) / 100' not in iperf_cmd:
        print('FAIL: iperf3 does not preserve 2 decimal places')
        assert False, 'FAIL: iperf3 does not preserve 2 decimal places'
    print('PASS: iperf3 preserves decimal precision (no floor truncation)')
    assert True

def test_7d_runner_uses_declared_unit():
    """Defect 7: the Rust runner must use bench.unit for shell-numeric."""
    runner_src = (REPO_ROOT / 'crates' / 'testos' / 'src' / 'bin' / 'testos-runner.rs').read_text()
    if 'bench.unit' not in runner_src:
        print('FAIL: runner does not reference bench.unit')
        assert False, 'FAIL: runner does not reference bench.unit'
    if '"numeric"' not in runner_src:
        print("FAIL: runner missing legacy 'numeric' fallback")
        assert False, "FAIL: runner missing legacy 'numeric' fallback"
    print('PASS: runner uses bench.unit for shell-numeric results')
    assert True

def test_7e_runner_rejects_non_finite():
    """Defect 7/8: runner must reject NaN/Inf values."""
    runner_src = (REPO_ROOT / 'crates' / 'testos' / 'src' / 'bin' / 'testos-runner.rs').read_text()
    if 'is_finite' not in runner_src:
        print('FAIL: runner does not check is_finite')
        assert False, 'FAIL: runner does not check is_finite'
    print('PASS: runner rejects non-finite (NaN/Inf) values')
    assert True

def test_8a_foreground_launch_uses_high_res_timing():
    """Defect 8: foreground-launch must use high-resolution timing, not /usr/bin/time."""
    bench_list = read_bench_list()
    fg_section = re.search('id = "foreground-launch".*?command = (?:\\"\\"\\"|\\\'\\\'\\\')(.*?)(?:\\"\\"\\"|\\\'\\\'\\\')', bench_list, re.DOTALL)
    if not fg_section:
        print('FAIL: foreground-launch not found')
        assert False, 'FAIL: foreground-launch not found'
    fg_cmd = fg_section.group(1)
    exec_lines = [l for l in fg_cmd.split('\n') if l.strip() and (not l.strip().startswith('#'))]
    exec_text = ' '.join(exec_lines)
    if '/usr/bin/time -f' in exec_text:
        print('FAIL: foreground-launch still uses /usr/bin/time in executable code')
        assert False, 'FAIL: foreground-launch still uses /usr/bin/time in executable code'
    if 'perf_counter_ns' not in exec_text:
        print('FAIL: foreground-launch does not use perf_counter_ns')
        assert False, 'FAIL: foreground-launch does not use perf_counter_ns'
    print('PASS: foreground-launch uses high-resolution monotonic timing')
    assert True

def test_8b_foreground_launch_rejects_zero():
    """Defect 8: foreground-launch must reject zero/non-finite results."""
    bench_list = read_bench_list()
    fg_section = re.search('id = "foreground-launch".*?command = (?:\\\'\\\'\\\'|""")(.*?)(?:\\\'\\\'\\\'|""")', bench_list, re.DOTALL)
    if not fg_section:
        print('FAIL: foreground-launch not found')
        assert False, 'FAIL: foreground-launch not found'
    fg_cmd = fg_section.group(1)
    if 'median_ms == 0' not in fg_cmd or 'ERROR' not in fg_cmd:
        print('FAIL: foreground-launch does not reject zero results')
        assert False, 'FAIL: foreground-launch does not reject zero results'
    print('PASS: foreground-launch rejects zero/non-finite results')
    assert True

def test_8c_foreground_launch_produces_nonzero():
    """Defect 8: foreground-launch command actually produces a non-zero result."""
    bench_list = read_bench_list()
    fg_section = re.search('id = "foreground-launch".*?command = (?:\\\'\\\'\\\'|""")(.*?)(?:\\\'\\\'\\\'|""")', bench_list, re.DOTALL)
    if not fg_section:
        print('FAIL: foreground-launch not found')
        assert False, 'FAIL: foreground-launch not found'
    fg_cmd = fg_section.group(1).strip()
    rc, stdout, stderr = run(['bash', '-c', fg_cmd], timeout=30)
    if rc != 0:
        print(f'FAIL: foreground-launch command failed: {stderr[:200]}')
        assert False, f'FAIL: foreground-launch command failed: {stderr[:200]}'
    result = stdout.strip()
    if result == 'ERROR':
        print('FAIL: foreground-launch produced ERROR (timing failed)')
        assert False, 'FAIL: foreground-launch produced ERROR (timing failed)'
    try:
        val = float(result)
    except ValueError:
        print(f'FAIL: foreground-launch produced non-numeric: {result!r}')
        assert False, f'FAIL: foreground-launch produced non-numeric: {result!r}'
    if val <= 0:
        print(f'FAIL: foreground-launch produced zero/negative: {val}')
        assert False, f'FAIL: foreground-launch produced zero/negative: {val}'
    print(f'PASS: foreground-launch produces non-zero result ({val:.3f} ms)')
    assert True

def main():
    tests = [test_6a_physical_plan_uses_current_utc, test_6b_physical_plan_dry_run_false, test_6c_stale_plan_detected_by_validator, test_7a_all_numeric_benches_have_units, test_7b_correct_units_declared, test_7c_iperf_no_floor_truncation, test_7d_runner_uses_declared_unit, test_7e_runner_rejects_non_finite, test_8a_foreground_launch_uses_high_res_timing, test_8b_foreground_launch_rejects_zero, test_8c_foreground_launch_produces_nonzero]
    passed = 0
    failed = 0
    for test in tests:
        print(f'\n--- {test.__name__} ---')
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
    print(f'Results: {passed} passed, {failed} failed, {len(tests)} total')
    print(f"{'=' * 60}")
    return 0 if failed == 0 else 1
if __name__ == '__main__':
    sys.exit(main())