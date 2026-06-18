#!/usr/bin/env python3
"""
Fetch OSADL cyclictest latency data.

OSADL runs cyclictest continuously on real-time kernels and publishes
per-system latency histograms and summary statistics.

Output: osadl_latency.json
  List of records: { system_id, kernel, hardware_hint,
                     max_latency_us, p99_9_us, p99_us,
                     test_duration_hours, url }

Use: validate rushbench latency-critical contract bounds;
     understand real-world cyclictest max latency distributions.
"""
import urllib.request
import urllib.parse
import html.parser
import json
import re
import sys
import time
from pathlib import Path

OSADL_BASE = "https://www.osadl.org"
OSADL_INDEX = "https://www.osadl.org/Latency-plots.latency-plots.0.html"
OUT = Path(__file__).parent / "fetched" / "osadl_latency.json"


def fetch(url, retries=3):
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "rush-collect-research/1.0"})
            with urllib.request.urlopen(req, timeout=30) as r:
                return r.read().decode("utf-8", errors="replace")
        except Exception as e:
            if attempt == retries - 1:
                print(f"  Failed to fetch {url}: {e}", file=sys.stderr)
                return None
            time.sleep(2 ** attempt)


def parse_number(s):
    try:
        return float(re.sub(r"[^\d.]", "", s))
    except (ValueError, TypeError):
        return None


def extract_system_links(html_content):
    """Find links to individual system latency pages."""
    links = []
    for match in re.finditer(r'href="([^"]*latency[^"]*)"', html_content, re.IGNORECASE):
        href = match.group(1)
        if not href.startswith("http"):
            href = OSADL_BASE + "/" + href.lstrip("/")
        links.append(href)
    return list(dict.fromkeys(links))  # deduplicate preserving order


def parse_system_page(url, content):
    """Extract summary stats from an individual OSADL system page."""
    record = {"url": url}

    # Max latency
    m = re.search(r"Maximum latency[:\s]+(\d+)\s*(?:µs|us|nsec|μs)", content, re.IGNORECASE)
    if m:
        record["max_latency_us"] = int(m.group(1))

    # Kernel version
    m = re.search(r"kernel[:\s]+([0-9]+\.[0-9]+\.[0-9]+[^\s<]*)", content, re.IGNORECASE)
    if m:
        record["kernel"] = m.group(1)

    # CPU / hardware hint from page title or heading
    m = re.search(r"<h[12][^>]*>([^<]{5,80})</h[12]>", content)
    if m:
        record["hardware_hint"] = m.group(1).strip()

    # Test duration
    m = re.search(r"(\d+)\s*hours?", content, re.IGNORECASE)
    if m:
        record["test_duration_hours"] = int(m.group(1))

    return record if "max_latency_us" in record else None


def main():
    OUT.parent.mkdir(parents=True, exist_ok=True)
    print("Fetching OSADL cyclictest index...")

    index_html = fetch(OSADL_INDEX)
    if not index_html:
        print("Could not fetch OSADL index — site may be unavailable", file=sys.stderr)
        sys.exit(1)

    links = extract_system_links(index_html)
    print(f"  Found {len(links)} system links")

    records = []
    for i, url in enumerate(links[:50]):  # cap at 50 to be polite
        print(f"  [{i+1}/{min(len(links),50)}] {url}")
        content = fetch(url)
        if not content:
            continue
        rec = parse_system_page(url, content)
        if rec:
            records.append(rec)
        time.sleep(0.5)  # rate limit

    print(f"\n  Parsed {len(records)} usable records")
    OUT.write_text(json.dumps(records, indent=2))
    print(f"  Written to {OUT}")

    if records:
        lats = [r["max_latency_us"] for r in records if "max_latency_us" in r]
        if lats:
            lats.sort()
            print(f"  Max latency range: {lats[0]}–{lats[-1]} µs, p50={lats[len(lats)//2]} µs")
            over_100 = sum(1 for l in lats if l > 100)
            print(f"  Systems with max_latency > 100 µs: {over_100}/{len(lats)}")


if __name__ == "__main__":
    main()
