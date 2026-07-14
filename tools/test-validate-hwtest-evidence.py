#!/usr/bin/env python3
"""
pytest tests for tools/validate-hwtest-evidence.py and the hardware evidence
schema validator.

Tests:
  - Each fixture has the expected outcome (good passes, bad fixtures fail with
    the expected error substrings).
  - The validator's schema checking catches type mismatches, missing required
    fields, and invalid enum values.
  - The event chain validation catches tampering.
  - The secret detection catches unredacted tokens.

Run with:
  python3 -m pytest tools/test-validate-hwtest-evidence.py -v
  # or
  python3 tools/test-validate-hwtest-evidence.py  # standalone
"""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent
_FIXTURES = _TOOLS_DIR / "test-fixtures" / "hwtest"


def _load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


validator = _load_module("validate_hwtest_evidence", _TOOLS_DIR / "validate-hwtest-evidence.py")


def _fixtures() -> list[Path]:
    """Return all fixture directories (sorted)."""
    if not _FIXTURES.exists():
        return []
    return sorted(d for d in _FIXTURES.iterdir() if d.is_dir())


def _run_validator(bundle: Path) -> tuple[int, str, str]:
    """Run the validator on a single bundle. Returns (exit_code, stdout, stderr)."""
    r = subprocess.run(
        ["python3", str(_TOOLS_DIR / "validate-hwtest-evidence.py"), "--bundle", str(bundle)],
        capture_output=True,
        text=True,
        timeout=30,
        cwd=str(_ROOT),
    )
    return r.returncode, r.stdout, r.stderr


# ─── Tests: each fixture matches its expected outcome ────────────────────────


def test_good_laptop_passes():
    """The good-laptop fixture should pass all 14 checks."""
    d = _FIXTURES / "good-laptop"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert ok, f"good-laptop should pass, got errors: {errors}"


def test_missing_manifest_fails():
    """Missing manifest is detected."""
    d = _FIXTURES / "missing-manifest"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert not ok
    assert any("missing required file: hwtest-manifest.json" in e for e in errors)


def test_wrong_version_fails():
    """Wrong source_version is detected."""
    d = _FIXTURES / "wrong-version"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert not ok
    assert any("does not match VERSION" in e for e in errors)


def test_laptop_no_battery_fails():
    """Laptop slot without battery is detected."""
    d = _FIXTURES / "laptop-no-battery"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert not ok
    assert any("laptop slot requires battery_design_uwh" in e for e in errors)


def test_battery_run_on_ac_fails():
    """Battery run with AC online is detected."""
    d = _FIXTURES / "battery-run-on-ac"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert not ok
    assert any("power_source=battery but ac_online=true" in e for e in errors)


def test_missing_baseline_pair_fails():
    """Missing baseline result is detected."""
    d = _FIXTURES / "missing-baseline-pair"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert not ok
    assert any("baseline result file missing" in e for e in errors)


def test_insufficient_samples_fails():
    """Insufficient sample count is detected."""
    d = _FIXTURES / "insufficient-samples"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert not ok
    assert any("insufficient" in e.lower() for e in errors)


def test_malformed_results_fails():
    """Malformed baseline result JSON is detected."""
    d = _FIXTURES / "malformed-results"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert not ok
    assert any("cannot parse" in e for e in errors)


def test_secret_leakage_fails():
    """Unredacted secret in transcript is detected."""
    d = _FIXTURES / "secret-leakage"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert not ok
    assert any("unredacted secret detected" in e for e in errors)


def test_ai_only_verdict_fails():
    """AI-generated verdict as evidence is detected."""
    d = _FIXTURES / "ai-only-verdict"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert not ok
    assert any("AI-summary-as-evidence" in e for e in errors)


def test_broken_event_chain_fails():
    """Tampered event chain is detected."""
    d = _FIXTURES / "broken-event-chain"
    ok, errors, _ = validator.validate_bundle(d, _ROOT)
    assert not ok
    assert any("event chain" in e.lower() for e in errors)


# ─── Tests: --fixtures mode ──────────────────────────────────────────────────


def test_fixtures_mode_all_match_expectations():
    """Running `validate-hwtest-evidence.py --fixtures` should report 11 passed, 0 failed."""
    r = subprocess.run(
        ["python3", str(_TOOLS_DIR / "validate-hwtest-evidence.py"), "--fixtures"],
        capture_output=True,
        text=True,
        timeout=60,
        cwd=str(_ROOT),
    )
    assert r.returncode == 0, f"fixtures mode should pass, got rc={r.returncode}\n{r.stdout}\n{r.stderr}"
    assert "11 passed" in r.stdout
    assert "0 failed" in r.stdout


# ─── Tests: schema validation ────────────────────────────────────────────────


def test_schema_rejects_missing_required_field():
    """The schema validator catches missing required fields."""
    obj = {"schema_version": 1}  # missing most required fields
    errors = validator.validate_against_schema(obj, validator.SCHEMAS["manifest"], "$")
    assert len(errors) > 0
    assert any("missing required field" in e for e in errors)


def test_schema_rejects_wrong_type():
    """The schema validator catches type mismatches."""
    obj = {
        "schema_version": "1",  # string, not integer
        "manifest_kind": "hwtest-manifest",
        "source_version": "0.7.0-beta.1",
        "source_commit": "a" * 40,
        "hardware_slot": "laptop",
        "bundle_created_at": "2026-07-04T12:00:00Z",
        "plan_path": "p.json",
        "host_path": "h.json",
        "baseline_result_path": "b.json",
        "optid_result_path": "o.json",
        "verdict_path": "v.md",
        "events_path": "e.jsonl",
        "privacy_report_path": "p.json",
    }
    errors = validator.validate_against_schema(obj, validator.SCHEMAS["manifest"], "$")
    assert any("expected type integer" in e for e in errors)


def test_schema_rejects_invalid_enum():
    """The schema validator catches invalid enum values."""
    obj = {"hardware_slot": "tablet"}  # not in enum
    errors = validator.validate_against_schema(obj, validator.SCHEMAS["manifest"], "$")
    assert any("not in enum" in e for e in errors)


def test_schema_rejects_additional_properties():
    """The schema validator catches additional properties."""
    obj = {
        "schema_version": 1,
        "manifest_kind": "hwtest-manifest",
        "source_version": "0.7.0-beta.1",
        "source_commit": "a" * 40,
        "hardware_slot": "laptop",
        "bundle_created_at": "2026-07-04T12:00:00Z",
        "plan_path": "p.json",
        "host_path": "h.json",
        "baseline_result_path": "b.json",
        "optid_result_path": "o.json",
        "verdict_path": "v.md",
        "events_path": "e.jsonl",
        "privacy_report_path": "p.json",
        "extra_field": "not allowed",
    }
    errors = validator.validate_against_schema(obj, validator.SCHEMAS["manifest"], "$")
    assert any("additional property not allowed" in e for e in errors)


def test_schema_rejects_bad_pattern():
    """The schema validator catches pattern mismatches."""
    obj = {"source_commit": "not-a-sha"}
    errors = validator.validate_against_schema(obj, validator.SCHEMAS["manifest"], "$")
    assert any("does not match pattern" in e for e in errors)


def test_schema_accepts_valid_manifest():
    """A valid manifest passes schema validation."""
    obj = {
        "schema_version": 1,
        "manifest_kind": "hwtest-manifest",
        "source_version": "0.7.0-beta.1",
        "source_commit": "a" * 40,
        "hardware_slot": "laptop",
        "bundle_created_at": "2026-07-04T12:00:00Z",
        "plan_path": "hwtest-plan.json",
        "host_path": "hwtest-host.json",
        "baseline_result_path": "hwtest-result-baseline.json",
        "optid_result_path": "hwtest-result-optid.json",
        "verdict_path": "VERDICT.md",
        "events_path": "events.jsonl",
        "privacy_report_path": "privacy-report.json",
    }
    errors = validator.validate_against_schema(obj, validator.SCHEMAS["manifest"], "$")
    assert errors == [], f"valid manifest should pass, got: {errors}"


# ─── Tests: --bundle mode ────────────────────────────────────────────────────


def test_bundle_mode_passes_good_fixture():
    """`--bundle <good-laptop>` should exit 0."""
    r = _run_validator(_FIXTURES / "good-laptop")
    assert r[0] == 0, f"good-laptop should pass, got rc={r[0]}\n{r[1]}"


def test_bundle_mode_fails_bad_fixture():
    """`--bundle <secret-leakage>` should exit 1."""
    r = _run_validator(_FIXTURES / "secret-leakage")
    assert r[0] == 1
    assert "unredacted secret" in r[1]


# ─── Tests: validate-evidence.py integration ─────────────────────────────────


def test_validate_evidence_still_passes():
    """The extended validate-evidence.py should still pass on the current repo."""
    r = subprocess.run(
        ["python3", str(_TOOLS_DIR / "validate-evidence.py")],
        capture_output=True,
        text=True,
        timeout=30,
        cwd=str(_ROOT),
    )
    assert r.returncode == 0, f"validate-evidence.py should pass, got rc={r.returncode}\n{r.stdout}\n{r.stderr}"


# ─── Tests: fixture count ────────────────────────────────────────────────────


def test_there_are_11_fixtures():
    """Verify all 11 fixtures exist."""
    fixtures = _fixtures()
    assert len(fixtures) == 11, f"expected 11 fixtures, got {len(fixtures)}: {[f.name for f in fixtures]}"


def test_every_fixture_has_expected_json():
    """Every fixture directory has an expected.json file."""
    for f in _fixtures():
        assert (f / "expected.json").exists(), f"{f.name} missing expected.json"


# ─── Standalone runner ───────────────────────────────────────────────────────


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
