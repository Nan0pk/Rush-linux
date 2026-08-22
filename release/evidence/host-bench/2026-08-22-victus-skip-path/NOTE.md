# On-battery skip-path capture — HP Victus 16-r0xxx, 2026-08-22

**What this is.** `optid --apply` running as root on real hardware, unplugged,
with the workload class pinned to `idle`, for 52 control cycles. Every target it
considered is recorded with the gate that stopped it.

**What this is not.** It is not a baseline-vs-optid comparison and carries no
timing or energy numbers. It establishes only that the daemon states a reason
for every setting it declines to write.

## Setup

See `meta.txt` for the literal host facts. In short: Fedora 44, kernel
7.1.8-200.fc44, i7-13700HX, `intel_pstate`, board `HP`/`8BC2`, on battery
(`ACAD/online=0`, `BAT1` discharging), `tuned` stopped for the run and restarted
afterwards, `optid-recover` run first as the packaged units require.

Two deliberate interventions, both recorded here because they change what the
transcript means:

1. **The class was pinned to `idle`.** The deeper per-device settings are
   nominated only when on battery *and* the class is `idle`
   (`policy.rs:1146`), and `idle` requires a 1-minute load average at or below
   0.05. This machine idles at 0.31 with a normal desktop session, so the branch
   is unreachable without the pin. That is worth its own attention: on a live
   desktop, optid's device-level power savings never engage.

2. **A latched global circuit was cleared first.** An earlier run this evening
   left the breaker open with a stale reason (`StaleGeneration` on
   `user.slice:property:CPUWeight`) from a defect fixed the same evening. While
   it was open, every domain was denied at the `apply_armed` stage, so nothing
   downstream was ever evaluated. The latched file is kept here as
   `latched-circuit-before-clear.json`.

   **Correction.** This note first said there was no supported way to clear a
   latched circuit. That was wrong: `optid --clear-all-circuits` and
   `optid --clear-circuit-domain DOMAIN` both exist, are one-shot, require
   effective UID 0, and refuse to run alongside `--apply`
   (`main.rs:145`). Verified: `optid --clear-all-circuits` reported
   `cleared 20 S5D circuit record(s)`. The file for this capture was cleared by
   hand before that was found, which is why the copy is kept here.

   What remains true is that nothing tells you to run it. The daemon keeps
   starting, keeps logging, and keeps reporting `apply_armed` denials with the
   original fault as their detail — a fault that may have been fixed long ago.
   Nothing in the status output says "a latch is holding this open, clear it
   with X".

## What the cycles show

Every one of the 46 considered targets was refused, with a full gate chain:

| Domain | Targets | Where it stopped |
|--------|---------|------------------|
| `runtime_pm` | 27 | `capability_validation: capability_denied` (11), `apply_armed: apply_disarmed_by_boot_state` (16, while the latch was still open) |
| `sata_alpm` | 8 | `capability_validation: capability_denied` |
| `vm_sysctl` | 3 | `capability_validation: capability_denied` |
| `pci_aspm` | 2 | `capability_validation: capability_denied` |
| `cpu_epp`, `backlight` | 1 each | `capability_validation: capability_denied` |
| `platform_profile`, `cpu_dma_latency` | 1 each | `apply_armed: apply_disarmed_by_boot_state` |
| `cgroup_reweight` | 2 | passed every gate; `write=redundant`, the value was already correct |

So the "states a reason" behaviour holds, and holds richly: each target carries
its pipeline stage, disposition, typed reason, and detail.

## The finding that matters more

**Nothing was written, and nothing can be.** `config/optid/policy.toml` ships
`[safety] capability_sealing = "observe"`, which suppresses every non-systemd
kernel write. `capability_validation` denies before the hardware allowlist is
ever consulted, so the `entry_unverified` refusal this capture was opened to
record cannot occur in the shipped configuration at all. The only lever that
reached a write was the systemd cgroup weight, and its value was already
correct.

A baseline-vs-optid comparison run against this configuration would show no
difference, because optid changes nothing — and would read as "optid does not
help" rather than "optid did not act". The policy comment gates `enforce` on the
D0 mechanism proof, which `docs/IMPLEMENTATION_STATUS.md` lists as unfinished.
That, not machine time, is what stands between here and a meaningful comparison.

Left as found: `tuned` active, no daemon running, `user.slice` weights unset,
EPP back at `balance_performance`.
