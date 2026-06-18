#!/usr/bin/env python3
"""
Fetch Geekbench 6 Browser results for CPU performance baselines.

The Geekbench Browser has a public JSON API paginated by page number.
We filter for x86_64 Linux results to get performance baselines comparable
to Rush Linux's target hardware.

Output: geekbench_results.json
  List of records: { cpu_model, single_core, multi_core, memory_mb,
                     os, arch, date, url }

Use: normalise rushbench performance measurements across CPU families;
     understand single/multi-core ratio (relevant to workload class selection).
"""
import urllib.request
import json
import time
import sys
from pathlib import Path

API_BASE = "https://browser.geekbench.com"
# Public JSON endpoint — returns array of result objects
CPU_LIST_URL = API_BASE + "/v6/cpu.json?page={page}"
OUT = Path(__file__).parent / "fetched" / "geekbench_results.json"

MAX_PAGES = 20       # 20 pages × ~25 results = ~500 systems
TARGET_OS = "linux"  # filter to Linux results for comparability


def fetch_page(page):
    url = CPU_LIST_URL.format(page=page)
    try:
        req = urllib.request.Request(
            url, headers={"User-Agent": "rush-collect-research/1.0",
                           "Accept": "application/json"}
        )
        with urllib.request.urlopen(req, timeout=20) as r:
            data = json.loads(r.read())
            return data
    except Exception as e:
        print(f"  Error on page {page}: {e}", file=sys.stderr)
        return None


def parse_entry(entry):
    """Extract fields from a Geekbench result entry."""
    try:
        return {
            "cpu_model": entry.get("name", ""),
            "single_core": entry.get("score", entry.get("single_core_score")),
            "multi_core": entry.get("multicore_score", entry.get("multi_core_score")),
            "memory_mb": entry.get("memory"),
            "os": entry.get("platform", ""),
            "arch": entry.get("architecture", ""),
            "date": entry.get("created_at", ""),
            "url": API_BASE + entry.get("path", ""),
        }
    except Exception:
        return None


def main():
    OUT.parent.mkdir(parents=True, exist_ok=True)
    results = []

    for page in range(1, MAX_PAGES + 1):
        print(f"Fetching Geekbench page {page}/{MAX_PAGES}...")
        data = fetch_page(page)
        if not data:
            break

        entries = data if isinstance(data, list) else data.get("items", [])
        if not entries:
            print("  No entries — stopping")
            break

        page_results = 0
        for entry in entries:
            rec = parse_entry(entry)
            if rec and TARGET_OS in rec.get("os", "").lower():
                results.append(rec)
                page_results += 1

        print(f"  {page_results} Linux results on this page (total so far: {len(results)})")
        time.sleep(0.75)  # polite rate limiting

    print(f"\nTotal Linux results: {len(results)}")
    OUT.write_text(json.dumps(results, indent=2))
    print(f"Written to {OUT}")

    if results:
        sc = [r["single_core"] for r in results if r.get("single_core")]
        mc = [r["multi_core"] for r in results if r.get("multi_core")]
        if sc:
            sc.sort()
            print(f"Single-core range: {sc[0]}–{sc[-1]}, p50={sc[len(sc)//2]}")
        if mc:
            mc.sort()
            print(f"Multi-core range: {mc[0]}–{mc[-1]}, p50={mc[len(mc)//2]}")


if __name__ == "__main__":
    main()
