# Reference Hardware — v0.6 Phase D (D1)

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

| Field | Value |
|-------|-------|
| Machine (make/model) | _TBD_ |
| CPU | _TBD_ |
| GPU | _TBD_ |
| RAM | _TBD_ |
| `dmi_board` (board name) | _TBD_ |
| Baseline distro | _TBD (suggested: Ubuntu 24.04 LTS, PPD `balanced`)_ |
| Physical-access owner | _TBD_ |
| Battery present | No → **Criterion 3 = N/A** |
| HWID seeded in allowlist? | _TBD — confirm `config/optid/hardware-allowlist.toml` covers this board_ |

### Laptop slot

| Field | Value |
|-------|-------|
| Machine (make/model) | _TBD_ |
| CPU | _TBD_ |
| GPU (iGPU/dGPU) | _TBD_ |
| RAM | _TBD_ |
| `dmi_board` (board name) | _TBD_ |
| Baseline distro | _TBD (suggested: Ubuntu 24.04 LTS, PPD `balanced`)_ |
| Physical-access owner | _TBD_ |
| Battery present | Yes → **Criterion 3 in scope** |
| Battery design capacity (µWh) | _TBD (from `/sys/class/power_supply/BAT0/energy_full_design`)_ |
| HWID seeded in allowlist? | _TBD — confirm `config/optid/hardware-allowlist.toml` covers this board_ |

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
