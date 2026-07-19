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
| HWID seeded in allowlist? | <!-- FILL: yes/no — check `config/optid/hardware-allowlist.toml` and add this board if missing --> |

### Laptop slot

> **TODO (human decision required):** Fill every `<!-- FILL -->` field below.
> The HP Victus (i7-13700HX, 24 cores, Fedora 44) is a candidate but its
> existing sample is defective (see "HP Victus sample" section below) and
> must be re-captured cleanly if reused.

| Field | Value |
|-------|-------|
| Machine (make/model) | <!-- FILL: e.g. "HP Victus 16 (i7-13700HX)" or "ThinkPad T14 Gen 4" --> |
| CPU | <!-- FILL: e.g. "Intel i7-13700HX, 8P+8E, 24 threads" --> |
| GPU (iGPU/dGPU) | <!-- FILL: e.g. "Intel UHD Graphics 770 (iGPU) + RTX 4050 (dGPU)" --> |
| RAM | <!-- FILL: e.g. "32 GB DDR5-4800" --> |
| `dmi_board` (board name) | <!-- FILL: run `cat /sys/class/dmi/id/board_vendor` + `/sys/class/dmi/id/board_name` --> |
| Baseline distro | <!-- FILL: e.g. "Ubuntu 24.04 LTS, PPD balanced" (recommended) --> |
| Physical-access owner | <!-- FILL: e.g. "@Nan0pk" --> |
| Battery present | Yes → **Criterion 3 in scope** |
| Battery design capacity (µWh) | <!-- FILL: run `cat /sys/class/power_supply/BAT0/energy_full_design` --> |
| HWID seeded in allowlist? | <!-- FILL: yes/no — check `config/optid/hardware-allowlist.toml` and add this board if missing --> |

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
- [ ] Laptop slot filled (machine, access owner, baseline distro, battery present).
- [ ] Both boards confirmed present in `config/optid/hardware-allowlist.toml`
      (so `optid --apply` operates on allowlisted HWIDs in D4).
- [ ] Physical access confirmed for both, with lead time for ~30-minute runs ×
      baseline + optid (≈ 4 runs total across the two machines, ×5 repeats each).

Once this file's two slots are filled, Phase D2 (workload definition,
[`mixed-load-workload.md`](mixed-load-workload.md)) and D3 (baseline runs) may
begin.
