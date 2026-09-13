#!/usr/bin/env python3
"""Canonical Optid package-contract entry point.

The implementation module performs structural checks and detects completed-package
proof paths that changed after their cold-verification receipt. ADR 0029 makes
that file-level freshness signal review context rather than automatic proof
invalidation: semantic impact must be decided by an independent reviewer.
"""

from __future__ import annotations

import argparse
import importlib.util
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
_IMPL_PATH = Path(__file__).with_name("_validate_optid_packages_impl.py")
_SPEC = importlib.util.spec_from_file_location("_validate_optid_packages_impl", _IMPL_PATH)
assert _SPEC and _SPEC.loader
_impl = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_impl)

# Preserve the validator's existing import surface for its focused tests and any
# repository tooling that imports helpers from this canonical file.
for _name in dir(_impl):
    if not _name.startswith("__"):
        globals()[_name] = getattr(_impl, _name)

_STALE_MARKER = ": verification receipt is stale — "
_OLD_REMEDY = (
    "A fresh cold verification receipt is required before this package may remain "
    "`completed`. Demote to `merged_incomplete` and record the precise blocker in "
    "`blocking_reason`."
)


def _impact_notice(error: str) -> str | None:
    """Convert raw proof-path staleness into ADR-0029 review context.

    Missing/unavailable receipt commits remain blocking errors. Only the narrow
    signal produced when known proof paths changed after a known verified commit
    becomes non-blocking review context.
    """
    if _STALE_MARKER not in error:
        return None
    detail = error.replace(_OLD_REMEDY, "").strip()
    return (
        f"{detail} ADR 0029 requires an independent impact review on the exact "
        "head/base before merge. The package may keep its existing completion "
        "receipt only if that review records `proof preserved`; `re-verification "
        "required` or `inconclusive` requires fresh independent cold verification."
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--base",
        help="git base ref for change-aware checks (for example origin/main)",
    )
    args = parser.parse_args()

    try:
        ledger = load_toml(ROOT / LEDGER)
        raw_errors = validate_ledger(ledger)
        if args.base:
            raw_errors.extend(validate_change(args.base))
    except (OSError, tomllib.TOMLDecodeError, RuntimeError) as exc:
        raw_errors = [str(exc)]

    errors: list[str] = []
    notices: list[str] = []
    for error in raw_errors:
        notice = _impact_notice(error)
        if notice is None:
            errors.append(error)
        else:
            notices.append(notice)

    if notices:
        print("NOTICE: completed-package proof paths changed; impact review required")
        for notice in notices:
            print(f"  - {notice}")

    if errors:
        print("FAILED: optid package contract")
        for error in errors:
            print(f"  - {error}")
        return 1

    print("PASS: optid package ledger and change contract are truthful")
    return 0


if __name__ == "__main__":
    sys.exit(main())
