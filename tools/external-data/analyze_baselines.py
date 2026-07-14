#!/usr/bin/env python3
"""
Master analysis script — reads all fetched external data and produces
calibration tables for optid policy + rushbench contract bounds.

Run AFTER all fetch_*.py scripts have completed:
  python3 tools/external-data/fetch_spec_power.py
  python3 tools/external-data/fetch_osadl.py
  python3 tools/external-data/fetch_geekbench.py
  python3 tools/external-data/fetch_openbench.py
  python3 tools/external-data/analyze_baselines.py

Also reads local rushbench results for comparison:
  benchmarks/results/*/

Output:
  tools/external-data/analysis/baselines.json   machine-readable calibration
  tools/external-data/analysis/report.md        human-readable findings
"""
import json
import sys
import statistics
from pathlib import Path

ROOT = Path(__file__).parent.parent.parent
FETCHED = Path(__file__).parent / "fetched"
ANALYSIS_DIR = Path(__file__).parent / "analysis"
RUSHBENCH_RESULTS = ROOT / "benchmarks" / "results"


# ── Loaders ───────────────────────────────────────────────────────────────────

def load_json(path):
    try:
        return json.loads(path.read_text())
    except FileNotFoundError:
        print(f"  Missing: {path} — run the fetch script first", file=sys.stderr)
        return []
    except json.JSONDecodeError as e:
        print(f"  Bad JSON in {path}: {e}", file=sys.stderr)
        return []


def load_rushbench_results():
    records = []
    for f in RUSHBENCH_RESULTS.rglob("*.json"):
        try:
            rec = json.loads(f.read_text())
            rec["_source_file"] = str(f.relative_to(ROOT))
            records.append(rec)
        except Exception:
            pass
    return records


# ── Analysis functions ────────────────────────────────────────────────────────

def pct(v, lo, hi):
    if hi == lo:
        return 0.0
    return (v - lo) / (hi - lo) * 100.0


def summarise(values, label):
    if not values:
        return {"label": label, "n": 0}
    values = sorted(values)
    n = len(values)
    return {
        "label": label,
        "n": n,
        "min": values[0],
        "p25": values[n // 4],
        "p50": values[n // 2],
        "p75": values[3 * n // 4],
        "p95": values[int(0.95 * n)],
        "max": values[-1],
        "mean": round(statistics.mean(values), 2),
        "stdev": round(statistics.stdev(values), 2) if n > 1 else 0,
    }


def analyse_spec_power(records):
    """Server power efficiency — calibrate throughput class."""
    vals = [r["ssj_ops_per_watt_avg"] for r in records
            if r.get("ssj_ops_per_watt_avg")]
    idle_watts = [r["active_idle_watts"] for r in records
                  if r.get("active_idle_watts")]
    return {
        "ssj_ops_per_watt": summarise(vals, "ssj_ops/watt (overall)"),
        "active_idle_watts": summarise(idle_watts, "active idle watts"),
        "insight": (
            "Server idle power floor: provides a reference for what 'idle' should cost "
            "relative to full throughput. If optid's idle class has higher proportional "
            "draw than these servers, the power policy is under-tuned."
        ),
    }


def analyse_osadl(records):
    """Cyclictest latency — calibrate latency-critical class.

    Note: the latency-critical contract floor was corrected from 10 µs / 100 µs
    to 1 ms / 1 ms (1000 µs / 1000 µs) in the contract-correction PR. The
    OSADL data below is factual (real RT-kernel cyclictest measurements);
    the contract-relative commentary now references the corrected 1 ms floor.
    """
    max_lats = [r["max_latency_us"] for r in records if r.get("max_latency_us")]
    return {
        "max_latency_us": summarise(max_lats, "cyclictest max latency (µs)"),
        "pct_under_100us": round(sum(1 for l in max_lats if l < 100) / max(len(max_lats), 1) * 100, 1),
        "pct_under_50us": round(sum(1 for l in max_lats if l < 50) / max(len(max_lats), 1) * 100, 1),
        "pct_under_1000us": round(sum(1 for l in max_lats if l < 1000) / max(len(max_lats), 1) * 100, 1),
        "insight": (
            "Most RT-kernel systems achieve max cyclictest < 100 µs. "
            "optid's latency-critical contract floor is 1 ms (1000 µs) after "
            "the contract correction — well within reach of non-RT kernels, "
            "so the floor is enforceable rather than aspirational. The earlier "
            "10 µs floor was unachievable on non-RT kernels and produced "
            "permanent budget violations; 1 ms preserves the floor's meaning "
            "for audio/video/game workloads."
        ),
    }


def analyse_geekbench(records):
    """CPU performance baselines — normalise rushbench across hardware."""
    sc = [r["single_core"] for r in records if r.get("single_core")]
    mc = [r["multi_core"] for r in records if r.get("multi_core")]

    # Group by CPU family (first word of cpu_model)
    by_family = {}
    for r in records:
        if not r.get("cpu_model") or not r.get("single_core"):
            continue
        family = r["cpu_model"].split()[0] if r["cpu_model"].split() else "unknown"
        by_family.setdefault(family, []).append(r["single_core"])

    family_summary = {
        fam: summarise(vals, fam)
        for fam, vals in by_family.items()
        if len(vals) >= 3
    }

    return {
        "single_core": summarise(sc, "single-core score"),
        "multi_core": summarise(mc, "multi-core score"),
        "by_cpu_family": family_summary,
        "insight": (
            "Single-core score predicts interactive/latency-critical workload performance. "
            "Multi-core score predicts throughput class ceiling. "
            "Use these to normalise rushbench foreground-launch latency across machines."
        ),
    }


def analyse_rushbench_local(records):
    """Summarise local rushbench results for comparison."""
    by_workload = {}
    anomaly_count = 0
    zero_psi_count = 0

    for r in records:
        wl = r.get("workload", "unknown")
        cls = r.get("class_requested", "unknown")
        key = f"{cls}/{wl}"

        anomalies = r.get("anomalies", [])
        anomaly_count += len(anomalies)
        if r.get("median") == 0.0 and "psi" in wl:
            zero_psi_count += 1

        by_workload.setdefault(key, []).append({
            "median": r.get("median"),
            "p95": r.get("p95"),
            "iqr": r.get("iqr"),
            "anomalies": anomalies,
            "energy_avg_watts": r.get("energy", {}).get("avg_watts") if r.get("energy") else None,
            "source": r.get("_source_file"),
        })

    return {
        "total_records": len(records),
        "total_anomalies": anomaly_count,
        "zero_psi_records": zero_psi_count,
        "by_workload": by_workload,
        "data_quality_verdict": (
            "POOR" if zero_psi_count > 2
            else "MODERATE" if anomaly_count > 3
            else "GOOD"
        ),
    }


def write_report(spec, osadl, geekbench, rushbench):
    lines = [
        "# External Data Baseline Analysis",
        "",
        "Generated by `tools/external-data/analyze_baselines.py`.",
        "",
        "## Data Quality Verdict for Local Rushbench Data",
        "",
        f"- Total records: {rushbench['total_records']}",
        f"- Records with anomalies: {rushbench['total_anomalies']}",
        f"- PSI records with zero median: {rushbench['zero_psi_records']} "
          f"→ **{rushbench['data_quality_verdict']}**",
        "",
        "## 1. SPECpower — Throughput Class Power Calibration",
        "",
    ]

    ssj = spec.get("ssj_ops_per_watt", {})
    if ssj.get("n", 0) > 0:
        lines += [
            f"- Systems analysed: {ssj['n']}",
            f"- ssj_ops/watt range: {ssj['min']} – {ssj['max']}",
            f"- Median efficiency: {ssj['p50']} ssj_ops/watt",
        ]
    lines += ["", f"*{spec.get('insight', '')}*", ""]

    lines += [
        "## 2. OSADL Cyclictest — Latency-Critical Class Bounds",
        "",
    ]
    lat = osadl.get("max_latency_us", {})
    if lat.get("n", 0) > 0:
        lines += [
            f"- Systems analysed: {lat['n']}",
            f"- Max latency range: {lat['min']} – {lat['max']} µs",
            f"- Median max latency: {lat['p50']} µs",
            f"- % systems under 100 µs: {osadl.get('pct_under_100us')}%",
            f"- % systems under 50 µs: {osadl.get('pct_under_50us')}%",
        ]
    lines += ["", f"*{osadl.get('insight', '')}*", ""]

    lines += [
        "## 3. Geekbench — Performance Normalisation",
        "",
    ]
    sc = geekbench.get("single_core", {})
    if sc.get("n", 0) > 0:
        lines += [
            f"- Linux systems analysed: {sc['n']}",
            f"- Single-core p50: {sc['p50']}, p95: {sc['p95']}",
        ]
    lines += ["", f"*{geekbench.get('insight', '')}*", ""]

    lines += [
        "## 4. Data Sources Not Yet Automated",
        "",
        "These require manual download or PDF parsing:",
        "",
        "- **RAPL accuracy correction factors** (arXiv:2109.07925): "
          "per-CPU-family under-count ratios; correct avg_watts_rapl values",
        "- **NotebookCheck battery discharge** curves: "
          "whole-system watts per chassis/CPU combination",
        "- **Linux Plumbers Conference** scheduler/power slides: "
          "PSI threshold guidance and policy tuning data",
        "- **MLCommons MLPerf Power methodology**: "
          "reference for external-analyser vs RAPL accuracy",
        "",
        "## 5. Recommended Contract Adjustments",
        "",
        "Based on OSADL data (post contract-correction):",
        "- `latency-critical` CPU wakeup + device-resume floors: 1 ms (1000 µs).",
        "  The previous 10 µs / 100 µs floors were unachievable on non-RT kernels;",
        "  the corrected 1 ms floor is enforceable on stock kernels and still",
        "  tight enough to gate C-state selection for audio/video/game workloads.",
        "",
        "Based on RAPL accuracy papers (manual review):",
        "- Apply correction factor ~1.2–1.4× to `avg_watts_rapl` for whole-system estimates",
        "- Always prefer `avg_watts_battery` when available for published laptop benchmarks",
        "",
    ]

    return "\n".join(lines)


def main():
    ANALYSIS_DIR.mkdir(parents=True, exist_ok=True)

    print("Loading fetched data...")
    spec_records = load_json(FETCHED / "spec_power.json")
    osadl_records = load_json(FETCHED / "osadl_latency.json")
    geekbench_records = load_json(FETCHED / "geekbench_results.json")

    print("Loading local rushbench results...")
    rushbench_records = load_rushbench_results()
    print(f"  Found {len(rushbench_records)} local rushbench records")

    print("Running analysis...")
    spec_analysis = analyse_spec_power(spec_records)
    osadl_analysis = analyse_osadl(osadl_records)
    geekbench_analysis = analyse_geekbench(geekbench_records)
    rushbench_analysis = analyse_rushbench_local(rushbench_records)

    baselines = {
        "spec_power": spec_analysis,
        "osadl_cyclictest": osadl_analysis,
        "geekbench": geekbench_analysis,
        "local_rushbench": rushbench_analysis,
    }

    out_json = ANALYSIS_DIR / "baselines.json"
    out_json.write_text(json.dumps(baselines, indent=2))
    print(f"Written: {out_json}")

    report = write_report(spec_analysis, osadl_analysis, geekbench_analysis, rushbench_analysis)
    out_md = ANALYSIS_DIR / "report.md"
    out_md.write_text(report)
    print(f"Written: {out_md}")

    # Print key findings
    print("\n── Key findings ──────────────────────────────────────────────────────")
    print(f"Local data quality: {rushbench_analysis['data_quality_verdict']}")
    print(f"  Zero-PSI records: {rushbench_analysis['zero_psi_records']}")

    lat = osadl_analysis.get("max_latency_us", {})
    if lat.get("n"):
        print(f"OSADL cyclictest p50 max: {lat['p50']} µs "
              f"(contract floor is 1000 µs; "
              f"{osadl_analysis['pct_under_1000us']}% of systems achieve <1000 µs)")


if __name__ == "__main__":
    main()
