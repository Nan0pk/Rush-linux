# Reference Hardware — v0.6 Phase D (D1)

> **⚠️ ACTION REQUIRED (2026-07-20, FINAL-AUDIT-REPORT.md section 5.4):**
> Both slots below are unfilled. This blocks v0.6 Phase D, which blocks
> v0.6 milestone closure, which blocks v0.7 edition validation. Fill both
> slots (machine make/model, CPU, GPU, RAM, dmi_board, baseline distro,
> physical-access owner, HWID allowlist status), then on each machine run:
>
> ```sh
> # Capture a baseline (Ubuntu 24.04 LTS, PPD balanced)
> curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/rush-host-bench.sh \
>   | sudo bash -s -- --submit --mode baseline
>
> # Capture an optid run (with --apply)
> curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/rush-host-bench.sh \
>   | sudo bash -s -- --submit --mode optid --apply
> ```
>
> See FINAL-AUDIT-REPORT.md section 5.4 for the full Phase D protocol
> and section 10 (Roadmap, 30-day) for the closure checklist.

> **Status:** ⬜ Awaiting project-owner nomination. This file is the canonical
> record of the two physical reference machines used to certify the
> `0.6.0-beta.1` ("Hardware-Aware optid") quantitative exit criteria:
>
> - **Criterion 2** — *mixed-load responsiveness improves on two machines*
> - **Criterion 3** — *battery behavior matches or improves mainstream defaults*
>
> Until both slots below are filled and physical access is confirmed, Phase D
> (D3–D5) is a hard gate and cannot proceed. See
> `docs/plans/v0.6-hardware-aware-optid-proposal.md` §"Phase D" for the full
> protocol.

## Why two machines

The v0.6 milestone exit criteria require the responsiveness improvement to hold
on **two** physically distinct machines so the result is not an artifact of a
single SKU. One must be battery-equipped so Criterion 3 (battery behavior) can
be measured at all. The desktop slot has no battery and is therefore `N/A` for
Criterion 3.

## Slot definitions

| Slot | Suggested profile | Criteria it certifies |
|------|-------------------|------------------------|
| **Desktop** | Modern Intel or AMD, 6+ cores, discrete GPU, always on AC | Criterion 2 (throughput + latency-critical classes). Criterion 3 = N/A (no battery). |
| **Laptop** | Modern Intel U-class (or similar), iGPU, battery, runs on both AC and battery | Criterion 2 (idle/light/interactive classes) **and** Criterion 3 (battery). Exercises `platform_profile` on real hardware. |

## Nomination (to be filled by the project owner)

The project owner must record, for each slot: (a) the specific machine, (b) who
holds physical access for the benchmark runs, and (c) the baseline distro.

> **Suggested baseline (per proposal):** Ubuntu 24.04 LTS, default install, with
> `power-profiles-daemon` in its default `balanced` profile — the most common
> mainstream default and the fair comparison point for Criterion 3.

### Desktop slot

> **TODO (human decision required):** Fill every `<!-- FILL -->` field below.
> Until all fields are populated, the v0.6 Phase D baseline run cannot start.

| Field | Value |
|-------|-------|
| Machine (make/model) | <!-- FILL: e.g. "Custom desktop, i7-12700K" --> |
| CPU | <!-- FILL: e.g. "Intel i7-12700K, 12C/20T" --> |
| GPU | <!-- FILL: e.g. "Intel UHD 770 (iGPU) or RTX 4070 (dGPU)" --> |
| RAM | <!-- FILL: e.g. "32 GB DDR4-3200" --> |
| `dmi_board` (board name) | <!-- FILL: run `cat /sys/class/dmi/id/board_vendor` + `/sys/class/dmi/id/board_name` --> |
| Baseline distro | <!-- FILL: e.g. "Ubuntu 24.04 LTS, PPD balanced" (recommended) --> |
| Physical-access owner | <!-- FILL: e.g. "@Nan0pk" --> |
| Battery present | No → **Criterion 3 = N/A** |
| HWID seeded in allowlist? | <!-- FILL: yes/no — check `crates/optid/data/allowlist.toml` (the compiled-in seeded baseline; `config/optid/hardware-allowlist.toml` does not exist) and add this board if missing --> |

### Laptop slot

**Nominated 2026-08-22 by the project owner (@Nan0pk), who holds physical
access.** Every field below is a literal value read from the machine, not a
suggestion. The defective 2026-06-10 sample from this same laptop stays retired;
this nomination requires a clean re-capture through
[`tools/phase-d-capture.sh`](../../tools/phase-d-capture.sh).

| Field | Value |
|-------|-------|
| Machine (make/model) | HP `Victus by HP Gaming Laptop 16-r0xxx` |
| CPU | Intel Core i7-13700HX, 24 threads, `intel_pstate` |
| GPU (iGPU/dGPU) | Intel UHD Graphics 770 (`00:02.0`, Alder Lake-HX GT1). **No discrete GPU is enumerated by `lspci`** on this unit, so D4 (conservative dGPU runtime PM) cannot be exercised here and needs the desktop slot or a second laptop. |
| RAM | 16 GB (`MemTotal: 16034280 kB`) |
| `dmi_board` (board name) | `HP` / `8BC2`, BIOS `F.31` |
| Baseline distro | **Fedora 44, `tuned` in its default `balanced` profile.** This deviates from the suggested "Ubuntu 24.04 + PPD balanced": on Fedora 44 `power-profiles-daemon` is inactive and `tuned` is the shipped default, so `tuned balanced` — not PPD — is what "mainstream defaults" means on this machine. Recorded in each arm's `meta.txt`. |
| Physical-access owner | @Nan0pk |
| Battery present | Yes → **Criterion 3 in scope** |
| Battery design capacity (µWh) | `70070000` (`/sys/class/power_supply/BAT1/energy_full_design` — note `BAT1`, not `BAT0`) |
| HWID seeded in allowlist? | No. The seeded baseline in `crates/optid/data/allowlist.toml` carries 14 entries and none is this board; every seeded row is `verified = false` regardless, so per-device depth writes answer `entry_unverified` on this host. That denial path is Criterion 1's evidence ("unsupported knobs are skipped with reasons"), not a blocker: the global levers (EPP, platform profile, VM sysctls, `cpu_dma_latency`, cgroup weights) are not allowlist-gated and do actuate. |

#### Known hardware quirk found during nomination

`/sys/class/power_supply` on this board enumerates `BAT1`, an idle
`ucsi-source-psy-USBC000:001` USB-C source, and then the online `ACAD` barrel
jack. `optid`'s `read_on_ac` answered from the first external supply it walked
and so reported battery power with the charger attached, which put the daemon on
its battery policy branch while on mains. Fixed before any capture; see the
`read_on_ac_with` aggregation rule in
[`docs/adaptive-engine.md`](../adaptive-engine.md).

## The existing HP Victus sample is NOT a candidate-by-default

A laptop is already in the loop: the **HP Victus (i7-13700HX, 24 cores,
Fedora 44)** that produced the `2026-06-10` ambient-telemetry sample at
`release/evidence/host-bench/2026-06-10-victus/`.

That sample is **defective and is not evidence** — see its
[`NOTE.md`](../../release/evidence/host-bench/2026-06-10-victus/NOTE.md) (the
`optid_version` field captured usage text, and `transcript.log` begins
mid-line). The owner may re-use this machine **only with a clean re-capture**
following the Dragnet `meta.txt` template, or nominate a different laptop.

## Definition of done for D1

- [ ] Desktop slot filled (machine, access owner, baseline distro).
- [x] Laptop slot filled (machine, access owner, baseline distro, battery present) — 2026-08-22.
- [ ] Both boards' HWIDs recorded in `crates/optid/data/allowlist.toml`. This is
      a *record*, not an enablement: seeded rows are `verified = false`, so
      per-device depth writes stay denied with `entry_unverified` until a
      separate promotion decision (I3) marks a row verified against committed
      evidence. The non-allowlisted global levers actuate either way, so D4 is
      not blocked on it.
- [ ] Physical access confirmed for both, with lead time for ~30-minute runs ×
      baseline + optid (≈ 4 runs total across the two machines, ×5 repeats each).

Once this file's two slots are filled, Phase D2 (workload definition,
[`mixed-load-workload.md`](mixed-load-workload.md)) and D3 (baseline runs) may
begin.
