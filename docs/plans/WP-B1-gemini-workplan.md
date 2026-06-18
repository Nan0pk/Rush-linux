# WP-B1 — Work Plan: Measurement rig (contract-validation evidence)

**Assignee:** Gemini (Antigravity CLI), Builder role.
**Authority:** Scoped by `docs/SPEC-northstar.md` (canon on `main`). You implement
the EVIDENCE producer for the §6 B1 row: a tool that measures real battery drain
and real responsiveness on a real laptop, producing the dataset that validates —
or refutes — the provisional latency budgets currently load-bearing in
`config/optid/contracts.toml`. You do NOT change those budgets. You do NOT add a
new lever. Output is a measurement tool + a verifier verdict, not a tuning memo.
**Version:** 1.0
**Depends on:** WP-N0 merged (✓ PR #65), WP-N1 merged (✓ PR #66),
WP-N2 merged (✓ PR #67). SPEC-northstar canon on `main` (✓ PR #64).

---

## 1. Objective (one ledger role)

Build a **measurement rig** that, for each SPEC §1 workload class, captures on a
single real laptop:

1. **Energy:** wall-clock-normalised average platform power draw on battery
   (W and J), via the kernel's standard energy counters
   (`/sys/class/power_supply/BAT*/energy_now` deltas or, where present,
   `/sys/class/powercap/intel-rapl:0/energy_uj` deltas) — whichever is exposed
   on the host. No vendor-specific reads.
2. **Responsiveness:** the SPEC's already-declared metrics
   (`benchmarks/manifest.toml`): `input-latency-p95-ms`, `input-latency-p99-ms`,
   `foreground-launch-ms`, `cyclictest-max-us`, `psi-cpu-avg10`, `psi-io-avg10`.
   The rig MUST use these names verbatim; no parallel metric vocabulary.
3. **Provenance:** for every run, the active class published by N1, the resolved
   floors published by N2 (`optctl status --json`), kernel/optid versions, AC vs
   battery, and a content-hash of `config/optid/contracts.toml`.

The deliverable is the dataset under `benchmarks/results/`, in a schema stable
enough that a later PR can tighten `contracts.toml` against it. This is the §6
B1 row interpreted as **validation evidence** for the contract — the competitor
comparison (vs PPD/TLP/baseline) is a strict superset and explicitly OUT OF
SCOPE here (§10). Build the rig that produces apples-to-apples numbers on one
machine first; cross-distro comes later.

Per SPEC §6 gate: until B1 evidence exists, no enabler can claim it "works."
This WP is what unlocks that gate for N5/N6.

---

## 2. Hard constraints (do not violate)

1. **No tuning.** This WP does not edit `config/optid/contracts.toml`. Not one
   µs. The rig REPORTS whether observed exit latencies and idle drains are
   consistent with the declared floors. Any change to those floors is a separate
   PR after this one's data is reviewed by a human.
2. **No new lever, no new actuation.** B1 is observe-only. It reads counters,
   runs declared workloads, parses `optctl status`. It MUST NOT write
   `/proc/sys`, EPP, PM QoS, slices, runtime PM, APST, ASPM, or any device
   state. If you reach for an actuator, STOP — that is N5/N6/etc.
3. **One machine, honest.** First-cut is a single-host harness on the
   maintainer's laptop. Do NOT pretend to multi-host coverage. The results
   directory MUST record the host fingerprint (kernel, CPU model, board DMI,
   battery design capacity) so future readers know what the numbers describe.
4. **Reuse the §1 vocabulary.** Use SPEC §1 class names (`idle`, `light`,
   `interactive`, `latency-critical`, `throughput`) and `manifest.toml` metric
   names verbatim. Do NOT invent a parallel metric set. If the existing manifest
   metric for a thing is wrong, file an issue — do not silently substitute.
5. **Kernel counters only.** No `powerstat`, `s-tui`, vendor agents, or
   userspace wrappers as the *energy* source — read `energy_now` /
   `energy_uj` directly. External tools may be used for *workload generation*
   (`cyclictest`, `stress-ng`, `fio`) where they are the canonical generators
   for the metric; declare them in `Cargo.toml` / a host-deps doc, do not vendor.
6. **AC vs battery is a first-class axis.** Every run records the power source.
   Energy figures from AC runs are recorded but flagged as non-authoritative for
   the discharge metrics. Battery runs are the contract-validation runs.
7. **Statistical honesty.** Each (class × workload) cell needs ≥ N=5 runs and
   reports median + IQR, not just mean. N, seed, and warm-up policy are recorded
   per cell. A cell with N<5 is marked `insufficient`, never silently averaged.
8. **§3 actuation rule does not apply to B1** (no writes) — but the
   *explainability* clause does, in spirit: every result row records `(class,
   resolved floors, host, kernel, optid SHA, contracts.toml SHA, power source,
   N, timestamp)`. A row without provenance is a fabricated row.
9. **No agent-fabricated numbers, ever.** The rig records what counters returned.
   It does not "estimate," "extrapolate," or "fill gaps." A missing measurement
   is a missing measurement and is recorded as such (`null` + reason). This is
   the §7 fabrication rule applied to evidence.
10. **Doc-sync acceptance rule.** Behaviour change updates docs in the same PR.

---

## 3. Preconditions (idempotent — run first)

```bash
git checkout main && git pull
test -f docs/SPEC-northstar.md       || { echo "MISSING canon spec — STOP"; exit 1; }
test -f config/optid/contracts.toml  || { echo "MISSING contracts.toml — STOP"; exit 1; }
test -f benchmarks/manifest.toml     || { echo "MISSING benchmarks manifest — STOP"; exit 1; }
# N2 must be merged: fits_contract / pm_qos enforcement landed
grep -q "WP-N2" docs/IMPLEMENTATION_STATUS.md || echo "review N2 status row"
# At least one energy counter must exist on the host the rig will eventually run on
ls /sys/class/power_supply/BAT*/energy_now /sys/class/powercap/intel-rapl:0/energy_uj 2>/dev/null \
  | grep -q . || echo "WARN: host has no energy_now or RAPL — rig will refuse to run there"
```

If `SPEC-northstar.md`, `contracts.toml`, or `benchmarks/manifest.toml` is
absent, stop — the dependency chain is broken.

---

## 4. Discovery (repo tooling, no broad reads)

```bash
graphify query "where does optctl expose status JSON and the resolved PM QoS floors published by N2?" --graph graphify-out/graph.json
graphify query "where in the workspace are the workload class names and the manifest.toml metrics referenced?" --graph graphify-out/graph.json
graphify query "where is the state directory and journal path so I can record runs without colliding with optid state?" --graph graphify-out/graph.json
```

Read only what they point to: the `optctl status --json` schema (B1 ingests
it), the class-name constants (B1 must reuse them), the manifest metric names,
and the state-dir convention. The rig writes under `benchmarks/results/`, not
under optid's state dir.

---

## 5. Implementation requirements

### 5.1 Crate layout
- New binary crate `crates/rushbench` (or `benchmarks/rig/`, pick one and
  document it in `IMPLEMENTATION_STATUS.md`). Pure Rust, workspace member.
- No new top-level dependencies beyond what the workspace already pins, unless
  unavoidable; if you add one, justify it in the PR body.

### 5.2 Energy probe (pure read)
- `EnergySource::detect()` → first available of `BAT*/energy_now` (µWh) or
  `intel-rapl:0/energy_uj` (µJ). Reject the host if neither exists.
- `EnergyProbe::sample()` returns a monotonic `(t, joules_cumulative)` tuple.
- `EnergyProbe::window(start, end) -> Result<JoulesAndAvgWatts>`. Reject windows
  where the counter wrapped or the AC state changed mid-window — return
  `Err(WrapOrAcSwitch)`, do not silently smooth.

### 5.3 Responsiveness probes (one per declared metric)
- `input-latency-p95-ms` / `input-latency-p99-ms`: use `evemu`-style synthetic
  input + a known consumer (a tiny X/Wayland-agnostic helper is acceptable; if
  the host is headless, mark the metric `unsupported_here` and move on).
- `foreground-launch-ms`: cold-launch a declared benign app (e.g. `xterm`,
  `gnome-calculator`); wall-clock from `exec` to first frame / window-mapped.
- `cyclictest-max-us`: shell out to `cyclictest`; record max + p99.
- `psi-cpu-avg10`, `psi-io-avg10`: read `/proc/pressure/cpu`, `/proc/pressure/io`
  directly at window boundaries.
- Every probe is independently runnable and independently fails — one broken
  probe MUST NOT poison a run.

### 5.4 Class pinning + readback
- Drive optid via `optctl pin <class>` (N1's manual override path), then read
  `optctl status --json` and assert: (a) `class == requested`, (b) resolved
  floors match `contracts.toml` row, (c) `--apply` is in effect. If readback
  disagrees with intent, the run is aborted and recorded as `class_mismatch`.
  This is the rig's load-bearing check that we are actually measuring the class
  we think we are.

### 5.5 Run record schema (frozen)
Per cell, write `benchmarks/results/<UTC-date>/<host>/<class>/<workload>.json`:

```jsonc
{
  "schema_version": 1,
  "host": { "kernel": "...", "cpu_model": "...", "dmi_board": "...",
            "battery_design_uwh": 0 },
  "rush": { "optid_sha": "...", "contracts_sha256": "...",
            "rig_sha": "...", "rig_version": "0.1.0" },
  "class_requested": "interactive",
  "class_observed":  "interactive",
  "resolved_floors": { "cpu_wakeup_latency_us": 1000,
                       "device_resume_latency_us": 10000 },
  "power_source": "battery",            // "battery" | "ac"
  "workload": "foreground-launch",
  "metric":   "foreground-launch-ms",
  "n": 5,
  "samples": [123, 119, 131, 122, 127],
  "median": 123, "p95": 130, "iqr": 8,
  "energy": { "window_joules": 41.2, "avg_watts": 4.6,
              "counter": "BAT0/energy_now" },
  "started_at": "2026-06-14T09:01:22Z",
  "warmup_runs": 2,
  "anomalies": []                        // e.g. "ac_switch_mid_window"
}
```

`schema_version: 1` is a contract: later PRs that change the shape bump the
version, they don't mutate it in place.

### 5.6 CLI
- `rushbench run --class <C> --workload <W> [--n 5] [--ac-ok]`
- `rushbench matrix` — runs every (class × workload-supported-here) cell.
- `rushbench report <results-dir>` — emits a markdown summary (no opinions, only
  what the data says); used by the SPEC §6 gate review.
- Default: refuse to run on AC unless `--ac-ok`. Default: refuse to run if
  `optctl status` is not reachable.

### 5.7 What the rig MUST report on, even when uncomfortable
- Per class, observed average watts on battery vs the implicit assumption that
  deeper floors should yield lower drain. If `idle` does not draw less than
  `interactive`, the rig says so. The rig's job is to surface the gap, not hide
  it.
- For `latency-critical`, observed `cyclictest-max-us` vs the declared
  `cpu_wakeup_latency_us = 10`. If the observation blows the budget, the rig
  flags it. This is the whole point of B1.

---

## 6. Tests required (verifier PASS criteria) — each with a stated kill condition

For each test, the **kill condition** is the specific observable that, if it
occurs, MUST cause the test to be marked FAIL. Green ≠ hollow: every PASS is a
PASS against a named failure mode.

- **T1 — energy probe wrap/AC-switch rejection.**
  Synthetic counter stream containing (a) a wrap and (b) an AC→battery flip
  mid-window. Probe MUST return `Err(WrapOrAcSwitch)` for both.
  *Kill condition:* probe returns a finite watts value for either case → FAIL.

- **T2 — energy probe arithmetic.**
  Synthetic monotonic counter, known endpoints, known elapsed time. Computed
  joules and avg watts match analytic values within 1%.
  *Kill condition:* avg watts off by >1% from analytic on a noiseless stream
  → FAIL (means the math itself is wrong, not hardware noise).

- **T3 — class readback enforcement.**
  Mock `optctl status` returning a class ≠ requested. Run MUST abort with
  `class_mismatch` recorded.
  *Kill condition:* a result JSON is written with `class_requested !=
  class_observed` and no `class_mismatch` anomaly → FAIL.

- **T4 — schema freeze.**
  Snapshot test against a golden `schema_version: 1` record. Any field added,
  renamed, removed, or retyped without bumping `schema_version` fails.
  *Kill condition:* schema diff against golden is non-empty AND
  `schema_version` is still `1` → FAIL.

- **T5 — N<5 honesty.**
  Forced single-run cell. The record MUST mark `n: 1` AND carry an
  `insufficient_n` anomaly; the report MUST NOT roll it up into the median for
  that cell.
  *Kill condition:* an N=1 record contributes to a class-level median in
  `rushbench report` output → FAIL.

- **T6 — no-write guarantee.**
  `strace -e trace=write,openat -f rushbench run …` on a sandboxed host. The
  only writes outside `benchmarks/results/**` and stdout/stderr/log paths are
  to `/tmp` scratch. No write to `/proc/sys/**`, `/sys/devices/**/power/**`,
  `/dev/cpu_dma_latency`, `/sys/class/powercap/**/constraint_*_power_limit_uw`.
  *Kill condition:* any write under those paths appears in the trace → FAIL.
  This is the test that B1 has not silently become an actuator.

- **T7 — provenance completeness.**
  Every emitted JSON has non-empty `host.kernel`, `host.cpu_model`,
  `rush.optid_sha`, `rush.contracts_sha256`, `rush.rig_sha`, `power_source`,
  `class_observed`, `resolved_floors`, `started_at`.
  *Kill condition:* a single emitted record has any of those fields `null`,
  `""`, or absent → FAIL. A row without provenance is a fabricated row (§2.9).

- **T8 — `latency-critical` honesty path.**
  On a host where `cyclictest-max-us` empirically exceeds
  `contracts.toml[latency-critical].cpu_wakeup_latency`, the rig MUST NOT
  silently pass: the report MUST surface a `budget_violation` flag for that
  cell. Test fixture forces this with an injected high reading.
  *Kill condition:* fixture forces a budget breach and `rushbench report`
  output contains no `budget_violation` marker for that cell → FAIL. (This is
  the test that prevents B1 from becoming a rubber-stamp for whatever numbers
  contracts.toml happens to contain.)

- **T9 — host-reject when no energy counter present.**
  Mocked sysfs with neither `energy_now` nor `energy_uj`. `rushbench run` MUST
  refuse to start, exit non-zero, and emit `no_energy_counter` reason.
  *Kill condition:* the command runs to completion and writes a record with
  `"energy": null` instead of refusing → FAIL.

- **T10 — workspace gates.**
  `cargo fmt`, `cargo test --workspace`, `cargo clippy -D warnings` green.
  *Kill condition:* any of the three non-zero → FAIL.

---

## 7. Docs to update in the SAME PR

- `IMPLEMENTATION_STATUS.md` — new row: measurement rig (`rushbench`)
  implemented; status `defined, runs locally, no committed results yet`.
  Explicitly state: contracts.toml values **remain provisional**; this PR ships
  the tool, not the validation dataset.
- `docs/SPEC-northstar.md` — §6 WP-B1 row: clarify scope split. Add a
  one-sentence note that B1's first deliverable is the rig + single-host
  evidence; the cross-distro PPD/TLP/baseline comparison is a follow-up under
  the same WP row. Do not move the gate language — the §6 gate still says no
  enabler ships without B1 evidence; it just now means *evidence produced by
  this rig*.
- `docs/testing-and-benchmarks.md` — how to run `rushbench`, what
  `benchmarks/results/` means, the schema, the no-write guarantee.
- `benchmarks/results/.gitkeep` + a short `benchmarks/results/README.md`
  describing the directory layout and that committed result files are
  human-reviewed snapshots, not CI artefacts.
- `AGENTS.md` (if it references B1): keep §7's no-fabrication rule applicable
  to B1 outputs — agents may run the rig but may not author result JSONs.

---

## 8. PR instructions

```bash
git checkout -b feat/wp-b1-measurement-rig
# ... commits ...
git push -u origin feat/wp-b1-measurement-rig
gh pr create \
  --title "feat(rushbench): WP-B1 measurement rig for contract validation" \
  --body "Implements SPEC-northstar §6 WP-B1 (rig half): single-host harness that measures battery drain (BAT/energy_now or intel-rapl) and responsiveness (manifest.toml metrics) per SPEC §1 class, pinning class via optctl and reading resolved floors from N2's status JSON. No actuation, no tuning of contracts.toml — observe-only. Result schema v1 frozen under benchmarks/results/. Tests T1–T10 green with kill conditions stated in WP-B1-gemini-workplan.md §6. Cross-distro PPD/TLP/baseline comparison is OUT OF SCOPE for this PR and tracked as B1 follow-up."
```

- Explicit `--title` / `--body`; never `--fill`. **Open as Draft** until verified.
- **Honest PR body.** State plainly: no result data is committed in this PR; the
  contracts.toml values remain provisional; the §6 gate is not yet satisfied —
  this PR makes satisfying it *possible*, not done.

---

## 9. Definition of done — post this verdict

```
WP-B1 VERDICT: PASS|FAIL
T1  wrap/AC-switch rejection:      PASS|FAIL  <evidence>
T2  energy probe arithmetic:       PASS|FAIL  <evidence>
T3  class readback enforcement:    PASS|FAIL  <evidence>
T4  schema freeze (v1):            PASS|FAIL  <evidence>
T5  N<5 honesty:                   PASS|FAIL  <evidence>
T6  no-write guarantee (strace):   PASS|FAIL  <evidence>
T7  provenance completeness:       PASS|FAIL  <evidence>
T8  latency-critical honesty path: PASS|FAIL  <evidence>
T9  host-reject w/o energy ctr:    PASS|FAIL  <evidence>
T10 fmt/test/clippy:               PASS|FAIL  <CI link>
Docs synced:                       YES|NO     <files>
Observe-only (no actuation writes outside results dir): YES|NO  <strace>
contracts.toml unchanged in this PR:                    YES|NO  <git diff>
No result JSONs hand-authored (only rig-emitted):       YES|NO  <provenance check>
```

FAIL any line → not ready. Human owns merge.

---

## 10. Out of scope (explicit)

- Tuning `config/optid/contracts.toml` (separate PR after this rig's data lands
  and is reviewed by a human).
- Cross-distro comparison vs PPD / TLP / `power-profiles-daemon` / `tuned` /
  vendor stacks. The §6 B1 row eventually requires this; this PR is the
  prerequisite rig, not the comparison run.
- Multi-host orchestration, CI runners, cloud test fleets.
- New workload classes, renamed metrics, or any change to
  `benchmarks/manifest.toml`'s metric vocabulary.
- New levers, new actuators, new sysfs writers. B1 is observe-only.
- GUI / dashboard. `rushbench report` emits markdown; visualisation is later.
- Wiring `fits_contract()` (N2's unwired helper) to devices — that is N5/N6.
- Any "smart" budget recommendation, ML-driven tuning, or auto-adjustment loop.
  The rig surfaces gaps; humans decide what to do about them.

If the work seems to need any of the above, it doesn't — stop and report.
