# ADR 0011: Benchmark Methodology And Baselines

Status: proposed

> Marked **proposed**; needs human ratification. Addresses review item C5.

## Context

The v1.0 release claim is that benchmarks show better mixed-load responsiveness
and competitive battery behaviour. `benchmarks/manifest.toml` lists competitors
but pins nothing about how the comparison is run. An uncontrolled baseline makes
the claim trivially attackable, and methodology changes after publication
invalidate prior results — so the methodology must be fixed *before* the first
benchmark runs.

## Decision (proposed)

Every published benchmark must record, as machine-readable metadata alongside
results:

1. **Hardware identity** — exact machine model, CPU, RAM, storage, GPU; one
   results set per machine class (never aggregate across classes).
2. **Competitor image provenance** — distro, version, image checksum, install
   date, and whether it is a fresh install or maintained.
3. **Competitor policy daemons** — explicit state of each competitor's
   power/perf daemon (e.g. `power-profiles-daemon`, `tuned`, `tlp`): enabled at
   default, or disabled. Default is to test competitors **as shipped** (daemons
   at their distro default), with an optional "daemons disabled" control run.
4. **Kernel identity** — kernel version and relevant config for each system,
   including Rush's `optid` state (on vs off).
5. **Baseline definition** — `minimal-tuned-baseline` is defined precisely as
   *Rush Linux with `optid` disabled and the static network defaults only*; it is
   the control that isolates `optid`'s contribution.
6. **Run discipline** — fixed number of repetitions, warm-up handling, reported
   variance, and a fixed workload definition per scenario.

Results without this metadata are not publishable as release evidence.

## Consequences

- `benchmarks/manifest.toml` gains required methodology fields; the benchmark
  runner refuses to emit a "release" artifact without them.
- The published artifact is reproducible and defensible.
- Ties into the T4 benchmark-lab and the hardware lab (see
  `docs/project-sustainability.md`).
