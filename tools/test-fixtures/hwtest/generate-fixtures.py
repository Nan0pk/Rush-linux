#!/usr/bin/env python3
"""
Generate hardware evidence fixtures under tools/test-fixtures/hwtest/.

Each fixture is a directory containing:
  - hwtest-manifest.json
  - hwtest-plan.json
  - hwtest-host.json
  - hwtest-result-baseline.json
  - hwtest-result-optid.json
  - VERDICT.md
  - events.jsonl
  - privacy-report.json
  - expected.json (tells the validator what to expect)

11 fixtures:
  1. good-laptop           — valid laptop bundle (expected pass)
  2. missing-manifest      — no hwtest-manifest.json (expected fail)
  3. wrong-version         — source_version doesn't match VERSION (expected fail)
  4. laptop-no-battery     — laptop slot, battery_design_uwh=0 (expected fail)
  5. battery-run-on-ac     — power_source=battery but ac_online=true (expected fail)
  6. missing-baseline-pair — baseline result file missing (expected fail)
  7. insufficient-samples  — n < min_samples (expected fail)
  8. malformed-results     — baseline result JSON is malformed (expected fail)
  9. secret-leakage        — unredacted GitHub token in transcript (expected fail)
  10. ai-only-verdict      — VERDICT.md says "AI-generated verdict" (expected fail)
  11. broken-event-chain   — events.jsonl has a tampered event (expected fail)

Run:
  python3 tools/test-fixtures/hwtest/generate-fixtures.py
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

# Import the shared capture library for event-chain generation.
_TOOLS_DIR = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(_TOOLS_DIR))
import rush_capture_lib as lib  # noqa: E402

ROOT = _TOOLS_DIR.parent
FIXTURES = ROOT / "tools" / "test-fixtures" / "hwtest"

# Read the current VERSION so the "good" fixture matches.
VERSION = (ROOT / "VERSION").read_text().strip()

# A real commit SHA from the repo (main HEAD).
import subprocess
GIT_COMMIT = subprocess.check_output(
    ["git", "-C", str(ROOT), "rev-parse", "HEAD"]
).decode().strip()


def write_json(path: Path, obj: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w") as f:
        json.dump(obj, f, sort_keys=True, indent=2)
        f.write("\n")


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def make_plan() -> dict:
    return {
        "schema_version": 1,
        "plan_kind": "hwtest-plan",
        "plan_name": "mixed-load-001",
        "workload": "mixed-load-001",
        "phases": [
            {
                "name": "interactive",
                "duration_sec": 60,
                "expected_class": "interactive",
                "metrics": ["input-latency-p95-ms", "psi-cpu-avg10"],
            },
            {
                "name": "latency-critical",
                "duration_sec": 60,
                "expected_class": "latency-critical",
                "metrics": ["frametime-p95-ms", "psi-cpu-avg10"],
            },
        ],
        "min_samples": 5,
        "pass_conditions": {
            "criterion_2_responsiveness": {
                "applies_to_slots": ["desktop", "laptop"],
                "description": "median and p99 latency under optid --apply are LOWER than baseline by more than the CI",
            },
            "criterion_3_battery": {
                "applies_to_slots": ["laptop"],
                "description": "optid --apply energy-per-workload-unit (joules) <= baseline within the CI",
            },
        },
    }


def make_host(slot: str = "laptop", battery_uwh: int = 48_000_000) -> dict:
    return {
        "schema_version": 1,
        "host_kind": "hwtest-host",
        "slot": slot,
        "kernel": "6.1.0-test",
        "cpu_model": "Test CPU Model",
        "dmi_board": "TestVendor TestBoard",
        "battery_design_uwh": battery_uwh,
        "fingerprint": "0123456789abcdef",
        "captured_at": "2026-07-04T12:00:00Z",
        "ncpu": 8,
        "cpufreq_driver": "intel_pstate",
        "governor": "powersave",
        "platform_profile_available": True,
        "rapl_domain": "/sys/class/powercap/intel-rapl:0",
        "optid_version": "optid 0.7.0-beta.1",
        "git_commit": GIT_COMMIT,
        "baseline_distro": "Ubuntu 24.04 LTS, PPD balanced",
    }


def make_result(
    lever: str = "baseline",
    power_source: str = "ac",
    ac_online: bool = True,
    n: int = 5,
    anomalies: list[str] | None = None,
) -> dict:
    return {
        "schema_version": 1,
        "result_kind": "hwtest-result",
        "lever": lever,
        "power_source": power_source,
        "started_at": "2026-07-04T12:00:00Z",
        "finished_at": "2026-07-04T12:30:00Z",
        "phases": [
            {
                "name": "interactive",
                "expected_class": "interactive",
                "observed_class": "interactive",
                "metrics": [
                    {
                        "name": "input-latency-p95-ms",
                        "unit": "ms",
                        "samples": [0.06, 0.06, 0.06, 0.07, 0.06][:n],
                        "median": 0.06,
                        "p95": 0.07,
                        "iqr": 0.01,
                        "n": n,
                    },
                    {
                        "name": "psi-cpu-avg10",
                        "unit": "ratio",
                        "samples": [0.01, 0.01, 0.02, 0.01, 0.01][:n],
                        "median": 0.01,
                        "p95": 0.02,
                        "iqr": 0.005,
                        "n": n,
                    },
                ],
            },
            {
                "name": "latency-critical",
                "expected_class": "latency-critical",
                "observed_class": "latency-critical",
                "metrics": [
                    {
                        "name": "frametime-p95-ms",
                        "unit": "ms",
                        "samples": [16.5, 16.6, 16.7, 16.5, 16.6][:n],
                        "median": 16.6,
                        "p95": 16.7,
                        "iqr": 0.1,
                        "n": n,
                    },
                ],
            },
        ],
        "battery_pct": 80 if power_source == "battery" else None,
        "ac_online": ac_online,
        "energy_joules": 1200.0 if power_source == "battery" else None,
        "anomalies": anomalies or [],
    }


def make_events_chain(tampered: bool = False) -> str:
    """Generate a valid events.jsonl chain. If tampered, corrupt one event."""
    e0 = lib.make_event(seq=0, kind="start", payload={"started_at": "2026-07-04T12:00:00Z"})
    e1 = lib.make_event(
        seq=1, kind="command", payload={"argv": ["rushbench", "run"]},
        prev_event_sha256=e0["event_sha256"],
    )
    e2 = lib.make_event(
        seq=2, kind="finish", payload={"finished_at": "2026-07-04T12:30:00Z"},
        prev_event_sha256=e1["event_sha256"],
    )
    events = [e0, e1, e2]
    if tampered:
        # Tamper with e1's payload but keep its old event_sha256.
        e1["payload"]["exit_code"] = 999
    lines = [json.dumps(e, sort_keys=True, separators=(",", ":")) for e in events]
    return "\n".join(lines) + "\n"


def make_privacy_report() -> dict:
    return {
        "schema_version": 1,
        "redactors": [],
        "counts": {},
        "total": 0,
    }


def make_verdict(ai_generated: bool = False) -> str:
    if ai_generated:
        return (
            "# Verdict (AI-generated verdict — NOT evidence)\n\n"
            "This verdict was generated by AI. No human verification performed.\n\n"
            "Criterion 2: PASS\n"
            "Criterion 3: PASS\n"
        )
    return (
        "# Verdict (advisory only)\n\n"
        "This verdict is advisory only. AI summaries do not count as evidence.\n"
        "The human verifier must independently confirm the results.\n\n"
        "Criterion 2 (responsiveness): PASS\n"
        "Criterion 3 (battery): PASS\n"
    )


def make_manifest(
    slot: str = "laptop",
    source_version: str = VERSION,
    source_commit: str = GIT_COMMIT,
    baseline_path: str = "hwtest-result-baseline.json",
    optid_path: str = "hwtest-result-optid.json",
) -> dict:
    return {
        "schema_version": 1,
        "manifest_kind": "hwtest-manifest",
        "source_version": source_version,
        "source_commit": source_commit,
        "hardware_slot": slot,
        "bundle_created_at": "2026-07-04T12:30:00Z",
        "plan_path": "hwtest-plan.json",
        "host_path": "hwtest-host.json",
        "baseline_result_path": baseline_path,
        "optid_result_path": optid_path,
        "verdict_path": "VERDICT.md",
        "events_path": "events.jsonl",
        "privacy_report_path": "privacy-report.json",
        "operator": "test-fixture",
        "notes": "TEST FIXTURE — not real evidence.",
    }


def write_expected(fixture_dir: Path, expect_pass: bool, expect_errors: list[str], description: str) -> None:
    write_json(fixture_dir / "expected.json", {
        "expect_pass": expect_pass,
        "expect_errors": expect_errors,
        "description": description,
    })


# ─── Fixture 1: good-laptop ──────────────────────────────────────────────────


def gen_good_laptop() -> None:
    d = FIXTURES / "good-laptop"
    write_json(d / "hwtest-manifest.json", make_manifest(slot="laptop"))
    write_json(d / "hwtest-plan.json", make_plan())
    write_json(d / "hwtest-host.json", make_host(slot="laptop", battery_uwh=48_000_000))
    write_json(d / "hwtest-result-baseline.json", make_result(lever="baseline", power_source="ac", ac_online=True))
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid", power_source="ac", ac_online=True))
    write_text(d / "VERDICT.md", make_verdict(ai_generated=False))
    write_text(d / "events.jsonl", make_events_chain(tampered=False))
    write_json(d / "privacy-report.json", make_privacy_report())
    write_expected(d, expect_pass=True, expect_errors=[], description="Valid laptop bundle — all checks pass.")


# ─── Fixture 2: missing-manifest ─────────────────────────────────────────────


def gen_missing_manifest() -> None:
    d = FIXTURES / "missing-manifest"
    # Write everything EXCEPT hwtest-manifest.json.
    write_json(d / "hwtest-plan.json", make_plan())
    write_json(d / "hwtest-host.json", make_host())
    write_json(d / "hwtest-result-baseline.json", make_result())
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid"))
    write_text(d / "VERDICT.md", make_verdict())
    write_text(d / "events.jsonl", make_events_chain())
    write_json(d / "privacy-report.json", make_privacy_report())
    write_expected(d, expect_pass=False, expect_errors=["missing required file: hwtest-manifest.json"], description="Missing manifest — check 1 fails.")


# ─── Fixture 3: wrong-version ────────────────────────────────────────────────


def gen_wrong_version() -> None:
    d = FIXTURES / "wrong-version"
    write_json(d / "hwtest-manifest.json", make_manifest(source_version="99.99.99"))
    write_json(d / "hwtest-plan.json", make_plan())
    write_json(d / "hwtest-host.json", make_host())
    write_json(d / "hwtest-result-baseline.json", make_result())
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid"))
    write_text(d / "VERDICT.md", make_verdict())
    write_text(d / "events.jsonl", make_events_chain())
    write_json(d / "privacy-report.json", make_privacy_report())
    write_expected(d, expect_pass=False, expect_errors=["does not match VERSION"], description="Wrong source_version — check 3 fails.")


# ─── Fixture 4: laptop-no-battery ────────────────────────────────────────────


def gen_laptop_no_battery() -> None:
    d = FIXTURES / "laptop-no-battery"
    write_json(d / "hwtest-manifest.json", make_manifest(slot="laptop"))
    write_json(d / "hwtest-plan.json", make_plan())
    write_json(d / "hwtest-host.json", make_host(slot="laptop", battery_uwh=0))
    write_json(d / "hwtest-result-baseline.json", make_result())
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid"))
    write_text(d / "VERDICT.md", make_verdict())
    write_text(d / "events.jsonl", make_events_chain())
    write_json(d / "privacy-report.json", make_privacy_report())
    write_expected(d, expect_pass=False, expect_errors=["laptop slot requires battery_design_uwh"], description="Laptop slot with no battery — check 6 fails.")


# ─── Fixture 5: battery-run-on-ac ────────────────────────────────────────────


def gen_battery_run_on_ac() -> None:
    d = FIXTURES / "battery-run-on-ac"
    write_json(d / "hwtest-manifest.json", make_manifest(slot="laptop"))
    write_json(d / "hwtest-plan.json", make_plan())
    write_json(d / "hwtest-host.json", make_host(slot="laptop"))
    # baseline result claims power_source=battery but ac_online=true.
    write_json(d / "hwtest-result-baseline.json", make_result(lever="baseline", power_source="battery", ac_online=True))
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid", power_source="battery", ac_online=False))
    write_text(d / "VERDICT.md", make_verdict())
    write_text(d / "events.jsonl", make_events_chain())
    write_json(d / "privacy-report.json", make_privacy_report())
    write_expected(d, expect_pass=False, expect_errors=["power_source=battery but ac_online=true"], description="Battery run with AC online — check 7 fails.")


# ─── Fixture 6: missing-baseline-pair ────────────────────────────────────────


def gen_missing_baseline_pair() -> None:
    d = FIXTURES / "missing-baseline-pair"
    # Manifest points to a baseline file that doesn't exist.
    write_json(d / "hwtest-manifest.json", make_manifest(baseline_path="hwtest-result-baseline.json"))
    write_json(d / "hwtest-plan.json", make_plan())
    write_json(d / "hwtest-host.json", make_host())
    # NOTE: no hwtest-result-baseline.json
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid"))
    write_text(d / "VERDICT.md", make_verdict())
    write_text(d / "events.jsonl", make_events_chain())
    write_json(d / "privacy-report.json", make_privacy_report())
    write_expected(d, expect_pass=False, expect_errors=["baseline result file missing", "no baseline pair"], description="Missing baseline result — check 9 fails.")


# ─── Fixture 7: insufficient-samples ─────────────────────────────────────────


def gen_insufficient_samples() -> None:
    d = FIXTURES / "insufficient-samples"
    write_json(d / "hwtest-manifest.json", make_manifest())
    write_json(d / "hwtest-plan.json", make_plan())  # min_samples=5
    write_json(d / "hwtest-host.json", make_host())
    # baseline result with n=3 (< min_samples=5).
    write_json(d / "hwtest-result-baseline.json", make_result(lever="baseline", n=3))
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid", n=5))
    write_text(d / "VERDICT.md", make_verdict())
    write_text(d / "events.jsonl", make_events_chain())
    write_json(d / "privacy-report.json", make_privacy_report())
    write_expected(d, expect_pass=False, expect_errors=["insufficient_n", "n="], description="Sample count < min_samples — check 10 fails.")


# ─── Fixture 8: malformed-results ────────────────────────────────────────────


def gen_malformed_results() -> None:
    d = FIXTURES / "malformed-results"
    write_json(d / "hwtest-manifest.json", make_manifest())
    write_json(d / "hwtest-plan.json", make_plan())
    write_json(d / "hwtest-host.json", make_host())
    # Write a malformed baseline result (not valid JSON).
    write_text(d / "hwtest-result-baseline.json", "{ this is not valid json = = =")
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid"))
    write_text(d / "VERDICT.md", make_verdict())
    write_text(d / "events.jsonl", make_events_chain())
    write_json(d / "privacy-report.json", make_privacy_report())
    write_expected(d, expect_pass=False, expect_errors=["cannot parse", "no baseline pair"], description="Malformed baseline result JSON — checks 9+11 fail.")


# ─── Fixture 9: secret-leakage ───────────────────────────────────────────────


def gen_secret_leakage() -> None:
    d = FIXTURES / "secret-leakage"
    write_json(d / "hwtest-manifest.json", make_manifest())
    write_json(d / "hwtest-plan.json", make_plan())
    write_json(d / "hwtest-host.json", make_host())
    write_json(d / "hwtest-result-baseline.json", make_result())
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid"))
    write_text(d / "VERDICT.md", make_verdict())
    write_text(d / "events.jsonl", make_events_chain())
    write_json(d / "privacy-report.json", make_privacy_report())
    # Write a transcript file with an unredacted GitHub token.
    # Construct the token by concatenation so the source file doesn't
    # contain a single token string that secret-scanners would flag.
    token = "ghp_" + "aBcDeFgHiJkLmNoPqRsTuVwXyZ" + "1234567890abcd"
    write_text(d / "transcript.log", f"Running benchmark...\nGITHUB_TOKEN={token}\nDone.\n")
    write_expected(d, expect_pass=False, expect_errors=["unredacted secret detected"], description="Unredacted GitHub token in transcript — check 13 fails.")


# ─── Fixture 10: ai-only-verdict ─────────────────────────────────────────────


def gen_ai_only_verdict() -> None:
    d = FIXTURES / "ai-only-verdict"
    write_json(d / "hwtest-manifest.json", make_manifest())
    write_json(d / "hwtest-plan.json", make_plan())
    write_json(d / "hwtest-host.json", make_host())
    write_json(d / "hwtest-result-baseline.json", make_result())
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid"))
    write_text(d / "VERDICT.md", make_verdict(ai_generated=True))
    write_text(d / "events.jsonl", make_events_chain())
    write_json(d / "privacy-report.json", make_privacy_report())
    write_expected(d, expect_pass=False, expect_errors=["AI-summary-as-evidence"], description="AI-generated verdict — check 14 fails.")


# ─── Fixture 11: broken-event-chain ──────────────────────────────────────────


def gen_broken_event_chain() -> None:
    d = FIXTURES / "broken-event-chain"
    write_json(d / "hwtest-manifest.json", make_manifest())
    write_json(d / "hwtest-plan.json", make_plan())
    write_json(d / "hwtest-host.json", make_host())
    write_json(d / "hwtest-result-baseline.json", make_result())
    write_json(d / "hwtest-result-optid.json", make_result(lever="optid"))
    write_text(d / "VERDICT.md", make_verdict())
    write_text(d / "events.jsonl", make_events_chain(tampered=True))
    write_json(d / "privacy-report.json", make_privacy_report())
    write_expected(d, expect_pass=False, expect_errors=["event chain:", "event_sha256 mismatch"], description="Tampered event chain — event chain validation fails.")


# ─── Main ────────────────────────────────────────────────────────────────────


def main() -> int:
    print(f"Generating fixtures in {FIXTURES}")
    print(f"  VERSION = {VERSION}")
    print(f"  GIT_COMMIT = {GIT_COMMIT[:12]}...")
    print()

    # Clean old fixtures.
    if FIXTURES.exists():
        import shutil
        for d in FIXTURES.iterdir():
            if d.is_dir():
                shutil.rmtree(d)

    generators = [
        gen_good_laptop,
        gen_missing_manifest,
        gen_wrong_version,
        gen_laptop_no_battery,
        gen_battery_run_on_ac,
        gen_missing_baseline_pair,
        gen_insufficient_samples,
        gen_malformed_results,
        gen_secret_leakage,
        gen_ai_only_verdict,
        gen_broken_event_chain,
    ]
    for gen in generators:
        gen()
        print(f"  generated: {gen.__name__.replace('gen_', '')}")

    print(f"\n{len(generators)} fixtures generated.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
