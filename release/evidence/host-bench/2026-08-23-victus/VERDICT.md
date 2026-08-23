# Verdict — &lt;hostname&gt; (v0.6 Phase D, D5)

> Template. Replace every `<...>` with literal values. This file's verdict text
> is the **literal source** for the `note` field of the matching
> `criteria_status` row in `release/milestones.toml`.

| Field | Value |
|-------|-------|
| Machine | `<make/model>` |
| Slot | `<desktop \| laptop>` |
| `dmi_board` | `<board>` |
| Baseline | `<distro + PPD profile, e.g. Ubuntu 24.04 LTS / PPD balanced>` |
| optid version | `0.6.0-beta.1` |
| Workload preset | `mixed-load-001` |
| Samples (N) | `<n>` (must be ≥ 5) |
| Run date | `<YYYY-MM-DD>` |
| Operator (physical access) | `<name>` |

## Method

- Baseline transcripts: [`baseline/`](baseline/)
- optid transcripts: [`optid/`](optid/)
- Reports regenerated with `rushbench report`:
  - baseline → `<baseline-report.md path>`
  - optid → `<optid-report.md path>`

## Per-phase deltas (optid vs. baseline)

| Phase | Class | Metric | Baseline median | optid median | Δ (lower=better for latency) | Exceeds CI? |
|-------|-------|--------|-----------------|--------------|------------------------------|-------------|
| Interactive | `interactive` | `input-latency-p95-ms` | `<>` | `<>` | `<>` | `<yes/no>` |
| Interactive | `interactive` | `input-latency-p99-ms` | `<>` | `<>` | `<>` | `<yes/no>` |
| Latency-critical | `latency-critical` | `frametime-p99-ms` | `<>` | `<>` | `<>` | `<yes/no>` |
| Throughput | `throughput` | joules/work-unit | `<>` | `<>` | `<>` (must NOT regress) | `<yes/no>` |
| Battery (laptop, on battery) | — | joules/work-unit | `<>` | `<>` | `<>` (must NOT increase) | `<yes/no>` |

## Verdict

**Criterion 2 (mixed-load responsiveness improves): `<PASS \| FAIL>`** — `<one-line evidence: which latency metric improved by how much, exceeding the CI, with no throughput regression>`

**Criterion 3 (battery behavior matches or improves): `<PASS \| FAIL \| N/A>`** — `<for laptop: joules/work-unit on battery ≤ baseline within CI; for desktop: N/A — no battery>`

## Anomalies / caveats

- `<thermal drift, any insufficient_n records dropped, any class_mismatch, etc.>`
