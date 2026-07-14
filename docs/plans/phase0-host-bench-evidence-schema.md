# Phase 0 Evidence Schema: Host-Bench Transcript Validation

**Date:** 2026-07-02
**Status:** Draft for review
**Effort:** 2 dev-days
**Blocks:** v0.6 closure
**Severity:** High
**Auditor:** Z.ai audit (third pass)

## 1. Problem statement

The `validate-evidence.py` CI gate enforces that every milestone criterion marked `verified = true` has a transcript file at the expected path under `release/evidence/`, and that the file is non-empty. This is a file-existence check. It does not verify that the transcript contains the expected command, machine identity, kernel version, pass/fail marker, or measured thresholds.

This matters because the project has documented eight historical instances (L-001 through L-008 in `LESSONS.md`) of criteria being marked verified without real transcripts, with citations to files that were never committed, or with status docs drifting from `milestones.toml`. The current gate prevents the first failure mode (missing file) but not the second (file exists but says the wrong thing). A transcript that contains `git commit abc123` when the actual release commit was `def456` passes the gate today. A transcript that records a failed benchmark run also passes, as long as it is non-empty.

The v0.6.0-beta.1 milestone has two quantitative exit criteria that require host-benchmark evidence. Before v0.6 tries to close, the gate must be extended to verify that those transcripts actually contain the measurements the milestone claims.

## 2. Schema design

Each evidence class (boot, host-bench, unpriv-vm, regression) gets a schema defining required markers. A marker is a line-level pattern that the validator searches for in the transcript. The validator does not parse the full transcript structure; it checks for the presence and content of required markers, and rejects transcripts that are missing markers or whose markers contain values inconsistent with the milestone claim.

### 2.1 Host-bench transcript schema

A host-bench transcript must contain all of the following markers. Each marker is a line beginning with a known prefix; the validator extracts the value after the prefix and checks it against the milestone's expected value.

```
# Required markers in a host-bench transcript

VERSION: 0.7.0-beta.1
GIT_COMMIT: <40-char SHA, must match the release commit>
RUN_ID: <uuid, unique per run>
BASELINE_RUN_ID: <uuid of the baseline this run is compared against>
HOSTNAME: <reference hardware hostname>
HWID: <reference hardware identifier, e.g. CPU model + board>
UNAME: <output of uname -a at run time>
KERNEL: <kernel version, must match milestone's required kernel>
OPTID_VERSION: <output of optid --version>
OPTID_GIT_COMMIT: <must match GIT_COMMIT>
RUN_STARTED: <ISO 8601 timestamp>
RUN_FINISHED: <ISO 8601 timestamp>

# Metric table (one row per metric, tab-separated)
METRIC<TAB>BASELINE<TAB>RUN<TAB>DELTA<TAB>CONFIDENCE<TAB>PASS|FAIL
ps:cpu:avg10<TAB>0.012<TAB>0.008<TAB>-33%<TAB>0.95<TAB>PASS
ps:mem:avg10<TAB>0.004<TAB>0.005<TAB>+25%<TAB>0.87<TAB>FAIL

# Final verdict
VERDICT: PASS|FAIL|INCONCLUSIVE
VALIDATOR_VERSION: 1
```

### 2.2 Validation rules

The validator applies the following checks in order. A transcript must pass all checks to be accepted as evidence for a verified criterion.

- **Presence:** every required marker is present in the transcript.
- **Non-empty:** every marker has a non-empty value after the prefix.
- **Version match:** `VERSION` matches the milestone's `expected_version` field in `milestones.toml`.
- **Commit match:** `GIT_COMMIT` matches the release tag's commit SHA. The validator fetches this from git, not from the transcript.
- **Hardware match:** `HWID` matches one of the milestone's `reference_hardware` entries. Unknown hardware is rejected.
- **Kernel match:** `KERNEL` matches the milestone's required kernel version (or range).
- **Metric coverage:** the metric table contains at least every metric named in the milestone's `required_metrics` list.
- **Confidence floor:** every metric row's `CONF` value is above the milestone's `min_confidence` (default 0.80).
- **Verdict consistency:** if any metric row is `FAIL`, the transcript-level `VERDICT` must not be `PASS`. (It may be `INCONCLUSIVE` if the operator explicitly flagged it.)
- **Validator version:** `VALIDATOR_VERSION` is at least the minimum required by the gate. (Lets us evolve the schema without breaking old transcripts in a single step.)

## 3. Validator contract

The validator is a small Python module loaded by `validate-evidence.py` when it encounters a criterion with `evidence_class = "host-bench"`. It exposes one function:

```python
# tools/validate_host_bench.py

from dataclasses import dataclass
from pathlib import Path

@dataclass
class ValidationResult:
    ok: bool
    errors: list[str]      # human-readable, one per failed check
    warnings: list[str]    # non-blocking observations
    markers: dict          # extracted marker values, for audit log

def validate(transcript: Path, milestone_claim: dict) -> ValidationResult:
    """Validate a host-bench transcript against a milestone claim.

    Args:
        transcript: path to the transcript file.
        milestone_claim: dict from milestones.toml containing
            expected_version, reference_hardware, required_kernel,
            required_metrics, min_confidence.

    Returns:
        ValidationResult with ok=True iff every check passes.
    """
    ...
```

The validator is registered in `validate-evidence.py`'s `SCHEMA_REGISTRY` and dispatched by evidence class. The main loop already iterates over criteria; this change adds one branch for content-aware validation when a schema is registered for the evidence class.

## 4. Migration from existence-check

Existing transcripts under `release/evidence/` were not written with these markers. A hard cutover would invalidate all prior evidence. The migration is phased:

- **Phase 1 (this PR):** add the schema and validator. `validate-evidence.py` checks the schema only for criteria with `evidence_class = "host-bench"`. Other evidence classes continue with the existence check.
- **Phase 2 (next milestone):** the benchmark runner that produces host-bench transcripts is updated to emit the required markers. Old transcripts remain valid for already-closed milestones; new evidence must use the new format.
- **Phase 3 (v0.7):** the schema is extended to cover the boot and unpriv-vm evidence classes. The existence check is removed for any evidence class with a registered schema.

> **Why not validate all evidence classes now?** The boot and unpriv-vm evidence classes have transcripts produced by different runners with different formats. Forcing them all through one schema in a single PR would require touching every producer and consumer at once. Phasing by evidence class lets us prove the schema design on host-bench (the highest-stakes class, since it gates v0.6 closure) before generalizing.

## 5. Test fixtures

The validator's test suite must prove it catches each failure mode the schema exists to prevent. Fixtures live under `tools/tests/fixtures/evidence/`.

| Fixture | Expected result |
|---|---|
| `good_full.txt` | All markers present, all values consistent with milestone claim. **Must pass.** |
| `missing_verdict.txt` | VERDICT marker absent. **Must fail** with `missing: VERDICT`. |
| `wrong_commit.txt` | GIT_COMMIT does not match release tag. **Must fail** with `commit_mismatch`. |
| `unknown_hardware.txt` | HWID is not in milestone's reference_hardware list. **Must fail** with `unknown_hardware`. |
| `low_confidence.txt` | One metric row has CONF=0.65, below the 0.80 floor. **Must fail** with `low_confidence: ps:mem:avg10`. |
| `verdict_inconsistency.txt` | One metric row is FAIL but transcript VERDICT is PASS. **Must fail** with `verdict_inconsistency`. |
| `missing_metric.txt` | Metric table lacks a metric named in milestone's required_metrics. **Must fail** with `missing_metric: ps:io:avg10`. |
| `empty_marker.txt` | VERSION: line present but value empty. **Must fail** with `empty: VERSION`. |
| `malformed_toml.txt` | Transcript is binary garbage. **Must fail** with `unparseable`, not crash. |
| `stale_validator_version.txt` | VALIDATOR_VERSION is 0; current minimum is 1. **Must fail** with `stale_validator_version`. |

## 6. Acceptance criteria

- [ ] `tools/validate_host_bench.py` exists and exposes the `validate(transcript, milestone_claim)` interface.
- [ ] `validate-evidence.py` dispatches to the host-bench validator when `evidence_class = "host-bench"` is set on a criterion.
- [ ] All ten fixtures in §5 pass or fail as specified.
- [ ] Coverage report shows `validate_host_bench.py` at ≥ 90% line coverage.
- [ ] The v0.6 milestone's two quantitative criteria in `milestones.toml` have `evidence_class = "host-bench"` set.
- [ ] The benchmark runner that produces host-bench transcripts is updated to emit the required markers (separate PR, but blocked-on by this one).
- [ ] Dragnet self-test fixture (audit #10) is extended to include a host-bench transcript with wrong content, asserting Dragnet catches it.

## References

- Audit third pass, finding #4 (evidence gate validates transcript existence, not content).
- Audit third pass, finding #10 (Dragnet itself is a recursive single-point-of-trust, unaudited).
- `LESSONS.md` L-001 through L-008 (evidence fabrication recurrence pattern).
- `release/milestones.toml` (v0.6 quantitative exit criteria).
- `tools/validate-evidence.py` (current CI gate).
