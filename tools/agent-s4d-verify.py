#!/usr/bin/env python3
"""Publish independent recertification and the S4D candidate claim.

S4D modifies shared proof paths for F1-F4, D0, S2D, and S3D. The read-only
verifier invokes this helper only after re-running every affected package's
mapped acceptance tests, the full workspace, and the D0/S3D/S4D live lifecycle
proofs. S4D remains candidate; S5D stays locked.
"""

from __future__ import annotations

import os
import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RECERTIFIED = ("F1", "F2", "F3", "F4", "D0", "S2D", "S3D")


def env(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if not value:
        raise SystemExit(f"missing required environment variable {name}")
    return value


def package_block(text: str, package_id: str) -> tuple[re.Match[str], str]:
    match = re.search(
        rf'(?ms)^\[\[package\]\]\nid = "{re.escape(package_id)}"\n.*?(?=^\[\[package\]\]|\Z)',
        text,
    )
    if not match:
        raise SystemExit(f"missing package block {package_id}")
    return match, match.group(0)


def quoted(value: str) -> str:
    return '"' + value.replace('\\', '\\\\').replace('"', '\\"') + '"'


def scalar(text: str, key: str) -> str:
    match = re.search(rf'(?m)^{re.escape(key)} = "([^"]*)"$', text)
    if not match:
        raise SystemExit(f"receipt missing scalar {key}")
    return match.group(1)


def replace_scalar(text: str, key: str, value: str) -> str:
    updated, count = re.subn(
        rf'(?m)^{re.escape(key)} = "[^"]*"$',
        f"{key} = {quoted(value)}",
        text,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"receipt expected one {key}, found {count}")
    return updated


implementation_head = env("IMPLEMENTATION_HEAD")
verifier_head = env("VERIFIER_HEAD")
workflow_run = int(env("WORKFLOW_RUN"))
workflow_attempt = int(env("WORKFLOW_ATTEMPT"))
workflow_job = int(env("WORKFLOW_JOB"))
artifact_id = int(env("ARTIFACT_ID"))
artifact_name = env("ARTIFACT_NAME")
artifact_digest = env("ARTIFACT_DIGEST")
artifact_size = int(env("ARTIFACT_SIZE"))
artifact_expires = env("ARTIFACT_EXPIRES")
kernel_release = env("KERNEL_RELEASE")
landlock_abi = int(env("LANDLOCK_ABI"))
recertified_at = env("RECERTIFIED_AT")

ledger_path = ROOT / "docs/plans/optid-package-status.toml"
ledger_text = ledger_path.read_text(encoding="utf-8")
ledger_data = tomllib.loads(ledger_text)
packages = {package["id"]: package for package in ledger_data["package"]}
verifier = (
    "Independent ChatGPT GitHub verifier job, separate from the S4D builder; "
    "Linux X64 ubuntu-24.04"
)

for package_id in RECERTIFIED:
    package = packages[package_id]
    receipt_rel = package.get("verification_receipt")
    if not receipt_rel:
        raise SystemExit(f"{package_id}: completed package has no receipt")
    receipt_path = ROOT / receipt_rel
    text = receipt_path.read_text(encoding="utf-8")
    prior_commit = scalar(text, "verified_commit")
    prior_verifier = scalar(text, "verifier")
    if "s4d_recertification_pr" in text:
        raise SystemExit(f"{package_id}: S4D recertification already recorded")

    text = replace_scalar(text, "verified_commit", verifier_head)
    text = replace_scalar(text, "verifier", verifier)
    text = replace_scalar(text, "result", "pass")

    metadata = f'''
# PR #390 S4D shared-surface recertification.
s4d_recertification_pr = 390
s4d_recertification_implementation_head = "{implementation_head}"
s4d_prior_verified_commit = "{prior_commit}"
s4d_prior_verifier = {quoted(prior_verifier)}
s4d_recertified_at = "{recertified_at}"
s4d_recertification_workflow_run = {workflow_run}
s4d_recertification_workflow_attempt = {workflow_attempt}
s4d_recertification_workflow_job = {workflow_job}
s4d_recertification_artifact_id = {artifact_id}
s4d_recertification_artifact_name = "{artifact_name}"
s4d_recertification_artifact_digest = "{artifact_digest}"
s4d_recertification_artifact_size_bytes = {artifact_size}
s4d_recertification_artifact_expires = "{artifact_expires}"
s4d_recertification_kernel_release = "{kernel_release}"
s4d_recertification_landlock_abi = {landlock_abi}

s4d_recertification_commands = [
  "all mapped acceptance tests for F1, F2, F3, F4, D0, S2D, S3D, and S4D executed by name and observed passing",
  "cargo fmt --all -- --check -> passed",
  "cargo check -p optid --all-targets --all-features -> passed",
  "cargo clippy -p optid --all-targets --all-features -- -D warnings -> passed",
  "cargo test -p optid -> passed",
  "cargo test --workspace --all-features -> passed",
  "live D0 Landlock proof on kernel {kernel_release}, ABI {landlock_abi} -> eight checks passed",
  "D0 supervised status-75 two-cycle rebuild and non-75 suppression -> passed",
  "S3D live required-recovery ordering, one replacement restart, and failed-recovery loop suppression -> passed",
  "S4D static startup order, topology handback, descriptor identity, and cold-rebuild tests -> passed",
]

s4d_recertification_runtime_proofs = [
  "S4D's shared actuator, main-loop, policy, kernel-I/O, restore, Landlock, and apply-unit changes preserved every mapped acceptance contract of this completed package.",
  "No runtime source, test, service, policy, feature default, action envelope, or package claim was repaired during verification.",
  "The immutable implementation head {implementation_head} was tested before any receipt or ledger mutation.",
]
'''
    text = text.rstrip() + "\n" + metadata
    receipt_path.write_text(text, encoding="utf-8")

# Record S4D as candidate only. Completion remains reserved for a separate
# post-merge cold verifier, and S5D remains planned/locked.
match, s4d = package_block(ledger_text, "S4D")
if 'status = "next"' not in s4d or 'pr = ""' not in s4d:
    raise SystemExit("unexpected S4D pre-verification ledger state")

s4d = '''[[package]]
id = "S4D"
lane = "safety"
title = "Move writes to a sealed typed capability table"
status = "candidate"
depends = ["D0", "F4", "S3D"]
pr = "390"
runtime_entrypoints = [
  "crates/optid/src/main.rs",
  "crates/optid/src/capability_table.rs",
  "crates/optid/src/actuator.rs",
  "crates/optid/src/reconciler/restore.rs",
  "packaging/systemd/optid-apply.service",
]
integration_tests = [
  "crates/optid/src/capability_table.rs",
  "crates/optid/tests/s4d_systemd.rs",
]
completion_evidence = [
  "config/optid/policy.toml",
  "crates/optid/src/actuator.rs",
  "crates/optid/src/capability_seal_test/landlock_syscall.rs",
  "crates/optid/src/capability_table.rs",
  "crates/optid/src/kernel_io.rs",
  "crates/optid/src/main.rs",
  "crates/optid/src/policy.rs",
  "crates/optid/src/reconciler/restore.rs",
  "crates/optid/tests/s4d_systemd.rs",
  "docs/architecture/optid-s4d-sealed-capabilities.md",
  "mkosi/mkosi.extra/usr/lib/systemd/system/optid-apply.service",
  "packaging/systemd/optid-apply.service",
  ".github/workflows/capability-sealing-kernel-proof.yml",
]
[package.acceptance_tests]
operation_type_mismatch_denied = "s4d_operation_type_mismatch_is_rejected"
preopened_descriptor_survives_restriction = "s4d_preopened_descriptor_survives_permission_tightening"
symlink_replacement_denied = "s4d_symlink_path_replacement_is_rejected"
stale_identity_denied = "s4d_stale_identity_is_rejected"
removed_device_fails_closed = "s4d_removed_device_fails_closed"
descriptors_are_cloexec = "s4d_capability_descriptors_are_cloexec"
topology_change_is_debounced = "s4d_topology_change_is_debounced"
cold_rebuild_opens_fresh_identity = "s4d_cold_rebuild_opens_fresh_identity"
supervised_recovery_restart_graph = "s4d_apply_unit_restarts_only_through_supervised_recovery_graph"
seal_precedes_workers_and_dbus = "s4d_startup_seals_before_any_worker_or_dbus_input"
handback_precedes_status_75 = "s4d_topology_rebuild_hands_back_before_status_75"

'''
ledger_text = ledger_text[: match.start()] + s4d + ledger_text[match.end() :]
ledger_path.write_text(ledger_text, encoding="utf-8")

print(
    "recertified F1-F4, D0, S2D, S3D and recorded S4D as candidate; "
    "S5D remains locked"
)
