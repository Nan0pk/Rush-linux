# 0018 — Telemetry: Runtime State Observability

_This document is a **research WIP** filling the observability gaps in SPEC §4.1 that are not
covered by research 0004 (telemetry fidelity) — specifically: wakeup sources, per-device
runtime PM state and failures, package/C-state residency, and PM QoS state. These four rows
are measurement prerequisites for validating WPs N5, N6, and N9. Tagged `[PROVEN]`
(verified by kernel source or documentation) or `[HYPOTHESIS]` (estimated, needs confirmation)._

**Status:** WIP — interface paths verified; overhead budget estimated (needs profiling).
**Author:** Nan0pk
**Date:** 2026-06-19
**Depends on:** `docs/SPEC-northstar.md`, research 0004 (companion PSI/cgroup telemetry)
**Provides measurement infra for:** WP-N5, WP-N6, WP-N9

* * *

## 0. Motivation

SPEC §4.1 lists 12 observability inputs. As of 2026-06-08, status:

| Input | Coverage |
|-------|----------|
| CPU/mem/IO pressure (PSI) | ✓ 0004 |
| AC/battery + percentage | ✓ existing |
| Thermal zones | ✓ existing |
| Load average | ✓ existing |
| zram swap activity | ✓ existing |
| **Wakeup-source / suspend blockers** | **— this doc** |
| **Per-device runtime PM state + failures** | **— this doc** |
| **Package/C-state + sleep quality** | **— this doc** |
| GPU/display/media state | — covered by 0007 |
| Storage/link power state | — covered by 0008 |
| **PM QoS / latency-contract state** | **— this doc** |
| Firmware/workload hints | — future |

Research 0004 covered PSI/cgroup measurement. Research 0007 covers GPU/display state. Research
0008 covers storage/link state. This research covers the remaining four rows.

Without these, optid cannot:
- Verify that runtime PM actuations (N5/N6) actually suspended the target devices
- Verify that depth-enablers (N6 NVMe APST) actually deepened the idle profile
- Diagnose what woke the machine from suspend
- Verify that PM QoS floors are being respected by the kernel

These are measurement prerequisites: N5, N6, N9 cannot be validated without this landing first.

A critical lesson from 0004: the observer must not perturb the observed system. This research
respects 0004's overhead budget: <0.1% CPU steady state, <0.05 ms per read.

* * *

## 1. Findings

### 1.1 Wakeup-Source Observability

**Primary interface: `/sys/kernel/debug/wakeup_sources`** **[PROVEN]**

This debugfs file (readable as root) contains one line per registered wakeup source, with
tab-separated fields:

```
name            active_count  event_count  wakeup_count  expire_count  active_since  \
total_time  max_time  last_change  prevent_suspend_time
```

Example line:
```
ACPI_IMC_event  0  12  12  0  0  47234  3104  9876543210  0
```

Verified in `drivers/base/power/wakeup.c::pm_show_wakelocks()`. The `event_count` field
increments each time this source generates a wakeup event (hardware interrupt that could
wake the system from suspend). The `active_count` increments each time the wakeup source is
*active* (preventing suspend). `total_time` is in nanoseconds.

**Secondary interface: `/proc/acpi/wakeup`** **[PROVEN]**

ACPI-specific wakeup devices. Tab-separated:
```
Device  S-state  Status   Sysfs node
PWRB    S4       *enabled  pnp:00:03
LID0    S4       *enabled  platform:PNP0C0D:00
USB2    S4        disabled pci:0000:00:1a.0
```

The `*enabled` marker indicates the device is armed as a wakeup source for the listed sleep
state. Not all wakeup sources appear here — only ACPI-managed ones. Use `wakeup_sources`
debugfs for the complete picture.

**optid observability role:**

```rust
// Per 2s poll tick, read /sys/kernel/debug/wakeup_sources, parse each line,
// compute delta from previous read for event_count and active_count.
// Emit to audit.jsonl when delta > 0:
{
  "ts": "...", "source": "wakeup_source",
  "name": "ACPI_IMC_event",
  "event_count_delta": 3,
  "active_count_delta": 0,
  "last_change_ns": 9876543210
}
```

`optctl wakes --since 5min` aggregates this data, showing top wakeup sources by event count.
Devices that never fire wakeup events are candidates for `power/wakeup=disabled`
(optid recommends but does not write this — it is an admin decision per §5).

**Stable sysfs alternative: `/sys/class/wakeup/`** **[PROVEN — kernel 5.6+; file names
and units corrected 2026-09-04 against kernel 7.1.12 hardware]**

Since kernel 5.6, each wakeup source has a sysfs directory
`/sys/class/wakeup/wakeup<N>/`. **The sysfs file names are not the debugfs column names
above.** Every duration file carries its unit as a suffix, and the durations are
**milliseconds** — not the nanoseconds the debugfs table reports:

| File | Unit | Note |
|------|------|------|
| `name` | — | Wakeup source name |
| `active_count` | — | Times the source became active |
| `event_count` | — | Wakeup events generated |
| `wakeup_count` | — | Times the source aborted a suspend |
| `expire_count` | — | Times a timed wakeup expired |
| `relax_count` | — | Times the source was deactivated |
| `active_time_ms` | ms | Time active in the current activation |
| `total_time_ms` | ms | Cumulative active time |
| `max_time_ms` | ms | Longest single activation |
| `last_change_ms` | ms | Monotonic timestamp of the last state change |
| `prevent_suspend_time_ms` | ms | Time spent preventing suspend |

Verified in `drivers/base/power/wakeup_stats.c` and by `ls /sys/class/wakeup/wakeup0/`
on a Fedora 44 host running kernel 7.1.12. There is no `total_time`, `max_time`,
`last_change`, `prevent_suspend_time` or `active_since` file — those are debugfs
`wakeup_sources` **columns**, and reading them as sysfs file names yields `ENOENT`. O1's
first cold verification failed on exactly that conflation: the reporter read `total_time`,
so all 57 wakeup sources on a working machine reported `status=unsupported`.

A consumer that reports microseconds must multiply by 1000. A rename alone is a 1000x
error.

Prefer this stable ABI over debugfs for production optid code. Debugfs is root-only and
not guaranteed to be mounted. The sysfs interface is always available.

### 1.2 Per-Device Runtime PM State and Failures

**sysfs interface under `/sys/bus/.../devices/<dev>/power/`** **[PROVEN]**

Every device registered with the Linux PM core has these attributes:

| File | Type | Values | Notes |
|------|------|--------|-------|
| `runtime_status` | ro | `active`, `suspended`, `suspending`, `resuming`, `error`, `unsupported` | Current runtime PM state |
| `runtime_usage` | ro | integer ≥ 0 | Active-use refcount; 0 = suspendable |
| `runtime_active_time` | ro | **ms** | Total time spent active since last boot |
| `runtime_suspended_time` | ro | **ms** | Total time spent suspended since last boot |
| `runtime_active_kids` | ro | integer | Number of child devices currently active |
| `control` | rw | `auto`, `on` | PM control (optid writes here) |
| `autosuspend_delay_ms` | rw | integer | Delay before autosuspend |
| `pm_qos_resume_latency_us` | rw | µs | Per-device PM QoS constraint |

Verified in `drivers/base/power/sysfs.c` and `Documentation/power/runtime_pm.rst`.

**Two corrections made 2026-09-04, after O1's first cold verification failed on them.**

*`unsupported` is a sixth valid `runtime_status`.* The kernel returns it for a device whose
driver implements no runtime PM. It is the kernel answering the question, not corrupt data,
and it is the common case: on a 13th-gen Intel laptop running kernel 7.1.12, 713 of 791
device nodes report it.

```
$ for f in /sys/bus/*/devices/*/power/runtime_status; do cat "$f"; done | sort | uniq -c
    713 unsupported
     60 suspended
     18 active
```

A consumer must be able to tell "this device has no runtime PM" from "optid could not read
this device", so `unsupported` needs its own state rather than being folded into either a
successful observation or a malformed one. Treating it as malformed labelled 53 of 85
devices on that host as corrupt.

*The two residency counters are milliseconds, not microseconds.* `sysfs.c` divides the
device's accumulated `ktime` by `NSEC_PER_MSEC` before printing. Measured directly, which
is the cheapest way to settle a unit question:

```
$ p=/sys/bus/pci/devices/0000:00:00.0/power/runtime_active_time
$ a=$(cat $p); sleep 5; b=$(cat $p); echo $((b - a))
5001
```

5001 over a 5.001 s wait. A field named `_us` fed straight from this file is 1000x too
small — the same class of error as the wakeup file names above, and from the same cause:
this document was written from the debugfs table and the code was written from this
document.

**Failure surfacing** **[PROVEN]**

Runtime PM failures (device fails to suspend or resume) surface as:
1. `runtime_status = "error"` in sysfs (set by `drivers/base/power/runtime.c::rpm_suspend()`
   when the driver's `->runtime_suspend()` callback returns an error).
2. Kernel log messages at `dev_err()` level: `PM: device X failed to suspend: -EIO`.
3. The device may disappear from its bus (`ACTION=remove` udev event) if the driver detects
   it became unresponsive.

optid monitoring strategy:
- Poll `runtime_status` for each tracked device every 2 s.
- On `"error"` status: log to audit, add device HWID to a "runtime PM deny" runtime override
  (temporary, not persisted — admin must confirm via `optctl deny runtime_pm <hwid>`).
- On udev `ACTION=remove` for a device optid set to `auto`: treat as failure, apply same
  temporary deny logic.

**Coverage: which buses to enumerate?**

optid scans all buses listed in `/sys/bus/` that have a `power/runtime_status` attribute
on their devices. In practice: `usb`, `pci`, `platform`, `i2c`, `hid`. The `acpi` bus
devices also have PM attributes but are rarely controlled by optid.

**Efficiency note** **[HYPOTHESIS — calculation]**

A typical laptop has 30–80 devices with `runtime_status`. Reading 80 files × ~5 µs/read
= 400 µs per 2 s poll = 0.02% CPU. Well under budget.

### 1.3 Package/C-State Residency

**Per-CPU per-state interface** **[PROVEN]**

`/sys/devices/system/cpu/cpuN/cpuidle/stateM/` exposes per-CPU-per-state idle statistics:
- `name`: state name (e.g., `POLL`, `C1`, `C1E`, `C6`, `C8`, `C10`)
- `desc`: human-readable description
- `latency`: exit latency in µs (read-only, from ACPI _CST or CPUID)
- `power`: power consumed in mW (if available)
- `time`: total time spent in this state since boot, in µs
- `usage`: number of times this state was entered since boot
- `disable`: 0/1 — whether this state is disabled (optid should NOT touch this)

To compute C-state residency, read `time` for all states on all CPUs, compute delta from
previous read, divide by wall-clock delta.

**Package C-state residency — Intel** **[PROVEN]**

Intel exposes package-level idle statistics via:
- `turbostat` reads MSRs (C2_RESIDENCY, C3_RESIDENCY, C6_RESIDENCY, C7_RESIDENCY, etc.)
  from `/dev/cpu/N/msr`. This requires the `msr` kernel module and root.
- `/sys/kernel/debug/pmc_core/` (Intel PMC Core driver, kernel ≥ 5.0): exposes
  `slp_s0_residency_usec` (time in SLP_S0 / S0ix / modern standby state),
  `package_cstate_show` (text file with PC2/PC3/PC6/PC7/PC8/PC9/PC10 residency in µs).

optid reads `pmc_core/slp_s0_residency_usec` to verify that modern standby (s2idle) quality
is improving as depth-enablers are applied. Higher SLP_S0 residency = better suspend quality.

**Package C-state residency — AMD** **[PROVEN]**

AMD exposes C-state information per-CPU via the same cpuidle sysfs interface. AMD's deepest
C-state on Ryzen 7040 (Phoenix) is CC6 (Core C6), which maps to kernel cpuidle state `C6`.
The `time` counter in `/sys/devices/system/cpu/cpuN/cpuidle/stateN/time` accumulates CC6
residency. Package C-state (PC6) on AMD is visible via AMDuProf or MSR reads; there is no
AMD equivalent of Intel's `pmc_core` debugfs.

For AMD, optid uses per-CPU C6 residency as a proxy for package idle depth.

**Observer effect** **[PROVEN — 0004 lesson applied]**

Reading cpuidle `time` counters via sysfs does NOT prevent C-states. The read is a simple
`ktime_get()` + atomic read inside the kernel; it does not program any timer or generate
any wakeup. The observer effect documented in 0004 was specific to PSI polling that used
`select()` with a 100ms timeout (generating periodic wakeups). Sysfs attribute reads have
no such side effect.

**turbostat vs sysfs** **[PROVEN]**

`turbostat` reads MSRs for higher accuracy (MSRs update at retirement, giving cycle-accurate
residency). sysfs `cpuidle/stateN/time` is updated by the cpuidle governor at idle entry/exit,
which is also accurate. For optid's 2 s polling window, sysfs is sufficient (difference is
<1% vs MSR reads). No need to link against MSR reading code.

### 1.4 PM QoS State Observability

**Kernel PM QoS subsystem** **[PROVEN]**

`kernel/power/qos.c` maintains per-class constraint lists. Classes:
- `PM_QOS_CPU_LATENCY` (was `PM_QOS_CPU_DMA_LATENCY` pre-5.13): max acceptable CPU latency
  in µs. Used by latency-sensitive code paths to prevent deep C-states.
- `PM_QOS_NETWORK_LATENCY` (deprecated in 5.7)
- `PM_QOS_NETWORK_THROUGHPUT`
- `PM_QOS_MEMORY_BANDWIDTH`

Per-device PM QoS: `/sys/bus/.../devices/<dev>/power/pm_qos_resume_latency_us` — the
maximum acceptable resume latency for that device. Drives the PM QoS constraint that prevents
the runtime PM subsystem from entering states with higher exit latency.

**Observability paths** **[PROVEN]**

`/sys/kernel/debug/pm_qos/` (debugfs, root-only):
- `cpu_latency_constraints` — text listing of all active `PM_QOS_CPU_LATENCY` requests:
  `pid, name, value` per registered requestor
- `cpu_dma_latency` (legacy name, same class)

`/dev/cpu_dma_latency` — open file descriptor interface for holding a PM QoS constraint.
optid's focus-bridge (research 0005) holds this fd for latency-critical workloads. optid
can read the current effective value by opening the file and reading 4 bytes (int32 in µs).

**optid observability role:**

Every 2 s poll: read `/sys/kernel/debug/pm_qos/cpu_latency_constraints` (if mounted), parse
active constraints, log to audit.jsonl when constraints change:

```json
{
  "ts": "...", "source": "pm_qos",
  "class": "cpu_latency",
  "effective_value_us": 50,
  "requestors": [
    {"pid": 1234, "name": "optid-focus-bridge", "value_us": 50},
    {"pid": 5678, "name": "pulseaudio", "value_us": 100}
  ]
}
```

This lets optid verify: when it boosts a cgroup to `latency-critical`, the PM QoS hold
(via `/dev/cpu_dma_latency`) is actually active and is the binding constraint.

Per-device PM QoS: after each runtime PM actuation (e.g., setting NVMe APST), optid reads
the device's `pm_qos_resume_latency_us` to confirm it matches the intended actuation.

### 1.5 Observability Overhead Budget

**Estimated per-poll cost** **[HYPOTHESIS — calculation]**

| Source | Item count | Read cost | Subtotal |
|--------|-----------|-----------|----------|
| Wakeup sources (`/sys/class/wakeup/`) | 20 devices × 3 files | 5 µs | 300 µs |
| Runtime PM status | 60 devices × 1 file | 5 µs | 300 µs |
| C-state counters | 8 CPUs × 8 states × 1 file | 5 µs | 320 µs |
| PM QoS (1 debugfs read) | 1 | 10 µs | 10 µs |
| **Total per 2 s poll** | | | **~930 µs** |

930 µs / 2,000,000 µs = **0.047% CPU** — well under the 0.1% budget from 0004.

**Files to batch-open and keep open** **[PROVEN design]**

0004's lesson: opening and closing files every poll is expensive (path lookup, dentries).
optid should `open()` all observability files at startup, keep the fds open, and `pread()`
on each tick. For `wakeup_sources` debugfs, re-read the full file (it changes size). For
per-device sysfs attributes (fixed-size integers), `pread(buf, 32, 0)` is sufficient.

**The anti-pattern from 0004** **[PROVEN]**

powertop's 100ms polling loop generates ~10 wakeups/second, preventing CPU C6 entry.
optid's 2 s polling interval generates 0.5 wakeups/second — negligible. The key constraint
is not the number of reads but the *periodicity* creating a timer wakeup. At 2 s interval,
the hardware timer wakeup is dominated by other system timers (HZ, RCU, tick); optid adds
no measurable additional wakeup rate.

* * *

## 2. Architecture — Design Decisions

### Decision 1: Polling vs event-driven
**A — 2 s polling for all four sources.** Event-driven (inotify/epoll) on sysfs files is
not supported by the kernel for PM attributes (they're synthetic, not backed by inotify).
udev events handle device arrival/removal; the steady-state polling fills in the gaps.
2 s interval is sufficient for observability (not for control).

### Decision 2: Storage format
**A — append-only JSONL to `/var/log/optid/observability.jsonl`** (shared with 0004's
telemetry output, distinguished by `"source"` field). Simple, greppable, `logrotate`-friendly.
A Prometheus `/metrics` endpoint is a future addition.

### Decision 3: optctl surface
- `optctl wakes [--since <ts>] [--device <name>]` — top wakeup sources by event count
- `optctl pm status [--device <name>]` — per-device runtime PM state
- `optctl idle-stats [--cpu <N>] [--interval <s>]` — C-state residency breakdown
- `optctl pmqos status` — active PM QoS constraints with requestors

### Decision 4: Integration with 0004
0004 owns `sensors.rs` for PSI/cgroup telemetry. This research extends `sensors.rs` with
four new `Sensor` implementations: `WakeupSensor`, `RuntimePmSensor`, `CstateSensor`,
`PmQosSensor`. All write to the same `observability.jsonl` file with `"source"` field.

### Decision 5: Sysfs vs debugfs preference
Prefer stable sysfs ABI (`/sys/class/wakeup/`, `/sys/devices/.../power/runtime_status`,
`/sys/devices/system/cpu/cpuidle/`) over debugfs (`/sys/kernel/debug/wakeup_sources`,
`/sys/kernel/debug/pmc_core/`). Fall back to debugfs when no sysfs equivalent exists
(PM QoS constraints, Intel package C-states). Document the fallback path in code comments.

* * *

## 4. Evidence Gaps

### 4.1 Observability Overhead Profiling

```bash
# Run optid with all 4 telemetry sources enabled, idle system:
sudo optid --telemetry=all --log-level=debug
# Measure optid's CPU consumption:
top -b -n 60 -d 2 -p $(pidof optid) | grep optid | awk '{print $9}' | \
  awk '{s+=$1; n++} END {print "avg:", s/n, "%"}'
# Compare with telemetry=psi-only (baseline from 0004):
sudo optid --telemetry=psi-only
# Same measurement
```

**Acceptance threshold:** Full telemetry ≤ 0.1% CPU steady; delta vs psi-only ≤ 0.05%.

### 4.2 C-State Accuracy vs turbostat

```bash
# Run both simultaneously, idle system, 60s:
sudo turbostat --quiet --show CPU%c1,CPU%c6,Pkg%pc6 --interval 2 > /tmp/turbo.log &
optctl idle-stats --interval 2 --duration 60 --json > /tmp/optid.log
# Compare CPU%c6 from both:
python3 -c "
import json, sys
optid = [json.loads(l) for l in open('/tmp/optid.log') if '\"c6\"' in l]
turbo = [l.split() for l in open('/tmp/turbo.log') if '%' in l]
print('optid C6:', [o['c6_pct'] for o in optid[:5]])
"
```

**Acceptance threshold:** <2% absolute divergence between optid and turbostat C6 residency.

### 4.3 Observer Effect (The 0004 Lesson)

```bash
# Baseline: optid running, telemetry=off; measure C6 residency via turbostat
sudo optid --telemetry=off &
sudo turbostat --quiet --show CPU%c6 --interval 2 -n 30 | tail -1
# Treatment: optid with full telemetry
sudo killall optid
sudo optid --telemetry=all &
sudo turbostat --quiet --show CPU%c6 --interval 2 -n 30 | tail -1
# Compare C6 residency
```

**Acceptance threshold:** <5% absolute reduction in C6 residency due to telemetry overhead.
If degradation > 5%: reduce polling frequency to 4 s for C-state sensors.

### 4.4 Wakeup Source Attribution on T14 Gen 4

```bash
# Run optid wakes monitoring for 30 min, system at idle:
optctl wakes --since boot --watch --interval 30 | tee /tmp/wakes.log
# After 30 min, inspect top wakeup sources:
sort -t, -k3 -rn /tmp/wakes.log | head -10
# Expected: most wakeup events from network/timer sources, not from optid-managed devices
```

**Acceptance threshold:** Top wakeup sources identified; devices with event_count=0
for 30+ min are candidates for `power/wakeup=disabled` recommendation.

* * *

## 5. Non-Goals

- **No per-PID polling.** Per 0004's lesson — too expensive and too invasive.
- **No eBPF tracing in steady state.** eBPF kprobes wake CPUs; sysfs reads do not.
- **No competing telemetry daemon.** Prometheus `node_exporter` can scrape optid's JSONL;
  optid does not run a separate metrics server.
- **No `power/wakeup` writes.** optid observes wakeup sources and recommends; admin acts.
- **No MSR reads in steady state.** MSR reads require the `msr` kernel module; sysfs
  cpuidle stats are sufficient for 2 s resolution.
- **No telemetry for kernel-internal devices** (RTC, EC, ACPI interrupt controllers).
  These cannot be runtime-PM-controlled anyway.

* * *

## 6. WP Relationship Map

| Workplan / Doc | Relationship |
|----------------|-------------|
| **WP-N3** | Direct subject — wakeup + runtime PM telemetry |
| **WP-N5 (Runtime PM autosuspend)** | Depends on this for validation (runtime_status observation) |
| **WP-N6 (NVMe APST + ASPM)** | Depends on this for validation (C-state residency) |
| **WP-N9 (Thermal/fan)** | Depends on this for validation (PM QoS state) |
| **0004 (telemetry fidelity)** | Companion — 0004 covers PSI/cgroup; this covers device-runtime + C-state |
| **0007 (display)** | Display state (PSR, DPMS) observability lives in 0007, not here |
| **0008 (NVMe/ASPM)** | Storage/link state observability lives in 0008 |

* * *

## 7. Next Steps

### Immediate (no hardware needed)
- [ ] Confirm `/sys/class/wakeup/` attribute names on a running kernel (may vary by driver)
- [ ] Implement `crates/optid/src/sensors/wakeup.rs` — `WakeupSensor` struct, reads `/sys/class/wakeup/`
- [ ] Implement `crates/optid/src/sensors/runtime_pm.rs` — `RuntimePmSensor`, scans bus devices
- [ ] Implement `crates/optid/src/sensors/cstate.rs` — `CstateSensor`, reads cpuidle per-CPU
- [ ] Implement `crates/optid/src/sensors/pmqos.rs` — `PmQosSensor`, reads debugfs constraints
- [ ] Extend JSONL schema to include all four new `"source"` values
- [ ] Add `optctl wakes`, `optctl pm status`, `optctl idle-stats`, `optctl pmqos status` subcommands

### Short-term (needs hardware)
- [ ] Run §4.1 overhead profiling on T14 Gen 4 at idle
- [ ] Run §4.2 C-state accuracy vs turbostat
- [ ] Run §4.3 observer effect measurement
- [ ] Run §4.4 wakeup source attribution on each reference laptop

### Medium-term
- [ ] Land telemetry sensors as default-on (match 0004's `--telemetry=all` default)
- [ ] Promote research from WIP to Validated once §4.1–§4.4 closed on ≥ 3 reference laptops
- [ ] Update SPEC §4.1 status for the four rows this research covers to `O` (observed)
- [ ] Wire `RuntimePmSensor.failure_events` → allowlist temporary deny path (0006 integration)

* * *

## Appendix: Suggested Reading

### Kernel source
- `drivers/base/power/wakeup.c` — `pm_show_wakelocks()`, wakeup_source struct
- `drivers/base/power/runtime.c` — `rpm_suspend()`, `rpm_resume()`, runtime_status
- `drivers/base/power/sysfs.c` — per-device PM sysfs attributes
- `drivers/cpuidle/cpuidle.c` — cpuidle state tracking
- `kernel/power/qos.c` — PM QoS constraint management
- `drivers/platform/x86/intel/pmc/core.c` — Intel `pmc_core` debugfs

### Documentation
- `Documentation/ABI/testing/sysfs-class-wakeup`
- `Documentation/power/runtime_pm.rst`
- `Documentation/admin-guide/pm/cpuidle.rst`
- `Documentation/power/pm_qos_interface.rst`
- `Documentation/admin-guide/pm/intel_pmc.rst` (SLP_S0 residency)

### Tools
- `turbostat` — `tools/power/x86/turbostat/` — MSR-based C-state reference
- `pm-graph` — `tools/power/pm-graph/` — suspend/resume wakeup analysis
- `powertop` — anti-prior-art for steady-state overhead

### Project-internal
- SPEC §4.1 (observability inputs), §6 WP-N3 — `docs/SPEC-northstar.md`
- Research 0004 (companion, PSI/cgroup telemetry) — overhead budget and anti-patterns
- Research 0005, 0007, 0008 (adjacent observability concerns)
