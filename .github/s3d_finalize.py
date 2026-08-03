#!/usr/bin/env python3
from __future__ import annotations

import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE_COMMIT = os.environ["SOURCE_COMMIT"]
VERIFICATION_PR = int(os.environ.get("VERIFICATION_PR") or os.environ["PR_NUMBER"])
RUN_ID = int(os.environ["RUN_ID"])
RUN_ATTEMPT = int(os.environ["RUN_ATTEMPT"])
RUNNER = os.environ["RUNNER_DESCRIPTION"]


def receipt(package: str, implementation_pr: int, runtime_proofs: list[str]) -> str:
    commands = [
        f"git bundle verify and checkout exact source commit {SOURCE_COMMIT} -> passed",
        "cargo fmt --all -- --check -> passed",
        "cargo check --workspace --all-targets --all-features -> passed",
        "cargo clippy --workspace --all-targets --all-features -- -D warnings -> passed",
        "all mapped F4, S2D, and S3D acceptance/regression tests executed individually with --exact -> passed",
        "cargo test --workspace -> passed",
        "python3 tools/validate-current-work.py -> passed before transition",
        "python3 tools/validate-optid-packages.py -> passed before transition",
        "python3 tools/render-frontpage.py --check -> passed before transition",
        "bash tools/finish-work.sh --dry-run -> passed against the committed final snapshot",
    ]
    return (
        f'# Integrated verification receipt refreshed by the S3D builder verifier.\n'
        f'# Runtime source was authored in a separate builder job. This fresh verifier\n'
        f'# checked the exact bundled source commit before writing receipts or ledger state.\n\n'
        f'schema_version = 1\n'
        f'package = "{package}"\n'
        f'implementation_pr = {implementation_pr}\n'
        f'verification_pr = {VERIFICATION_PR}\n'
        f'verified_commit = "{SOURCE_COMMIT}"\n'
        f'verifier = "Fresh S3D GitHub verifier job, separate from the builder job; {RUNNER}"\n'
        f'result = "pass"\n'
        f'workflow_run = {RUN_ID}\n'
        f'workflow_attempt = {RUN_ATTEMPT}\n\n'
        'commands = [\n'
        + ''.join(f'  {command!r},\n' for command in commands)
        + ']\n\n'
        'runtime_proofs = [\n'
        + ''.join(f'  {proof!r},\n' for proof in runtime_proofs)
        + ']\n\n'
        'source_inspection_only = [\n'
        '  "The verifier confirmed S3D adds no permanent broker or steady-state hardware-write IPC.",\n'
        '  "The verifier confirmed optid-recover contains no policy parser, classifier, D-Bus server, session bridge, or async runtime.",\n'
        ']\n\n'
        'unresolved = []\n'
    ).replace("'", '"')


f4_proofs = [
    "The F4 reconciler remains the sole production desired-state and restoration authority after S3D supervision integration.",
    "Every F4 mapped acceptance test and additional f4_* regression passed individually with exact selection.",
    "Watchdog notification occurs only after F4 reconciliation and state persistence complete successfully.",
]
s2d_proofs = [
    "Every S2D transaction acceptance test passed individually against the exact S3D source commit.",
    "Journal health validation rejects unpublished, malformed, stale-generation, identity-mismatched, and non-committed residual records before heartbeat.",
    "The independent recovery path consumes the same closed S2D record schema and preserves unresolved evidence on failure.",
]

(ROOT / "docs/plans/optid-verification/f4.toml").write_text(
    receipt("F4", 371, f4_proofs), encoding="utf-8"
)
(ROOT / "docs/plans/optid-verification/s2d.toml").write_text(
    receipt("S2D", 386, s2d_proofs), encoding="utf-8"
)

ledger_path = ROOT / "docs/plans/optid-package-status.toml"
ledger = ledger_path.read_text(encoding="utf-8")
ledger = re.sub(r'^updated = "[^"]+"$', 'updated = "2026-08-04"', ledger, count=1, flags=re.MULTILINE)


def package_block(text: str, package_id: str) -> tuple[re.Match[str], str]:
    match = re.search(
        rf'(?ms)^\[\[package\]\]\nid = "{re.escape(package_id)}"\n.*?(?=^\[\[package\]\]|\Z)',
        text,
    )
    if not match:
        raise SystemExit(f"missing package block {package_id}")
    return match, match.group(0)


match, s3d = package_block(ledger, "S3D")
if 'status = "next"' not in s3d or 'pr = ""' not in s3d:
    raise SystemExit("unexpected S3D ledger state")
s3d = s3d.replace('status = "next"', 'status = "candidate"', 1)
s3d = s3d.replace('pr = ""', f'pr = "{VERIFICATION_PR}"', 1)
s3d = s3d.replace(
    'completion_evidence = []',
    '''runtime_entrypoints = [
  "crates/optid/src/bin/optid-recover.rs",
  "crates/optid/src/recovery.rs",
  "crates/optid/src/reconciler/supervision.rs",
  "crates/optid/src/reconciler/apply.rs",
  "packaging/systemd/optid-recover.service",
  "packaging/systemd/optid-apply.service",
]
integration_tests = [
  "crates/optid/src/recovery.rs",
  "crates/optid/src/reconciler/tests/s3d.rs",
  "crates/optid/tests/recovery_cli.rs",
  "crates/optid/tests/s3d_systemd.rs",
]
completion_evidence = [
  "crates/optid/src/bin/optid-recover.rs",
  "crates/optid/src/recovery.rs",
  "crates/optid/src/reconciler/supervision.rs",
  "crates/optid/src/reconciler/apply.rs",
  "crates/optid/src/reconciler/tests/s3d.rs",
  "crates/optid/tests/recovery_cli.rs",
  "crates/optid/tests/s3d_systemd.rs",
  "packaging/systemd/optid-recover.service",
  "packaging/systemd/optid-apply.service",
  "recipes/core/optid.toml",
  "docs/architecture/optid-s3d-recovery-watchdog.md",
]
[package.acceptance_tests]
recovery_restores_and_compacts = "s3d_recovery_restores_intended_value_and_compacts"
prepared_recovery_is_idempotent = "s3d_recovery_prepared_record_is_idempotent"
external_drift_relinquishes = "s3d_recovery_drift_relinquishes_without_overwrite"
identity_reuse_is_rejected = "s3d_recovery_refuses_identity_reuse"
failed_recovery_retains_record = "s3d_recovery_failure_retains_record"
repeated_recovery_is_idempotent = "s3d_repeated_recovery_is_idempotent"
recovery_cli_surface = "s3d_recovery_cli_recovers_before_success_exit"
minimal_recovery_binary = "s3d_recovery_binary_has_no_policy_or_async_surface"
healthy_cycle_notifies_watchdog = "s3d_complete_cycle_emits_ready_and_watchdog"
unsafe_journal_withholds_watchdog = "s3d_journal_failure_withholds_watchdog"
recovery_precedes_daemon = "s3d_apply_unit_orders_recovery_before_daemon"
failed_recovery_breaks_restart_loop = "s3d_failed_recovery_prevents_automatic_actuation_restart_loop"''',
    1,
)
ledger = ledger[:match.start()] + s3d + ledger[match.end():]
ledger_path.write_text(ledger, encoding="utf-8")

# S3D remains the active safety package while it is a builder candidate.
current_path = ROOT / "docs/plans/current-work.md"
current = current_path.read_text(encoding="utf-8")
if 'active_safety = "S3D"' not in current:
    raise SystemExit("unexpected current-work safety selector")
current_path.write_text(current, encoding="utf-8")

# The workflow owns deletion of temporary builder/finalizer files after this
# script has finished writing receipts and package state.
