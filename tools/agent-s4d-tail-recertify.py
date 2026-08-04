#!/usr/bin/env python3
"""Refresh only the F2 and D0 receipts after S4D lint-policy cleanup."""

from __future__ import annotations

import os
import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACKAGES = ("F2", "D0")


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
verifier_head = required("VERIFIER_HEAD")
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

ledger = tomllib.loads((ROOT / "docs/plans/optid-package-status.toml").read_text())
by_id = {item["id"]: item for item in ledger["package"]}
verifier = (
    "Independent ChatGPT GitHub verifier job for the S4D policy cleanup; "
    "Linux X64 ubuntu-24.04"
)

for package_id in PACKAGES:
    receipt_rel = by_id[package_id].get("verification_receipt")
    if not receipt_rel:
        raise SystemExit(f"{package_id}: missing receipt path")
    path = ROOT / receipt_rel
    text = path.read_text(encoding="utf-8")
    previous = re.search(r'(?m)^verified_commit = "([^"]+)"$', text)
    if not previous:
        raise SystemExit(f"{package_id}: missing verified_commit")

    text = replace_quoted(text, "verified_commit", verifier_head)
    text = replace_quoted(text, "verifier", verifier)
    text = replace_quoted(text, "result", "pass")
    if "s4d_cleanup_recertification_pr" in text:
        raise SystemExit(f"{package_id}: cleanup recertification already present")

    text = text.rstrip() + f'''

# PR #390 final lint-policy cleanup recertification.
s4d_cleanup_recertification_pr = 390
s4d_cleanup_source_head = "{source_head}"
s4d_cleanup_prior_verified_commit = "{previous.group(1)}"
s4d_cleanup_verified_at = "{verified_at}"
s4d_cleanup_workflow_run = {run_id}
s4d_cleanup_workflow_attempt = {attempt}
s4d_cleanup_workflow_job = {job_id}
s4d_cleanup_artifact_id = {artifact_id}
s4d_cleanup_artifact_name = "{artifact_name}"
s4d_cleanup_artifact_digest = "{artifact_digest}"
s4d_cleanup_artifact_size_bytes = {artifact_size}
s4d_cleanup_artifact_expires = "{artifact_expires}"
s4d_cleanup_kernel_release = "{kernel_release}"
s4d_cleanup_landlock_abi = {landlock_abi}

s4d_cleanup_commands = [
  "validator pre-state reported exactly F2 and D0 stale",
  "every mapped F2 and D0 acceptance test was discovered and passed under all features",
  "cargo fmt --all -- --check -> passed",
  "cargo check -p optid --all-targets --all-features -> passed",
  "cargo clippy -p optid --all-targets --all-features -- -D warnings -> passed",
  "cargo test -p optid -> passed",
  "cargo test --workspace --all-features -> passed",
  "live D0 capability-sealing probe on kernel {kernel_release}, Landlock ABI {landlock_abi} -> passed",
  "supervised status-75 cold rebuild and non-75 restart suppression -> passed",
]

s4d_cleanup_runtime_proofs = [
  "F2's binary-test override remained reachable through a typed compile-time reference without a dead-code suppression.",
  "D0's proof-only empty-ruleset and kernel-release helpers remained reachable through typed compile-time references without dead-code suppressions.",
  "No runtime behavior, package state, policy default, action envelope, service ordering, or S4D candidate claim changed during this recertification.",
]
'''
    path.write_text(text.rstrip() + "\n", encoding="utf-8")

print("refreshed F2 and D0 receipts only")
