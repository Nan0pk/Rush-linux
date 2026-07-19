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
test-cloud-safe-livedev.py — cloud-safe regression tests for the testOS
run-intent / provenance / strict-evidence / submission-safety foundation.

These tests verify the Linux/testOS/shared-code portion of the cloud-safe
LiveDev contract WITHOUT real hardware, WITHOUT a USB, and WITHOUT
downloading/running an old testOS image. They build fixtures in temp
directories using the real repo commit, VERSION, and computed SHA-256
digests so the validators are exercised against authentic data.

Cloud-safe scenarios covered (all 11 required):
  1.  valid intent/manifest acceptance
  2.  stale generated_at rejection
  3.  dry_run=true rejection
  4.  wrong source commit
  5.  wrong plan/catalog/image hashes
  6.  changed result file after manifest creation (result-hashes sidecar)
  7.  mismatched run_id/checkpoint
  8.  symlink escape (environment-dependent: skips if os.symlink unavailable)
  9.  privacy-secret rejection
  10. draft PR payload (submission safety: draft=True enforced)
  11. absence of token-bearing Git URLs/argv (submission safety)

Plus:
  - run-intent schema validation (required fields, patterns, discriminator)
  - fail-closed behavior of the strict validator on placeholder metadata
  - rush_path_safety.prove_containment / safe_copy_tree
  - rush-submit-evidence.assert_no_token_argv / assert_safe_api_path

Environment-dependent tests (clearly separated):
  - Rust fmt / cargo test / clippy: cargo is unavailable in this cloud env;
    they run in CI. Documented in the test output.
  - symlink escape: os.symlink may be unavailable in restricted sandboxes;
    the test skips with a clear message when so.
"""
from __future__ import annotations
import datetime as dt
import hashlib
import importlib.machinery
import importlib.util
import json
import os
import re
import re
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit
from pathlib import Path
from urllib.parse import urlsplit
TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent

def _load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, str(path), loader=importlib.machinery.SourceFileLoader(name, str(path)))
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod
validate_testos = _load_module('validate_testos_evidence', TOOLS_DIR / 'validate-testos-evidence.py')
rps = _load_module('rush_path_safety', TOOLS_DIR / 'rush_path_safety.py')
rse = _load_module('rush_submit_evidence', TOOLS_DIR / 'rush-submit-evidence')

def _git_head() -> str:
    r = subprocess.run(['git', '-C', str(REPO_ROOT), 'rev-parse', 'HEAD'], capture_output=True, text=True, timeout=5)
    assert r.returncode == 0, f'git rev-parse HEAD failed: {r.stderr}'
    return r.stdout.strip()

def _version() -> str:
    return (REPO_ROOT / 'VERSION').read_text().strip()

def _sha256_bytes(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()

def _sha256_file(p: Path) -> str:
    return _sha256_bytes(p.read_bytes())

def _now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')

def _bench_list_bytes() -> bytes:
    return (REPO_ROOT / 'testos' / 'bench-list.toml').read_bytes()

def _make_intent(*, run_id: str='run-2026-07-15T10-00-00Z', source_commit: str | None=None, source_version: str | None=None, testos_version: str | None=None, testos_image_digest: str | None=None, testos_image_commit: str | None=None, plan_sha256: str | None=None, benchmark_catalog_sha256: str | None=None, generated_at: str | None=None, dry_run: bool=False, checkpoint_nonce: str='ckpt-2026-07-15-abcd1234', campaign_id: str | None='campaign-2026-07') -> dict:
    """Build a valid run-intent with the given overrides."""
    # AUDIT (2026-07-20, FINAL-AUDIT-REPORT.md Fix 11+12): added testos_image_commit
    # to satisfy the strict validator's required-field check (separate from source_commit).
    # Tests use the same SHA as source_commit by default; override via the param if needed.
    _commit = source_commit or _git_head()
    return {'schema_version': 1, 'intent_kind': 'testos-run-intent', 'run_id': run_id, 'source_commit': _commit, 'source_version': source_version or _version(), 'testos_version': testos_version or _version(), 'testos_image_digest': testos_image_digest or f"sha256:{'a' * 64}", 'testos_image_commit': testos_image_commit or _commit, 'plan_sha256': plan_sha256 or 'b' * 64, 'benchmark_catalog_sha256': benchmark_catalog_sha256 or _sha256_bytes(_bench_list_bytes()), 'generated_at': generated_at or _now_iso(), 'dry_run': dry_run, 'checkpoint_nonce': checkpoint_nonce, 'campaign_id': campaign_id}

def _write_run_dir(tmp: Path, intent: dict, *, plan_bytes: bytes | None=None, bench_list_bytes: bytes | None=None, result_files: dict[str, dict] | None=None, intent_raw_override: bytes | None=None, manifest_provenance_override: dict | None=None, include_source_sha: bool=True, include_plan: bool=True, include_intent: bool=True, extra_files: dict[str, bytes] | None=None, result_hashes: dict[str, str] | None=None) -> Path:
    """Write a complete testOS run directory to tmp and return its path."""
    run_dir = tmp / 'run'
    run_dir.mkdir(parents=True, exist_ok=True)
    bl = bench_list_bytes if bench_list_bytes is not None else _bench_list_bytes()
    intent_raw = intent_raw_override if intent_raw_override is not None else json.dumps(intent, indent=2, sort_keys=True).encode()
    if plan_bytes is None:
        plan_obj = {'schema_version': 1, 'plan_kind': 'rush-autopilot-plan', 'generated_at': intent['generated_at'], 'source_version': intent['source_version'], 'source_commit': intent['source_commit'], 'dry_run': intent['dry_run'], 'campaign_scope': 'baseline-only' if intent['dry_run'] else 'comparative', 'hardware_slot': 'laptop', 'slot_confidence': 'high', 'ambiguities': [], 'open_criteria': [], 'existing_evidence': [], 'steps': [], 'repo_root': '.'}
        plan_bytes = json.dumps(plan_obj).encode()
    intent_sha = _sha256_bytes(intent_raw)
    plan_sha = _sha256_bytes(plan_bytes)
    # AUDIT (2026-07-20): include testos_image_commit in provenance to satisfy the strict validator.
    prov = manifest_provenance_override or {'run_id': intent['run_id'], 'source_commit': intent['source_commit'], 'source_version': intent['source_version'], 'testos_version': intent['testos_version'], 'testos_image_digest': intent['testos_image_digest'], 'testos_image_commit': intent.get('testos_image_commit', intent['source_commit']), 'plan_sha256': plan_sha, 'benchmark_catalog_sha256': intent['benchmark_catalog_sha256'], 'intent_generated_at': intent['generated_at'], 'intent_dry_run': intent['dry_run'], 'checkpoint_nonce': intent['checkpoint_nonce'], 'intent_sha256': intent_sha, 'campaign_id': intent.get('campaign_id')}
    if prov.get('campaign_id') is None:
        prov.pop('campaign_id', None)
    manifest = {'schema_version': 1, 'started_at': _now_iso(), 'finished_at': _now_iso(), 'mode': 'all', 'attempted': [], 'passed': [], 'failed': [], 'skipped': [], 'host': {'fingerprint': 'test-host-0012', 'kernel': 'x', 'cpu_model': 'y', 'dmi_board': 'z', 'battery_design_uwh': 0}, 'testos_version': intent['testos_version'], 'provenance': prov}
    if result_files:
        for name, data in result_files.items():
            rf = run_dir / name
            rf.write_text(json.dumps(data))
            manifest['attempted'].append(data.get('bench_id', rf.stem))
            manifest['passed'].append(data.get('bench_id', rf.stem))
    (run_dir / 'manifest.json').write_text(json.dumps(manifest, indent=2))
    if include_intent:
        (run_dir / 'run-intent.json').write_bytes(intent_raw)
    if include_plan:
        (run_dir / 'plan.json').write_bytes(plan_bytes)
    (run_dir / 'bench-list.toml').write_bytes(bl)
    if include_source_sha:
        (run_dir / 'source-sha.txt').write_text(intent['source_commit'][:12])
    if extra_files:
        for name, content in extra_files.items():
            (run_dir / name).write_bytes(content)
    if result_hashes is not None:
        (run_dir / 'result-hashes.json').write_text(json.dumps(result_hashes))
    elif result_files:
        auto_hashes = {}
        for name in result_files:
            auto_hashes[name] = _sha256_file(run_dir / name)
        (run_dir / 'result-hashes.json').write_text(json.dumps(auto_hashes))
    return run_dir

def _good_result_file(bench_id: str='shell-pass') -> dict:
    return {'schema_version': 1, 'bench_id': bench_id, 'bench_name': 'test', 'scenario': 'server-throughput', 'status': 'pass', 'started_at': _now_iso(), 'finished_at': _now_iso(), 'elapsed_seconds': 0.1, 'host': {'fingerprint': 'test-host-0012'}}

def run_tests():
    tests = [test_1_valid_intent_manifest_acceptance, test_2_stale_generated_at_rejection, test_3_dry_run_true_rejection, test_4_wrong_source_commit, test_5a_wrong_plan_hash, test_5b_wrong_catalog_hash, test_5c_wrong_image_digest, test_6_changed_result_file_after_manifest, test_7_mismatched_run_id_checkpoint, test_8_symlink_escape, test_9_privacy_secret_rejection, test_10_draft_pr_payload, test_11a_no_token_bearing_git_urls, test_11b_no_token_argv, test_12_placeholder_rejected, test_13_containment_and_safe_copy_tree, test_14_run_intent_schema_required_fields, test_15_missing_intent_fail_closed, test_16_unexpected_files_rejected, test_17_no_merge_no_milestone_api_paths]
    passed = 0
    failed = 0
    skipped = 0
    for t in tests:
        print(f'\n--- {t.__name__} ---')
        try:
            result = t()
            if result == 'skip':
                skipped += 1
            elif result:
                passed += 1
            else:
                failed += 1
        except Exception:
            import traceback
            traceback.print_exc()
            failed += 1
    print(f"\n{'=' * 60}")
    print(f'Results: {passed} passed, {failed} failed, {skipped} skipped, {len(tests)} total')
    print(f"{'=' * 60}")
    return 0 if failed == 0 else 1

def test_1_valid_intent_manifest_acceptance():
    """Scenario 1: a valid intent + manifest is accepted by the strict validator."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()})
        ok, errors, warnings = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if not ok:
            print(f'FAIL: valid bundle rejected: {errors}')
            assert False, f'FAIL: valid bundle rejected: {errors}'
        print('PASS: valid intent/manifest accepted')
        assert True

def test_2_stale_generated_at_rejection():
    """Scenario 2: a stale generated_at is rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        stale = (dt.datetime.now(dt.timezone.utc) - dt.timedelta(hours=48)).strftime('%Y-%m-%dT%H:%M:%SZ')
        intent = _make_intent(generated_at=stale)
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()})
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: stale generated_at was not rejected')
            assert False, 'FAIL: stale generated_at was not rejected'
        if not any(('stale' in e.lower() for e in errors)):
            print(f'FAIL: stale not reported: {errors}')
            assert False, f'FAIL: stale not reported: {errors}'
        print('PASS: stale generated_at rejected')
        assert True

def test_3_dry_run_true_rejection():
    """Scenario 3: dry_run=true is rejected for a physical run."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent(dry_run=True)
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()})
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: dry_run=true was not rejected')
            assert False, 'FAIL: dry_run=true was not rejected'
        if not any(('dry_run' in e for e in errors)):
            print(f'FAIL: dry_run not reported: {errors}')
            assert False, f'FAIL: dry_run not reported: {errors}'
        print('PASS: dry_run=true rejected')
        assert True

def test_4_wrong_source_commit():
    """Scenario 4: a wrong/unknown source commit is rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent(source_commit='0' * 40)
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()})
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: all-zero source_commit was not rejected')
            assert False, 'FAIL: all-zero source_commit was not rejected'
        if not any(('source_commit' in e for e in errors)):
            print(f'FAIL: source_commit issue not reported: {errors}')
            assert False, f'FAIL: source_commit issue not reported: {errors}'
        print('PASS: wrong source_commit rejected')
        assert True

def test_5a_wrong_plan_hash():
    """Scenario 5: a wrong plan_sha256 is rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent(plan_sha256='c' * 64)
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()}, manifest_provenance_override={'run_id': intent['run_id'], 'source_commit': intent['source_commit'], 'source_version': intent['source_version'], 'testos_version': intent['testos_version'], 'testos_image_digest': intent['testos_image_digest'], 'plan_sha256': 'd' * 64, 'benchmark_catalog_sha256': intent['benchmark_catalog_sha256'], 'intent_generated_at': intent['generated_at'], 'intent_dry_run': intent['dry_run'], 'checkpoint_nonce': intent['checkpoint_nonce'], 'intent_sha256': _sha256_bytes(json.dumps(intent, indent=2).encode())})
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: wrong plan_sha256 was not rejected')
            assert False, 'FAIL: wrong plan_sha256 was not rejected'
        if not any(('plan_sha256' in e for e in errors)):
            print(f'FAIL: plan_sha256 mismatch not reported: {errors}')
            assert False, f'FAIL: plan_sha256 mismatch not reported: {errors}'
        print('PASS: wrong plan_sha256 rejected')
        assert True

def test_5b_wrong_catalog_hash():
    """Scenario 5: a wrong benchmark_catalog_sha256 is rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent(benchmark_catalog_sha256='e' * 64)
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()})
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: wrong catalog hash was not rejected')
            assert False, 'FAIL: wrong catalog hash was not rejected'
        if not any(('benchmark_catalog_sha256' in e or 'catalog' in e.lower() for e in errors)):
            print(f'FAIL: catalog hash mismatch not reported: {errors}')
            assert False, f'FAIL: catalog hash mismatch not reported: {errors}'
        print('PASS: wrong catalog hash rejected')
        assert True

def test_5c_wrong_image_digest():
    """Scenario 5: a malformed testos_image_digest is rejected by the schema."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent(testos_image_digest='not-a-digest')
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()})
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: malformed image digest was not rejected')
            assert False, 'FAIL: malformed image digest was not rejected'
        print(f'PASS: malformed image digest rejected ({len(errors)} errors)')
        assert True

def test_6_changed_result_file_after_manifest():
    """Scenario 6: a result file changed after the result-hashes sidecar was written."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        rf = _good_result_file()
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': rf})
        original = _sha256_file(run_dir / 'shell-pass.json')
        (run_dir / 'result-hashes.json').write_text(json.dumps({'shell-pass.json': original}))
        rf['value'] = 999.0
        (run_dir / 'shell-pass.json').write_text(json.dumps(rf))
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT, strict=True)
        if ok:
            print('FAIL: changed result file was not detected')
            assert False, 'FAIL: changed result file was not detected'
        if not any(('changed' in e.lower() and 'shell-pass' in e for e in errors)):
            print(f'FAIL: tamper not reported: {errors}')
            assert False, f'FAIL: tamper not reported: {errors}'
        print('PASS: changed result file detected')
        assert True

def test_7_mismatched_run_id_checkpoint():
    """Scenario 7: a mismatched run_id/checkpoint between manifest and intent is rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()})
        manifest = json.loads((run_dir / 'manifest.json').read_text())
        manifest['provenance']['run_id'] = 'different-run-id-xyz'
        (run_dir / 'manifest.json').write_text(json.dumps(manifest))
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: mismatched run_id was not rejected')
            assert False, 'FAIL: mismatched run_id was not rejected'
        if not any(('run_id' in e for e in errors)):
            print(f'FAIL: run_id mismatch not reported: {errors}')
            assert False, f'FAIL: run_id mismatch not reported: {errors}'
        print('PASS: mismatched run_id rejected')
        assert True

def test_8_symlink_escape():
    """Scenario 8: symlink escape is rejected (environment-dependent)."""
    if not _can_symlink():
        print('SKIP: os.symlink unavailable in this environment (cloud sandbox); symlink-escape rejection is verified by rush_path_safety.reject_symlinks logic but cannot be exercised here. CI on Linux runners with symlink permission will run this test.')
        return 'skip'
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()})
        os.symlink('/etc/passwd', run_dir / 'evil.json')
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        symlinks = rps.reject_symlinks(run_dir)
        if not symlinks:
            print('FAIL: reject_symlinks did not find the symlink')
            assert False, 'FAIL: reject_symlinks did not find the symlink'
        try:
            rps.safe_copy_tree(run_dir, Path(tmp) / 'dst')
            print('FAIL: safe_copy_tree did not reject symlink-containing tree')
            assert False, 'FAIL: safe_copy_tree did not reject symlink-containing tree'
        except ValueError:
            pass
        print('PASS: symlink escape rejected by reject_symlinks and safe_copy_tree')
        assert True

def test_9_privacy_secret_rejection():
    """Scenario 9: an unredacted secret in the bundle is rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        rf = _good_result_file()
        rf['stdout'] = 'ghp_' + 'a' * 36
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': rf})
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: unredacted secret was not rejected')
            assert False, 'FAIL: unredacted secret was not rejected'
        if not any(('secret' in e.lower() or 'unredacted' in e.lower() for e in errors)):
            print(f'FAIL: secret not reported: {errors}')
            assert False, f'FAIL: secret not reported: {errors}'
        print('PASS: privacy-secret rejected')
        assert True

def test_10_draft_pr_payload():
    """Scenario 10: submission safety — evidence PRs are always drafts."""
    src = (TOOLS_DIR / 'rush-submit-evidence').read_text()
    if '"draft": True' not in src:
        print('FAIL: rush-submit-evidence does not enforce draft=True on PR creation')
        assert False, 'FAIL: rush-submit-evidence does not enforce draft=True on PR creation'
    if 'assert_safe_api_path' not in src:
        print('FAIL: assert_safe_api_path guard not wired into _gh_api')
        assert False, 'FAIL: assert_safe_api_path guard not wired into _gh_api'
    manifest = {'started_at': '2026-07-15T10:00:00Z', 'host': {'fingerprint': 'test-host-0012'}, 'passed': ['x'], 'failed': []}
    branch = rse.deterministic_branch(manifest)
    msg = rse.deterministic_commit_msg(manifest)
    if not branch.startswith('evidence/'):
        print(f'FAIL: bad branch shape: {branch}')
        assert False, f'FAIL: bad branch shape: {branch}'
    if 'evidence(' not in msg:
        print(f'FAIL: bad commit msg: {msg}')
        assert False, f'FAIL: bad commit msg: {msg}'
    print(f'PASS: draft PR enforced; branch={branch}')
    assert True

def test_11a_no_token_bearing_git_urls():
    src = (TOOLS_DIR / 'rush-submit-evidence').read_text()
    bad = []
    for line in src.splitlines():
        for m in re.finditer('https?://[^\\s\'\\"()<>]+', line):
            candidate = m.group(0).rstrip('.,;')
            parts = urlsplit(candidate)
            if parts.scheme not in ('http', 'https'):
                continue
            if parts.hostname != 'github.com':
                continue
            if parts.username is not None or parts.password is not None or '@' in parts.netloc:
                bad.append(line.strip())
                break
    if bad:
        print(f'FAIL: token-bearing git URL found: {bad}')
        assert False, f'FAIL: token-bearing git URL found: {bad}'
    if 'https://github.com/Nan0pk/Rush-linux.git' not in src:
        print('FAIL: bare clone URL not found')
        assert False, 'FAIL: bare clone URL not found'
    print('PASS: no token-bearing git URLs in submission source')
    assert True

def test_11b_no_token_argv():
    """Scenario 11: assert_no_token_argv rejects token-bearing argv elements."""
    try:
        rse.assert_no_token_argv(['git', 'clone', 'https://ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa@github.com/x.git'])
        print('FAIL: token URL not rejected')
        assert False, 'FAIL: token URL not rejected'
    except SystemExit:
        pass
    try:
        rse.assert_no_token_argv(['x', 'github_pat_' + 'a' * 82])
        print('FAIL: github_pat_ not rejected')
        assert False, 'FAIL: github_pat_ not rejected'
    except SystemExit:
        pass
    try:
        rse.assert_no_token_argv(['curl', '-H', 'Authorization: Bearer sometoken12345678901234567890'])
        print('FAIL: bearer header not rejected')
        assert False, 'FAIL: bearer header not rejected'
    except SystemExit:
        pass
    rse.assert_no_token_argv(['git', 'clone', 'https://github.com/Nan0pk/Rush-linux.git', '/tmp/x'])
    print('PASS: assert_no_token_argv rejects tokens, allows clean argv')
    assert True

def test_12_placeholder_rejected():
    """Fail-closed: placeholder metadata is never valid provenance."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()})
        manifest = json.loads((run_dir / 'manifest.json').read_text())
        for k in ('source_commit', 'source_version', 'testos_version', 'testos_image_digest', 'plan_sha256', 'benchmark_catalog_sha256', 'intent_sha256', 'checkpoint_nonce'):
            manifest['provenance'][k] = 'unknown'
        (run_dir / 'manifest.json').write_text(json.dumps(manifest))
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: placeholder provenance was accepted')
            assert False, 'FAIL: placeholder provenance was accepted'
        if not any(('placeholder' in e.lower() for e in errors)):
            print(f'FAIL: placeholder not reported: {errors}')
            assert False, f'FAIL: placeholder not reported: {errors}'
        print('PASS: placeholder provenance rejected')
        assert True

def test_13_containment_and_safe_copy_tree():
    """rush_path_safety: prove_containment + safe_copy_tree reject escape."""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / 'src'
        root.mkdir()
        (root / 'a').mkdir()
        (root / 'a' / 'f.txt').write_text('hello')
        rps.prove_containment(root / 'a' / 'f.txt', root)
        try:
            rps.prove_containment(Path('/etc/passwd'), root)
            print('FAIL: containment escape not caught')
            assert False, 'FAIL: containment escape not caught'
        except ValueError:
            pass
        dst = Path(tmp) / 'dst'
        copied = rps.safe_copy_tree(root, dst)
        if len(copied) != 1:
            print(f'FAIL: expected 1 copied file, got {len(copied)}')
            assert False, f'FAIL: expected 1 copied file, got {len(copied)}'
        if (dst / 'a' / 'f.txt').read_text() != 'hello':
            print('FAIL: copied content mismatch')
            assert False, 'FAIL: copied content mismatch'
        print('PASS: containment + safe_copy_tree work')
        assert True

def test_14_run_intent_schema_required_fields():
    """The run-intent schema rejects missing required fields and wrong discriminator."""
    schema = validate_testos.SCHEMAS['intent']
    bad = _make_intent()
    del bad['run_id']
    errs = validate_testos.validate_against_schema(bad, schema, '$')
    if not any(('run_id' in e for e in errs)):
        print(f'FAIL: missing run_id not caught: {errs}')
        assert False, f'FAIL: missing run_id not caught: {errs}'
    bad = _make_intent()
    bad['intent_kind'] = 'wrong'
    errs = validate_testos.validate_against_schema(bad, schema, '$')
    if not any(('intent_kind' in e for e in errs)):
        print(f'FAIL: wrong intent_kind not caught: {errs}')
        assert False, f'FAIL: wrong intent_kind not caught: {errs}'
    bad = _make_intent()
    bad['schema_version'] = 2
    errs = validate_testos.validate_against_schema(bad, schema, '$')
    if not any(('schema_version' in e for e in errs)):
        print(f'FAIL: wrong schema_version not caught: {errs}')
        assert False, f'FAIL: wrong schema_version not caught: {errs}'
    bad = _make_intent()
    bad['extra_field'] = 'x'
    errs = validate_testos.validate_against_schema(bad, schema, '$')
    if not any(('additional property' in e for e in errs)):
        print(f'FAIL: additional property not caught: {errs}')
        assert False, f'FAIL: additional property not caught: {errs}'
    print('PASS: run-intent schema enforces required fields + discriminator + version')
    assert True

def test_15_missing_intent_fail_closed():
    """Fail-closed: a manifest with provenance but no run-intent.json sidecar fails."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()}, include_intent=False)
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: missing run-intent.json was not rejected')
            assert False, 'FAIL: missing run-intent.json was not rejected'
        if not any(('run-intent.json' in e for e in errors)):
            print(f'FAIL: missing intent not reported: {errors}')
            assert False, f'FAIL: missing intent not reported: {errors}'
        print('PASS: missing run-intent.json fails closed')
        assert True

def test_16_unexpected_files_rejected():
    """Unexpected evidence files are rejected."""
    with tempfile.TemporaryDirectory() as tmp:
        intent = _make_intent()
        run_dir = _write_run_dir(Path(tmp), intent, result_files={'shell-pass.json': _good_result_file()}, extra_files={'surprise.bin': b'\x00\x01\x02'})
        ok, errors, _ = validate_testos.validate_run_dir(run_dir, REPO_ROOT)
        if ok:
            print('FAIL: unexpected file was not rejected')
            assert False, 'FAIL: unexpected file was not rejected'
        if not any(('unexpected' in e.lower() for e in errors)):
            print(f'FAIL: unexpected file not reported: {errors}')
            assert False, f'FAIL: unexpected file not reported: {errors}'
        print('PASS: unexpected file rejected')
        assert True

def test_17_no_merge_no_milestone_api_paths():
    """assert_safe_api_path rejects merge/milestone/release paths."""
    for path, method in [('pulls/42/merge', 'PUT'), ('milestones', 'POST'), ('releases', 'POST')]:
        try:
            rse.assert_safe_api_path(path, method)
            print(f'FAIL: {method} {path} not rejected')
            assert False, f'FAIL: {method} {path} not rejected'
        except SystemExit:
            pass
    rse.assert_safe_api_path('pulls?head=Nan0pk:x&state=open', 'GET')
    rse.assert_safe_api_path('pulls', 'POST')
    rse.assert_safe_api_path('issues/42/labels', 'POST')
    print('PASS: merge/milestone/release API paths rejected; allowed paths pass')
    assert True

def _can_symlink() -> bool:
    """Return True if os.symlink works in this environment."""
    try:
        with tempfile.TemporaryDirectory() as t:
            os.symlink('/etc/passwd', Path(t) / 'lnk')
            return True
    except (OSError, PermissionError):
        return False
import re
if __name__ == '__main__':
    sys.exit(run_tests())