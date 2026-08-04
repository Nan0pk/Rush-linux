#!/usr/bin/env python3
"""Publish independent D0 recertification and the S4D candidate claim.

The read-only verifier invokes this only after exact tests, full regression,
Landlock proof, supervised cold restart, and artifact publication succeed.
S4D remains candidate; this script never creates an S4D completion receipt and
never unlocks S5D.
"""

from __future__ import annotations

import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


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

receipt = f'''# Independent D0 recertification required by S4D.
#
# PR #390 extended the accepted D0 Landlock syscall layer to grant writes only
# beneath explicit daemon-state roots. A separate read-only verifier re-ran the
# D0 unit, CLI, live Landlock, child-inheritance, cold-restart, and non-75
# suppression proofs before refreshing this receipt. No D0/S4D runtime source,
# test, service, policy, feature default, or action envelope was repaired.

schema_version = 1
package = "D0"
implementation_pr = 374
recertification_pr = 390
verification_pr = 390
verified_commit = "{verifier_head}"
implemented_commit = "{implementation_head}"
verifier = "Independent ChatGPT GitHub verifier job, separate from the S4D builder; Linux X64 ubuntu-24.04"
result = "pass"
workflow_run = {workflow_run}
workflow_attempt = {workflow_attempt}
workflow_job = {workflow_job}
artifact_id = {artifact_id}
artifact_name = "{artifact_name}"
artifact_digest = "{artifact_digest}"
artifact_size_bytes = {artifact_size}
artifact_expires = "{artifact_expires}"
kernel_release = "{kernel_release}"
landlock_abi = {landlock_abi}

commands = [
  "git diff --name-only {implementation_head}..{verifier_head} -> only the temporary verifier workflow and publication helper",
  "cargo fmt --all -- --check -> passed",
  "cargo check -p optid --all-targets --all-features -> passed",
  "cargo clippy -p optid --all-targets --all-features -- -D warnings -> passed",
  "all eight capability_table::tests::s4d_* tests executed individually with --exact -> passed",
  "all three crates/optid/tests/s4d_systemd.rs tests executed individually with --exact -> passed",
  "cargo test -p optid -> passed",
  "cargo test --workspace --all-features -> passed",
  "cargo test -p optid --features experimental-capability-sealing --bin optid-capability-seal-test -> passed",
  "cargo test -p optid --features experimental-capability-sealing --test capability_sealing_cli -> passed",
  "optid-capability-seal-test --probe on kernel {kernel_release}, Landlock ABI {landlock_abi} -> eight checks passed",
  "systemd supervised status-75 two-cycle cold restart -> passed",
  "systemd non-75 failure restart suppression -> passed",
  "python3 tools/validate-current-work.py -> passed before state transition",
  "python3 tools/validate-optid-packages.py -> exactly one pre-transition error: stale D0 receipt",
]

runtime_proofs = [
  "A descriptor opened before restriction remained writable after Landlock was installed.",
  "A new write-capable descriptor open was denied after sealing.",
  "A descriptor opened read-only before sealing could not be used for writes.",
  "A child process inherited the restriction and could not open the target for writing.",
  "no_new_privs remained set after restriction.",
  "The proof process created no unrestricted helper or sibling process.",
  "Unsupported-kernel handling remained fail-closed.",
  "The test-only supervisor performed exactly one status-75 restart and rebuilt capabilities in a fresh process after recovery.",
  "A non-75 failure produced no restart.",
  "S4D writable-root extension preserved the empty-ruleset D0 API while permitting only explicit daemon-state roots for new state-file writes.",
]

unresolved = []
'''
(ROOT / "docs/plans/optid-verification/d0.toml").write_text(receipt, encoding="utf-8")

ledger_path = ROOT / "docs/plans/optid-package-status.toml"
ledger = ledger_path.read_text(encoding="utf-8")
match, s4d = package_block(ledger, "S4D")
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
ledger = ledger[: match.start()] + s4d + ledger[match.end() :]
ledger_path.write_text(ledger, encoding="utf-8")

print("D0 recertified and S4D recorded as candidate; S5D remains locked")
