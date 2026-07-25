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
                if not value.startswith(INTEGRATION_TEST_PREFIXES):
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


def validate_acceptance_test_mapping(
    package_id: str,
    package: dict[str, Any],
    root: Path,
    errors: list[str],
) -> None:
    """Candidate packages need a minimal acceptance→test name mapping
    whose referenced tests exist (in listed integration tests or src).
    """
    mapping = package.get("acceptance_tests", {})
    if not isinstance(mapping, dict) or not mapping:
        # Allow mapping embedded in completion_evidence files as a simple
        # alternative: require at least one integration test file that
        # defines `#[test] fn` names (existence check only).
        tests = package.get("integration_tests", [])
        if not isinstance(tests, list) or not tests:
            errors.append(
                f"{package_id}: candidate requires acceptance_tests mapping "
                "or integration_tests with real #[test] functions"
            )
            return
        found_any = False
        for test_path in tests:
            absolute = root / str(test_path)
            if absolute.is_file():
                text = absolute.read_text(encoding="utf-8", errors="replace")
                if re.search(r"#\[test\]\s*(?:async\s+)?fn\s+\w+", text):
                    found_any = True
                    break
        if not found_any:
            errors.append(
                f"{package_id}: candidate integration_tests must contain "
                "at least one #[test] fn (behavioral mapping)"
            )
        return

    # Explicit mapping: acceptance requirement → test function name.
    search_roots: list[Path] = []
    for field in ("integration_tests", "completion_evidence", "runtime_entrypoints"):
        for value in package.get(field, []) or []:
            p = root / str(value)
            if p.is_file():
                search_roots.append(p)
            elif p.is_dir():
                search_roots.extend(p.rglob("*.rs"))

    corpus = ""
    for path in search_roots:
        try:
            corpus += path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue

    for requirement, test_name in mapping.items():
        if not isinstance(test_name, str) or not test_name.strip():
            errors.append(
                f"{package_id}: acceptance_tests[{requirement!r}] must name a test"
            )
            continue
        pattern = rf"fn\s+{re.escape(test_name.strip())}\s*\("
        if not re.search(pattern, corpus):
            errors.append(
                f"{package_id}: acceptance requirement {requirement!r} maps to "
                f"test {test_name!r} which was not found in declared evidence paths"
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
