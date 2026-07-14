# Mixed-Load Workload — `mixed-load-001` (v0.6 Phase D, D2)

> **Purpose:** Define the single, reproducible workload used to certify the
> `0.6.0-beta.1` quantitative exit criteria (Criteria 2 & 3). This is the
> comparison point between a mainstream-default baseline and `optid --apply`,
> captured by `rushbench` and committed under `release/evidence/host-bench/`.
>
> Companion files: [`reference-hardware.md`](reference-hardware.md) (D1, which
> machines) and `docs/plans/v0.6-hardware-aware-optid-proposal.md` §"Phase D"
> (the D1–D5 protocol).

## Design goal

The workload must exercise **all four active workload classes in a single run**
so the optimizer's class transitions are observable and the per-phase deltas
are attributable to a specific class. The five active classes from SPEC §1 are
`idle`, `light`, `interactive`, `latency-critical`, `throughput`; this workload
drives the four that produce measurable load (`light` is covered by the
interactive phase's quiescent edges).

This intentionally aligns with the `mixed-load-responsiveness` scenario already
declared in [`benchmarks/manifest.toml`](../../benchmarks/manifest.toml).

## Preset name

```
mixed-load-001
```

Invoked as:

```bash
rushbench run preset=mixed-load-001 --tag=<baseline|optid-0.6.0-beta.1>-<hostname>
```

D2's coding task is to wire this definition into `rushbench` as the named preset
`mixed-load-001` (the harness already supports structured energy windows and
N-sample collection — this adds the preset, not new measurement machinery).

## Phase sequence (one cycle ≈ 4 min 30 s)

| # | Phase | Duration | Expected class | Driver command | Primary metrics |
|---|-------|----------|----------------|----------------|-----------------|
| 1 | Idle (warm) | 60 s | `idle` | _(nothing — quiescent)_ | `psi-cpu-avg10`, idle discharge (W) |
| 2 | Interactive | 60 s | `interactive` | `firefox` rendering a fixed JavaScript benchmark (e.g. Speedometer-style local page) | `input-latency-p95-ms`, `foreground-launch-ms`, `psi-cpu-avg10` |
| 3 | Throughput | 60 s | `throughput` | `ninja -C build` on a pinned medium C++ project (fixed revision) | `psi-cpu-avg10`, `psi-io-avg10`, joules/work-unit |
| 4 | Latency-critical | 60 s | `latency-critical` | `glmark2 --fullscreen` targeting 60 FPS | `frametime-p95-ms`, `frametime-p99-ms`, `psi-cpu-avg10` |
| 5 | Idle (cool-down) | 30 s | `idle` | _(nothing)_ | settle / discharge (W) |

**Repeat the full cycle ×5** for statistical stability (`rushbench` N=5).
Total wall-clock: **≈ 22–30 minutes per tagged run** (per machine, per lever).

> Phases must be launched from a TTY (no desktop session required); the desktop
> reference machine can run the server edition. `firefox`/`glmark2`/`ninja` are
> launched non-interactively by the preset harness so each phase is reproducible.

## Why N=5 (and why a confidence interval)

A single sample cannot distinguish an `optid` improvement from run-to-run noise.
The harness records `n`, `median`, `p95`, and `iqr` per metric. **Report PASS
only when the baseline→optid delta exceeds the run-to-run variability** (the
proposal's risk control: "report PASS only if the delta exceeds the confidence
interval"). Records with `n < 5` carry an `insufficient_n` anomaly and are not
acceptable as milestone evidence.

## Metrics & pass conditions

### Criterion 2 — mixed-load responsiveness improves (both machines)

PASS on a machine when, for the interactive + latency-critical phases:

- the median and p99 latency metrics under `optid --apply` are **lower** than
  the baseline by more than the confidence interval, **and**
- the throughput phase shows **no regression** in joules-per-work-unit beyond
  the confidence interval (responsiveness must not be bought with throughput).

### Criterion 3 — battery behavior matches or improves (laptop only)

On the battery-equipped laptop, running the full cycle **on battery**:

- `optid --apply` energy-per-workload-unit (joules) must be **≤** the baseline
  (PPD `balanced`) within the confidence interval — i.e. it must not *increase*
  energy per unit of work.
- Desktop slot: **N/A** (no battery).

## Output layout (consumed by D3/D4/D5)

Each tagged run produces, under
`release/evidence/host-bench/<date>-<hostname>/<baseline|optid>/`:

- `meta.txt` — host facts (kernel, CPU, `cpufreq_driver`, governor,
  `platform_profile_available`, RAPL domain, `optid_version`, `git_commit`),
  following the Dragnet `meta.txt` template (sanity-checked by the evidence gate).
- `transcript.log` — literal command transcript of the run.
- `results.csv` — per-phase, per-metric rows
  (`phase,lever,scenario,metric,median,iters,batt_pct,ambient_cpu_pct`),
  matching the existing CSV schema.
- `*.json` — per-metric `RunRecord`s (schema_version 1) consumed by
  `rushbench report <results-dir>`.

The verdict per machine is written to `VERDICT.md` (see D5 template in the
Phase D proposal); its text is the literal source for the `note` field of the
v0.6 `criteria_status` rows in `release/milestones.toml`.

## Reproducibility checklist

- [ ] Same baseline distro + PPD `balanced` on both machines (per D1).
- [ ] Firefox JS benchmark page pinned to a fixed local revision.
- [ ] `ninja` C++ project pinned to a fixed commit; build dir pre-configured so
      phase 3 measures compile, not configure.
- [ ] `glmark2` version recorded in `meta.txt`.
- [ ] Thermal soak: run baseline and optid on the same machine in the same
      session where possible, alternating, to control ambient temperature drift.
- [ ] `n=5` per metric; reject any record carrying `insufficient_n`.
