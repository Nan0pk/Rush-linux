#!/usr/bin/env python3
"""Publish the S4D post-merge verification receipt after read-only proof."""

from __future__ import annotations

import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / "docs/plans/optid-package-status.toml"
CURRENT = ROOT / "docs/plans/current-work.md"
RECEIPT = ROOT / "docs/plans/optid-verification/s4d.toml"


def required(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if not value:
        raise SystemExit(f"missing {name}")
    return value


def package_section(text: str, package_id: str) -> tuple[str, int, int]:
    marker = f'[[package]]\nid = "{package_id}"'
    start = text.find(marker)
    if start < 0:
        raise SystemExit(f"missing package {package_id}")
    end = text.find("\n[[package]]", start + len(marker))
    if end < 0:
        end = len(text)
    return text[start:end], start, end


def update_package(text: str, package_id: str, mutate) -> str:
    section, start, end = package_section(text, package_id)
    updated = mutate(section)
    if updated == section:
        raise SystemExit(f"package {package_id} was not updated")
    return text[:start] + updated + text[end:]


merged_commit = required("MERGED_COMMIT")
implementation_head = required("IMPLEMENTATION_HEAD")
verified_commit = required("VERIFIER_HEAD")
verification_pr = int(required("VERIFICATION_PR"))
verified_at = required("VERIFIED_AT")
workflow_run = int(required("WORKFLOW_RUN"))
workflow_attempt = int(required("WORKFLOW_ATTEMPT"))
workflow_job = int(required("WORKFLOW_JOB"))
artifact_id = int(required("ARTIFACT_ID"))
artifact_name = required("ARTIFACT_NAME")
artifact_digest = required("ARTIFACT_DIGEST")
artifact_size = int(required("ARTIFACT_SIZE"))
artifact_expires = required("ARTIFACT_EXPIRES")
kernel_release = required("KERNEL_RELEASE")
landlock_abi = int(required("LANDLOCK_ABI"))

ledger = LEDGER.read_text(encoding="utf-8")
ledger = re.sub(
    r"# Independent post-merge cold verification now covers F1-F4 and S1D-S3D\.\n"
    r"# PR #389 verified merged S3D recovery, watchdog, and service ordering\n"
    r"# through exact acceptance tests, production recovery fault injection,\n"
    r"# and a live cold systemd lifecycle\. S4D is now the active safety package\.\n"
    r"# No package self-certified\.\n",
    f"# Independent post-merge cold verification now covers F1-F4 and S1D-S4D.\n"
    f"# PR #{verification_pr} verified merged S4D sealed capabilities through all\n"
    f"# mapped acceptance tests, full regression, live Landlock enforcement, and\n"
    f"# supervisor-managed cold rebuild. S5D is now the active safety package.\n"
    f"# No package self-certified.\n",
    ledger,
    count=1,
)
if 'active_safety = "S4D"' not in ledger:
    raise SystemExit("ledger active_safety was not S4D")
ledger = ledger.replace('active_safety = "S4D"', 'active_safety = "S5D"', 1)


def complete_s4d(section: str) -> str:
    if 'status = "candidate"' not in section:
        raise SystemExit("S4D was not candidate")
    if 'pr = "390"' not in section:
        raise SystemExit("S4D implementation PR mismatch")
    if "verification_receipt" in section:
        raise SystemExit("S4D already has a receipt")
    section = section.replace('status = "candidate"', 'status = "completed"', 1)
    section = section.replace(
        'pr = "390"',
        'pr = "390"\nverification_receipt = "docs/plans/optid-verification/s4d.toml"',
        1,
    )
    return section


def unlock_s5d(section: str) -> str:
    if 'status = "planned"' not in section:
        raise SystemExit("S5D was not planned")
    return section.replace('status = "planned"', 'status = "next"', 1)


ledger = update_package(ledger, "S4D", complete_s4d)
ledger = update_package(ledger, "S5D", unlock_s5d)
LEDGER.write_text(ledger.rstrip() + "\n", encoding="utf-8")

current = CURRENT.read_text(encoding="utf-8")
if 'active_safety = "S4D"' not in current:
    raise SystemExit("current-work active_safety was not S4D")
if 'unlocks_after_active_safety = ["S5D"]' not in current:
    raise SystemExit("current-work unlock projection was not S5D")
current = current.replace('active_safety = "S4D"', 'active_safety = "S5D"', 1)
current = current.replace(
    'unlocks_after_active_safety = ["S5D"]',
    'unlocks_after_active_safety = []',
    1,
)
CURRENT.write_text(current.rstrip() + "\n", encoding="utf-8")

if RECEIPT.exists():
    raise SystemExit(f"receipt already exists: {RECEIPT}")

receipt = f'''# Independent post-merge cold-verification receipt for S4D:
# Move writes to a sealed typed capability table.
#
# Verification was read-only before this state transition. No S4D runtime
# implementation, test, service, policy, action envelope, or safety default was
# repaired by the verifier.

schema_version = 1
package = "S4D"
implementation_pr = 390
verification_pr = {verification_pr}
verified_commit = "{verified_commit}"
implementation_head = "{implementation_head}"
integrated_commit = "{merged_commit}"
source_base_commit = "{merged_commit}"
verifier = "Independent ChatGPT GitHub verifier job, separate from the PR #390 builder; Linux X64 ubuntu-24.04"
result = "pass"
verified_at = "{verified_at}"

acceptance_workflow_run = {workflow_run}
acceptance_workflow_attempt = {workflow_attempt}
acceptance_workflow_job = {workflow_job}
acceptance_artifact_id = {artifact_id}
acceptance_artifact_name = "{artifact_name}"
acceptance_artifact_digest = "{artifact_digest}"
acceptance_artifact_size_bytes = {artifact_size}
acceptance_artifact_expires = "{artifact_expires}"
kernel_release = "{kernel_release}"
landlock_abi = {landlock_abi}

commands = [
  "source-bound verifier tree to merged commit {merged_commit}; only the temporary verifier workflow and receipt writer differed before proof",
  "pre-transition current-work, package ledger, generated README, and whitespace validation -> passed",
  "cargo fmt --all -- --check -> passed",
  "cargo check --workspace --all-targets --all-features -> passed",
  "cargo clippy --workspace --all-targets --all-features -- -D warnings -> passed",
  "all eleven mapped S4D acceptance tests were discovered uniquely and executed individually with --exact -> passed",
  "cargo test -p optid --all-features -> passed",
  "cargo test --workspace --all-features -> passed",
  "packaged and mkosi optid-apply units were byte-identical; packaged apply/recovery units passed systemd-analyze verify",
  "live D0 capability-sealing probe on kernel {kernel_release}, Landlock ABI {landlock_abi} -> eight checks passed",
  "supervised status-75 two-cycle cold rebuild and non-75 restart suppression -> passed",
]

runtime_proofs = [
  "The production startup acceptance path completed discovery and capability construction, installed sealing, and proved sealing precedes worker and D-Bus input startup.",
  "Typed operation/path mismatches, symlink replacement, stale device/inode identity, removed targets, and descriptor inheritance hazards all failed closed.",
  "Every pre-opened hardware descriptor remained CLOEXEC and the live Landlock proof denied new write opens while preserving pre-opened descriptor writes and explicitly allowed state-root writes.",
  "The topology lifecycle debounced a changed fingerprint, handed owned targets back through the existing reconciliation/transaction path before status 75, and a fresh process opened the replacement identity.",
  "The shipped apply unit retained required recovery ordering and the supervisor restarted exactly once for status 75 while refusing to restart a non-75 failure.",
  "S2D durable transactions, S3D independent recovery/watchdog ordering, F4 handback authority, the observe-safe default, and every existing action envelope remained unchanged.",
]

evidence_notes = [
  "The hosted live proof uses synthetic filesystem targets and the repository's D0 Landlock probe; it does not promote any physical HWID or expand hardware support.",
  "S4D completion unlocks S5D construction only. It does not implement circuit breakers or canary re-entry.",
]

unresolved = []
'''
RECEIPT.write_text(receipt.rstrip() + "\n", encoding="utf-8")
print("published S4D completed receipt and advanced S5D to next")
