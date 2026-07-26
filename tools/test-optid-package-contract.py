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

    # ── Post-#337: evidence-pointer rejection ──────────────────────────

    def test_pointer_only_test_file_is_detected(self):
        # A file that only contains a &[&str] constant naming tests in
        # other files plus a list-length assertion is a pointer file,
        # not behavioral evidence.
        pointer_text = """\
const TESTS: &[&str] = &["real_test_one", "real_test_two"];

#[test]
fn matrix_has_required_cases() {
    assert!(TESTS.len() >= 2);
    for name in TESTS {
        assert!(!name.is_empty());
    }
}
"""
        self.assertTrue(validator._is_pointer_only_test_file(pointer_text))

    def test_behavioral_test_file_is_not_pointer(self):
        # A file that calls a production-path function (Policy::, etc.)
        # is real behavioral evidence, not a pointer file.
        behavioral_text = """\
#[test]
fn real_test_one() {
    let policy = Policy::default();
    let snapshot = Snapshot::collect();
    let decision = policy.decide_resolved(&snapshot, Mode::Auto, WorkloadClass::Idle, "reason".into(), &Contracts::default(), Some(Mode::Balanced), None);
    assert!(!decision.actions.is_empty());
}
"""
        self.assertFalse(validator._is_pointer_only_test_file(behavioral_text))

    def test_candidate_rejects_pointer_only_integration_test(self):
        # A candidate that declares a pointer file as integration_tests
        # is rejected even if an acceptance_tests mapping exists,
        # because the pointer file is not behavioral evidence.
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_root = Path(tmp)
            (tmp_root / "crates" / "optid" / "tests").mkdir(parents=True)
            pointer = tmp_root / "crates" / "optid" / "tests" / "pointer.rs"
            pointer.write_text(
                """\
const TESTS: &[&str] = &["real_test_one"];

#[test]
fn matrix_has_required_cases() {
    assert!(TESTS.len() >= 1);
}
""",
                encoding="utf-8",
            )
            first = package(
                "F1",
                status="candidate",
                pr="324",
                runtime_entrypoints=["crates/optid/src/main.rs"],
                integration_tests=["crates/optid/tests/pointer.rs"],
                completion_evidence=["crates/optid/tests/pointer.rs"],
                acceptance_tests={"behavior": "real_test_one"},
            )
            data = ledger(first)
            errors = validator.validate_ledger(data, tmp_root)
            self.assertTrue(
                any("pointer file" in error for error in errors),
                f"expected pointer-file rejection, got: {errors}",
            )

    def test_candidate_rejects_missing_acceptance_mapping(self):
        # A candidate without an explicit acceptance_tests mapping is
        # rejected — the loose "any #[test] fn exists" fallback was
        # removed in the post-#337 repair.
        first = package(
            "F1",
            status="candidate",
            pr="324",
            runtime_entrypoints=["crates/optid/src/main.rs"],
            integration_tests=["crates/optid/tests/real.rs"],
            completion_evidence=["crates/optid/tests/real.rs"],
        )
        data = ledger(first)
        errors = validator.validate_ledger(data, ROOT)
        self.assertTrue(
            any("acceptance_tests mapping" in error for error in errors),
            f"expected missing-mapping rejection, got: {errors}",
        )

    def test_candidate_rejects_acceptance_name_not_test_fn(self):
        # An acceptance_tests mapping that names a bare fn (not a
        # #[test] fn) is rejected.
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_root = Path(tmp)
            (tmp_root / "crates" / "optid" / "tests").mkdir(parents=True)
            test_file = tmp_root / "crates" / "optid" / "tests" / "real.rs"
            test_file.write_text(
                """\
fn helper_not_a_test() {}

#[test]
fn real_test_one() {
    assert!(true);
}
""",
                encoding="utf-8",
            )
            first = package(
                "F1",
                status="candidate",
                pr="324",
                runtime_entrypoints=["crates/optid/src/main.rs"],
                integration_tests=["crates/optid/tests/real.rs"],
                completion_evidence=["crates/optid/tests/real.rs"],
                acceptance_tests={"behavior": "helper_not_a_test"},
            )
            data = ledger(first)
            errors = validator.validate_ledger(data, tmp_root)
            self.assertTrue(
                any("not a #[test] fn" in error for error in errors),
                f"expected not-a-test-fn rejection, got: {errors}",
            )

    # ── Post-#337: stale receipt freshness rule ───────────────────────

    def test_stale_receipt_is_flagged_when_proof_path_changed(self):
        """A completed package's receipt must be invalidated when a
        later change modifies any declared proof path. Uses a synthetic
        temporary Git repository so the test does not depend on the
        real repository's history being available (which fails in
        shallow CI checkouts).
        """
        import tempfile
        import subprocess

        with tempfile.TemporaryDirectory() as tmp:
            tmp_root = Path(tmp)
            # Initialize a synthetic git repo with the structure the
            # validator expects.
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=tmp_root, check=True)
            subprocess.run(
                ["git", "config", "user.email", "test@example.com"], cwd=tmp_root, check=True
            )
            subprocess.run(
                ["git", "config", "user.name", "Test"], cwd=tmp_root, check=True
            )
            # Create the proof paths the package declares.
            proof_dir = tmp_root / "crates" / "optid" / "src"
            proof_dir.mkdir(parents=True)
            (proof_dir / "policy.rs").write_text("// v1\n", encoding="utf-8")
            # Commit the initial version → this is the verified commit.
            subprocess.run(["git", "add", "-A"], cwd=tmp_root, check=True)
            subprocess.run(
                ["git", "commit", "-q", "-m", "initial"], cwd=tmp_root, check=True
            )
            verified = subprocess.run(
                ["git", "rev-parse", "HEAD"], cwd=tmp_root, capture_output=True, text=True
            ).stdout.strip()
            # Modify a declared proof path after the verified commit.
            (proof_dir / "policy.rs").write_text("// v2 (modified)\n", encoding="utf-8")
            subprocess.run(["git", "add", "-A"], cwd=tmp_root, check=True)
            subprocess.run(
                ["git", "commit", "-q", "-m", "modify proof path"], cwd=tmp_root, check=True
            )

            # Write a receipt that verifies the initial commit.
            receipt_dir = tmp_root / "docs" / "plans" / "optid-verification"
            receipt_dir.mkdir(parents=True)
            receipt_path = receipt_dir / "f1.toml"
            receipt_path.write_text(
                f'''schema_version = 1
package = "F1"
implementation_pr = 332
verified_commit = "{verified}"
verifier = "test"
result = "pass"
commands = ["cargo test"]
runtime_proofs = ["proof"]
unresolved = []
''',
                encoding="utf-8",
            )

            first = package(
                "F1",
                status="completed",
                pr="332",
                runtime_entrypoints=["crates/optid/src/policy.rs"],
                integration_tests=["crates/optid/src/policy.rs"],
                completion_evidence=[
                    "crates/optid/src/policy.rs",
                    "docs/plans/optid-verification/f1.toml",
                ],
                verification_receipt="docs/plans/optid-verification/f1.toml",
            )
            data = ledger(first)
            errors = validator.validate_ledger(data, tmp_root)
            stale_errors = [e for e in errors if "stale" in e.lower()]
            self.assertTrue(
                stale_errors,
                f"expected stale-receipt error, got: {errors}",
            )

    def test_receipt_freshness_fails_closed_when_commit_unavailable(self):
        """When the verified commit is unavailable (e.g. shallow clone,
        typo'd SHA, pruned object), the freshness check must FAIL CLOSED
        rather than skip. The previous revision treated 'unavailable'
        the same as 'divergent' (skip), which was fail-open: a shallow
        CI checkout could not compare the verified commit against HEAD
        and silently passed.
        """
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_root = Path(tmp)
            # Write a receipt with a verified_commit that does not exist
            # in any repo (40-char hex but not a real object). The
            # validator's _git_ancestry_contains will return rc >= 2
            # (git error) because the commit is unavailable.
            receipt_dir = tmp_root / "docs" / "plans" / "optid-verification"
            receipt_dir.mkdir(parents=True)
            receipt_path = receipt_dir / "f1.toml"
            # A syntactically valid SHA that does not exist in the repo.
            fake_sha = "0" * 40
            receipt_path.write_text(
                f'''schema_version = 1
package = "F1"
implementation_pr = 332
verified_commit = "{fake_sha}"
verifier = "test"
result = "pass"
commands = ["cargo test"]
runtime_proofs = ["proof"]
unresolved = []
''',
                encoding="utf-8",
            )
            first = package(
                "F1",
                status="completed",
                pr="332",
                runtime_entrypoints=["crates/optid/src/policy.rs"],
                integration_tests=["crates/optid/src/policy.rs"],
                completion_evidence=[
                    "crates/optid/src/policy.rs",
                    "docs/plans/optid-verification/f1.toml",
                ],
                verification_receipt="docs/plans/optid-verification/f1.toml",
            )
            data = ledger(first)
            errors = validator.validate_ledger(data, tmp_root)
            unavailable_errors = [e for e in errors if "unavailable" in e.lower()]
            self.assertTrue(
                unavailable_errors,
                f"expected unavailable-commit fail-closed error, got: {errors}",
            )

    def test_receipt_freshness_skips_for_divergent_commit(self):
        """When the verified commit exists but is NOT an ancestor of
        HEAD (divergent history, e.g. an unmerged branch), the
        freshness check SKIPS rather than false-positive. This is the
        legitimate skip case.
        """
        import tempfile
        import subprocess

        with tempfile.TemporaryDirectory() as tmp:
            tmp_root = Path(tmp)
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=tmp_root, check=True)
            subprocess.run(
                ["git", "config", "user.email", "test@example.com"], cwd=tmp_root, check=True
            )
            subprocess.run(
                ["git", "config", "user.name", "Test"], cwd=tmp_root, check=True
            )
            proof_dir = tmp_root / "crates" / "optid" / "src"
            proof_dir.mkdir(parents=True)
            (proof_dir / "policy.rs").write_text("// v1\n", encoding="utf-8")
            subprocess.run(["git", "add", "-A"], cwd=tmp_root, check=True)
            subprocess.run(
                ["git", "commit", "-q", "-m", "initial"], cwd=tmp_root, check=True
            )
            verified = subprocess.run(
                ["git", "rev-parse", "HEAD"], cwd=tmp_root, capture_output=True, text=True
            ).stdout.strip()
            # Create a divergent branch: reset main to a different commit
            # so `verified` is no longer an ancestor of HEAD.
            subprocess.run(
                ["git", "checkout", "-q", "--orphan", "divergent"],
                cwd=tmp_root,
                check=True,
            )
            subprocess.run(["git", "add", "-A"], cwd=tmp_root, check=True)
            subprocess.run(
                ["git", "commit", "-q", "-m", "divergent"], cwd=tmp_root, check=True
            )
            # Now HEAD is on `divergent`; `verified` (on the old main
            # branch) is NOT an ancestor of HEAD.

            receipt_dir = tmp_root / "docs" / "plans" / "optid-verification"
            receipt_dir.mkdir(parents=True)
            receipt_path = receipt_dir / "f1.toml"
            receipt_path.write_text(
                f'''schema_version = 1
package = "F1"
implementation_pr = 332
verified_commit = "{verified}"
verifier = "test"
result = "pass"
commands = ["cargo test"]
runtime_proofs = ["proof"]
unresolved = []
''',
                encoding="utf-8",
            )
            first = package(
                "F1",
                status="completed",
                pr="332",
                runtime_entrypoints=["crates/optid/src/policy.rs"],
                integration_tests=["crates/optid/src/policy.rs"],
                completion_evidence=[
                    "crates/optid/src/policy.rs",
                    "docs/plans/optid-verification/f1.toml",
                ],
                verification_receipt="docs/plans/optid-verification/f1.toml",
            )
            data = ledger(first)
            errors = validator.validate_ledger(data, tmp_root)
            # Divergent → skip. The only errors should be from other
            # validator checks (e.g. the receipt file existing), not
            # from the freshness rule.
            freshness_errors = [
                e
                for e in errors
                if "stale" in e.lower() or "unavailable" in e.lower()
            ]
            self.assertFalse(
                freshness_errors,
                f"divergent commit must skip freshness, not error: {freshness_errors}",
            )

    # ── Post-#337: multi-package repair PR exemption ──────────────────

    def test_demotion_does_not_count_as_advancement(self):
        """A corrective PR that demotes a package (completed →
        merged_incomplete) and corrects evidence paths for another
        non-proof-status package (merged_incomplete → merged_incomplete
        with corrected integration_tests) is not 'multi-package
        advancement'. The rule's intent is to prevent silent
        promotion; demotion and evidence correction are honest
        corrections.
        """
        # This test uses the real repo history: F1 was completed and is
        # now merged_incomplete (demotion); T1 stayed merged_incomplete
        # but its integration_tests/completion_evidence changed
        # (evidence correction). The validator's --base origin/main
        # check must not flag this as multi-package advancement.
        errors = validator.validate_change("origin/main", ROOT)
        advancement_errors = [
            e for e in errors if "may advance only one package" in e
        ]
        self.assertFalse(
            advancement_errors,
            f"corrective PR (F1 demotion + T1 evidence correction) must not be "
            f"flagged as multi-package advancement: {advancement_errors}",
        )


if __name__ == "__main__":
    unittest.main()
