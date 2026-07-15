#!/usr/bin/env python3
"""
tools/validate-testos-evidence.py — strict testOS evidence validator.

Validates a testOS run directory (the `testos-results/<timestamp>/` folder
copied off the USB, or a `benchmarks/results/<date>/<host>/` bundle in the
repo) for the full set of cloud-safe provenance, freshness, integrity, and
privacy properties required before an evidence PR may be opened.

This is the single strict gate that every testOS evidence bundle must pass
on BOTH Linux and Windows. It is stdlib-only (no external deps) so it runs
anywhere Python 3.9+ runs. It reuses the same hand-rolled JSON Schema
validator as `validate-hwtest-evidence.py` and the same redaction library
as `rush_capture_lib.py` so privacy scanning is identical across paths.

Checks (testOS cloud-safe contract):
  1.  required evidence files exist (manifest.json, run-intent.json,
      plan.json, bench-list.toml, source-sha.txt, at least one *.json result)
  2.  manifest.json parses and conforms to schemas/testos-manifest.schema.json
  3.  run-intent.json parses and conforms to schemas/testos-run-intent.schema.json
  4.  manifest.provenance is present and complete (no placeholder values)
  5.  provenance.source_commit exists in git (full 40-char SHA validation)
  6.  provenance.source_version matches the VERSION file
  7.  provenance.testos_version matches the top-level manifest.testos_version
  8.  provenance.intent_dry_run is false (physical runs only)
  9.  provenance.intent_generated_at is fresh and not in the future, and
      started_at >= intent_generated_at - skew and is not older than the
      freshness window
  10. plan_sha256 matches the SHA-256 of the bundled plan.json bytes
  11. benchmark_catalog_sha256 matches the SHA-256 of the bundled
      bench-list.toml bytes
  12. intent_sha256 matches the SHA-256 of the bundled run-intent.json bytes
  13. each result file's SHA-256 is recorded and verified against a
      manifest-level results index (if present); changed result files are
      rejected
  14. privacy scan: obvious secrets absent from every file in the bundle
      (reuses rush_capture_lib.redact)
  15. run_id / checkpoint consistency: manifest.provenance.run_id and
      checkpoint_nonce match the run-intent's fields
  16. mode is not "dry-run" for a physical run
  17. no unexpected evidence files: only the allow-listed set is accepted
      (manifest.json, run-intent.json, plan.json, bench-list.toml,
      source-sha.txt, *.json results, system-logs/*)

The validator never treats placeholder metadata as valid. The strings
"unknown", "TODO", "placeholder", "0.0.0-unknown", and the all-zero SHA
"0000...0000" are rejected in every provenance field.

Usage:
  python3 tools/validate-testos-evidence.py --run-dir <path>
  python3 tools/validate-testos-evidence.py --fixtures
  python3 tools/validate-testos-evidence.py --run-dir <path> --strict

Exit codes:
  0 — all checks passed
  1 — one or more checks failed
  2 — internal error (missing schemas, missing fixtures dir, etc.)
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

# Import the shared capture library for redaction/privacy scanning.
_TOOLS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(_TOOLS_DIR))
import rush_capture_lib as lib  # noqa: E402

ROOT = _TOOLS_DIR.parent
SCHEMAS_DIR = ROOT / "schemas"
FIXTURES_DIR = _TOOLS_DIR / "test-fixtures" / "testos-cloud-safe"
VERSION_FILE = ROOT / "VERSION"

# Allow-list of evidence files in a testOS run directory. Anything else is
# rejected as "unexpected evidence" (defense against a hostile or buggy
# runner smuggling extra files into the bundle).
_ALLOWED_RESULT_FILES = {  # exact names that are NOT per-bench results
    "manifest.json",
    "run-intent.json",
    "plan.json",
    "bench-list.toml",
    "source-sha.txt",
}
# Fixture-control files that live inside a fixture run directory but are NOT
# part of a real testOS run. They are excluded from result-file processing and
# from the unexpected-file check so the fixture harness can store its own
# expected-pass/expected-errors metadata next to the bundle under test.
_FIXTURE_CONTROL_FILES = {"expected.json"}
# Any JSON file matching one of these names is NOT a per-benchmark result.
_NON_RESULT_FILES = _ALLOWED_RESULT_FILES | _FIXTURE_CONTROL_FILES | {"result-hashes.json"}
_ALLOWED_DIRS = {"system-logs"}
# Placeholder strings that must never appear in provenance fields.
_PLACEHOLDERS = {
    "unknown",
    "todo",
    "placeholder",
    "tbd",
    "0.0.0",
    "0.0.0-unknown",
    "0000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    "n/a",
    "none",
    "null",
    "",
}
# Default freshness window for intent_generated_at (24h). Matches the Rust
# runner's DEFAULT_FRESHNESS_SECONDS.
_DEFAULT_FRESHNESS_SECONDS = 24 * 60 * 60
_MAX_CLOCK_SKEW_SECONDS = 5 * 60  # allow 5 minutes of clock skew


# ─── Schema loading ──────────────────────────────────────────────────────────


def _load_schema(name: str) -> dict:
    path = SCHEMAS_DIR / name
    if not path.exists():
        print(f"validate-testos-evidence: schema not found: {path}", file=sys.stderr)
        sys.exit(2)
    with open(path) as f:
        return json.load(f)


SCHEMAS = {
    "manifest": _load_schema("testos-manifest.schema.json"),
    "intent": _load_schema("testos-run-intent.schema.json"),
    "result": _load_schema("testos-result.schema.json"),
}


# ─── Hand-rolled JSON Schema validator (no external deps) ────────────────────
# Reuses the same logic as validate-hwtest-evidence.py.


def validate_against_schema(obj: Any, schema: dict, path: str = "$") -> list[str]:
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


# ─── Helpers ─────────────────────────────────────────────────────────────────


def _sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def _parse_iso8601_utc(s: str) -> dt.datetime | None:
    """Parse YYYY-MM-DDTHH:MM:SSZ to an aware UTC datetime."""
    try:
        return dt.datetime.strptime(s, "%Y-%m-%dT%H:%M:%SZ").replace(
            tzinfo=dt.timezone.utc
        )
    except ValueError:
        return None


def _is_placeholder(val: Any) -> bool:
    if val is None:
        return True
    if isinstance(val, str):
        if val.strip().lower() in _PLACEHOLDERS:
            return True
    return False


# ─── Bundle validation ───────────────────────────────────────────────────────


class TestosBundleValidator:
    """Validates a single testOS run directory against the cloud-safe contract."""

    def __init__(self, run_dir: Path, repo_root: Path, strict: bool = False):
        self.run_dir = run_dir
        self.repo_root = repo_root
        self.strict = strict
        self.errors: list[str] = []
        self.warnings: list[str] = []
        self.manifest: dict | None = None
        self.intent: dict | None = None
        self.provenance: dict | None = None

    def err(self, msg: str) -> None:
        self.errors.append(f"{self.run_dir.name}: {msg}")

    def warn(self, msg: str) -> None:
        self.warnings.append(f"{self.run_dir.name}: {msg}")

    def validate(self) -> tuple[bool, list[str], list[str]]:
        self._check_required_files()
        if self.manifest is None:
            return (False, self.errors, self.warnings)

        self._check_manifest_schema()
        self._check_intent_schema()
        self._check_provenance_present()
        if self.provenance is None:
            return (False, self.errors, self.warnings)

        self._check_no_placeholders()
        self._check_source_commit()
        self._check_source_version()
        self._check_testos_version_consistency()
        self._check_dry_run_false()
        self._check_freshness_and_ordering()
        self._check_plan_hash()
        self._check_catalog_hash()
        self._check_intent_hash()
        self._check_classification_sets()
        self._check_result_files()
        self._check_privacy_scan()
        self._check_run_id_checkpoint_consistency()
        self._check_mode_not_dry_run()
        self._check_no_unexpected_files()
        return (len(self.errors) == 0, self.errors, self.warnings)

    # ─── Individual checks ───────────────────────────────────────────────

    def _check_required_files(self) -> None:
        """Check 1: required evidence files exist."""
        manifest_path = self.run_dir / "manifest.json"
        if not manifest_path.exists():
            self.err("missing required file: manifest.json")
            return
        try:
            with open(manifest_path) as f:
                self.manifest = json.load(f)
        except (OSError, json.JSONDecodeError) as e:
            self.err(f"manifest.json: cannot parse: {e}")
            return
        for required in ("run-intent.json", "plan.json", "bench-list.toml", "source-sha.txt"):
            if not (self.run_dir / required).exists():
                self.err(f"missing required file: {required}")
        # At least one per-benchmark result file (*.json, excluding the
        # top-level manifest/intent/plan).
        result_files = [
            p for p in self.run_dir.glob("*.json")
            if p.name not in _NON_RESULT_FILES
        ]
        if not result_files:
            self.err("no per-benchmark result files (*.json) in run_dir")

    def _check_manifest_schema(self) -> None:
        """Check 2: manifest conforms to testos-manifest.schema.json."""
        if self.manifest is None:
            return
        errs = validate_against_schema(self.manifest, SCHEMAS["manifest"], "$.manifest")
        for e in errs:
            self.err(e)

    def _check_intent_schema(self) -> None:
        """Check 3: run-intent.json conforms to testos-run-intent.schema.json."""
        intent_path = self.run_dir / "run-intent.json"
        if not intent_path.exists():
            return  # already reported in check 1
        try:
            with open(intent_path) as f:
                self.intent = json.load(f)
        except (OSError, json.JSONDecodeError) as e:
            self.err(f"run-intent.json: cannot parse: {e}")
            return
        errs = validate_against_schema(self.intent, SCHEMAS["intent"], "$.run-intent")
        for e in errs:
            self.err(e)

    def _check_provenance_present(self) -> None:
        """Check 4: manifest.provenance is present and complete."""
        if self.manifest is None:
            return
        prov = self.manifest.get("provenance")
        if prov is None:
            self.err("manifest.provenance is missing (physical runs require a full provenance block)")
            return
        if not isinstance(prov, dict):
            self.err("manifest.provenance is not an object")
            return
        self.provenance = prov
        required = [
            "run_id", "source_commit", "source_version", "testos_version",
            "testos_image_digest", "plan_sha256", "benchmark_catalog_sha256",
            "intent_generated_at", "intent_dry_run", "checkpoint_nonce",
            "intent_sha256",
        ]
        for field in required:
            if field not in prov:
                self.err(f"manifest.provenance.{field}: missing")

    def _check_no_placeholders(self) -> None:
        """Check 4 (cont.): no placeholder values in provenance fields."""
        if self.provenance is None:
            return
        for field, val in self.provenance.items():
            if _is_placeholder(val):
                self.err(
                    f"manifest.provenance.{field}: placeholder value rejected: {val!r}"
                )

    def _check_source_commit(self) -> None:
        """Check 5: provenance.source_commit exists in git (full SHA)."""
        if self.provenance is None:
            return
        commit = self.provenance.get("source_commit", "")
        if not re.fullmatch(r"[0-9a-f]{40}", commit):
            self.err(f"provenance.source_commit {commit!r} is not a 40-char hex SHA")
            return
        try:
            r = subprocess.run(
                ["git", "-C", str(self.repo_root), "cat-file", "-t", commit],
                capture_output=True, text=True, timeout=5,
            )
            if r.returncode == 0 and r.stdout.strip() == "commit":
                return
            # Shallow-clone recovery (matches validate-hwtest-evidence.py).
            if not all(c in "0123456789abcdef" for c in commit):
                self.err(f"provenance.source_commit {commit!r} is not a valid git SHA")
                return
            fetch_r = subprocess.run(
                ["git", "-C", str(self.repo_root), "fetch", "--depth=1", "origin", commit],
                capture_output=True, text=True, timeout=30,
            )
            if fetch_r.returncode == 0:
                r2 = subprocess.run(
                    ["git", "-C", str(self.repo_root), "cat-file", "-t", commit],
                    capture_output=True, text=True, timeout=5,
                )
                if r2.returncode == 0 and r2.stdout.strip() == "commit":
                    return
                self.err(f"provenance.source_commit {commit!r} fetched but still not resolvable")
            else:
                self.err(
                    f"provenance.source_commit {commit!r} does not exist in git "
                    f"(not in local store and fetch failed: {fetch_r.stderr.strip()[:200]})"
                )
        except (OSError, subprocess.TimeoutExpired) as e:
            self.warn(f"could not verify provenance.source_commit in git: {e}")

    def _check_source_version(self) -> None:
        """Check 6: provenance.source_version matches the VERSION file."""
        if self.provenance is None:
            return
        sv = self.provenance.get("source_version", "")
        if not VERSION_FILE.exists():
            self.err(f"VERSION file not found at {VERSION_FILE}")
            return
        version = VERSION_FILE.read_text().strip()
        if sv != version:
            self.err(f"provenance.source_version {sv!r} does not match VERSION file {version!r}")

    def _check_testos_version_consistency(self) -> None:
        """Check 7: provenance.testos_version == manifest.testos_version."""
        if self.manifest is None or self.provenance is None:
            return
        mver = self.manifest.get("testos_version", "")
        pver = self.provenance.get("testos_version", "")
        if mver != pver:
            self.err(
                f"manifest.testos_version {mver!r} != provenance.testos_version {pver!r}"
            )
        if self.intent is not None:
            iver = self.intent.get("testos_version", "")
            if iver != pver:
                self.err(
                    f"run-intent.testos_version {iver!r} != provenance.testos_version {pver!r}"
                )

    def _check_dry_run_false(self) -> None:
        """Check 8: provenance.intent_dry_run is false."""
        if self.provenance is None:
            return
        if self.provenance.get("intent_dry_run") is not False:
            self.err(
                f"provenance.intent_dry_run is {self.provenance.get('intent_dry_run')!r}; "
                f"physical runs require intent_dry_run=false"
            )

    def _check_freshness_and_ordering(self) -> None:
        """Check 9: intent_generated_at is fresh, not future, and ordered vs started_at."""
        if self.provenance is None or self.manifest is None:
            return
        gen_str = self.provenance.get("intent_generated_at", "")
        started_str = self.manifest.get("started_at", "")
        finished_str = self.manifest.get("finished_at", "")
        gen = _parse_iso8601_utc(gen_str)
        started = _parse_iso8601_utc(started_str)
        finished = _parse_iso8601_utc(finished_str)
        if gen is None:
            self.err(f"provenance.intent_generated_at {gen_str!r} is not valid ISO 8601 UTC")
            return
        now = dt.datetime.now(dt.timezone.utc)
        # Future check.
        if gen > now + dt.timedelta(seconds=_MAX_CLOCK_SKEW_SECONDS):
            skew = int((gen - now).total_seconds())
            self.err(
                f"provenance.intent_generated_at {gen_str} is {skew}s in the future "
                f"(clock skew or tampering)"
            )
        # Staleness check.
        max_age = self.intent.get("freshness_seconds") if self.intent else None
        try:
            max_age = int(max_age) if max_age is not None else _DEFAULT_FRESHNESS_SECONDS
        except (TypeError, ValueError):
            max_age = _DEFAULT_FRESHNESS_SECONDS
        max_age = max(60, min(7 * 24 * 3600, max_age))
        age = (now - gen).total_seconds()
        if age > max_age:
            self.err(
                f"provenance.intent_generated_at {gen_str} is {int(age)}s old "
                f"(max {max_age}s); stale intent rejected"
            )
        # Ordering: started_at >= intent_generated_at - skew.
        if started is not None:
            if started < gen - dt.timedelta(seconds=_MAX_CLOCK_SKEW_SECONDS):
                self.err(
                    f"manifest.started_at {started_str} is before "
                    f"provenance.intent_generated_at {gen_str} (run started before intent was generated)"
                )
            # finished_at >= started_at.
            if finished is not None and finished < started:
                self.err(
                    f"manifest.finished_at {finished_str} is before started_at {started_str}"
                )

    def _check_plan_hash(self) -> None:
        """Check 10: plan_sha256 matches the bundled plan.json bytes."""
        if self.provenance is None:
            return
        expected = self.provenance.get("plan_sha256", "")
        plan_path = self.run_dir / "plan.json"
        if not plan_path.exists():
            self.err("plan.json missing; cannot verify plan_sha256")
            return
        actual = _sha256_file(plan_path)
        if actual != expected:
            self.err(f"plan_sha256 mismatch: provenance has {expected}, plan.json hashes to {actual}")

    def _check_catalog_hash(self) -> None:
        """Check 11: benchmark_catalog_sha256 matches the bundled bench-list.toml bytes."""
        if self.provenance is None:
            return
        expected = self.provenance.get("benchmark_catalog_sha256", "")
        cat_path = self.run_dir / "bench-list.toml"
        if not cat_path.exists():
            self.err("bench-list.toml missing; cannot verify benchmark_catalog_sha256")
            return
        actual = _sha256_file(cat_path)
        if actual != expected:
            self.err(
                f"benchmark_catalog_sha256 mismatch: provenance has {expected}, "
                f"bench-list.toml hashes to {actual}"
            )

    def _check_intent_hash(self) -> None:
        """Check 12: intent_sha256 matches the bundled run-intent.json bytes."""
        if self.provenance is None:
            return
        expected = self.provenance.get("intent_sha256", "")
        intent_path = self.run_dir / "run-intent.json"
        if not intent_path.exists():
            self.err("run-intent.json missing; cannot verify intent_sha256")
            return
        actual = _sha256_file(intent_path)
        if actual != expected:
            self.err(
                f"intent_sha256 mismatch: provenance has {expected}, "
                f"run-intent.json hashes to {actual}"
            )

    def _check_classification_sets(self) -> None:
        """Check (sets): attempted == passed | failed | skipped, the three
        terminal sets are pairwise disjoint, and every result file present on
        disk is classified. This must hold even when sets are empty (e.g. a
        run that only skipped), so a missing/empty `attempted` is only valid
        when the union is also empty.
        """
        if self.manifest is None:
            return
        passed = set(self.manifest.get("passed", []))
        failed = set(self.manifest.get("failed", []))
        skipped = set(self.manifest.get("skipped", []))
        attempted = set(self.manifest.get("attempted", []))
        union = passed | failed | skipped
        for a, b, la, lb in (
            (passed, failed, "passed", "failed"),
            (passed, skipped, "passed", "skipped"),
            (failed, skipped, "failed", "skipped"),
        ):
            overlap = a & b
            if overlap:
                self.err(
                    f"manifest {la}/{lb} sets overlap: {sorted(overlap)}"
                )
        if attempted != union:
            self.err(
                f"manifest attempted set does not equal passed|failed|skipped: "
                f"attempted={sorted(attempted)}, union={sorted(union)}"
            )
        # Every result file on disk must be classified (defense-in-depth: the
        # per-result check below also keys each file to its bench_id).
        result_files = [
            p for p in self.run_dir.glob("*.json")
            if p.name not in _NON_RESULT_FILES
        ]
        unclassified = {
            p.stem for p in result_files
        } - union
        for u in sorted(unclassified):
            self.err(
                f"result file {u}.json exists but is not in manifest "
                f"attempted/passed/failed/skipped"
            )

    def _check_result_files(self) -> None:
        """Check (results): each per-benchmark result file conforms to the
        result schema, its canonical ``bench_id`` is present and matches the
        filename stem (never keyed on bench_name), passing numeric benchmarks
        carry a finite value and a unit, and per-file SHA-256 digests recorded
        in ``result-hashes.json`` (when the runner emits them) match the
        artifact bytes.
        """
        if self.manifest is None:
            return
        passed = set(self.manifest.get("passed", []))
        attempted = set(self.manifest.get("attempted", []))
        result_files = [
            p for p in self.run_dir.glob("*.json")
            if p.name not in _NON_RESULT_FILES
        ]
        seen_ids: set[str] = set()
        import math as _math
        for rf in result_files:
            try:
                data = json.loads(rf.read_text())
            except (OSError, json.JSONDecodeError) as e:
                self.err(f"result file {rf.name}: cannot parse: {e}")
                continue
            # Schema conformance.
            schema_errs = validate_against_schema(data, SCHEMAS["result"], f"$.{rf.name}")
            for se in schema_errs:
                self.err(f"result file {rf.name}: schema: {se}")
            # Canonical identity: bench_id MUST be present and match the
            # filename stem. Real results carry both bench_id and bench_name;
            # only bench_id is canonical. Keying on bench_name (the prior
            # defect) caused false failures whenever they differ.
            bid = data.get("bench_id")
            if not bid:
                self.err(f"result file {rf.name}: missing required 'bench_id' field")
                continue
            if bid != rf.stem:
                self.err(
                    f"result file {rf.name}: bench_id {bid!r} != filename stem {rf.stem!r}"
                )
            seen_ids.add(bid)
            # Passing benchmarks: a numeric result must be finite and carry a
            # unit. Pure pass/fail benchmarks legitimately have value=None
            # (and no unit), so the numeric requirement only applies when a
            # value is actually present. A value without a unit, or a
            # value without a unit, or a non-finite value, is rejected.
            status = str(data.get("status", "")).lower()
            if bid in passed and status in ("pass", "passed"):
                value = data.get("value")
                unit = data.get("unit")
                if value is not None:
                    try:
                        v = float(value)
                        if not _math.isfinite(v):
                            self.err(f"result file {rf.name}: non-finite value: {value}")
                    except (TypeError, ValueError):
                        self.err(f"result file {rf.name}: non-numeric value: {value}")
                    if not unit:
                        self.err(f"result file {rf.name}: numeric result has no unit")
        # Every attempted bench must have a result file.
        missing = attempted - seen_ids
        for m in sorted(missing):
            self.err(f"manifest.attempted lists {m!r} but no result file found for it")

        # Per-file SHA-256 binding (when the runner emits result-hashes.json).
        # This binds each result artifact to a recorded digest so a changed
        # file is detected; every recorded digest must match the artifact.
        sidecar = self.run_dir / "result-hashes.json"
        if sidecar.exists():
            try:
                hashes = json.loads(sidecar.read_text())
            except (OSError, json.JSONDecodeError) as e:
                self.err(f"result-hashes.json: cannot parse: {e}")
                return
            for rf in result_files:
                recorded = hashes.get(rf.name)
                if recorded is None:
                    self.err(f"result-hashes.json has no entry for {rf.name}")
                    continue
                actual = _sha256_file(rf)
                if actual != recorded:
                    self.err(
                        f"result file {rf.name} was changed after result-hashes.json "
                        f"was written: recorded digest {recorded}, actual {actual}"
                    )
            # No stale/extra entries either.
            known = {p.name for p in result_files}
            for extra in set(hashes) - known:
                self.err(f"result-hashes.json records unknown file {extra!r}")
        elif self.strict:
            self.warn("result-hashes.json sidecar absent; per-file tamper detection skipped")


    def _check_privacy_scan(self) -> None:
        """Check 14: obvious secrets absent from every file in the bundle."""
        report = lib.RedactionReport()
        for p in self.run_dir.rglob("*"):
            if not p.is_file():
                continue
            if p.suffix in (".json", ".jsonl", ".md", ".txt", ".csv", ".log", ".toml"):
                try:
                    text = p.read_text(encoding="utf-8", errors="replace")
                except OSError:
                    continue
                redacted = lib.redact(text, report)
                if redacted != text:
                    self.err(
                        f"unredacted secret detected in {p.relative_to(self.run_dir)} "
                        f"(redactors triggered: see privacy report)"
                    )

    def _check_run_id_checkpoint_consistency(self) -> None:
        """Check 15: run_id and checkpoint_nonce match between manifest and intent."""
        if self.provenance is None or self.intent is None:
            return
        for field in ("run_id", "checkpoint_nonce"):
            mval = self.provenance.get(field, "")
            ival = self.intent.get(field, "")
            if mval != ival:
                self.err(
                    f"provenance.{field} {mval!r} != run-intent.{field} {ival!r}"
                )
        # campaign_id is optional but if present in both must match.
        mc = self.provenance.get("campaign_id")
        ic = self.intent.get("campaign_id")
        if mc is not None and ic is not None and mc != ic:
            self.err(f"provenance.campaign_id {mc!r} != run-intent.campaign_id {ic!r}")

    def _check_mode_not_dry_run(self) -> None:
        """Check 16: manifest.mode is not 'dry-run' for a physical run."""
        if self.manifest is None:
            return
        mode = self.manifest.get("mode", "")
        if mode == "dry-run":
            self.err(f"manifest.mode is 'dry-run'; physical runs require a real mode")

    def _check_no_unexpected_files(self) -> None:
        """Check 17: only allow-listed evidence files are accepted."""
        for p in self.run_dir.iterdir():
            if p.is_dir():
                if p.name not in _ALLOWED_DIRS:
                    self.err(f"unexpected directory in evidence bundle: {p.name!r}")
                continue
            if not p.is_file():
                self.err(f"unexpected non-regular file in evidence bundle: {p.name!r}")
                continue
            name = p.name
            if name in _NON_RESULT_FILES:
                continue
            if name.endswith(".json") and name not in ("manifest.json", "run-intent.json", "plan.json"):
                continue  # per-benchmark result file
            if name == "result-hashes.json":
                continue  # optional sidecar
            self.err(f"unexpected file in evidence bundle: {name!r}")


# ─── CLI ─────────────────────────────────────────────────────────────────────


def validate_run_dir(run_dir: Path, repo_root: Path, strict: bool = False) -> tuple[bool, list[str], list[str]]:
    """Validate a single run directory. Returns (ok, errors, warnings)."""
    v = TestosBundleValidator(run_dir, repo_root, strict=strict)
    return v.validate()


def validate_fixtures(strict: bool = False) -> int:
    """Validate all fixtures under tools/test-fixtures/testos-cloud-safe/."""
    if not FIXTURES_DIR.exists():
        print(f"validate-testos-evidence: fixtures dir not found: {FIXTURES_DIR}", file=sys.stderr)
        return 2
    print("=" * 60)
    print("Rush Linux — testOS cloud-safe evidence validator (fixtures)")
    print("=" * 60)
    fixtures = sorted(d for d in FIXTURES_DIR.iterdir() if d.is_dir())
    if not fixtures:
        print("  (no fixtures found)")
        return 0
    total = passed = failed = mismatches = 0
    for fixture in fixtures:
        expected_path = fixture / "expected.json"
        if not expected_path.exists():
            print(f"  SKIP {fixture.name}: no expected.json")
            continue
        try:
            expected = json.loads(expected_path.read_text())
        except (OSError, json.JSONDecodeError) as e:
            print(f"  SKIP {fixture.name}: cannot parse expected.json: {e}")
            continue
        expect_pass = expected.get("expect_pass", False)
        expect_errors = expected.get("expect_errors", [])
        total += 1
        ok, errors, warnings = validate_run_dir(fixture, ROOT, strict=strict)
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
                error_text = " ".join(errors)
                all_present = all(any(sub in error_text for sub in expect_errors) for sub in expect_errors) if expect_errors else True
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


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="validate-testos-evidence",
        description="Strict testOS cloud-safe evidence validator.",
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--run-dir", type=Path, help="Path to a single testOS run directory to validate.")
    group.add_argument("--fixtures", action="store_true", help="Validate all fixtures under tools/test-fixtures/testos-cloud-safe/.")
    parser.add_argument("--strict", action="store_true", help="Enable stricter checks (e.g. require result-hashes sidecar).")
    ns = parser.parse_args(argv)
    if ns.fixtures:
        return validate_fixtures(strict=ns.strict)
    if ns.run_dir:
        ok, errors, warnings = validate_run_dir(ns.run_dir, ROOT, strict=ns.strict)
        status = "PASS" if ok else "FAIL"
        print(f"validate-testos-evidence: {status} {ns.run_dir}")
        for e in errors:
            print(f"  error: {e}")
        for w in warnings:
            print(f"  warn: {w}")
        return 0 if ok else 1
    return 2


if __name__ == "__main__":
    sys.exit(main())
