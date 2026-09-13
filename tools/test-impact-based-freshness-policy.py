from __future__ import annotations

import importlib.util
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "validate_optid_packages_policy", ROOT / "tools" / "validate-optid-packages.py"
)
assert SPEC and SPEC.loader
validator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(validator)


def test_known_proof_path_change_becomes_impact_review_notice() -> None:
    stale = (
        "F1: verification receipt is stale — verified_commit 0123456789ab is an "
        "ancestor of HEAD but the following declared proof paths were modified after "
        "it: crates/optid/src/actuator.rs. A fresh cold verification receipt is "
        "required before this package may remain `completed`. Demote to "
        "`merged_incomplete` and record the precise blocker in `blocking_reason`."
    )

    notice = validator._impact_notice(stale)

    assert notice is not None
    assert "ADR 0029" in notice
    assert "independent impact review" in notice
    assert "`proof preserved`" in notice
    assert "`re-verification required`" in notice
    assert "`inconclusive`" in notice
    assert "Demote to `merged_incomplete`" not in notice


def test_unavailable_receipt_commit_remains_blocking() -> None:
    unavailable = (
        "F1: verification receipt verified_commit 0123456789ab is unavailable in "
        "this checkout (unavailable). The freshness check cannot prove the receipt "
        "is still valid."
    )

    assert validator._impact_notice(unavailable) is None


def test_unrelated_contract_error_remains_blocking() -> None:
    assert validator._impact_notice("F1: completed status requires verification_receipt") is None
