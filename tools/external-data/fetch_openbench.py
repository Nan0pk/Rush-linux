#!/usr/bin/env python3
"""
Fetch benchmark data from OpenBenchmarking.org (Phoronix Test Suite).

Queries the public API for CPU-specific results across tests relevant to
optid policy calibration:
  - pts/idle-power       baseline power draw
  - pts/cpu-stress-ng    CPU throughput (for throughput class baseline)
  - pts/boot-time        system responsiveness proxy
  - pts/latency-bench    scheduling latency
  - pts/fio              I/O throughput (for server class)

Output: openbench_results.json
  List of records: { test_id, cpu_model, result_value, result_unit,
                     result_identifier, date, url }

Use: ground optid policy class boundaries against real hardware measurements.
"""
import urllib.request
import urllib.parse
import json
import sys
import time
from pathlib import Path

API_BASE = "https://openbenchmarking.org"
# Phoronix public API endpoint for test results by identifier
SEARCH_URL = "https://openbenchmarking.org/result/{result_id}.json"
OUT = Path(__file__).parent / "fetched" / "openbench_results.json"

# Tests most relevant to rush linux policy calibration
TARGET_TESTS = [
    "pts/idle-power",
    "pts/stress-ng",
    "pts/boot-time",
    "pts/latency-bench",
    "pts/fio",
    "pts/sysbench",
    "pts/compress-7zip",
]

# Search endpoint: returns list of result IDs matching test name
SEARCH_ENDPOINT = "https://openbenchmarking.org/search.php?q={query}&type=result"


def fetch(url, retries=3):
    for attempt in range(retries):
        try:
            req = urllib.request.Request(
                url, headers={"User-Agent": "rush-collect-research/1.0",
                               "Accept": "application/json"}
            )
            with urllib.request.urlopen(req, timeout=20) as r:
                raw = r.read()
                ct = r.headers.get("Content-Type", "")
                if "json" in ct:
                    return json.loads(raw)
                return raw.decode("utf-8", errors="replace")
        except Exception as e:
            if attempt == retries - 1:
                print(f"  Failed: {url}: {e}", file=sys.stderr)
                return None
            time.sleep(2 ** attempt)


def search_results(test_name):
    """Return list of result IDs for a given test."""
    # OpenBenchmarking search is HTML-based; parse result IDs from links
    import re
    query = urllib.parse.quote(test_name.replace("pts/", ""))
    url = SEARCH_ENDPOINT.format(query=query)
    content = fetch(url)
    if not content or not isinstance(content, str):
        return []
    # Result IDs look like: href="/result/XXXXXXX"
    return list(dict.fromkeys(re.findall(r'/result/([A-Z0-9]{7,})', content)))[:10]


def parse_result(result_id):
    """Fetch and parse a single result JSON."""
    url = SEARCH_URL.format(result_id=result_id)
    data = fetch(url)
    if not data or not isinstance(data, dict):
        return []

    records = []
    # OpenBenchmarking result JSON structure varies; extract what we can
    system = data.get("System", {})
    cpu = system.get("Hardware", {}).get("Processor", "unknown")
    date = data.get("Generated", "")

    for test_name, test_data in data.get("Results", {}).items():
        if not isinstance(test_data, dict):
            continue
        for identifier, runs in test_data.items():
            if not isinstance(runs, list):
                continue
            for run in runs:
                val = run.get("Value")
                unit = run.get("Scale", "")
                if val is not None:
                    try:
                        records.append({
                            "test_id": test_name,
                            "result_id": result_id,
                            "cpu_model": cpu,
                            "result_value": float(val),
                            "result_unit": unit,
                            "result_identifier": identifier,
                            "date": date,
                            "url": f"{API_BASE}/result/{result_id}",
                        })
                    except (ValueError, TypeError):
                        pass
    return records


def main():
    OUT.parent.mkdir(parents=True, exist_ok=True)
    all_records = []

    for test in TARGET_TESTS:
        print(f"Searching OpenBenchmarking.org for: {test}")
        result_ids = search_results(test)
        print(f"  Found {len(result_ids)} result IDs")

        for rid in result_ids:
            print(f"  Fetching {rid}...")
            records = parse_result(rid)
            all_records.extend(records)
            time.sleep(1.0)

    print(f"\nTotal records: {len(all_records)}")
    OUT.write_text(json.dumps(all_records, indent=2))
    print(f"Written to {OUT}")

    if all_records:
        tests_seen = set(r["test_id"] for r in all_records)
        print(f"Tests covered: {sorted(tests_seen)}")


if __name__ == "__main__":
    main()
