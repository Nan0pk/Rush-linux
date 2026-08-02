# Platform primitive disposition for optid

Status: research disposition for package R2. This paper does not override an
accepted decision, the Northstar, the D2 amendment, or the package ledger.

Updated: 2026-08-03

## Question

Which ideas from the Apple power-stack analysis should Rush implement, observe,
defer, or reject now that the project has a fail-passive D2 architecture and a
staged package plan?

The answer must preserve a small architecture. A kernel or firmware mechanism is
not automatically an optid feature. Rush should add a primitive only when there
is a stable Linux-facing contract, one clear owner, a bounded failure mode, and
evidence that fits the package model.

## Disposition summary

| Primitive or idea | Disposition | Rush package or boundary | Reason |
|---|---|---|---|
| Apple-style dual-loop thermal control | **Translate, do not port** | T1, T2, F4 | The useful idea is layered control and anti-windup. Linux already supplies thermal, powercap, cpufreq, and device-control contracts; no Apple ABI should be copied. |
| Clutch/Edge scheduler policy | **Reject for v1** | Accepted scheduler-scope decision | Rush should not ship a custom scheduler or `sched_ext` policy in the first product. Workload classes remain inputs to existing kernel policy. |
| AMD Dynamic Preferred Core / AMD HFI and Intel HFI | **Observe kernel-owned behavior** | O1 observation; no writer | HFI is consumed by the scheduler or surfaced as hints. Optid should not invent a competing core-ranking writer. |
| Frequency-invariant capacity | **Kernel-owned; use as interpretation context** | O1/F2 observation quality | Linux computes scheduler capacity from architecture callbacks and counters. There is no optid control surface to own. |
| Intel DPTF workload hints, power-floor state, and bounds | **Observe only by default** | O1; T2 may consume bounds | Read-only hints and limits are useful context. Workload requests and enable switches create firmware/daemon ownership conflicts and need a separate accepted contract before any write. |
| Generic devfreq state | **Observe generic state; defer generic writes** | O1 observation; device-specific future package for writes | Devfreq is a framework, not one semantic device. Current frequency and limits can be observed, but governors and min/max controls are device- and driver-specific. |
| S0ix / low-power-idle residency | **Implement read-only observation** | O1 | ACPI LPIT exposes stable residency counters. The metric is useful for diagnosis and validation without changing platform policy. |
| PCIe Latency Tolerance Reporting overrides | **Reject generic writes** | Existing PCIe/device packages may observe | LTR is topology- and firmware-sensitive. No generic Rush contract currently proves safe ownership, rollback, or platform-wide effects. |
| PSI threshold triggers | **Implement through the event reactor** | E1 | `/proc/pressure/*` and cgroup pressure files support descriptor-scoped poll/epoll triggers with bounded windows. This is a stable event source, not another polling loop. |
| Arm AMU and generic MPMM control | **Use kernel-derived signals; reject direct userspace AMU access; defer MPMM** | O1 observation; future accepted device-specific package only | The kernel intentionally keeps AMU registers out of userspace. Generic MPMM control lacks a stable cross-platform Linux ABI and firmware proof. |
| Memory-controller and uncore frequency | **Observe where stable; reject generic writes** | O1; a future device-specific package may own a proven writer | Some platforms expose devfreq or Intel uncore sysfs, but scope and semantics differ by driver, package, die, and fabric cluster. |
| Intel LPMD coexistence | **Detect and yield; do not compete** | C1 contracts and F1 domain ownership | LPMD can hotplug CPUs and change EPP based on HFI/WLT. Running a second autonomous policy owner would violate single-owner safety. |
| Broad idle injection | **Reject as a generic optid lever** | Possible future hardware-specific thermal package only | Intel powerclamp is an active cooling mechanism with visible performance impact. It needs hardware-specific thermal proof, bounds, and rollback, not a generic platform primitive. |
| Adaptive IRQ affinity steering | **Observe; reject autonomous writes for v1** | O1 observation | `/proc/irq/*/smp_affinity*` is writable for some IRQs, while managed interrupts and irqbalance may own placement. The conflict and device-locality risks outweigh current evidence. |
| IBM OCC and Qualcomm AOSS state-machine patterns | **Keep as design inspiration only** | F4/S2D/S3D/S5D architecture | The reusable lesson is explicit state, recovery, and verification. Their platform protocols are not portable optid ABIs. |

## Detailed design notes

### Layered thermal control

**User value.** Better thermal stability with less oscillation and fewer abrupt
performance changes.

**Linux contract.** Use the existing T1 pure thermal budget, T2 bounded
actuation package, powercap/thermal/cpufreq/device interfaces, and the F4
reconciler. The Apple CLPC model is a control-system reference, not an ABI.

**Ownership and risk.** T2 owns only explicitly contracted thermal actions.
Firmware emergency protection remains authoritative. The controller must not
write undocumented registers or bypass hardware limits.

**Fallback.** Observe and explain when a safe actuator is unavailable.

**Feature state.** Already mapped; no new primitive or package.

**Tests and evidence.** Deterministic model tests, anti-windup and saturation
tests, monotonic-bound tests, and independent physical thermal evidence already
belong to T1/T2.

### Scheduler preference and hardware feedback

**User value.** Place latency-sensitive work on CPUs that can deliver the best
performance without hard-coding core types.

**Linux contract.** AMD HFI/Dynamic Preferred Core feeds per-CPU ranking into
the scheduler. Intel HFI and DPTF workload hints likewise originate in hardware
or firmware. Capacity-aware scheduling and frequency invariance are kernel
scheduler mechanisms, not a stable userspace control protocol.

**Ownership and risk.** The kernel scheduler owns placement. Optid may record
available rankings, capacities, or workload hints when a documented read
interface exists, but must not create a second ranking or migration authority.

**Fallback.** Standard scheduler behavior with no optid intervention.

**Feature state.** Read-only observation in O1; no custom scheduler in v1.

**Tests and evidence.** Parser fixtures for present, absent, malformed, and
changing hints; topology identity checks; proof that observations never cause
CPU hotplug, affinity writes, or scheduler-policy changes.

### Frequency invariance

**User value.** Avoid interpreting the same workload as heavier merely because
the CPU ran at a lower frequency.

**Linux contract.** Linux normalizes scheduler utilization through
architecture-provided frequency and capacity scaling. On x86 this can use
APERF/MPERF; on arm64 it can use AMU or cpufreq transitions.

**Ownership and risk.** Kernel-owned and read-only from Rush's perspective.
Recomputing a competing value in userspace risks disagreement with the
scheduler and false decisions.

**Fallback.** Treat missing invariant-capacity visibility as reduced telemetry
confidence, not as permission to infer or write.

**Feature state.** Observation-quality context only.

**Tests and evidence.** Fixture tests for missing/zero/heterogeneous capacity
metadata and an explanation path that clearly labels unavailable normalization.

### Intel DPTF hints and bounds

**User value.** Understand firmware workload classification, power-floor state,
and platform-provided RAPL bounds.

**Linux contract.** The DPTF sysfs interface exposes workload hints, workload
requests, power-floor state, RAPL limit bounds, and related controls on
supported Intel systems. Platform profile is already a separate generic ABI
with an S1D contract.

**Ownership and risk.** Read-only hint and bound collection is acceptable.
Writing `workload_type`, enabling hint generation, changing notification rates,
or changing TCC offsets can conflict with firmware, LPMD, power-profiles-daemon,
or platform policy. Those writes are excluded until an accepted package names
the owner, original state, rollback, and hardware evidence.

**Fallback.** Use Rush's own workload classification and documented platform
limits when DPTF is absent; do not emulate firmware hints.

**Feature state.** Observe in O1. T2 may consume documented read-only bounds.
No generic DPTF writer.

**Tests and evidence.** Generation-aware fixture parsing, absent-device tests,
unknown-index handling, read-only permission tests, and hardware logs proving
that collection causes no sysfs writes.

### Device frequency scaling

**User value.** Explain why GPUs, memory controllers, interconnects, or other
devices are frequency-limited or under-utilized.

**Linux contract.** Devfreq provides a common kernel framework and sysfs model,
but each device supplies its own profile, frequency table, status semantics,
and governor support.

**Ownership and risk.** O1 may observe current frequency, available frequencies,
limits, governor, and transition statistics when present. Generic writes are
unsafe because the same attribute can represent different hardware domains and
firmware relationships.

**Fallback.** Preserve unsupported state and the driver identity; do not guess a
governor or frequency.

**Feature state.** Generic read-only observation; future writes require a
separate device-specific package and S1D/S4D identity contract.

**Tests and evidence.** Per-driver fixtures, unit normalization tests, device
identity and disappearance tests, and hardware evidence for any future writer.

### Low-power idle and S0ix

**User value.** Explain whether a system actually reaches deep low-power idle
and whether a change improves idle residency.

**Linux contract.** ACPI LPIT supplies read-only CPU package and system
low-power-idle residency counters through the cpuidle sysfs group. On Intel,
additional PMC diagnostics may exist, but debug and testing interfaces must not
be treated as universal product ABIs.

**Ownership and risk.** O1 owns observation only. Firmware, ACPI, device
runtime-PM, and the kernel own entry. Rush must not force S0ix or modify LTR to
chase a residency number.

**Fallback.** Report unsupported or unavailable. Use per-device runtime-PM and
PCIe evidence to explain blockers without mutating them automatically.

**Feature state.** Implement LPIT residency observation. Reject generic LTR
overrides.

**Tests and evidence.** Monotonic-counter tests, reset/wrap handling, suspend
interval correlation, and laptop hardware logs comparing residency before and
after a separately authorized experiment.

### Pressure stall triggers

**User value.** React quickly to CPU, memory, or I/O contention without frequent
polling.

**Linux contract.** PSI exposes system and cgroup pressure files. A process can
register one threshold/window trigger per file descriptor and wait with
`poll()` or `epoll()`. Kernel windows are bounded and notifications are rate
limited.

**Ownership and risk.** E1 owns descriptor lifecycle and event delivery. A PSI
trigger is an observation source; it does not authorize a hardware write.

**Fallback.** Fall back to slower aggregate pressure observation when trigger
registration is unsupported or denied.

**Feature state.** Implement in E1, reusing its event reactor.

**Tests and evidence.** Trigger grammar tests, one-descriptor-per-trigger tests,
POLLPRI/POLLERR handling, cgroup disappearance, rate-limit behavior, and a
synthetic pressure integration test.

### Arm AMU and MPMM

**User value.** Understand effective CPU frequency and memory-stall pressure on
Arm systems, and potentially avoid unsafe high-frequency workloads.

**Linux contract.** AMU counters are a kernel architecture facility. Upstream
documentation states that direct userspace AMU access is disabled for security
and system-management reasons. The kernel can use AMU for frequency invariance.
There is no generic stable userspace MPMM control ABI in the researched scope.

**Ownership and risk.** The kernel and firmware own AMU and any platform MPMM
policy. Direct register access or a vendor-specific control guessed from one SoC
would violate the stable-ABI requirement.

**Fallback.** Consume kernel-derived capacity/frequency signals and ordinary
performance counters where supported. Do not expose raw AMU or MPMM writes.

**Feature state.** AMU-derived observation through existing kernel signals;
MPMM remains deferred unless a separate accepted package establishes a
maintained, bounded, documented ABI and ownership contract.

**Tests and evidence.** Unsupported-platform tests, zero/broken-firmware signal
handling, and no-direct-register-access checks. Any future MPMM prototype must
remain feature-gated and require real hardware proof.

### Memory-controller and uncore frequency

**User value.** Diagnose memory-bound performance and power behavior.

**Linux contract.** Some SoCs expose memory controllers through devfreq. Intel
server platforms may expose package/die or fabric-cluster uncore frequency
through `intel_uncore_frequency`. These are not one cross-platform semantic
object.

**Ownership and risk.** Observe current frequency, initial limits, scope, and
agent types. Do not create a generic memory-frequency writer. Min/max changes
can affect cores, cache, memory, and I/O together and may conflict with firmware
power management.

**Fallback.** Report the driver-specific surface and leave hardware policy
unchanged.

**Feature state.** O1 observation. A future writer must be platform- and
identity-specific, with captured initial limits and a rollback transaction.

**Tests and evidence.** Scope parsing, package/die/fabric identity tests,
missing-current-frequency handling, read-only baseline logs, and platform-
specific performance/power/thermal proof before writes.

### Intel LPMD coexistence

**User value.** Avoid two daemons fighting over CPU availability, EPP, and
hardware hints.

**Linux contract.** Intel LPMD is a separate daemon that can select efficient
CPUs, offline/online CPUs, consume HFI/WLT, and change EPP. It is disabled by
default in template configurations but may be enabled by the owner.

**Ownership and risk.** C1 must detect the service and active mode. When LPMD is
active, optid must not independently own CPU hotplug, LPMD hints, or overlapping
EPP transitions. Observation remains allowed.

**Fallback.** Yield the conflicting domain and explain the detected owner.

**Feature state.** Coexistence contract in C1; no LPMD control integration.

**Tests and evidence.** Service-present/absent/failed fixtures, active-mode
identification, proof that conflicting actions are suppressed, and recovery
when ownership changes.

### Idle injection

**User value.** Emergency passive power reduction when ordinary frequency and
power controls are insufficient.

**Linux contract.** Intel powerclamp exposes synchronized idle injection as a
thermal cooling device, and the powercap framework also recognizes idle
injection as a control type.

**Ownership and risk.** This is active throttling with direct performance and
latency consequences. A generic optid action would be too broad, especially on
interactive and real-time systems.

**Fallback.** Use existing firmware thermal protection and T2's safer bounded
actuators. Observe cooling-device state where useful.

**Feature state.** Rejected as a generic v1 lever. A future Intel-specific
thermal package may be proposed only with a hard thermal root, explicit bounds,
rollback, and physical evidence.

**Tests and evidence.** If ever proposed: idle-ratio bounds, disabled-state
semantics, real-time exclusion, interactive latency limits, thermal benefit,
and immediate rollback on verification failure.

### IRQ affinity

**User value.** Improve device locality or reduce latency jitter.

**Linux contract.** Linux exposes IRQ affinity masks through procfs for IRQs
that permit userspace affinity changes. Managed interrupts may reject userspace
changes, and irqbalance may already own dynamic placement.

**Ownership and risk.** O1 may observe IRQ counts, effective affinity, device
locality, and whether an IRQ is managed. Optid must not autonomously rewrite
affinity in v1 because stale topology, queue remapping, CPU hotplug, and owner
conflicts can reduce performance or break device assumptions.

**Fallback.** Leave placement to the kernel, driver, and irqbalance; explain
locality anomalies.

**Feature state.** Observe only; adaptive writes rejected for v1.

**Tests and evidence.** Parser tests for masks/lists, managed-IRQ detection,
CPU-hotplug topology changes, irqbalance ownership detection, and device queue
identity. Any future writer requires workload-specific latency proof and exact
restoration.

### Platform state-machine patterns

**User value.** Predictable recovery and clear degraded states.

**Linux contract.** IBM OCC and Qualcomm AOSS demonstrate useful state-machine
and recovery patterns, but their transport protocols and firmware contracts are
platform-specific.

**Ownership and risk.** Reuse the architectural pattern only: explicit states,
bounded retries, durable transactions, cold restart, and independently verified
recovery. Do not add protocol adapters without an actual supported platform and
package.

**Fallback.** Existing F4/S2D/S3D/S5D state and recovery mechanisms.

**Feature state.** Design inspiration, not a standalone primitive.

**Tests and evidence.** State-transition matrices, malformed-state tests,
restart/recovery tests, and hardware evidence only when a concrete protocol is
implemented.

## Package mapping

| Outcome | Existing package |
|---|---|
| Read-only HFI, DPTF, devfreq, S0ix, uncore, and IRQ observations | O1 |
| PSI file-descriptor triggers | E1 |
| Thermal model and bounded controller | T1 and T2 |
| Per-lever identity, envelope, rollback, and verification | S1D and S4D |
| Durable transaction and recovery mechanics | S2D, S3D, and S5D |
| External-daemon ownership and LPMD coexistence | C1 and F1 |
| Device runtime-PM, PCIe, storage, display, and dGPU actions | D2-D5 |
| Vendor control experiments with a supported ABI | No existing package; require a new accepted device-specific package |

No new generic platform-control package is justified by this review.

## Source facts

The following upstream or vendor-maintained sources establish the ABI and
ownership facts used above:

- Linux AMD HFI documentation: <https://docs.kernel.org/arch/x86/amd-hfi.html>
- Linux capacity-aware scheduling and frequency invariance: <https://docs.kernel.org/scheduler/sched-capacity.html>
- Linux Intel DPTF sysfs interface: <https://docs.kernel.org/driver-api/thermal/intel_dptf.html>
- Linux platform-profile userspace API: <https://docs.kernel.org/userspace-api/sysfs-platform_profile.html>
- Linux devfreq framework: <https://docs.kernel.org/driver-api/devfreq.html>
- Linux ACPI LPIT low-power-idle residency ABI: <https://docs.kernel.org/firmware-guide/acpi/lpit.html>
- Linux PSI interface and trigger contract: <https://docs.kernel.org/accounting/psi.html>
- Linux arm64 AMU documentation: <https://docs.kernel.org/arch/arm64/amu.html>
- Linux IRQ affinity documentation: <https://docs.kernel.org/core-api/irq/irq-affinity.html>
- Linux generic IRQ rules, including userspace-settable affinity checks: <https://docs.kernel.org/core-api/genericirq.html>
- Linux Intel powerclamp driver: <https://docs.kernel.org/driver-api/thermal/intel_powerclamp.html>
- Linux power-capping framework: <https://docs.kernel.org/power/powercap/powercap.html>
- Linux Intel uncore frequency scaling: <https://docs.kernel.org/admin-guide/pm/intel_uncore_frequency_scaling.html>
- Intel LPMD project: <https://github.com/intel/intel-lpmd>

## Rush measurements

No new hardware measurement was performed for this research disposition. The
paper classifies interfaces and ownership; it does not claim performance,
power, thermal, or latency improvement.

## Assumptions

- The target remains Linux-first and uses upstream or distribution kernels.
- A testing/debug ABI is acceptable for diagnosis only when clearly labelled;
  it is not automatically a stable actuation contract.
- Existing package boundaries and the D2 fail-passive architecture remain
  authoritative.
- A feature may be absent on supported hardware without making the system
  unhealthy; unsupported observation must fail soft, while unsupported
  actuation must fail closed.

## Proposals

1. Implement the read-only observations in O1 rather than adding separate
   daemons or polling loops.
2. Add PSI triggers only through E1's descriptor-based reactor.
3. Make C1 explicitly detect LPMD, irqbalance, and other policy owners before
   any overlapping domain can actuate.
4. Keep direct AMU/MPMM, generic LTR, generic memory-frequency, generic idle-
   injection, and adaptive IRQ-affinity writes out of v1.
5. Require a new accepted package with S1D/S4D contracts and physical evidence
   before promoting any deferred writer.

## Unanswered questions

- Which O1 observations are available on the HP Victus and the secondary Intel
  laptop, and which are absent by hardware generation?
- Does the target Fedora configuration run irqbalance or Intel LPMD by default,
  and how should C1 identify active ownership without service-name assumptions?
- Which DPTF attributes are present on the HP Victus, and are workload hints
  firmware-generated, userspace-requested, or unavailable?
- Is there a maintained upstream userspace ABI for a future vendor MPMM control
  on any intended ARM target? None was established in this review.
- Would an Intel-specific idle-injection package ever provide enough value over
  T2's safer actuators to justify its latency and verification cost?

## Package-state consequence

This paper supplies the written implement/defer/reject disposition requested by
R2. It deliberately does not promote R2 in the ledger. Acceptance of this
research as the package outcome is a maintainer decision, and any resulting
runtime implementation must still follow the normal builder and independent-
verification contract.
