#!/usr/bin/env python3
"""
tools/validate-hwtest-evidence.py — semantic hardware evidence validator.

Validates hardware evidence bundles (release/evidence/host-bench/<date>-<hostname>/
or tools/test-fixtures/hwtest/<fixture-name>/) for structural and content
integrity. This is the content-aware validator that closes the "existence vs
truth" gap identified in the third-pass tech-debt audit (audit #4).

Checks (14):
  1.  required files exist (manifest, plan, host, baseline result, optid result,
      verdict, events, privacy report)
  2.  manifest parses as JSON and conforms to the manifest schema
  3.  source_version exists in the VERSION file
  4.  source_commit exists in git (rev-parse --verify)
  5.  hardware_slot is valid (desktop | laptop)
  6.  laptop evidence has battery metadata (battery_design_uwh > 0)
  7.  battery runs actually ran on battery/discharging (power_source=battery,
      ac_online=false)
  8.  AC runs actually ran on AC/online (power_source=ac, ac_online=true)
  9.  baseline and optid runs are paired (both result files exist and parse)
  10. sample count is sufficient (every metric n >= plan.min_samples)
  11. results parse as JSON and conform to the result schema
  12. privacy report exists and parses
  13. obvious secrets absent (scan all files in the bundle for token patterns)
  14. AI summaries do not count as evidence (VERDICT.md must not contain
      "AI-generated" or "AI summary" as a substitute for human verdict;
      submitted verdicts are ADVISORY ONLY)

  Plus: event chain validates (tamper-evident SHA-256 chain intact).

Usage:
  python3 tools/validate-hwtest-evidence.py --bundle <path>
  python3 tools/validate-hwtest-evidence.py --fixtures
  python3 tools/validate-hwtest-evidence.py --release-evidence

Exit codes:
  0 — all checked bundles pass
  1 — one or more bundles have violations
  2 — internal error (missing schemas, missing fixtures dir, etc.)
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

# Import the shared capture library for event-chain validation and redaction.
_TOOLS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(_TOOLS_DIR))
import rush_capture_lib as lib  # noqa: E402

ROOT = _TOOLS_DIR.parent
SCHEMAS_DIR = ROOT / "schemas"
FIXTURES_DIR = _TOOLS_DIR / "test-fixtures" / "hwtest"
VERSION_FILE = ROOT / "VERSION"


# ─── Schema loading ──────────────────────────────────────────────────────────


def _load_schema(name: str) -> dict:
    path = SCHEMAS_DIR / name
    if not path.exists():
        print(f"validate-hwtest-evidence: schema not found: {path}", file=sys.stderr)
        sys.exit(2)
    with open(path) as f:
        return json.load(f)


SCHEMAS = {
    "manifest": _load_schema("hwtest-manifest.schema.json"),
    "plan": _load_schema("hwtest-plan.schema.json"),
    "host": _load_schema("hwtest-host.schema.json"),
    "result": _load_schema("hwtest-result.schema.json"),
}


# ─── Hand-rolled JSON Schema validator (no external deps) ─────────────────────
#
# We don't depend on the `jsonschema` library (it's not in the repo's
# dependency set). This validator implements the subset of JSON Schema 2020-12
# we use: type, required, const, enum, pattern, minimum, maximum, minItems,
# maxItems, additionalProperties=false, properties, items. It's enough for our
# schemas and keeps the tool stdlib-only (matching the repo convention).


def validate_against_schema(obj: Any, schema: dict, path: str = "$") -> list[str]:
    """Validate `obj` against `schema`. Returns a list of error strings."""
    errors: list[str] = []
    stype = schema.get("type")
    if stype:
        if not _check_type(obj, stype):
            errors.append(f"{path}: expected type {stype}, got {type(obj).__name__}")
            return errors
    if "const" in schema:
        if obj != schema["const"]:
            errors.append(f"{path}: expected const {schema['const']!r}, got {obj!r}")
    if "enum" in schema:
        if obj not in schema["enum"]:
            errors.append(f"{path}: {obj!r} not in enum {schema['enum']}")
    if "pattern" in schema and isinstance(obj, str):
        if not re.match(schema["pattern"], obj):
            errors.append(f"{path}: {obj!r} does not match pattern {schema['pattern']!r}")
    if "minimum" in schema and isinstance(obj, (int, float)) and not isinstance(obj, bool):
        if obj < schema["minimum"]:
            errors.append(f"{path}: {obj} < minimum {schema['minimum']}")
    if "maximum" in schema and isinstance(obj, (int, float)) and not isinstance(obj, bool):
        if obj > schema["maximum"]:
            errors.append(f"{path}: {obj} > maximum {schema['maximum']}")
    if "minItems" in schema and isinstance(obj, list):
        if len(obj) < schema["minItems"]:
            errors.append(f"{path}: {len(obj)} items < minItems {schema['minItems']}")
    if isinstance(obj, dict):
        required = schema.get("required", [])
        for r in required:
            if r not in obj:
                errors.append(f"{path}: missing required field {r!r}")
        props = schema.get("properties", {})
        addl = schema.get("additionalProperties", True)
        for k, v in obj.items():
            if k in props:
                errors.extend(validate_against_schema(v, props[k], f"{path}.{k}"))
            elif addl is False:
                errors.append(f"{path}.{k}: additional property not allowed")
    if isinstance(obj, list) and "items" in schema:
        for i, item in enumerate(obj):
            errors.extend(validate_against_schema(item, schema["items"], f"{path}[{i}]"))
    return errors


def _check_type(obj: Any, t: str) -> bool:
    if t == "object":
        return isinstance(obj, dict)
    if t == "array":
        return isinstance(obj, list)
    if t == "string":
        return isinstance(obj, str)
    if t == "integer":
        return isinstance(obj, int) and not isinstance(obj, bool)
    if t == "number":
        return isinstance(obj, (int, float)) and not isinstance(obj, bool)
    if t == "boolean":
        return isinstance(obj, bool)
    if t == "null":
        return obj is None
    return True


# ─── Bundle validation ───────────────────────────────────────────────────────


class BundleValidator:
    """Validates a single hardware evidence bundle."""

    def __init__(self, bundle_dir: Path, repo_root: Path):
        self.bundle_dir = bundle_dir
        self.repo_root = repo_root
        self.errors: list[str] = []
        self.warnings: list[str] = []
        self.manifest: dict | None = None
        self.plan: dict | None = None
        self.host: dict | None = None
        self.baseline_result: dict | None = None
        self.optid_result: dict | None = None

    def err(self, msg: str) -> None:
        self.errors.append(f"{self.bundle_dir.name}: {msg}")

    def warn(self, msg: str) -> None:
        self.warnings.append(f"{self.bundle_dir.name}: {msg}")

    def validate(self) -> tuple[bool, list[str], list[str]]:
        """Run all 14 checks + event chain validation."""
        self._check_required_files()
        if self.manifest is None:
            return (False, self.errors, self.warnings)

        self._check_manifest_parses()
        self._check_source_version()
        self._check_source_commit()
        self._check_hardware_slot()
        self._check_laptop_battery()
        self._check_battery_runs_on_battery()
        self._check_ac_runs_on_ac()
        self._check_baseline_optid_paired()
        self._check_sample_count()
        self._check_results_parse()
        self._check_privacy_report()
        self._check_secrets_absent()
        self._check_ai_not_evidence()
        self._check_event_chain()
        return (len(self.errors) == 0, self.errors, self.warnings)

    # ─── Individual checks ───────────────────────────────────────────────────

    def _check_required_files(self) -> None:
        """Check 1: required files exist."""
        manifest_path = self.bundle_dir / "hwtest-manifest.json"
        if not manifest_path.exists():
            self.err("missing required file: hwtest-manifest.json")
            return
        # Load the manifest to find the other files.
        try:
            with open(manifest_path) as f:
                self.manifest = json.load(f)
        except (OSError, json.JSONDecodeError) as e:
            self.err(f"hwtest-manifest.json: cannot parse: {e}")
            return

        # Check the manifest-referenced files exist.
        # STEP 4D hardening: reject path traversal — manifest path fields
        # must be relative, must not contain "..", and must resolve to a
        # path under bundle_dir.
        for field in ["plan_path", "host_path", "baseline_result_path",
                       "optid_result_path", "verdict_path", "events_path",
                       "privacy_report_path"]:
            rel = self.manifest.get(field)
            if not rel:
                self.err(f"manifest missing field: {field}")
                continue
            # Reject absolute paths.
            if Path(rel).is_absolute():
                self.err(f"manifest.{field}: absolute path rejected: {rel!r}")
                continue
            # Reject paths containing "..".
            if ".." in Path(rel).parts:
                self.err(f"manifest.{field}: path traversal rejected (contains '..'): {rel!r}")
                continue
            # Resolve and verify it stays under bundle_dir.
            p = (self.bundle_dir / rel).resolve()
            try:
                p.relative_to(self.bundle_dir.resolve())
            except ValueError:
                self.err(f"manifest.{field}: path escapes bundle_dir: {rel!r}")
                continue
            if not p.exists():
                self.err(f"missing required file (from manifest.{field}): {rel}")

    def _check_manifest_parses(self) -> None:
        """Check 2: manifest parses and conforms to schema."""
        if self.manifest is None:
            return
        errors = validate_against_schema(self.manifest, SCHEMAS["manifest"], "$.manifest")
        for e in errors:
            self.err(e)

    def _check_source_version(self) -> None:
        """Check 3: source_version exists in the VERSION file."""
        if self.manifest is None:
            return
        sv = self.manifest.get("source_version")
        if not sv:
            return  # already caught by schema check
        if not VERSION_FILE.exists():
            self.err(f"VERSION file not found at {VERSION_FILE}")
            return
        version = VERSION_FILE.read_text().strip()
        if sv != version:
            self.err(
                f"source_version {sv!r} does not match VERSION file {version!r}"
            )

    def _check_source_commit(self) -> None:
        """Check 4: source_commit exists in git.

        Handles shallow clones: if ``git cat-file`` fails (the commit is
        not in the local object store), attempt ``git fetch --depth=1
        origin <sha>`` to retrieve just that commit before declaring it
        missing. This makes the validator work from a ``--depth 1`` clone,
        which is what ``livedev-bootstrap.sh`` and CI typically create.
        """
        if self.manifest is None:
            return
        commit = self.manifest.get("source_commit")
        if not commit:
            return  # already caught by schema check
        try:
            r = subprocess.run(
                ["git", "-C", str(self.repo_root), "cat-file", "-t", commit],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if r.returncode == 0 and r.stdout.strip() == "commit":
                return  # commit exists locally

            # Shallow-clone recovery: try to fetch just this commit.
            # This is safe — it only adds objects to the local store,
            # it does not modify any branch or working tree.
            # Validate the commit hash format before passing to git to
            # prevent injection (git is generally safe, but defense in
            # depth).
            if not all(c in "0123456789abcdefABCDEF" for c in commit) or len(commit) < 7:
                self.err(
                    f"source_commit {commit!r} is not a valid git SHA"
                )
                return

            fetch_r = subprocess.run(
                [
                    "git",
                    "-C",
                    str(self.repo_root),
                    "fetch",
                    "--depth=1",
                    "origin",
                    commit,
                ],
                capture_output=True,
                text=True,
                timeout=30,
            )
            if fetch_r.returncode == 0:
                # Re-check after fetch.
                r2 = subprocess.run(
                    [
                        "git",
                        "-C",
                        str(self.repo_root),
                        "cat-file",
                        "-t",
                        commit,
                    ],
                    capture_output=True,
                    text=True,
                    timeout=5,
                )
                if r2.returncode == 0 and r2.stdout.strip() == "commit":
                    return  # successfully fetched
                self.err(
                    f"source_commit {commit!r} fetched but still not resolvable"
                )
            else:
                self.err(
                    f"source_commit {commit!r} does not exist in git "
                    f"(not in local store and fetch failed: "
                    f"{fetch_r.stderr.strip()[:200]})"
                )
        except (OSError, subprocess.TimeoutExpired) as e:
            self.warn(f"could not verify source_commit in git: {e}")

    def _check_hardware_slot(self) -> None:
        """Check 5: hardware_slot is valid (desktop | laptop)."""
        if self.manifest is None:
            return
        slot = self.manifest.get("hardware_slot")
        if slot not in ("desktop", "laptop"):
            self.err(f"hardware_slot {slot!r} must be 'desktop' or 'laptop'")

    def _load_host(self) -> None:
        if self.host is not None or self.manifest is None:
            return
        host_path = self.manifest.get("host_path")
        if not host_path:
            return
        p = self.bundle_dir / host_path
        if not p.exists():
            return
        try:
            with open(p) as f:
                self.host = json.load(f)
            errors = validate_against_schema(self.host, SCHEMAS["host"], "$.host")
            for e in errors:
                self.err(e)
        except (OSError, json.JSONDecodeError) as e:
            self.err(f"host file: cannot parse: {e}")

    def _check_laptop_battery(self) -> None:
        """Check 6: laptop evidence has battery metadata (battery_design_uwh > 0)."""
        if self.manifest is None:
            return
        slot = self.manifest.get("hardware_slot")
        if slot != "laptop":
            return
        self._load_host()
        if self.host is None:
            self.err("laptop slot but host.json cannot be loaded")
            return
        batt = self.host.get("battery_design_uwh", 0)
        if not batt or batt <= 0:
            self.err(
                f"laptop slot requires battery_design_uwh > 0, got {batt}"
            )

    def _load_results(self) -> None:
        if self.manifest is None:
            return
        for field, target in [("baseline_result_path", "baseline_result"),
                              ("optid_result_path", "optid_result")]:
            rel = self.manifest.get(field)
            if not rel:
                continue
            p = self.bundle_dir / rel
            if not p.exists():
                continue
            try:
                with open(p) as f:
                    setattr(self, target, json.load(f))
            except (OSError, json.JSONDecodeError) as e:
                self.err(f"{rel}: cannot parse: {e}")

    def _check_battery_runs_on_battery(self) -> None:
        """Check 7: battery runs actually ran on battery/discharging."""
        self._load_results()
        for name, result in [("baseline", self.baseline_result),
                             ("optid", self.optid_result)]:
            if result is None:
                continue
            ps = result.get("power_source")
            if ps != "battery":
                continue
            ac_online = result.get("ac_online")
            if ac_online is True:
                self.err(
                    f"{name} result claims power_source=battery but ac_online=true "
                    f"(battery runs must have AC offline)"
                )
            batt_pct = result.get("battery_pct")
            if batt_pct is None:
                self.warn(
                    f"{name} result: power_source=battery but battery_pct is null"
                )

    def _check_ac_runs_on_ac(self) -> None:
        """Check 8: AC runs actually ran on AC/online."""
        self._load_results()
        for name, result in [("baseline", self.baseline_result),
                             ("optid", self.optid_result)]:
            if result is None:
                continue
            ps = result.get("power_source")
            if ps != "ac":
                continue
            ac_online = result.get("ac_online")
            if ac_online is False:
                self.err(
                    f"{name} result claims power_source=ac but ac_online=false "
                    f"(AC runs must have AC online)"
                )

    def _check_baseline_optid_paired(self) -> None:
        """Check 9: baseline and optid runs are paired (both result files exist and parse)."""
        if self.manifest is None:
            return
        baseline_rel = self.manifest.get("baseline_result_path")
        optid_rel = self.manifest.get("optid_result_path")
        if not baseline_rel:
            self.err("manifest missing baseline_result_path")
        elif not (self.bundle_dir / baseline_rel).exists():
            self.err(f"baseline result file missing: {baseline_rel}")
        if not optid_rel:
            self.err("manifest missing optid_result_path")
        elif not (self.bundle_dir / optid_rel).exists():
            self.err(f"optid result file missing: {optid_rel}")
        self._load_results()
        if self.baseline_result is None:
            self.err("baseline result file missing or unparseable (no baseline pair)")
        if self.optid_result is None:
            self.err("optid result file missing or unparseable (no optid pair)")

    def _load_plan(self) -> None:
        if self.plan is not None or self.manifest is None:
            return
        plan_rel = self.manifest.get("plan_path")
        if not plan_rel:
            return
        p = self.bundle_dir / plan_rel
        if not p.exists():
            return
        try:
            with open(p) as f:
                self.plan = json.load(f)
            errors = validate_against_schema(self.plan, SCHEMAS["plan"], "$.plan")
            for e in errors:
                self.err(e)
        except (OSError, json.JSONDecodeError) as e:
            self.err(f"plan file: cannot parse: {e}")

    def _check_sample_count(self) -> None:
        """Check 10: sample count is sufficient (every metric n >= plan.min_samples)."""
        self._load_plan()
        self._load_results()
        if self.plan is None:
            return
        min_samples = self.plan.get("min_samples", 1)
        for name, result in [("baseline", self.baseline_result),
                             ("optid", self.optid_result)]:
            if result is None:
                continue
            for phase in result.get("phases", []):
                phase_name = phase.get("name", "?")
                for metric in phase.get("metrics", []):
                    metric_name = metric.get("name", "?")
                    n = metric.get("n", 0)
                    if n < min_samples:
                        self.err(
                            f"{name} phase {phase_name!r} metric {metric_name!r}: "
                            f"n={n} < min_samples={min_samples} (insufficient_n)"
                        )
                    samples = metric.get("samples", [])
                    if len(samples) != n:
                        self.err(
                            f"{name} phase {phase_name!r} metric {metric_name!r}: "
                            f"len(samples)={len(samples)} != n={n}"
                        )
                    # Check for insufficient_n anomaly.
                    anomalies = result.get("anomalies", [])
                    if "insufficient_n" in anomalies:
                        self.err(
                            f"{name} result carries 'insufficient_n' anomaly — "
                            f"not acceptable as evidence"
                        )
                    if "class_mismatch" in anomalies:
                        self.err(
                            f"{name} result carries 'class_mismatch' anomaly — "
                            f"not acceptable as evidence"
                        )

    def _check_results_parse(self) -> None:
        """Check 11: results parse as JSON and conform to the result schema."""
        self._load_results()
        for name, result in [("baseline", self.baseline_result),
                             ("optid", self.optid_result)]:
            if result is None:
                continue
            errors = validate_against_schema(result, SCHEMAS["result"], f"$.{name}_result")
            for e in errors:
                self.err(e)

    def _check_privacy_report(self) -> None:
        """Check 12: privacy report exists and parses."""
        if self.manifest is None:
            return
        rel = self.manifest.get("privacy_report_path")
        if not rel:
            return
        p = self.bundle_dir / rel
        if not p.exists():
            self.err(f"privacy report missing: {rel}")
            return
        try:
            with open(p) as f:
                report = json.load(f)
            if "schema_version" not in report:
                self.warn(f"privacy report missing schema_version field")
        except (OSError, json.JSONDecodeError) as e:
            self.err(f"privacy report: cannot parse: {e}")

    def _check_secrets_absent(self) -> None:
        """Check 13: obvious secrets absent from all files in the bundle."""
        report = lib.RedactionReport()
        for p in self.bundle_dir.rglob("*"):
            if not p.is_file():
                continue
            if p.suffix in (".json", ".jsonl", ".md", ".txt", ".csv", ".log"):
                try:
                    text = p.read_text(encoding="utf-8", errors="replace")
                except OSError:
                    continue
                redacted = lib.redact(text, report)
                # If redaction changed the text, a secret was present.
                if redacted != text:
                    self.err(
                        f"unredacted secret detected in {p.relative_to(self.bundle_dir)} "
                        f"(redactors triggered: see privacy report)"
                    )

    def _check_ai_not_evidence(self) -> None:
        """Check 14: AI summaries do not count as evidence; verdicts are advisory only."""
        if self.manifest is None:
            return
        verdict_rel = self.manifest.get("verdict_path")
        if not verdict_rel:
            return
        p = self.bundle_dir / verdict_rel
        if not p.exists():
            return
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            return
        lower = text.lower()
        # Check for AI-summary-as-evidence patterns.
        ai_patterns = [
            "this verdict was generated by ai",
            "ai-generated verdict",
            "ai summary:",
            "no human verification performed",
            "automated verdict (no human review)",
        ]
        for pat in ai_patterns:
            if pat in lower:
                self.err(
                    f"verdict file contains AI-summary-as-evidence pattern: {pat!r} "
                    f"(AI summaries do not count as evidence; verdicts are advisory only)"
                )
        # Check that the verdict explicitly states it is advisory.
        if "advisory" not in lower and "advisory only" not in lower:
            self.warn(
                "verdict file does not state 'advisory' — verdicts are advisory only"
            )

    def _check_event_chain(self) -> None:
        """Bonus: event chain validates (tamper-evident SHA-256 chain intact)."""
        if self.manifest is None:
            return
        events_rel = self.manifest.get("events_path")
        if not events_rel:
            return
        p = self.bundle_dir / events_rel
        if not p.exists():
            self.err(f"events file missing: {events_rel}")
            return
        events = lib.read_jsonl(p)
        if not events:
            self.warn(f"events file is empty: {events_rel}")
            return
        ok, errors = lib.validate_chain(events)
        if not ok:
            for e in errors:
                self.err(f"event chain: {e}")


# ─── CLI ─────────────────────────────────────────────────────────────────────


def validate_bundle(bundle_dir: Path, repo_root: Path) -> tuple[bool, list[str], list[str]]:
    """Validate a single bundle. Returns (ok, errors, warnings)."""
    v = BundleValidator(bundle_dir, repo_root)
    return v.validate()


def validate_fixtures() -> int:
    """Validate all fixtures under tools/test-fixtures/hwtest/.

    Each fixture directory has a known expected outcome (pass or fail) recorded
    in a `expected.json` file. This function validates every fixture and checks
    that the outcome matches the expectation.
    """
    if not FIXTURES_DIR.exists():
        print(f"validate-hwtest-evidence: fixtures dir not found: {FIXTURES_DIR}", file=sys.stderr)
        return 2

    print("=" * 60)
    print("Rush Linux — Hardware Evidence Validator (fixtures)")
    print("=" * 60)

    fixtures = sorted(d for d in FIXTURES_DIR.iterdir() if d.is_dir())
    if not fixtures:
        print("  (no fixtures found)")
        return 0

    total = 0
    passed = 0
    failed = 0
    mismatches = 0

    for fixture in fixtures:
        expected_path = fixture / "expected.json"
        if not expected_path.exists():
            print(f"  SKIP {fixture.name}: no expected.json")
            continue
        try:
            with open(expected_path) as f:
                expected = json.load(f)
        except (OSError, json.JSONDecodeError) as e:
            print(f"  SKIP {fixture.name}: cannot parse expected.json: {e}")
            continue

        expect_pass = expected.get("expect_pass", False)
        expect_errors = expected.get("expect_errors", [])
        description = expected.get("description", "")

        total += 1
        ok, errors, warnings = validate_bundle(fixture, ROOT)

        if expect_pass:
            if ok:
                print(f"  PASS {fixture.name} (expected pass, got pass)")
                passed += 1
            else:
                print(f"  FAIL {fixture.name} (expected pass, got {len(errors)} errors)")
                for e in errors:
                    print(f"         error: {e}")
                failed += 1
        else:
            if not ok:
                # Check that the expected error substrings are present.
                error_text = " ".join(errors)
                all_present = all(
                    any(sub in e for e in errors) for sub in expect_errors
                )
                if all_present:
                    print(f"  PASS {fixture.name} (expected fail, got fail with expected errors)")
                    passed += 1
                else:
                    print(f"  FAIL {fixture.name} (expected fail, but expected error substrings not found)")
                    print(f"         expected one of: {expect_errors}")
                    print(f"         got errors: {errors}")
                    mismatches += 1
            else:
                print(f"  FAIL {fixture.name} (expected fail, got pass)")
                failed += 1

        for w in warnings:
            print(f"         warn: {w}")

    print()
    print(f"  {passed} passed, {failed} failed, {mismatches} mismatches, {total} total")
    return 0 if (failed == 0 and mismatches == 0) else 1


def validate_release_evidence() -> int:
    """Validate all hardware evidence bundles under release/evidence/host-bench/."""
    host_bench = ROOT / "release" / "evidence" / "host-bench"
    if not host_bench.exists():
        print("validate-hwtest-evidence: no release/evidence/host-bench/ directory")
        return 0

    print("=" * 60)
    print("Rush Linux — Hardware Evidence Validator (release evidence)")
    print("=" * 60)

    # Find every directory containing hwtest-manifest.json.
    bundles = []
    for p in host_bench.rglob("hwtest-manifest.json"):
        bundles.append(p.parent)

    if not bundles:
        print("  (no hardware evidence bundles found)")
        return 0

    total = 0
    all_ok = True
    for bundle in sorted(bundles):
        total += 1
        ok, errors, warnings = validate_bundle(bundle, ROOT)
        status = "PASS" if ok else "FAIL"
        print(f"  {status} {bundle.relative_to(ROOT)}")
        for e in errors:
            print(f"         error: {e}")
        for w in warnings:
            print(f"         warn: {w}")
        if not ok:
            all_ok = False

    print()
    print(f"  {total} bundles checked, {'all passed' if all_ok else 'FAILURES present'}")
    return 0 if all_ok else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="validate-hwtest-evidence",
        description="Semantic hardware evidence validator for Rush Linux.",
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--bundle", type=Path, help="Path to a single evidence bundle to validate.")
    group.add_argument("--fixtures", action="store_true", help="Validate all fixtures under tools/test-fixtures/hwtest/.")
    group.add_argument("--release-evidence", action="store_true", help="Validate all bundles under release/evidence/host-bench/.")
    ns = parser.parse_args(argv)

    if ns.fixtures:
        return validate_fixtures()
    if ns.release_evidence:
        return validate_release_evidence()
    if ns.bundle:
        ok, errors, warnings = validate_bundle(ns.bundle, ROOT)
        status = "PASS" if ok else "FAIL"
        print(f"validate-hwtest-evidence: {status} {ns.bundle}")
        for e in errors:
            print(f"  error: {e}")
        for w in warnings:
            print(f"  warn: {w}")
        return 0 if ok else 1
    return 2


if __name__ == "__main__":
    sys.exit(main())
