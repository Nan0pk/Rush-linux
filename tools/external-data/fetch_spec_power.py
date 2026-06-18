#!/usr/bin/env python3
"""
Fetch and parse SPECpower_ssj2008 public results.

Output: spec_power.json
  List of records: { vendor, system, cpu, cores, ram_gb,
                     watts_100pct, watts_50pct, watts_10pct,
                     ssj_ops_per_watt_100, ssj_ops_per_watt_avg,
                     published }

Use: calibrate throughput-class power budgets and check if optid's
     throughput power draw is competitive with server baselines.
"""
import urllib.request
import urllib.parse
import html.parser
import json
import sys
import time
from pathlib import Path

RESULTS_INDEX = "https://www.spec.org/power_ssj2008/results/"
OUT = Path(__file__).parent / "fetched" / "spec_power.json"


class TableParser(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.in_table = False
        self.in_row = False
        self.in_cell = False
        self.rows = []
        self.current_row = []
        self.current_cell = []
        self.headers = []
        self.header_done = False

    def handle_starttag(self, tag, attrs):
        if tag == "table":
            self.in_table = True
        elif tag == "tr" and self.in_table:
            self.in_row = True
            self.current_row = []
        elif tag in ("td", "th") and self.in_row:
            self.in_cell = True
            self.current_cell = []

    def handle_endtag(self, tag):
        if tag == "table":
            self.in_table = False
        elif tag == "tr" and self.in_row:
            self.in_row = False
            if self.current_row:
                if not self.header_done:
                    self.headers = self.current_row
                    self.header_done = True
                else:
                    self.rows.append(self.current_row)
        elif tag in ("td", "th") and self.in_cell:
            self.in_cell = False
            self.current_row.append(" ".join(self.current_cell).strip())

    def handle_data(self, data):
        if self.in_cell:
            self.current_cell.append(data.strip())


def fetch(url, retries=3):
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "rush-collect-research/1.0"})
            with urllib.request.urlopen(req, timeout=30) as r:
                return r.read().decode("utf-8", errors="replace")
        except Exception as e:
            if attempt == retries - 1:
                raise
            print(f"  retry {attempt+1}: {e}", file=sys.stderr)
            time.sleep(2 ** attempt)


def parse_number(s):
    try:
        return float(s.replace(",", "").replace("%", "").strip())
    except (ValueError, AttributeError):
        return None


def main():
    OUT.parent.mkdir(parents=True, exist_ok=True)
    print("Fetching SPECpower_ssj2008 index...")

    html_content = fetch(RESULTS_INDEX)
    parser = TableParser()
    parser.feed(html_content)

    print(f"  Found {len(parser.rows)} result entries, headers: {parser.headers[:6]}")

    records = []
    for row in parser.rows:
        if len(row) < 6:
            continue
        # Typical columns: System, # Chips, # Cores, Memory (GB),
        #                  ssj_ops/watt (overall), Active Idle (W), ...
        # Column indices vary by page version — map by header name
        def col(name):
            for i, h in enumerate(parser.headers):
                if name.lower() in h.lower() and i < len(row):
                    return row[i]
            return ""

        record = {
            "system": col("system") or (row[0] if row else ""),
            "cores": parse_number(col("cores") or col("# cores")),
            "ram_gb": parse_number(col("memory") or col("mem")),
            "ssj_ops_per_watt_avg": parse_number(col("overall")),
            "active_idle_watts": parse_number(col("idle") or col("active idle")),
            "published": col("date") or col("published"),
        }
        if record["ssj_ops_per_watt_avg"] is not None:
            records.append(record)

    print(f"  Parsed {len(records)} usable records")

    OUT.write_text(json.dumps(records, indent=2))
    print(f"  Written to {OUT}")

    # Quick summary
    if records:
        vals = [r["ssj_ops_per_watt_avg"] for r in records if r["ssj_ops_per_watt_avg"]]
        print(f"  ssj_ops/watt range: {min(vals):.0f} – {max(vals):.0f}, median: {sorted(vals)[len(vals)//2]:.0f}")


if __name__ == "__main__":
    main()
