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

- [ ] Same baseline distro + mainstream default power stack on both machines
      (per D1; `tuned balanced` on Fedora 44 — see "Deviations" below).
- [x] Firefox JS benchmark page pinned to a fixed local revision
      (`benchmarks/fixtures/interactive-load.html`, deterministic PRNG, fixed
      work per frame).
- [x] `ninja` C++ project pinned; generated from
      `THROUGHPUT_PROJECT_REVISION` with no configure step, so phase 3 measures
      compile only.
- [ ] `glmark2` version recorded in `meta.txt`.
- [ ] Thermal soak: run baseline and optid on the same machine in the same
      session where possible, alternating, to control ambient temperature drift.
- [ ] `n=5` per metric; reject any record carrying `insufficient_n`.

## Implementation status (D2, landed 2026-08-22)

The preset exists: `crates/rushbench/src/preset.rs` implements the five-phase
sequence and

```bash
rushbench run preset=mixed-load-001 --tag=<lever>-<hostname> \
    --cycles 5 --out release/evidence/host-bench/<date>-<hostname>/<arm>
```

writes the four artifacts above into `--out`. Both arms are driven by
[`tools/phase-d-capture.sh`](../../tools/phase-d-capture.sh), which owns the
daemon and service state the two arms must differ in and nothing else.

Unlike `rushbench run --class`, the preset does **not** call `optctl pin`.
Watching whether the classifier reaches the expected class under real load is
part of the evidence, so a mismatch is recorded as a `class_mismatch:<observed>`
anomaly on that phase's records rather than aborting the run. A baseline arm has
no daemon at all, so its `class_observed` is `unmeasured` with an `optid_absent`
anomaly.

### Deviations from the phase table above, and why

| Item | Spec text | What the implementation does |
|------|-----------|------------------------------|
| Baseline stack | "PPD `balanced`" | On Fedora 44, `power-profiles-daemon` is inactive and `tuned` is the shipped default, so the baseline arm runs `tuned` in `balanced`. `meta.txt` records it. A baseline that nobody actually runs would not support a "mainstream defaults" claim. |
| Throughput project | "a pinned medium C++ project (fixed revision)" | A generated project (`THROUGHPUT_PROJECT_REVISION`, 1 600 translation units) instead of a git checkout: byte-identical on every host, no network, and pinned by generator revision + unit count. The unit count is deliberately larger than a 60 s window can drain — the 96-unit first draft finished in ~5 s and the rest of the phase measured an idle machine (`psi-cpu-avg10` 0.06 %). A machine fast enough to finish it records `throughput_build_completed_early`. |
| Throughput work units | joules per work unit | Work units are object files the phase produced. Killing `ninja` at the window edge discards its buffered progress output, so counting `*.o` is the authoritative measure; the `[k/n]` parser is kept for builds that finish inside the window. |
| Frametime distribution | `frametime-p95-ms`, `frametime-p99-ms` | `glmark2` alone reports per-scene average FPS, which cannot yield a percentile. `glmark2 --run-forever --fullscreen` is therefore wrapped in MangoHud's per-frame CSV log, and the percentiles are computed from real frames. Without `mangohud` the two metrics record `unsupported_here` rather than a fabricated average. |
| Idle "discharge (W)" | prose in the phase table | Recorded under the metric name `discharge-w`, one sample per cycle, from the phase's own energy window. |
| `input-latency-p95-ms` / `-p99-ms` | interactive phase metrics | **Still `unsupported_here`.** The probe needs synthetic input injection (`evemu`) plus frame observation in a live session; no such probe exists, and inventing a proxy under the spec's metric name would misreport what was measured. **Criterion 2 must therefore be judged on `frametime-p95/p99-ms` and `foreground-launch-ms`, or stay open** — an owner decision, not the harness's. |

### Class observation

`optctl status --json` is now `schema_version = 2` and nests the fields under
`decision` (`decision.workload_class`, `decision.contract.cpu_wakeup_latency_us`,
`decision.contract.device_resume_latency_us`); version 1 had them at the top
level. `rushbench`'s `OptctlStatus` still described the flat v1 shape, so a
strict deserialize failed against every current daemon — the preset would have
recorded `optid_absent` for a live daemon, and `rushbench run --class` aborted
with a parse error. `contracts::parse_optctl_status` reads either generation and
names the schema version when a status carries no class at all.

### Energy counter, and why it is pinned

`EnergySource::detect()` prefers RAPL when `intel-rapl:0/energy_uj` is readable,
which on current kernels means root-only. An arm run as a user would fall back
to the battery charge counter while an arm run as root picked RAPL — two
different counters, silently incomparable. `tools/phase-d-capture.sh` pins
`RUSHBENCH_ENERGY_SOURCE=battery` for both arms, and each transcript records the
counter path so a verdict can assert the two match.

For the same reason both arms must run **on battery**: the battery counter
measures nothing while the charger holds the pack full, so an on-AC window would
report a real-looking `0 W`. The preset refuses an on-AC run unless `--ac-ok` is
given, and when it is, every energy-derived metric records
`unsupported_here: energy: battery counter cannot measure a window on AC`
instead of a zero.

### Sample units

`RunRecord.samples` are integers in whatever unit the probe emits, following the
pre-existing convention that fractional metrics are stored in milli-units
(`psi-*-avg10` is already ×1000): `frametime-*-ms` samples are microseconds,
`discharge-w` samples are milliwatts, and `joules-per-work-unit` samples are
millijoules per unit. `foreground-launch-ms` keeps its existing whole-millisecond
samples. `results.csv` always prints the median in the metric's *declared* unit,
so the human-facing artifact needs no scaling knowledge.

### Harness validation vs evidence

`RUSHBENCH_PHASE_SCALE=N` (or `--scale N`) divides every phase window so the
sequencer can be exercised in seconds. Any run with a scale other than 1 stamps
`phase_scale_shortened:N` on every record, and any run with fewer than five
cycles stamps `insufficient_n`. Neither is milestone evidence, and neither
belongs under `release/evidence/`.
