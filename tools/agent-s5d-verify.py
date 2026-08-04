#!/usr/bin/env python3
"""Refresh affected receipts and record S5D as candidate after independent verification."""

from __future__ import annotations

import os
import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
AFFECTED = ("F1", "F2", "F3", "F4", "S2D", "S3D", "S4D")


def required(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if not value:
        raise SystemExit(f"missing {name}")
    return value


def replace_quoted(text: str, key: str, value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    updated, count = re.subn(
        rf'(?m)^{re.escape(key)} = "[^"]*"$',
        f'{key} = "{escaped}"',
        text,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"expected one {key}, found {count}")
    return updated


source_head = required("SOURCE_HEAD")
run_id = int(required("WORKFLOW_RUN"))
attempt = int(required("WORKFLOW_ATTEMPT"))
job_id = int(required("WORKFLOW_JOB"))
artifact_id = int(required("ARTIFACT_ID"))
artifact_name = required("ARTIFACT_NAME")
artifact_digest = required("ARTIFACT_DIGEST")
artifact_size = int(required("ARTIFACT_SIZE"))
artifact_expires = required("ARTIFACT_EXPIRES")
kernel_release = required("KERNEL_RELEASE")
landlock_abi = int(required("LANDLOCK_ABI"))
verified_at = required("VERIFIED_AT")

ledger_path = ROOT / "docs/plans/optid-package-status.toml"
ledger_text = ledger_path.read_text(encoding="utf-8")
ledger = tomllib.loads(ledger_text)
packages = {item["id"]: item for item in ledger["package"]}

for package_id in AFFECTED:
    package = packages[package_id]
    if package.get("status") != "completed":
        raise SystemExit(f"{package_id}: expected completed before recertification")
    receipt_rel = package.get("verification_receipt")
    if not receipt_rel:
        raise SystemExit(f"{package_id}: missing receipt path")
    receipt_path = ROOT / receipt_rel
    text = receipt_path.read_text(encoding="utf-8")
    prior = re.search(r'(?m)^verified_commit = "([0-9a-f]{40})"$', text)
    if not prior:
        raise SystemExit(f"{package_id}: missing verified_commit")
    if "s5d_candidate_recertification_pr" in text:
        raise SystemExit(f"{package_id}: S5D recertification already present")
    text = replace_quoted(text, "verified_commit", source_head)
    text = replace_quoted(
        text,
        "verifier",
        "Independent ChatGPT GitHub verifier job for S5D shared surfaces; Linux X64 ubuntu-24.04",
    )
    text = replace_quoted(text, "result", "pass")
    text = text.rstrip() + f'''

# PR #392 S5D shared-surface candidate recertification.
s5d_candidate_recertification_pr = 392
s5d_candidate_source_head = "{source_head}"
s5d_candidate_prior_verified_commit = "{prior.group(1)}"
s5d_candidate_verified_at = "{verified_at}"
s5d_candidate_workflow_run = {run_id}
s5d_candidate_workflow_attempt = {attempt}
s5d_candidate_workflow_job = {job_id}
s5d_candidate_artifact_id = {artifact_id}
s5d_candidate_artifact_name = "{artifact_name}"
s5d_candidate_artifact_digest = "{artifact_digest}"
s5d_candidate_artifact_size_bytes = {artifact_size}
s5d_candidate_artifact_expires = "{artifact_expires}"
s5d_candidate_kernel_release = "{kernel_release}"
s5d_candidate_landlock_abi = {landlock_abi}

s5d_candidate_commands = [
  "source-bound fresh verifier to immutable S5D implementation commit {source_head}",
  "validator-derived affected completed packages were exactly F1, F2, F3, F4, S2D, S3D, and S4D",
  "every mapped acceptance test for all affected completed packages and S5D was uniquely discovered and executed with --exact -> passed",
  "cargo fmt --all -- --check -> passed",
  "cargo check --workspace --all-targets --all-features -> passed",
  "cargo clippy --workspace --all-targets --all-features -- -D warnings -> passed",
  "cargo test -p optid --all-features -> passed",
  "cargo test --workspace --all-features -> passed",
  "packaged and mkosi apply/recovery units were byte-identical and passed systemd-analyze verify",
  "live capability-sealing eight-check probe, status-75 cold rebuild, recovery ordering, and non-75 restart suppression -> passed",
]

s5d_candidate_runtime_proofs = [
  "S5D evaluates persisted circuit state before desired-state reconciliation and before actuator entry.",
  "Repeated scoped failures persist quarantine across restart, remove only the failed domain from desired state, and preserve unrelated domain actuation.",
  "Cooldown cannot authorize re-entry without a successful observe-only recovery cycle; one canary is persisted in-flight, confirmed readback closes it, and failure reopens immediately.",
  "Restore failures are attributed to their reconciler domain and counted; unisolatable shared failures open the process-wide observe-only circuit.",
  "Canonical MODALIAS or a privacy-safe platform HWID token and a separate firmware token scope the persistent record without promoting hardware.",
  "Root-only one-shot clear commands never start the daemon loop and refuse invalid/non-private state evidence.",
  "No new daemon, helper service, IPC path, write ABI, environment bypass, hardware allowlist promotion, or default actuation expansion was introduced.",
]
'''
    receipt_path.write_text(text.rstrip() + "\n", encoding="utf-8")

old_block = '''[[package]]
id = "S5D"
lane = "safety"
title = "Add domain circuit breakers and controlled canary re-entry"
status = "next"
depends = ["F3", "F4", "S4D"]
pr = ""
completion_evidence = []
'''
new_block = '''[[package]]
id = "S5D"
lane = "safety"
title = "Add domain circuit breakers and controlled canary re-entry"
status = "candidate"
depends = ["F3", "F4", "S4D"]
pr = "392"
runtime_entrypoints = [
  "crates/optid/src/main.rs",
  "crates/optid/src/circuit_breaker.rs",
  "crates/optid/src/reconciler/apply.rs",
  "crates/optid/src/args.rs",
]
integration_tests = [
  "crates/optid/src/circuit_breaker.rs",
  "crates/optid/src/args.rs",
]
completion_evidence = [
  "config/optid/policy.toml",
  "crates/optid/src/args.rs",
  "crates/optid/src/circuit_breaker.rs",
  "crates/optid/src/main.rs",
  "crates/optid/src/policy.rs",
  "crates/optid/src/reconciler/apply.rs",
  "docs/architecture/optid-s5d-circuit-breakers.md",
]
[package.acceptance_tests]
threshold_opens_persistent_scope = "s5d_repeated_failure_opens_persistent_scope"
restart_preserves_open_scope = "s5d_restart_preserves_open_circuit"
cooldown_requires_recovery = "s5d_cooldown_requires_recovery_before_one_canary"
canary_success_closes = "s5d_canary_success_closes_circuit"
canary_failure_reopens = "s5d_canary_failure_reopens_immediately"
firmware_change_is_independent = "s5d_firmware_change_uses_independent_scope"
manual_clear_is_root_only = "s5d_manual_clear_requires_root_authorization"
backward_clock_is_safe = "s5d_backward_clock_jump_never_shortens_cooldown"
multi_domain_isolation = "s5d_multi_domain_failure_isolation"
restore_failure_is_scoped = "s5d_restore_failures_open_only_the_affected_domain"
process_corruption_is_global = "s5d_unknown_process_corruption_forces_global_observe_only"
production_diagnostic_identity = "s5d_production_gate_emits_scoped_diagnostic_and_firmware_identity"
clear_cli_is_one_shot = "s5d_clear_commands_are_one_shot_and_mutually_exclusive"
'''
if ledger_text.count(old_block) != 1:
    raise SystemExit("expected one pristine S5D ledger block")
ledger_text = ledger_text.replace(old_block, new_block, 1)
ledger_path.write_text(ledger_text, encoding="utf-8")

print("refreshed affected receipts and recorded S5D candidate")
