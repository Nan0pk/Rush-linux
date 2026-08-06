#!/usr/bin/env python3
"""Publish the S5D post-merge verification receipt after read-only proof."""

from __future__ import annotations

import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / "docs/plans/optid-package-status.toml"
CURRENT = ROOT / "docs/plans/current-work.md"
RECEIPT = ROOT / "docs/plans/optid-verification/s5d.toml"


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
old_header = (
    "# Independent post-merge cold verification now covers F1-F4 and S1D-S4D.\n"
    "# PR #391 verified merged S4D sealed capabilities through all\n"
    "# mapped acceptance tests, full regression, live Landlock enforcement, and\n"
    "# supervisor-managed cold rebuild. S5D is now the active safety package.\n"
    "# No package self-certified.\n"
)
new_header = (
    "# Independent post-merge cold verification now covers F1-F4 and S1D-S5D.\n"
    f"# PR #{verification_pr} verified merged S5D scoped circuit breakers through all\n"
    "# mapped acceptance tests, full regression, persistent restart/canary proofs,\n"
    "# and live packaged recovery/supervisor checks. C1 is now the active safety-track follow-on.\n"
    "# No package self-certified.\n"
)
if ledger.count(old_header) != 1:
    raise SystemExit("expected one pre-S5D verification header")
ledger = ledger.replace(old_header, new_header, 1)

if 'active_safety = "S5D"' not in ledger:
    raise SystemExit("ledger active_safety was not S5D")
ledger = ledger.replace('active_safety = "S5D"', 'active_safety = "C1"', 1)


def complete_s5d(section: str) -> str:
    if 'status = "candidate"' not in section:
        raise SystemExit("S5D was not candidate")
    if 'pr = "392"' not in section:
        raise SystemExit("S5D implementation PR mismatch")
    if "verification_receipt" in section:
        raise SystemExit("S5D already has a receipt")
    section = section.replace('status = "candidate"', 'status = "completed"', 1)
    section = section.replace(
        'pr = "392"',
        'pr = "392"\nverification_receipt = "docs/plans/optid-verification/s5d.toml"',
        1,
    )
    return section


def advance_c1(section: str) -> str:
    if 'status = "planned"' not in section:
        raise SystemExit("C1 was not planned")
    if 'depends = ["F1", "F3"]' not in section:
        raise SystemExit("C1 dependencies changed unexpectedly")
    return section.replace('status = "planned"', 'status = "next"', 1)


ledger = update_package(ledger, "S5D", complete_s5d)
ledger = update_package(ledger, "C1", advance_c1)
LEDGER.write_text(ledger.rstrip() + "\n", encoding="utf-8")

current = CURRENT.read_text(encoding="utf-8")
if 'active_safety = "S5D"' not in current:
    raise SystemExit("current-work active_safety was not S5D")
if 'unlocks_after_active_safety = []' not in current:
    raise SystemExit("current-work S5D unlock projection changed unexpectedly")
current = current.replace('active_safety = "S5D"', 'active_safety = "C1"', 1)
current = current.replace(
    'unlocks_after_active_safety = []',
    'unlocks_after_active_safety = ["D5"]',
    1,
)
CURRENT.write_text(current.rstrip() + "\n", encoding="utf-8")

if RECEIPT.exists():
    raise SystemExit(f"receipt already exists: {RECEIPT}")

receipt = f'''# Independent post-merge cold-verification receipt for S5D:
# Add domain circuit breakers and controlled canary re-entry.
#
# Verification was read-only before this state transition. No S5D runtime
# implementation, test, service, policy, action envelope, safety threshold,
# or hardware authorization was repaired by the verifier.

schema_version = 1
package = "S5D"
implementation_pr = 392
verification_pr = {verification_pr}
verified_commit = "{verified_commit}"
implementation_head = "{implementation_head}"
integrated_commit = "{merged_commit}"
source_base_commit = "{merged_commit}"
verifier = "Independent ChatGPT GitHub verifier job, separate from the PR #392 builder; Linux X64 ubuntu-24.04"
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
  "all thirteen mapped S5D acceptance tests were discovered uniquely and executed individually with --exact -> passed",
  "cargo test -p optid --all-features -> passed",
  "cargo test --workspace --all-features -> passed",
  "packaged and mkosi apply/recovery units were byte-identical and passed systemd-analyze verify",
  "root-authorized production clear-all CLI persisted private empty circuit state without starting the daemon loop; unprivileged clear was denied",
  "live D0 capability-sealing probe on kernel {kernel_release}, Landlock ABI {landlock_abi} -> eight checks passed",
  "supervised status-75 two-cycle cold rebuild and non-75 restart suppression -> passed",
]

runtime_proofs = [
  "Repeated attributable failures open only the matching domain, operation, target, hardware, firmware, and failure-class scope while unrelated domains remain eligible to actuate.",
  "Circuit state survives a fresh CircuitBreaker load; cooldown alone is insufficient, a successful observe-only recovery cycle is required, and only one persisted canary may enter the affected scope.",
  "A successful canary closes the scope; a failed canary reopens immediately; backward clock movement cannot shorten cooldown; a firmware identity change creates an independent scope.",
  "Restore failures are charged to the affected domain, while only unisolatable process-wide corruption opens the global observe-only circuit.",
  "Machine-readable diagnostics include scoped domain, operation, stable target, hardware identity, firmware identity, failure class, state, cooldown, and recovery evidence.",
  "The production one-shot clear path required effective UID 0, rejected unprivileged use, wrote private state, and created no daemon lock, status, decision, or control-cycle files.",
  "The shipped recovery and supervisor graph remained valid, including exact status-75 cold rebuild and non-75 restart suppression.",
]

evidence_notes = [
  "The hosted live proof uses synthetic filesystem state and the repository's D0 Landlock probe; it does not promote any physical HWID or expand hardware support.",
  "Threshold and cooldown defaults remain the reviewed S5D implementation values; this verifier did not tune policy from hosted-runner observations.",
  "S5D completion closes the accepted D2 safety sequence and advances C1, the next package in the plan's safe-mutation track. Dependencies still unlock only from completed state.",
]

unresolved = []
'''
RECEIPT.write_text(receipt.rstrip() + "\n", encoding="utf-8")
print("published S5D completed receipt and advanced C1 to next")
