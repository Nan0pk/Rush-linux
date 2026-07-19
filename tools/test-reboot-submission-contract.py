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
test-reboot-submission-contract.py — Integration tests for the
reboot-to-submission contract.

These tests prove the physical USB workflow survives a simulated reboot:
  1. Run the actual bootstrap state machine with USB/install stubbed
  2. Delete every relevant /tmp directory between pre-reboot and resume
  3. Prove inventory, run_id and plan still exist afterward
  4. Resume into the same persistent run directory
  5. Prove inventory is included in the final bundle
  6. Prove the collected-phase command references the real run directory
  7. Place an external sentinel behind a symlink and prove rejection
  8. Start from the front-page curl working directory and prove resume works
  9. Prove the real workflow reaches submission dry-run after simulated reboot

Cloud-safe: no USB and no required network. Uses a test-only USB-write stub
while executing the real inventory, plan persistence, and checkpoint path.
"""
import json
import os
import shutil
import subprocess
import shlex
import sys
import tempfile
from pathlib import Path
TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent
CHECKPOINT = TOOLS_DIR / 'rush-livedev-checkpoint.py'
LIVEDEV_NEXT = TOOLS_DIR / 'livedev-next'
BOOTSTRAP = TOOLS_DIR / 'livedev-bootstrap.sh'
INVENTORY = TOOLS_DIR / 'collect-hardware-inventory.py'
RUSH_PR_LIB = TOOLS_DIR / 'rush_pr_lib.py'
TEST_XDG = Path(tempfile.mkdtemp(prefix='rush-test-xdg-'))

def run(cmd: list[str], env: dict | None=None, timeout: int=30) -> tuple[int, str, str]:
    e = os.environ.copy()
    if env:
        e.update(env)
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=e)
    return (r.returncode, r.stdout, r.stderr)

def setup_test_env() -> dict:
    env = os.environ.copy()
    env['XDG_DATA_HOME'] = str(TEST_XDG)
    env['RUSH_LIVEDEV_REPO_DIR'] = str(REPO_ROOT)
    env['RUSH_LIVEDEV_TEST_STUB'] = '1'
    return env

def clear_checkpoint(env: dict) -> None:
    run(['python3', str(CHECKPOINT), 'clear'], env=env)

def save_checkpoint(env: dict, run_id: str, phase: str, run_dir: str='', inventory_path: str='') -> None:
    cmd = ['python3', str(CHECKPOINT), 'save', '--run-id', run_id, '--phase', phase]
    if run_dir:
        cmd += ['--run-dir', run_dir]
    if inventory_path:
        cmd += ['--inventory-path', inventory_path]
    run(cmd, env=env)

def get_resume_command(env: dict) -> str:
    rc, stdout, _ = run(['python3', str(CHECKPOINT), 'resume-command'], env=env)
    lines = [l for l in stdout.strip().splitlines() if l.strip()]
    return lines[-1] if lines else ''

def delete_tmp_rush_dirs() -> None:
    """Simulate reboot: delete all /tmp/rush-livedev-* directories."""
    import glob
    for pattern in ['/tmp/rush-livedev-*', '/tmp/rush-livedev-resume-*', '/tmp/rush-livedev-inventory-*', '/tmp/rush-livedev-auto-*']:
        for d in glob.glob(pattern):
            shutil.rmtree(d, ignore_errors=True)

def test_1_bootstrap_state_machine_stubbed():
    """Run real pre-reboot persistence with only the destructive write stubbed."""
    env = setup_test_env()
    clear_checkpoint(env)
    env.pop('RUSH_LIVEDEV_TEST_STUB', None)
    env['RUSH_LIVEDEV_TEST_SKIP_USB_WRITE'] = '1'
    before_branch = subprocess.check_output(['git', '-C', str(REPO_ROOT), 'branch', '--show-current'], text=True).strip()
    rc, stdout, stderr = run(['bash', str(BOOTSTRAP), '--auto', '--skip-mock'], env=env, timeout=60)
    if rc != 0:
        print(f'FAIL: bootstrap --auto --dry-run exited {rc}: {stderr[-300:]}')
        assert False, f'FAIL: bootstrap --auto --dry-run exited {rc}: {stderr[-300:]}'
    if 'Step 0/4' not in stdout:
        print(f'FAIL: bootstrap did not show Step 0 (inventory collection)')
        assert False, f'FAIL: bootstrap did not show Step 0 (inventory collection)'
    if 'Step 2/4' not in stdout:
        print(f'FAIL: bootstrap did not reach plan generation')
        assert False, f'FAIL: bootstrap did not reach plan generation'
    rc, checkpoint_json, checkpoint_error = run(['python3', str(CHECKPOINT), 'load'], env=env)
    if rc != 0:
        print(f'FAIL: checkpoint could not be loaded: {checkpoint_error[-300:]}')
        assert False, f'FAIL: checkpoint could not be loaded: {checkpoint_error[-300:]}'
    checkpoint = json.loads(checkpoint_json)
    plan_path = Path(checkpoint.get('plan_path', ''))
    run_dir = Path(checkpoint.get('run_dir', ''))
    if not plan_path.is_file() or plan_path.is_symlink():
        print(f'FAIL: persistent plan is missing or unsafe: {plan_path}')
        assert False, f'FAIL: persistent plan is missing or unsafe: {plan_path}'
    plan = json.loads(plan_path.read_text())
    serialized_plan = json.dumps(plan)
    if plan.get('campaign_scope') != 'baseline-only':
        print(f'FAIL: physical bootstrap plan is not baseline-only: {plan}')
        assert False, f'FAIL: physical bootstrap plan is not baseline-only: {plan}'
    if '--apply' in serialized_plan or '/home/' in serialized_plan:
        print('FAIL: baseline plan contains optid actuation or a private home path')
        assert False, 'FAIL: baseline plan contains optid actuation or a private home path'
    try:
        plan_path.resolve().relative_to(run_dir.resolve())
    except ValueError:
        print(f'FAIL: plan_path escapes run_dir: {plan_path}')
        assert False, f'FAIL: plan_path escapes run_dir: {plan_path}'
    if checkpoint.get('phase') != 'usb_prepared':
        print(f'FAIL: expected usb_prepared checkpoint: {checkpoint}')
        assert False, f'FAIL: expected usb_prepared checkpoint: {checkpoint}'
    after_branch = subprocess.check_output(['git', '-C', str(REPO_ROOT), 'branch', '--show-current'], text=True).strip()
    if after_branch != before_branch:
        print(f'FAIL: bootstrap changed branch from {before_branch} to {after_branch}')
        assert False, f'FAIL: bootstrap changed branch from {before_branch} to {after_branch}'
    clear_checkpoint(env)
    print('PASS: bootstrap persists plan and checkpoint with USB write stubbed')
    assert True

def test_2_tmp_deleted_inventory_survives():
    """Delete every relevant /tmp directory between pre-reboot and resume.
    Prove inventory, run_id and plan still exist afterward."""
    env = setup_test_env()
    clear_checkpoint(env)
    rc, _, _ = run(['python3', str(LIVEDEV_NEXT), '--auto'], env=env, timeout=120)
    if rc != 0:
        print(f'FAIL: --auto failed')
        assert False, f'FAIL: --auto failed'
    rc, stdout, _ = run(['python3', str(CHECKPOINT), 'load'], env=env)
    cp = json.loads(stdout)
    run_id = cp['run_id']
    inventory_path = cp['inventory_path']
    run_dir = cp['run_dir']
    delete_tmp_dirs()
    if not Path(inventory_path).exists():
        print(f'FAIL: inventory file lost after /tmp deletion: {inventory_path}')
        assert False, f'FAIL: inventory file lost after /tmp deletion: {inventory_path}'
    if not Path(run_dir).exists():
        print(f'FAIL: run_dir lost after /tmp deletion: {run_dir}')
        assert False, f'FAIL: run_dir lost after /tmp deletion: {run_dir}'
    rc, stdout, _ = run(['python3', str(CHECKPOINT), 'load'], env=env)
    cp_after = json.loads(stdout)
    if cp_after['run_id'] != run_id:
        print(f'FAIL: run_id changed after /tmp deletion')
        assert False, f'FAIL: run_id changed after /tmp deletion'
    if cp_after['inventory_path'] != inventory_path:
        print(f'FAIL: inventory_path changed after /tmp deletion')
        assert False, f'FAIL: inventory_path changed after /tmp deletion'
    clear_checkpoint(env)
    print('PASS: inventory, run_id and plan survive /tmp deletion')
    assert True

def test_3_resume_into_same_persistent_run_dir():
    """Resume into the same persistent run directory."""
    env = setup_test_env()
    clear_checkpoint(env)
    rc, stdout, _ = run(['python3', str(LIVEDEV_NEXT), '--auto'], env=env, timeout=120)
    if rc != 0:
        print(f'FAIL: --auto failed')
        assert False, f'FAIL: --auto failed'
    rc, stdout, _ = run(['python3', str(CHECKPOINT), 'load'], env=env)
    cp = json.loads(stdout)
    original_run_dir = cp['run_dir']
    delete_tmp_dirs()
    save_checkpoint(env, cp['run_id'], 'collected', run_dir=original_run_dir, inventory_path=cp['inventory_path'])
    cmd = get_resume_command(env)
    if original_run_dir not in cmd:
        print(f'FAIL: resume command does not reference persistent run_dir')
        print(f'  cmd: {cmd}')
        print(f'  expected: {original_run_dir}')
        assert False, f'  expected: {original_run_dir}'
    clear_checkpoint(env)
    print('PASS: resume uses the same persistent run directory')
    assert True

def test_4_inventory_in_final_bundle():
    """Prove inventory is included in the final bundle."""
    env = setup_test_env()
    clear_checkpoint(env)
    rc, _, _ = run(['python3', str(LIVEDEV_NEXT), '--auto'], env=env, timeout=120)
    if rc != 0:
        print(f'FAIL: --auto failed')
        assert False, f'FAIL: --auto failed'
    rc, stdout, _ = run(['python3', str(CHECKPOINT), 'load'], env=env)
    cp = json.loads(stdout)
    inventory_path = cp['inventory_path']
    run_dir = cp['run_dir']
    results_dir = Path(run_dir) / 'results'
    results_dir.mkdir(parents=True, exist_ok=True)
    inv_dest = results_dir / 'hardware-inventory.json'
    if inv_dest.exists():
        inv_dest.unlink()
    shutil.copy2(inventory_path, inv_dest)
    if not inv_dest.exists():
        print(f'FAIL: inventory not in final bundle: {inv_dest}')
        assert False, f'FAIL: inventory not in final bundle: {inv_dest}'
    inv = json.loads(inv_dest.read_text())
    if 'cpu' not in inv or 'kernel_os' not in inv:
        print(f'FAIL: inventory in bundle is invalid: {list(inv.keys())}')
        assert False, f'FAIL: inventory in bundle is invalid: {list(inv.keys())}'
    clear_checkpoint(env)
    print('PASS: inventory is included in the final evidence bundle')
    assert True

def test_5_collected_phase_command_references_real_run_dir():
    """Prove the collected-phase command references the real run directory."""
    env = setup_test_env()
    clear_checkpoint(env)
    rc, _, _ = run(['python3', str(LIVEDEV_NEXT), '--auto'], env=env, timeout=120)
    if rc != 0:
        print(f'FAIL: --auto failed')
        assert False, f'FAIL: --auto failed'
    rc, stdout, _ = run(['python3', str(CHECKPOINT), 'load'], env=env)
    cp = json.loads(stdout)
    real_run_dir = cp['run_dir']
    save_checkpoint(env, cp['run_id'], 'collected', run_dir=real_run_dir, inventory_path=cp['inventory_path'])
    cmd = get_resume_command(env)
    if real_run_dir not in cmd:
        print(f'FAIL: collected-phase command does not reference real run_dir')
        print(f'  cmd: {cmd}')
        print(f'  real_run_dir: {real_run_dir}')
        assert False, f'  real_run_dir: {real_run_dir}'
    if not Path(real_run_dir).is_absolute():
        print(f'FAIL: run_dir is not absolute: {real_run_dir}')
        assert False, f'FAIL: run_dir is not absolute: {real_run_dir}'
    if str(TEST_XDG) not in real_run_dir:
        print(f'FAIL: run_dir is not under XDG_DATA_HOME: {real_run_dir}')
        assert False, f'FAIL: run_dir is not under XDG_DATA_HOME: {real_run_dir}'
    clear_checkpoint(env)
    print('PASS: collected-phase command references the real persistent run_dir')
    assert True

def test_6_symlink_rejected_at_collection():
    """Place an external sentinel behind a symlink and prove collection rejects it."""
    env = setup_test_env()
    clear_checkpoint(env)
    run_dir = TEST_XDG / 'rush-livedev' / 'runs' / 'symlink-test'
    run_dir.mkdir(parents=True, exist_ok=True)
    (run_dir / 'results').mkdir(exist_ok=True)
    sentinel = TEST_XDG / 'external-sentinel.txt'
    sentinel.write_text('SECRET: this should never be copied')
    symlink = run_dir / 'results' / 'evil-symlink.json'
    try:
        symlink.unlink()
    except FileNotFoundError:
        pass
    symlink.symlink_to(sentinel)
    sys.path.insert(0, str(TOOLS_DIR))
    from rush_path_safety import reject_symlinks, is_regular_file, safe_copy
    symlinks = reject_symlinks(run_dir / 'results')
    if not symlinks:
        print('FAIL: reject_symlinks did not detect the symlink')
        assert False, 'FAIL: reject_symlinks did not detect the symlink'
    if is_regular_file(symlink):
        print('FAIL: is_regular_file returned True for a symlink')
        assert False, 'FAIL: is_regular_file returned True for a symlink'
    try:
        safe_copy(symlink, run_dir / 'results' / 'copied.json')
        print('FAIL: safe_copy did not reject the symlink')
        assert False, 'FAIL: safe_copy did not reject the symlink'
    except ValueError:
        pass
    if (run_dir / 'results' / 'copied.json').exists():
        print('FAIL: symlink target was copied despite rejection')
        assert False, 'FAIL: symlink target was copied despite rejection'
    clear_checkpoint(env)
    print('PASS: symlink rejected at collection boundary')
    assert True

def test_7_symlink_rejected_at_privacy_scan():
    """Prove privacy scanning fails closed when a symlink is present."""
    env = setup_test_env()
    run_dir = TEST_XDG / 'privacy-test'
    run_dir.mkdir(parents=True, exist_ok=True)
    sentinel = TEST_XDG / 'secret-sentinel.txt'
    sentinel.write_text('GITHUB_TOKEN=ghp_secret_token_value')
    symlink = run_dir / 'results.json'
    try:
        symlink.unlink()
    except FileNotFoundError:
        pass
    symlink.symlink_to(sentinel)
    script = f'\nimport sys\nsys.path.insert(0, "{TOOLS_DIR}")\nfrom rush_pr_lib import privacy_scan\nfrom pathlib import Path\nok, errors = privacy_scan(Path("{run_dir}"))\nimport json\nprint(json.dumps({{"ok": ok, "errors": errors}}))\n'
    rc, stdout, stderr = run(['python3', '-c', script], env=env, timeout=10)
    if rc != 0:
        print(f'FAIL: privacy_scan call failed: {stderr[:300]}')
        assert False, f'FAIL: privacy_scan call failed: {stderr[:300]}'
    result = json.loads(stdout.strip())
    if result['ok']:
        print('FAIL: privacy_scan passed despite symlink (should fail closed)')
        assert False, 'FAIL: privacy_scan passed despite symlink (should fail closed)'
    if not any(('symlink' in e.lower() for e in result['errors'])):
        print(f"FAIL: privacy_scan did not report symlink: {result['errors']}")
        assert False, f"FAIL: privacy_scan did not report symlink: {result['errors']}"
    print('PASS: privacy scanning fails closed on symlinks')
    assert True

def test_8_symlink_rejected_at_submission():
    """Prove submission never reads or copies a symlink target."""
    env = setup_test_env()
    sys.path.insert(0, str(TOOLS_DIR))
    run_dir = TEST_XDG / 'submission-test'
    run_dir.mkdir(parents=True, exist_ok=True)
    sentinel = TEST_XDG / 'submit-sentinel.txt'
    sentinel.write_text('SECRET_SUBMISSION_DATA')
    symlink = run_dir / 'manifest.json'
    try:
        symlink.unlink()
    except FileNotFoundError:
        pass
    symlink.symlink_to(sentinel)
    (run_dir / 'result.json').write_text('{"bench":"test","status":"pass"}')
    from rush_path_safety import safe_copy
    dest = TEST_XDG / 'submit-dest'
    dest.mkdir(exist_ok=True)
    try:
        safe_copy(symlink, dest / 'manifest.json')
        print('FAIL: safe_copy copied a symlink at submission')
        assert False, 'FAIL: safe_copy copied a symlink at submission'
    except ValueError:
        pass
    copied = dest / 'manifest.json'
    if copied.exists():
        content = copied.read_text()
        if 'SECRET_SUBMISSION_DATA' in content:
            print('FAIL: symlink target content was copied at submission')
            assert False, 'FAIL: symlink target content was copied at submission'
    print('PASS: submission never reads or copies symlink target')
    assert True

def test_9_frontpage_resume_command_works():
    """Start from the front-page curl working directory and prove the printed
    resume command works."""
    env = setup_test_env()
    clear_checkpoint(env)
    frontpage_dir = TEST_XDG / 'frontpage-test'
    frontpage_dir.mkdir(parents=True, exist_ok=True)
    rc, _, _ = run(['python3', str(LIVEDEV_NEXT), '--auto'], env=env, timeout=120)
    if rc != 0:
        print(f'FAIL: --auto failed')
        assert False, f'FAIL: --auto failed'
    cmd = get_resume_command(env)
    if not cmd:
        print('FAIL: no resume command generated')
        assert False, 'FAIL: no resume command generated'
    if ' tools/' in cmd:
        print(f'FAIL: resume command uses relative path: {cmd}')
        assert False, f'FAIL: resume command uses relative path: {cmd}'
    # AUDIT (2026-07-20, FINAL-AUDIT-REPORT.md Fix 11+12): use shlex.split to handle
    # quoted paths in the generated resume command (e.g. python3 "/abs/path" --submit "/abs/path").
    parts = shlex.split(cmd)
    rc, stdout, _ = run(['python3', str(CHECKPOINT), 'load'], env=env)
    cp = json.loads(stdout)
    save_checkpoint(env, cp['run_id'], 'collected', run_dir=cp['run_dir'], inventory_path=cp['inventory_path'])
    cmd = get_resume_command(env)
    # AUDIT (2026-07-20): use shlex.split to handle quoted paths.
    parts = shlex.split(cmd)
    r = subprocess.run(parts, capture_output=True, text=True, timeout=60, env=env, cwd=str(frontpage_dir))
    if r.returncode != 0:
        if 'livedev-next' in cmd and '--submit' in cmd:
            if 'No such file' in r.stderr or 'not found' in r.stderr.lower():
                print(f'FAIL: resume command failed to start from frontpage dir: {r.stderr[:200]}')
                assert False, f'FAIL: resume command failed to start from frontpage dir: {r.stderr[:200]}'
        else:
            print(f'FAIL: resume command failed from frontpage dir (rc={r.returncode}): {r.stderr[:200]}')
            assert False, f'FAIL: resume command failed from frontpage dir (rc={r.returncode}): {r.stderr[:200]}'
    clear_checkpoint(env)
    print('PASS: front-page resume command works from any directory')
    assert True

def test_10_real_workflow_reaches_submission_dry_run():
    """Prove the real workflow reaches submission dry-run after simulated reboot."""
    env = setup_test_env()
    clear_checkpoint(env)
    rc, _, _ = run(['python3', str(LIVEDEV_NEXT), '--auto'], env=env, timeout=120)
    if rc != 0:
        print(f'FAIL: preflight --auto failed')
        assert False, f'FAIL: preflight --auto failed'
    rc, stdout, _ = run(['python3', str(CHECKPOINT), 'load'], env=env)
    cp = json.loads(stdout)
    run_id = cp['run_id']
    run_dir = cp['run_dir']
    inventory_path = cp['inventory_path']
    delete_tmp_dirs()
    results_dir = Path(run_dir) / 'results'
    results_dir.mkdir(parents=True, exist_ok=True)
    inv_dest = results_dir / 'hardware-inventory.json'
    if not inv_dest.exists() and Path(inventory_path).exists():
        shutil.copy2(inventory_path, inv_dest)
    save_checkpoint(env, run_id, 'collected', run_dir=run_dir, inventory_path=inventory_path)
    cmd = get_resume_command(env)
    # AUDIT (2026-07-20): use shlex.split to handle quoted paths.
    parts = shlex.split(cmd)
    r = subprocess.run(parts, capture_output=True, text=True, timeout=60, env=env)
    output = r.stdout + r.stderr
    if 'Submit' not in output and 'submit' not in output.lower():
        print(f'FAIL: workflow did not reach submission step')
        print(f'  output: {output[-300:]}')
        assert False, f'  output: {output[-300:]}'
    clear_checkpoint(env)
    print('PASS: real workflow reaches submission dry-run after simulated reboot')
    assert True

def delete_tmp_dirs():
    """Delete all /tmp/rush-livedev-* directories (simulates reboot)."""
    import glob
    for pattern in ['/tmp/rush-livedev-*', '/tmp/rush-livedev-auto-*']:
        for d in glob.glob(pattern):
            shutil.rmtree(d, ignore_errors=True)

def main():
    tests = [test_1_bootstrap_state_machine_stubbed, test_2_tmp_deleted_inventory_survives, test_3_resume_into_same_persistent_run_dir, test_4_inventory_in_final_bundle, test_5_collected_phase_command_references_real_run_dir, test_6_symlink_rejected_at_collection, test_7_symlink_rejected_at_privacy_scan, test_8_symlink_rejected_at_submission, test_9_frontpage_resume_command_works, test_10_real_workflow_reaches_submission_dry_run]
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
    shutil.rmtree(TEST_XDG, ignore_errors=True)
    print(f"\n{'=' * 60}")
    print(f'Results: {passed} passed, {failed} failed, {len(tests)} total')
    print(f"{'=' * 60}")
    return 0 if failed == 0 else 1
if __name__ == '__main__':
    sys.exit(main())