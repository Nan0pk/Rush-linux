#!/usr/bin/env python3
"""
trim-benchmark-samples.py
==========================

Trims `samples` arrays from psi-cpu / psi-io benchmark result JSONs to control
repo bloat. Run from the repo root after the WP-ENERGY-PROBE-TRIM handoff.

Reference trim: 311 MB -> ~12 KB across 8 psi records (99.96% reduction).
See docs/agent-bus/WP-ENERGY-PROBE-TRIM.handoff.md for the spec.
"""

from __future__ import annotations
import json
import os
import sys
from pathlib import Path

REPO_ROOT    = Path(__file__).resolve().parent.parent
RESULTS_DIR  = REPO_ROOT / "benchmarks" / "results" / "2026-06-14" / "fedora"
TRIM_DATE    = "2026-06-15"
PSI_WORKLOADS = ("psi-cpu.json", "psi-io.json")
SMALL_WORKLOADS = ("cyclictest.json", "foreground-launch.json")


def trim_one(path: Path) -> tuple[int, int, int]:
    """Trim a single psi-cpu / psi-io file. Returns (original_size, new_size, samples_len)."""
    original_size = path.stat().st_size
    with path.open() as f:
        data = json.load(f)
    samples = data.get("samples")
    samples_len = len(samples) if isinstance(samples, list) else 0
    data["samples"] = None
    data.setdefault("meta", {})
    if isinstance(data["meta"], dict):
        data["meta"].update({
            "samples_trimmed_at":     TRIM_DATE,
            "samples_trimmed_reason": "trim-bloat: all-zero entries; median/p95/iqr already capture summary",
            "original_samples_len":   samples_len,
        })
    with path.open("w") as f:
        json.dump(data, f, indent=2, sort_keys=False)
        f.write("\n")
    return original_size, path.stat().st_size, samples_len


def main() -> int:
    if not RESULTS_DIR.is_dir():
        print(f"ERROR: {RESULTS_DIR} not found. Run from repo root.", file=sys.stderr)
        return 2
    print(f"Trimming psi-cpu / psi-io files under {RESULTS_DIR}")
    total_before = total_after = 0
    file_count = 0
    for class_dir in sorted(p for p in RESULTS_DIR.iterdir() if p.is_dir()):
        for wl in PSI_WORKLOADS:
            path = class_dir / wl
            if not path.exists():
                continue
            before, after, slen = trim_one(path)
            total_before += before
            total_after  += after
            file_count   += 1
            saved_mb = (before - after) / 1024 / 1024
            print(f"  {path.relative_to(REPO_ROOT)}: {before:>10} -> {after:>6} bytes (saved {saved_mb:.1f} MB, was {slen:,} samples)")
    print()
    print(f"Trimmed {file_count} files. {total_before/1024/1024:.1f} MB -> {total_after/1024/1024:.1f} MB "
          f"(saved {(total_before - total_after)/1024/1024:.1f} MB).")
    print("Small files (cyclictest, foreground-launch) untouched.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
