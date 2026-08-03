from __future__ import annotations

import os
import re
from pathlib import Path

ROOT = Path(".")
INTEGRATED_COMMIT = "2247d460a67a8b1674bf7f8734008411d2471920"
IMPLEMENTATION_HEAD = "c7741c9017bba939571a82de4ea49b1fe1608108"

pr_number = int(os.environ["PR_NUMBER"])
run_id = int(os.environ["RUN_ID"])
run_attempt = int(os.environ["RUN_ATTEMPT"])
runner = os.environ["RUNNER_DESCRIPTION"]
verified_commit = os.environ["VERIFIED_COMMIT"]

commands_common = [
    f"git merge-base --is-ancestor {IMPLEMENTATION_HEAD} {INTEGRATED_COMMIT} -> passed",
    f"git diff --exit-code {INTEGRATED_COMMIT}..origin/main -- all declared F4 and S2D proof paths -> no changes",
    f"git diff --exit-code {INTEGRATED_COMMIT}..HEAD -- all declared F4 and S2D proof paths -> no verifier-side source changes",
    "cargo clean -> passed in fresh GitHub-hosted checkout",
    "cargo fmt --all -- --check -> passed",
    "cargo check --workspace --all-targets --all-features -> passed",
    "cargo clippy --workspace --all-targets --all-features -- -D warnings -> passed",
    "all mapped S2D acceptance tests executed individually with --exact -> passed",
    "all mapped F4 acceptance tests and additional f4_* regressions executed individually with --exact -> passed",
    "cargo test --workspace -> passed",
    "python3 tools/validate-current-work.py -> passed before transition",
    "python3 tools/validate-optid-packages.py -> passed before transition",
    "python3 tools/render-frontpage.py --check -> passed before transition",
    "git diff --check -> passed",
]

f4_runtime_proofs = [
    "Production reconciliation: the daemon continues to enter the F4 reconciler for complete desired-state application, transition restoration, shutdown restoration, and property-level systemd reconciliation.",
    "Integrated transaction boundary: every F4-owned kernel or systemd mutation is gated by the S2D durable transaction protocol before the write and commits only after typed readback.",
    "Handback safety: external drift, device disappearance, malformed state, missing baseline, and readback mismatch remain fail-closed and never produce a pretend restore or false ownership claim.",
    "Restart boundary: previous-generation transaction evidence is validated before any handback mutation; stale evidence is retained for S3D rather than compacted or overwritten.",
    "Source binding: all declared F4 proof paths are byte-unchanged from merged integration commit 2247d460a67a8b1674bf7f8734008411d2471920 through the verifier branch's read-only test phase.",
]

s2d_runtime_proofs = [
    "Write-ahead ordering: a complete record, file sync, atomic rename, and recovery-directory sync precede every reconciler-owned production mutation.",
    "Verified commit and compensation: intended writes commit only after exact typed readback; write/readback failures attempt exact-original compensation and retain non-terminal evidence when compensation cannot be verified.",
    "Identity safety: stable FNV-1a record naming, canonical target identity, stale-generation rejection, and path-reuse rejection were exercised through exact acceptance tests.",
    "Multi-target safety: compensation attempts every target even after an earlier failure and returns failure without falsely claiming complete recovery.",
    "Lifecycle safety: committed records remain until verified handback; compaction occurs only after exact restoration or explicit drift relinquishment.",
    "Production surface: the real daemon run path prepared, committed, restored, and compacted persistent transactions under the injected I/O boundary.",
    "Scope boundary: no S3D recovery helper, watchdog, boot ordering, S4D sealing, or S5D circuit breaker behavior was added or claimed.",
]


def toml_string(value: str) -> str:
    return '"' + value.replace('\\', '\\\\').replace('"', '\\"') + '"'


def render_receipt(package: str, implementation_pr: int, runtime_proofs: list[str]) -> str:
    title = (
        "Reconcile complete desired state and restore on transitions"
        if package == "F4"
        else "Implement persistent verified write-ahead transactions"
    )
    lines = [
        f"# Post-merge cold-verification receipt for {package}:",
        f"# {title}",
        "#",
        "# The read-only verification phase ran in a fresh GitHub-hosted checkout",
        "# after PR #386 merged. No runtime source, test, assertion, architecture,",
        "# policy, configuration, or packaging file was repaired during verification.",
        "# Receipt and work-state files were written only after every proof passed.",
        "",
        "schema_version = 1",
        f"package = {toml_string(package)}",
        f"implementation_pr = {implementation_pr}",
        f"verification_pr = {pr_number}",
        f"verified_commit = {toml_string(verified_commit)}",
        f"implementation_head = {toml_string(IMPLEMENTATION_HEAD)}",
        f"integrated_commit = {toml_string(INTEGRATED_COMMIT)}",
        "verifier = " + toml_string(
            "Post-merge cold-verification workflow in a fresh GitHub-hosted runner, "
            "separate execution environment from the PR #386 builder; initiated by "
            f"the continuation session; {runner}"
        ),
        'result = "pass"',
        f"workflow_run = {run_id}",
        f"workflow_attempt = {run_attempt}",
        "",
        "commands = [",
    ]
    lines.extend(f"  {toml_string(command)}," for command in commands_common)
    lines.extend(["]", "", "runtime_proofs = ["])
    lines.extend(f"  {toml_string(proof)}," for proof in runtime_proofs)
    lines.extend(
        [
            "]",
            "",
            "source_inspection_only = [",
            "  "
            + toml_string(
                "The verifier confirmed the transaction engine uses schema-versioned "
                "typed records rooted at /var/lib/optid/recovery and an explicit stable "
                "FNV-1a filename hash."
            )
            + ",",
            "  "
            + toml_string(
                "The verifier confirmed completed F2 proof files are not modified by "
                "the integrated S2D/F4 change."
            )
            + ",",
            "]",
            "",
            "unresolved = []",
            "",
        ]
    )
    return "\n".join(lines)


verification_dir = ROOT / "docs/plans/optid-verification"
verification_dir.mkdir(parents=True, exist_ok=True)
(verification_dir / "f4.toml").write_text(
    render_receipt("F4", 371, f4_runtime_proofs), encoding="utf-8"
)
(verification_dir / "s2d.toml").write_text(
    render_receipt("S2D", 386, s2d_runtime_proofs), encoding="utf-8"
)

ledger_path = ROOT / "docs/plans/optid-package-status.toml"
ledger = ledger_path.read_text(encoding="utf-8")
ledger = re.sub(
    r'^updated = "[^"]+"$',
    'updated = "2026-08-04"',
    ledger,
    count=1,
    flags=re.MULTILINE,
)
ledger = re.sub(
    r'# Independent cold-verification completed F1, F2, F3, and F4.*?active_general = "T1"',
    "# Independent post-merge cold verification now covers F1-F4 and S1D-S2D.\n"
    "# PR #386 integrated S2D into the F4 reconciliation surface; one fresh\n"
    "# read-only verifier re-certified F4 and completed S2D against the merged\n"
    "# commit. S3D is now the active safety package. No package self-certified.\n"
    "#\n"
    "# active_general remains T1 because its physical hardware proof and reviewed\n"
    "# threshold acceptance are still outstanding. R1-R3 remain independent.\n"
    'active_general = "T1"',
    ledger,
    count=1,
    flags=re.DOTALL,
)
ledger = ledger.replace('active_safety = "S2D"', 'active_safety = "S3D"', 1)


def package_block(text: str, package_id: str) -> tuple[re.Match[str], str]:
    match = re.search(
        rf'(?ms)^\[\[package\]\]\nid = "{re.escape(package_id)}"\n.*?(?=^\[\[package\]\]|\Z)',
        text,
    )
    if not match:
        raise SystemExit(f"missing package block {package_id}")
    return match, match.group(0)


match, f4 = package_block(ledger, "F4")
if 'status = "merged_incomplete"' not in f4:
    raise SystemExit("unexpected F4 status")
f4 = f4.replace('status = "merged_incomplete"', 'status = "completed"', 1)
f4 = re.sub(r'^blocking_reason = ".*"\n', "", f4, count=1, flags=re.MULTILINE)
if 'verification_receipt = ' not in f4:
    f4 = f4.replace(
        'pr = "371"\n',
        'pr = "371"\nverification_receipt = "docs/plans/optid-verification/f4.toml"\n',
        1,
    )
ledger = ledger[: match.start()] + f4 + ledger[match.end() :]

match, s2d = package_block(ledger, "S2D")
if 'status = "candidate"' not in s2d or 'pr = "386"' not in s2d:
    raise SystemExit("unexpected S2D state")
s2d = s2d.replace('status = "candidate"', 'status = "completed"', 1)
if 'verification_receipt = ' not in s2d:
    s2d = s2d.replace(
        'pr = "386"\n',
        'pr = "386"\nverification_receipt = "docs/plans/optid-verification/s2d.toml"\n',
        1,
    )
ledger = ledger[: match.start()] + s2d + ledger[match.end() :]

match, s3d = package_block(ledger, "S3D")
if 'status = "planned"' not in s3d:
    raise SystemExit("unexpected S3D state")
s3d = s3d.replace('status = "planned"', 'status = "next"', 1)
ledger = ledger[: match.start()] + s3d + ledger[match.end() :]
ledger_path.write_text(ledger, encoding="utf-8")

current_path = ROOT / "docs/plans/current-work.md"
current = current_path.read_text(encoding="utf-8")
current = current.replace('active_safety = "S2D"', 'active_safety = "S3D"', 1)
current = current.replace(
    'other_merged_incomplete = ["F4"]',
    'other_merged_incomplete = []',
    1,
)
current = current.replace(
    'unlocks_after_active_safety = ["S3D"]',
    'unlocks_after_active_safety = ["S4D"]',
    1,
)
current_path.write_text(current, encoding="utf-8")
