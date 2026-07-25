#!/usr/bin/env python3
"""Validate truthful optid package state and package-shaped code changes.

This gate deliberately separates three facts:

* code was merged;
* the package end state is integrated into a production entry point; and
* an independent verifier accepted the package.

Compilation and unit tests prove none of those facts by themselves.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
LEDGER = Path("docs/plans/optid-package-status.toml")
OPTID_CODE_PREFIXES = (
    "crates/optid/src/",
    "crates/optid/tests/",
    "crates/optctl/src/",
)
VALID_STATUSES = {
    "next",
    "ready_parallel",
    "planned",
    "candidate",
    "merged_incomplete",
    "completed",
    "blocked",
}
ACTIVE_STATUSES = {"next", "ready_parallel", "candidate", "merged_incomplete"}
PROOF_STATUSES = {"candidate", "completed"}
RUNTIME_SURFACE_PREFIXES = (
    "crates/optid/src/main.rs",
    "crates/optid/src/bin/",
    "crates/optctl/src/",
    "packaging/systemd/",
    "tools/optid",
)
INTEGRATION_TEST_PREFIXES = (
    "crates/optid/tests/",
    "crates/optctl/tests/",
    "tools/test-",
    "testos/",
)
# Binary-crate exception: `optid` is a `[[bin]]`-only crate, so production
# paths are `pub(crate)` and cannot be reached from `tests/` integration
# tests. Behavioral coverage for these packages lives inside the source
# modules under `#[cfg(test)] mod tests`. The validator accepts source
# modules listed in `runtime_entrypoints` or `completion_evidence` as
# integration-test locations for this reason — but only when an explicit
# `acceptance_tests` mapping proves the named `#[test] fn` definitions
# actually exist there (see `validate_acceptance_test_mapping`).
BINARY_CRATE_SOURCE_PREFIXES = (
    "crates/optid/src/",
    "crates/optctl/src/",
)
PR_RE = re.compile(r"^[1-9][0-9]*$")
SHA_RE = re.compile(r"^[0-9a-f]{40}$")


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def package_map(ledger: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {
        str(package.get("id")): package
        for package in ledger.get("package", [])
        if package.get("id")
    }


def _path_list(
    package_id: str,
    package: dict[str, Any],
    field: str,
    errors: list[str],
) -> list[str]:
    values = package.get(field, [])
    if not isinstance(values, list) or not values:
        errors.append(f"{package_id}: {field} must be a non-empty list")
        return []
    clean: list[str] = []
    for value in values:
        if not isinstance(value, str) or not value.strip():
            errors.append(f"{package_id}: {field} contains an empty/non-string value")
            continue
        clean.append(value)
    return clean


def validate_verification_receipt(
    package_id: str,
    package: dict[str, Any],
    root: Path,
    errors: list[str],
) -> None:
    receipt_value = package.get("verification_receipt", "")
    if not isinstance(receipt_value, str) or not receipt_value:
        errors.append(f"{package_id}: completed status requires verification_receipt")
        return

    receipt_path = Path(receipt_value)
    expected_parent = Path("docs/plans/optid-verification")
    if expected_parent not in receipt_path.parents:
        errors.append(
            f"{package_id}: verification_receipt must be under {expected_parent}"
        )
        return
    absolute = root / receipt_path
    if not absolute.is_file():
        errors.append(f"{package_id}: verification receipt does not exist: {receipt_value}")
        return

    try:
        receipt = load_toml(absolute)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        errors.append(f"{package_id}: verification receipt cannot be parsed: {exc}")
        return

    if receipt.get("package") != package_id:
        errors.append(f"{package_id}: verification receipt names another package")
    if str(receipt.get("implementation_pr", "")) != str(package.get("pr", "")):
        errors.append(f"{package_id}: verification receipt PR does not match ledger")
    if receipt.get("result") != "pass":
        errors.append(f"{package_id}: verification receipt result must be 'pass'")
    if not SHA_RE.fullmatch(str(receipt.get("verified_commit", ""))):
        errors.append(f"{package_id}: receipt verified_commit must be a full SHA")
    if not str(receipt.get("verifier", "")).strip():
        errors.append(f"{package_id}: receipt must identify the cold verifier")
    if not receipt.get("commands"):
        errors.append(f"{package_id}: receipt must record exact verification commands")
    if not receipt.get("runtime_proofs"):
        errors.append(f"{package_id}: receipt must record production-path runtime proofs")
    if receipt.get("unresolved"):
        errors.append(f"{package_id}: completed receipt cannot have unresolved findings")

    # Post-#337 freshness check: a completed package's receipt must be
    # invalidated when a later change modifies any declared proof path.
    # See `validate_receipt_freshness` for the full rule.
    validate_receipt_freshness(package_id, package, receipt, root, errors)


def _git_ancestry_contains(verified_commit: str, descendant: str, root: Path) -> bool:
    """Return True if `verified_commit` is an ancestor of `descendant`
    (or equal to it). Uses `git merge-base --is-ancestor`.
    """
    result = subprocess.run(
        ["git", "merge-base", "--is-ancestor", verified_commit, descendant],
        cwd=root,
        check=False,
        capture_output=True,
        timeout=30,
    )
    return result.returncode == 0


def _files_changed_since_commit(
    verified_commit: str, paths: list[str], root: Path
) -> list[str]:
    """Return the subset of `paths` that were modified between
    `verified_commit` (exclusive) and HEAD (inclusive). Uses
    `git diff --name-only verified_commit..HEAD -- <paths>`.
    """
    if not paths:
        return []
    result = subprocess.run(
        ["git", "diff", "--name-only", f"{verified_commit}..HEAD", "--", *paths],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    if result.returncode != 0:
        # If the commit is not in the repo (e.g. shallow clone), fail
        # closed: treat every declared path as potentially stale.
        return list(paths)
    return [line for line in result.stdout.splitlines() if line]


def validate_receipt_freshness(
    package_id: str,
    package: dict[str, Any],
    receipt: dict[str, Any],
    root: Path,
    errors: list[str],
) -> None:
    """Post-#337 rule: a completed package's receipt must be invalidated
    when a later change modifies any declared runtime_entrypoint,
    integration_test, or completion_evidence implementation file, unless
    a newer receipt verifies a commit containing that change.

    Uses Git ancestry: if `verified_commit` is an ancestor of HEAD and
    any declared proof path changed between `verified_commit` and HEAD,
    the receipt is stale and the package cannot be `completed`.

    This rule catches the F1 stale-receipt defect: PR #332's receipt
    verified commit `001515b`, but subsequent PRs (#333, #334, #336,
    #337) modified `policy.rs`, `action.rs`, `actuator.rs`, etc. — all
    declared F1 runtime entrypoints. The receipt continued to assert
    `result = "pass"` against an older commit, masking the regression.
    """
    verified_commit = str(receipt.get("verified_commit", "")).strip()
    if not SHA_RE.fullmatch(verified_commit):
        return  # already flagged by `validate_verification_receipt`

    # If the verified commit is not an ancestor of HEAD (e.g. it lives
    # on an unmerged branch), the freshness check is moot — the receipt
    # is for a divergent history. Skip rather than false-positive.
    if not _git_ancestry_contains(verified_commit, "HEAD", root):
        return

    # Collect every declared proof path. A change to any of them after
    # the verified commit invalidates the receipt.
    proof_paths: list[str] = []
    for field in ("runtime_entrypoints", "integration_tests", "completion_evidence"):
        for value in package.get(field, []) or []:
            if isinstance(value, str) and value.strip():
                proof_paths.append(value.strip())
    if not proof_paths:
        return

    stale = _files_changed_since_commit(verified_commit, proof_paths, root)
    if stale:
        errors.append(
            f"{package_id}: verification receipt is stale — verified_commit "
            f"{verified_commit[:12]} is an ancestor of HEAD but the following "
            f"declared proof paths were modified after it: {', '.join(stale)}. "
            "A fresh cold verification receipt is required before this package "
            "may remain `completed`. Demote to `merged_incomplete` and record "
            "the precise blocker in `blocking_reason`."
        )


def validate_ledger(ledger: dict[str, Any], root: Path = ROOT) -> list[str]:
    errors: list[str] = []
    packages = ledger.get("package", [])
    ids = [package.get("id") for package in packages]
    packages_by_id = package_map(ledger)

    if ledger.get("schema_version") != 2:
        errors.append("ledger schema_version must be 2")
    if len(packages) != 30 or len(set(ids)) != 30:
        errors.append("ledger must contain exactly 30 unique packages")

    for package in packages:
        package_id = str(package.get("id", "<missing>"))
        status = package.get("status")
        if status not in VALID_STATUSES:
            errors.append(f"{package_id}: invalid status {status!r}")
            continue
        if not str(package.get("title", "")).strip():
            errors.append(f"{package_id}: title/outcome is required")

        dependencies = package.get("depends", [])
        if not isinstance(dependencies, list):
            errors.append(f"{package_id}: depends must be a list")
            dependencies = []
        for dependency in dependencies:
            if dependency not in packages_by_id:
                errors.append(f"{package_id}: unknown dependency {dependency!r}")
            elif dependency == package_id:
                errors.append(f"{package_id}: cannot depend on itself")

        if status == "merged_incomplete":
            if not PR_RE.fullmatch(str(package.get("pr", ""))):
                errors.append(f"{package_id}: merged_incomplete requires a numeric PR")
            if not str(package.get("blocking_reason", "")).strip():
                errors.append(f"{package_id}: merged_incomplete requires blocking_reason")

        # Dependency gate: candidate/completed require all depends completed.
        if status in PROOF_STATUSES:
            incomplete_dependencies = [
                dependency
                for dependency in dependencies
                if packages_by_id.get(dependency, {}).get("status") != "completed"
            ]
            if incomplete_dependencies:
                errors.append(
                    f"{package_id}: status {status!r} while dependencies are incomplete: "
                    + ", ".join(incomplete_dependencies)
                    + f" (package {package_id} depends on incomplete: "
                    + ", ".join(incomplete_dependencies)
                    + ")"
                )

        if status in PROOF_STATUSES:
            entrypoints = _path_list(package_id, package, "runtime_entrypoints", errors)
            integration_tests = _path_list(
                package_id, package, "integration_tests", errors
            )
            evidence = _path_list(package_id, package, "completion_evidence", errors)
            for value in entrypoints + integration_tests + evidence:
                if not (root / value).exists():
                    errors.append(f"{package_id}: declared proof path does not exist: {value}")
            if entrypoints and not any(
                value.startswith(RUNTIME_SURFACE_PREFIXES) for value in entrypoints
            ):
                errors.append(
                    f"{package_id}: runtime_entrypoints must include a production "
                    "daemon, CLI, service, or executable surface"
                )
            for value in integration_tests:
                if not (
                    value.startswith(INTEGRATION_TEST_PREFIXES)
                    # Binary-crate exception: source modules under
                    # crates/optid/src/ (or optctl/src/) may be declared
                    # as integration_tests when the package has an
                    # explicit acceptance_tests mapping proving the
                    # named #[test] fn definitions exist there. The
                    # mapping is validated below; the prefix exemption
                    # only allows the file to be *listed*.
                    or (
                        value.startswith(BINARY_CRATE_SOURCE_PREFIXES)
                        and isinstance(package.get("acceptance_tests"), dict)
                        and package.get("acceptance_tests")
                    )
                ):
                    errors.append(
                        f"{package_id}: integration test is not outside the "
                        f"implementation module: {value}"
                    )

            # Candidate-only structural + acceptance gates. Completed packages
            # retain their historical evidence model; new candidates must not
            # claim production integration via include_str(src)+.contains.
            if status == "candidate":
                for value in integration_tests:
                    detect_structural_integration_test(
                        package_id, value, root, errors
                    )
                validate_acceptance_test_mapping(package_id, package, root, errors)

        if status == "completed":
            if not PR_RE.fullmatch(str(package.get("pr", ""))):
                errors.append(f"{package_id}: completed status requires a numeric PR")
            validate_verification_receipt(package_id, package, root, errors)

    for key in ("active_general", "active_safety"):
        active_id = ledger.get(key)
        if active_id not in packages_by_id:
            errors.append(f"{key} names unknown package {active_id!r}")
            continue
        status = packages_by_id[active_id].get("status")
        if status not in ACTIVE_STATUSES:
            errors.append(
                f"{key}={active_id} has non-actionable status {status!r}; "
                f"expected one of {sorted(ACTIVE_STATUSES)}"
            )

    return errors


def changed_files(base: str, root: Path = ROOT) -> list[str]:
    result = subprocess.run(
        ["git", "diff", "--name-only", f"{base}...HEAD"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or f"git diff failed for {base}")
    return [line for line in result.stdout.splitlines() if line]


def dead_code_allows_in_diff(diff: str) -> list[str]:
    findings: list[str] = []
    current_file = "<unknown>"
    for line in diff.splitlines():
        if line.startswith("+++ b/"):
            current_file = line[6:]
        elif line.startswith("+") and not line.startswith("+++"):
            if re.search(r"(allow|expect)\s*\(\s*dead_code\s*\)", line):
                findings.append(f"{current_file}: {line[1:].strip()}")
    return findings


def added_dead_code_allows(base: str, root: Path = ROOT) -> list[str]:
    result = subprocess.run(
        [
            "git",
            "diff",
            "--unified=0",
            f"{base}...HEAD",
            "--",
            "crates/optid/src",
            "crates/optctl/src",
        ],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or f"git diff failed for {base}")
    return dead_code_allows_in_diff(result.stdout)


CLAIM_FIELDS = {
    "status",
    "pr",
    "blocking_reason",
    "runtime_entrypoints",
    "integration_tests",
    "completion_evidence",
    "verification_receipt",
}

# Package identity / plan fields: may not change silently in an
# implementation PR without an accepted plan/spec change in the same diff.
PROTECTED_DEFINITION_FIELDS = {
    "id",
    "lane",
    "title",
    "depends",
    "promotion",
}

PLAN_SPEC_PATHS = (
    "OPTID-COMPLETION-PLAN.md",
    "docs/architecture/optid-d2-amendment.md",
)


def _load_base_ledger(base: str, root: Path = ROOT) -> dict[str, Any]:
    result = subprocess.run(
        ["git", "show", f"{base}:{LEDGER.as_posix()}"],
        cwd=root,
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            result.stderr.decode(errors="replace").strip()
            or f"cannot read ledger from {base}"
        )
    return tomllib.loads(result.stdout.decode())


def ledger_claim_changes(base: str, root: Path = ROOT) -> list[str]:
    previous = _load_base_ledger(base, root)
    current = load_toml(root / LEDGER)
    before = package_map(previous)
    after = package_map(current)
    return sorted(
        package_id
        for package_id in set(before) | set(after)
        if any(
            before.get(package_id, {}).get(field)
            != after.get(package_id, {}).get(field)
            for field in CLAIM_FIELDS
        )
    )


def ledger_definition_changes(base: str, root: Path = ROOT) -> list[str]:
    """Return human-readable protected-field changes vs base."""
    previous = _load_base_ledger(base, root)
    current = load_toml(root / LEDGER)
    before = package_map(previous)
    after = package_map(current)
    findings: list[str] = []
    for package_id in sorted(set(before) | set(after)):
        for field in PROTECTED_DEFINITION_FIELDS:
            old = before.get(package_id, {}).get(field)
            new = after.get(package_id, {}).get(field)
            if old != new:
                findings.append(f"{package_id}.{field}: {old!r} → {new!r}")
    return findings


def detect_structural_integration_test(
    package_id: str,
    test_path: str,
    root: Path,
    errors: list[str],
) -> None:
    """Reject integration tests that claim production integration via
    include_str of src + .contains only (not parser fixtures / golden files).
    """
    path = root / test_path
    if not path.is_file():
        return
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return

    # Narrow: only flag files under crates/optid/tests/ (or optctl) that
    # include production source modules and assert with .contains on them.
    include_src = re.findall(
        r'include_str!\s*\(\s*"(?:\.\./)+src/[^"]+\.rs"\s*\)',
        text,
    )
    if not include_src:
        return

    # Count .contains assertions on the included constants / source text.
    contains_asserts = len(re.findall(r"\.contains\s*\(", text))
    # Behavioral calls that indicate a real production-path test.
    behavioral_signals = (
        "collect_with",
        "compute_thermal_budget",
        "discover_thermal",
        "fits_contract",
        "contract_gate",
        "MemoryKernel",
        "FaultKernel",
        "Actuator::",
        "Policy::",
        "Snapshot::",
    )
    has_behavioral = any(sig in text for sig in behavioral_signals)

    # Primary structural pattern: multiple include_str of src + several
    # .contains and no behavioral production-path exercise.
    if len(include_src) >= 2 and contains_asserts >= 3 and not has_behavioral:
        errors.append(
            f"{package_id}: integration test {test_path} claims production "
            "integration primarily via include_str(src)+.contains; replace "
            "with behavioral tests through injected kernel I/O or real "
            "production entrypoints"
        )


def _is_pointer_only_test_file(text: str) -> bool:
    """Detect a file that claims to be behavioral evidence but only
    contains name strings and list-length assertions.

    A pointer file has all of:
      - one or more `&[&str]` constants naming tests in *other* files;
      - one or more `assert!(... .len() >= N)` or `assert!(!name.is_empty())`
        style checks;
      - no `#[test] fn` definitions of its own that exercise production
        behavior (the only `#[test] fn` it has is the length/uniqueness
        assertion itself).

    Such a file is not behavioral evidence — it is a claim about evidence
    that lives elsewhere. The post-#337 repair rejects this pattern.
    """
    has_str_array = bool(re.search(r"&\[&str\]|Vec<&str>", text))
    has_length_assert = bool(re.search(r"\.len\(\)\s*>=\s*\d+", text))
    # Count #[test] fn definitions in the file.
    test_fns = re.findall(r"#\[test\]\s*(?:async\s+)?fn\s+(\w+)", text)
    # A real behavioral test file has at least one #[test] fn whose body
    # calls a production-path function (Policy::, Snapshot::, Actuator::,
    # compute_thermal_budget, contract_gate_*, etc.). A pointer file's
    # only #[test] fn is the length/uniqueness assertion, whose body
    # contains no production-path call.
    behavioral_signals = (
        "collect_with",
        "compute_thermal_budget",
        "discover_thermal",
        "fits_contract",
        "contract_gate",
        "MemoryKernel",
        "FaultKernel",
        "Actuator::",
        "Policy::",
        "Snapshot::",
        "decide_resolved",
        "EffectiveConfig::",
        "Decision::render",
    )
    has_behavioral_test = any(
        sig in text for sig in behavioral_signals
    ) and len(test_fns) >= 1
    return has_str_array and has_length_assert and not has_behavioral_test


def validate_acceptance_test_mapping(
    package_id: str,
    package: dict[str, Any],
    root: Path,
    errors: list[str],
) -> None:
    """Candidate packages must declare an explicit acceptance→test name
    mapping whose referenced `#[test] fn` definitions actually exist in
    the declared evidence/integration-test/runtime-entrypoint files.

    Post-#337 strengthening:
      - The loose "any `#[test] fn` exists" fallback is removed. A
        pointer file (only name strings + list-length assertions) cannot
        satisfy the mapping.
      - Each named test must be a real `#[test] fn` (not just any `fn`)
        in one of the declared files.
      - Source-text `.contains(...)` tests remain rejected by
        `detect_structural_integration_test`.
    """
    mapping = package.get("acceptance_tests", {})
    if not isinstance(mapping, dict) or not mapping:
        errors.append(
            f"{package_id}: candidate requires an explicit acceptance_tests "
            "mapping (acceptance requirement → #[test] fn name); the loose "
            "'any #[test] fn exists' fallback was removed in the post-#337 "
            "repair because pointer files satisfied it without exercising "
            "production behavior"
        )
        return

    # Build a per-file corpus so we can both (a) confirm each named test
    # is a real #[test] fn and (b) reject pointer-only files.
    search_roots: list[Path] = []
    for field in ("integration_tests", "completion_evidence", "runtime_entrypoints"):
        for value in package.get(field, []) or []:
            p = root / str(value)
            if p.is_file():
                search_roots.append(p)
            elif p.is_dir():
                search_roots.extend(p.rglob("*.rs"))

    file_texts: list[tuple[str, str]] = []
    for path in search_roots:
        try:
            file_texts.append((str(path.relative_to(root)), path.read_text(encoding="utf-8", errors="replace")))
        except (OSError, ValueError):
            continue

    corpus = "\n".join(text for _, text in file_texts)

    # Reject pointer-only files declared as integration_tests or
    # completion_evidence. A pointer file claims to be behavioral
    # evidence but only contains name strings and list-length assertions.
    for rel, text in file_texts:
        if _is_pointer_only_test_file(text):
            errors.append(
                f"{package_id}: declared evidence {rel} is a pointer file "
                "(only name strings and list-length assertions, no "
                "production-path behavioral calls); replace with real "
                "behavioral tests or point at the source module that "
                "contains the #[test] fn definitions"
            )

    for requirement, test_name in mapping.items():
        if not isinstance(test_name, str) or not test_name.strip():
            errors.append(
                f"{package_id}: acceptance_tests[{requirement!r}] must name a test"
            )
            continue
        name = test_name.strip()
        # The named test must be a real #[test] fn (not just any fn) in
        # the corpus. A bare `fn name(...)` definition that is not a
        # #[test] fn does not satisfy acceptance evidence.
        test_pattern = rf"#\[test\]\s*(?:async\s+)?fn\s+{re.escape(name)}\s*\("
        if not re.search(test_pattern, corpus):
            # Distinguish "name not found at all" from "name found but
            # not a #[test] fn" so the error message is actionable.
            bare_pattern = rf"\bfn\s+{re.escape(name)}\s*\("
            if re.search(bare_pattern, corpus):
                errors.append(
                    f"{package_id}: acceptance requirement {requirement!r} maps to "
                    f"{name!r} but it is not a #[test] fn (a bare fn does not "
                    "satisfy acceptance evidence)"
                )
            else:
                errors.append(
                    f"{package_id}: acceptance requirement {requirement!r} maps to "
                    f"test {name!r} which was not found as a #[test] fn in declared "
                    "evidence paths"
                )


def validate_change(base: str, root: Path = ROOT) -> list[str]:
    errors: list[str] = []
    files = changed_files(base, root)
    optid_code_changed = any(
        path.startswith(OPTID_CODE_PREFIXES) for path in files
    )

    if optid_code_changed:
        if LEDGER.as_posix() not in files:
            errors.append(
                "optid production/test code changed without updating "
                "docs/plans/optid-package-status.toml"
            )
        else:
            claims = ledger_claim_changes(base, root)
            if not claims:
                errors.append(
                    "optid code changed, but no package status/proof claim changed"
                )
            else:
                # One package may advance status/proof. Other packages may
                # only refine blocking_reason (honest residual blockers).
                previous = _load_base_ledger(base, root)
                current = load_toml(root / LEDGER)
                before = package_map(previous)
                after = package_map(current)
                advancing: list[str] = []
                for package_id in claims:
                    non_blocker = any(
                        before.get(package_id, {}).get(field)
                        != after.get(package_id, {}).get(field)
                        for field in CLAIM_FIELDS
                        if field != "blocking_reason"
                    )
                    if non_blocker:
                        advancing.append(package_id)
                if len(advancing) > 1:
                    errors.append(
                        "one optid implementation PR may advance only one package; "
                        f"changed claims: {', '.join(advancing)}"
                    )

        for finding in added_dead_code_allows(base, root):
            errors.append(
                "new dead-code suppression is forbidden in production optid code: "
                + finding
            )

    # Package-definition protection: id/lane/title/depends/promotion
    # require an explicit accepted plan/spec change in the same PR.
    if LEDGER.as_posix() in files:
        def_changes = ledger_definition_changes(base, root)
        if def_changes:
            plan_touched = any(
                any(path == plan or path.startswith(plan + "/") for plan in PLAN_SPEC_PATHS)
                or path == plan
                for path in files
                for plan in PLAN_SPEC_PATHS
            )
            # Also accept a direct edit note in OPTID-COMPLETION-PLAN.md
            if not plan_touched:
                errors.append(
                    "package definition fields (id/lane/title/depends/promotion) "
                    "changed without an accepted plan/spec change in the same PR "
                    f"({', '.join(def_changes)}); touch OPTID-COMPLETION-PLAN.md "
                    "or docs/architecture/optid-d2-amendment.md to record the decision"
                )

    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--base",
        help="git base ref for change-aware checks (for example origin/main)",
    )
    args = parser.parse_args()

    try:
        ledger = load_toml(ROOT / LEDGER)
        errors = validate_ledger(ledger)
        if args.base:
            errors.extend(validate_change(args.base))
    except (OSError, tomllib.TOMLDecodeError, RuntimeError) as exc:
        errors = [str(exc)]

    if errors:
        print("FAILED: optid package contract")
        for error in errors:
            print(f"  - {error}")
        return 1

    print("PASS: optid package ledger and change contract are truthful")
    return 0


if __name__ == "__main__":
    sys.exit(main())
