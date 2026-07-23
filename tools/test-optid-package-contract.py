from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "validate_optid_packages", ROOT / "tools" / "validate-optid-packages.py"
)
assert SPEC and SPEC.loader
validator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(validator)


def package(
    package_id: str,
    *,
    lane: str = "foundation",
    status: str = "planned",
    depends: list[str] | None = None,
    **extra,
):
    value = {
        "id": package_id,
        "lane": lane,
        "title": f"{package_id} outcome",
        "status": status,
        "depends": depends or [],
        "pr": "",
        "completion_evidence": [],
    }
    value.update(extra)
    return value


def ledger(first, second=None):
    packages = [first, second or package("D0", lane="safety", status="next")]
    packages.extend(package(f"P{number}") for number in range(1, 29))
    return {
        "schema_version": 2,
        "active_general": first["id"],
        "active_safety": (second or packages[1])["id"],
        "package": packages,
    }


class OptidPackageContractTests(unittest.TestCase):
    def test_active_pointer_is_dynamic_not_hard_coded(self):
        data = ledger(package("F2", status="next"))
        self.assertEqual(validator.validate_ledger(data, ROOT), [])

    def test_merged_incomplete_requires_pr_and_root_cause(self):
        data = ledger(package("F1", status="merged_incomplete"))
        errors = validator.validate_ledger(data, ROOT)
        self.assertTrue(any("numeric PR" in error for error in errors))
        self.assertTrue(any("blocking_reason" in error for error in errors))

    def test_merged_incomplete_is_honest_and_does_not_unlock_dependency(self):
        first = package(
            "F1",
            status="merged_incomplete",
            pr="324",
            blocking_reason="Runtime path is not integrated.",
        )
        data = ledger(first)
        self.assertEqual(validator.validate_ledger(data, ROOT), [])

    def test_candidate_requires_runtime_and_integration_proof_paths(self):
        data = ledger(package("F1", status="candidate"))
        errors = validator.validate_ledger(data, ROOT)
        self.assertTrue(any("runtime_entrypoints" in error for error in errors))
        self.assertTrue(any("integration_tests" in error for error in errors))
        self.assertTrue(any("completion_evidence" in error for error in errors))

    def test_completed_requires_cold_verification_receipt(self):
        first = package(
            "F1",
            status="completed",
            pr="324",
            runtime_entrypoints=["AGENTS.md"],
            integration_tests=["AGENTS.md"],
            completion_evidence=["AGENTS.md"],
        )
        data = ledger(first)
        errors = validator.validate_ledger(data, ROOT)
        self.assertTrue(any("verification_receipt" in error for error in errors))

    def test_module_only_candidate_is_rejected(self):
        first = package(
            "F3",
            status="candidate",
            runtime_entrypoints=["crates/optid/src/envelope.rs"],
            integration_tests=["crates/optid/src/envelope.rs"],
            completion_evidence=["crates/optid/src/envelope.rs"],
        )
        data = ledger(first)
        errors = validator.validate_ledger(data, ROOT)
        self.assertTrue(any("production daemon" in error for error in errors))
        self.assertTrue(any("not outside" in error for error in errors))

    def test_completed_package_cannot_skip_dependency(self):
        dependency = package("F1", status="planned")
        completed = package(
            "F2",
            status="completed",
            depends=["F1"],
            pr="325",
            runtime_entrypoints=["AGENTS.md"],
            integration_tests=["AGENTS.md"],
            completion_evidence=["AGENTS.md"],
        )
        data = ledger(completed, dependency)
        errors = validator.validate_ledger(data, ROOT)
        self.assertTrue(
            any("dependencies are incomplete: F1" in error for error in errors)
        )

    def test_new_dead_code_suppression_is_detected(self):
        diff = """\
diff --git a/crates/optid/src/new.rs b/crates/optid/src/new.rs
+++ b/crates/optid/src/new.rs
@@ -0,0 +1 @@
+#![allow(dead_code)]
"""
        self.assertEqual(
            validator.dead_code_allows_in_diff(diff),
            ["crates/optid/src/new.rs: #![allow(dead_code)]"],
        )


if __name__ == "__main__":
    unittest.main()
