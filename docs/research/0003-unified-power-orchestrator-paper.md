# Closing the Loop on the Linux Platform Stack
## Rush Linux and the Quest for a Unified Power Orchestrator

## Abstract

Modern power management is no longer a question of CPU frequency scaling alone. The most effective systems coordinate multiple control loops across firmware, kernel, userspace policy, and device power states. The original research correctly identified this pattern in Apple’s Closed Loop Performance Controller, IBM’s On-Chip Controller, and Intel’s low-power orchestration efforts. It also correctly argued that Linux, broadly construed, exposes many of the same mechanisms but rarely governs them under a single, observable policy authority.

Rush Linux is a particularly interesting implementation response to this problem because it does not frame optimization as an auxiliary daemon bolted onto an otherwise conventional distribution. Instead, it proposes a distribution architecture in which runtime optimization is a first-class system function, centered on a privileged daemon, `optid`, and surrounded by strict rollback, update, documentation, and benchmark discipline. That architectural ambition is unusually well aligned with the requirements of a true unified power orchestrator.

This paper argues that Rush Linux already captures several of the necessary preconditions for bridging the vertical integration gap: single ownership of runtime power policy, explainable and reversible actuation, benchmark-gated engineering, and safety-conscious separation between observation and mutation. However, the project is not yet a full unified orchestrator. Its current implementation observes only a small subset of the relevant telemetry and actuates only a limited set of controls—chiefly CPU energy-performance preference, platform profile, systemd slice weights, and selected VM tunables. It does not yet make device runtime power management, sleep-quality attribution, wakeup-source analysis, PM QoS latency contracts, GPU/display/media power states, storage link policy, or hierarchical thermal/power budgeting into first-class policy domains.

Accordingly, the right conclusion is neither dismissal nor hype. Rush Linux already embodies the correct governance model and a credible control-plane nucleus. What remains is to broaden its observability and actuation surfaces, harden its control plane, and produce comparative evidence on real hardware. If it does so, it could become one of the most coherent open attempts to approximate the cross-layer orchestration that vertically integrated systems already perform.

## 1. Introduction

The central problem of power management in 2026 is not merely how to slow processors down, but how to avoid spending energy **where no useful work is being done** while preserving responsiveness where useful work matters. That objective immediately expands the problem beyond CPU frequency. It implicates:

- CPU placement and performance bias,
- device runtime power states,
- wakeup frequency,
- display and media pipelines,
- storage and I/O policy,
- thermal and acoustic budgeting,
- suspend and active-idle quality,
- and the interaction between firmware and operating-system policy.

The original research’s strongest claim remains valid: the best commercial systems do not optimize these areas independently. They **close the loop** across them.

The Linux platform stack already contains many of the necessary building blocks. The kernel provides PM QoS latency constraints, runtime power management, wakeup-source accounting, scheduler-coupled CPU frequency selection, utilization clamping, platform profile selection, power capping frameworks, dynamic thermal power management, and experimental scheduler extensibility. See the Linux kernel documentation for [PM QoS](https://docs.kernel.org/power/pm_qos_interface.html), [runtime PM](https://docs.kernel.org/power/runtime_pm.html), [wakeup-source stats](https://www.kernel.org/doc/Documentation/ABI/testing/sysfs-class-wakeup), [cpufreq/schedutil](https://docs.kernel.org/admin-guide/pm/cpufreq.html), [util_clamp](https://docs.kernel.org/scheduler/sched-util-clamp.html), [platform_profile](https://docs.kernel.org/userspace-api/sysfs-platform_profile.html), [powercap](https://docs.kernel.org/power/powercap/powercap.html), [DTPM](https://docs.kernel.org/power/powercap/dtpm.html), and [sched_ext](https://docs.kernel.org/scheduler/sched-ext.html). Yet in most distributions, those mechanisms are fragmented across distinct services and ad hoc defaults.

Rush Linux addresses the problem at a more appropriate level: not as a one-off tuning utility, but as a **distribution architecture whose differentiating feature is runtime optimization under a single default owner**. That choice deserves serious attention.

This paper therefore rewrites the original thesis with Rush Linux in mind. It preserves the original findings that remain correct, evaluates the repo against those findings, and proposes the next steps required to transform a promising optimization framework into a true unified power orchestrator.

## 2. Scope: from “Linux” to the Linux Platform Stack

A key clarification from the earlier analysis is essential here: “Linux” is too ambiguous a term for this problem. Four layers must be separated:

1. **The Linux kernel** — mechanisms, hooks, drivers, scheduler, cpufreq/cpuidle, runtime PM, powercap, sysfs interfaces.
2. **Linux userspace** — services such as power-profiles-daemon, thermald, TLP, TuneD, systemd, and desktop power integration.
3. **Linux distributions** — packaging, defaults, kernel selections, enabled services, update/rollback behavior.
4. **The Linux platform stack** — firmware, embedded controllers, vendor policy engines, kernel, userspace, and distro defaults together.

This distinction matters because the kernel already provides substantial substrate, while policy ownership is where fragmentation usually reappears. `platform_profile`, for example, exists precisely to offer a generic profile-selection ABI for platform behavior, including multi-driver coordination and common-profile intersection. DTPM explicitly places dynamic, application-aware power limitation in userspace because userspace has the platform context required to arbitrate between devices.

Rush Linux is compelling precisely because it attempts to operate at the **distribution and platform-stack layer**, not merely at the kernel layer.

## 3. Preserved Findings from the Original Research

The original paper contained several findings that should remain intact.

### 3.1 Per-workload policy is the correct unit of optimization

Static machine-wide profiles are insufficient. Linux already supports per-task and per-cgroup performance constraints through utilization clamping, which can influence both scheduler behavior and schedutil-based frequency selection. This strongly supports the original argument that optimization should track workload class, not merely power source.

Rush Linux already leans in that direction conceptually: foreground user work and background work are treated differently via systemd slice properties, and modes are chosen dynamically rather than purely by a static AC/battery toggle.

### 3.2 Nested loops remain the right control model

The original framing of inner performance loops and outer thermal/power loops also remains sound. Linux already exposes a power allocator thermal governor structured around sustainable power and closed-loop cooling decisions. Hardware feedback systems on Intel and AMD likewise expose changing performance and efficiency capabilities to software without surrendering all low-level autonomy; see [Intel HFI](https://docs.kernel.org/arch/x86/intel-hfi.html) and [AMD HFI](https://docs.kernel.org/arch/x86/amd-hfi.html).

Rush Linux does not yet implement a full outer-loop budget governor, but the control model it needs is the same one the original paper described.

### 3.3 Broad actuation matters more than CPU tuning alone

The original paper was correct that CPU frequency is not the whole story. Linux already devotes serious infrastructure to non-CPU power domains: runtime PM for I/O devices, [interconnect](https://docs.kernel.org/driver-api/interconnect.html) and [devfreq](https://docs.kernel.org/driver-api/devfreq.html) for non-CPU performance domains, [panel self-refresh](https://docs.kernel.org/gpu/i915.html) for display power, [SATA link power management](https://docs.kernel.org/scsi/link_power_management_policy.html), [USB power management](https://docs.kernel.org/driver-api/usb/power-management.html), and [powercap](https://docs.kernel.org/power/powercap/powercap.html)/[DTPM](https://docs.kernel.org/power/powercap/dtpm.html) for system budget management.

This reinforces the original claim: a true orchestrator cannot remain CPU-centric.

### 3.4 Event-driven control is preferable, but hybrid control is realistic

PSI’s triggerable pressure thresholds and pollable interfaces make it one of the most useful Linux-native signals for adaptive control. Rush Linux is right to build around PSI early. At the same time, a mature orchestrator must accept that not all useful telemetry is naturally interrupt-driven. `thermald`, for instance, retains polling support when asynchronous thermal notifications are unavailable.

Thus, the original paper’s event-driven argument should be preserved, but softened into a practical principle: **event-driven where possible, sparse polling where necessary**.

## 4. Rush Linux’s Architectural Contribution

Rush Linux’s greatest strength is not any single current feature. It is the **shape of the project**.

### 4.1 One owner of runtime optimization policy

Rush Linux explicitly declares that `optid` is the only default owner of runtime optimization policy. This is exactly the right answer to the policy-fight problem that plagues Linux desktops and laptops. `power-profiles-daemon` already exposes profile control over D-Bus and is integrated into desktop environments, but it also coexists uneasily with tools such as TLP, and such conflicts are well documented in the ecosystem.

Rush Linux’s insistence on one owner, with others supplying compatibility surfaces or user intent, is therefore a major architectural virtue.

### 4.2 Separation of observation and actuation

The project’s service split—dry-run by default, mutation only when explicitly enabled—demonstrates unusual maturity. It is consistent with the more cautious lessons from thermald and LPMD-like systems, where policy mistakes can easily degrade performance or stability if the daemon is allowed to act too broadly too early.

### 4.3 Explainability as a design requirement

The insistence that every action must have a visible reason is not ornamental. It is one of the defining characteristics that distinguish a coherent orchestrator from an opaque bag of heuristics. This also aligns with `platform_profile`’s design philosophy: profile selection is an intent surface, not a magic claim about achieved performance.

### 4.4 System architecture, not daemon-only tuning

Rush Linux also treats boot, rollback, update signing, and validation as part of its optimization architecture. That is entirely appropriate. A power orchestrator that owns privileged behavior but cannot be reverted safely is not deployable. By contrast, Rush Linux tries to make update integrity and rollback part of the same engineering discipline as optimization.

This is a real contribution.

## 5. What Rush Linux Already Implements

The current `optid` implementation, while limited, is a meaningful nucleus.

It presently observes:

- PSI for CPU, memory, and I/O pressure,
- AC/battery state,
- battery percentage,
- thermal zone temperatures,
- load average,
- zram swap activity.

It presently chooses among:

- battery,
- balanced,
- performance,
- realtime,
- and auto-resolved modes.

It presently actuates:

- CPU EPP through `energy_performance_preference`,
- `platform_profile`,
- systemd slice weights and selected runtime cgroup properties,
- selected `vm.*` sysctls, with zram-aware gating for high swappiness intent.

These are not trivial capabilities. They already embody a partial form of the original thesis: using real-time system pressure and hardware state to choose and apply a guarded policy. In addition, the repo contains:

- a D-Bus control plane,
- a CLI with explanatory and state-inspection functions,
- benchmark manifest definitions,
- rollback/update infrastructure,
- and systemd unit packaging that explicitly conflicts with other tuning daemons by default.

In short: this is not empty architecture. It is a **working first slice** of a much larger orchestrator.

## 6. The Limits of the Current Implementation

That said, the current Rush Linux implementation remains too narrow to satisfy the full thesis of the original paper.

### 6.1 Observability is still incomplete

The current sensor surface excludes several domains that are indispensable for a true platform orchestrator:

- device runtime PM state and failure attribution,
- wakeup-source accounting and suspend blocker analysis,
- package idle and system sleep quality,
- GPU/display/media state,
- storage and link power-state telemetry,
- PM QoS / latency-contract state,
- firmware/workload hint channels such as DPTF workload hints and HFI-like feedback.

Without those, Rush Linux can make useful decisions, but only within a limited subspace of the full problem.

### 6.2 Actuation is still incomplete

The current actuator set omits several of the highest-impact power and latency levers available in Linux:

- PM QoS latency requests,
- device runtime PM and autosuspend policy,
- storage link power policies,
- USB port power and wake policy,
- GPU runtime state policy,
- media/display idle-state coordination,
- DTPM/powercap budget arbitration,
- fan/acoustic state integration through ACPI fan performance data,
- idle injection and powerclamp for outer-loop budget control where appropriate.

The project’s own hardware-support documentation correctly recognizes that many of these are risky and require allowlists. That caution is wise. But eventually, if Rush Linux is to truly bridge the vertical integration gap, these domains must move from “possible future policy” into measured, controlled implementation.

### 6.3 `sched_ext` is strategically important but not yet operationally central

Rush Linux rightly keeps `sched_ext` on the roadmap. The Linux kernel explicitly documents `sched_ext` as a BPF-defined scheduler class with strong fallback safety: the system automatically restores the default scheduler when errors or stalls occur. That makes it an excellent long-term research hook.

But the project must continue to resist the temptation to oversell it as a current production pillar. The highest-value near-term work is still in runtime PM, sleep quality, latency budgeting, and truthful benchmarking.

### 6.4 The repo still contains implementation/prose asymmetries

There remains a gap between what the repository argues for and what the code demonstrably does. In a project with such a strong evidence culture, that is not fatal—but it must be treated as technical debt, not as harmless roadmap optimism. Features that are documented as eventual plans should be labeled as such consistently.

## 7. Linux Context Rush Linux Must Eventually Absorb

If Rush Linux is to realize the full orchestration thesis, several existing Linux facilities should become central to its future design.

### 7.1 PM QoS as the contract surface for responsiveness

PM QoS already provides global and per-device latency-related constraints, including CPU wakeup latency and per-device resume latency. It should become a first-class part of Rush Linux’s policy engine because it transforms vague “performance mode” semantics into explicit latency budgets.

### 7.2 Runtime PM as a first-class optimization plane

Linux runtime PM is not a side detail; it is one of the primary determinants of modern idle power. Rush Linux cannot approach “zero avoidable waste” without making device runtime PM state, failures, dependencies, and wake behavior visible to `optid`.

### 7.3 Wakeup-source accounting and sleep quality

The kernel’s stable wakeup-source sysfs interface makes suspend blockers and idle-waste sources attributable. Rush Linux should integrate this into both diagnostics and policy. A laptop-first orchestrator that cannot answer “what woke the machine?” or “what prevented deeper idle?” is still blind in one of the highest-value domains.

### 7.4 DTPM and powercap for outer-loop budget allocation

DTPM explicitly represents shared power constraints hierarchically and places dynamic action in userspace. Combined with powercap, this is the natural substrate for Rush Linux’s eventual outer loop.

### 7.5 Existing open orchestration precedents

Intel’s LPMD already demonstrates active-idle optimization via efficient CPU selection and hints. ChromeOS `powerd` shows what a Linux-based whole-device power manager can look like in production, including suspend, charging, brightness, and thermal coordination.

Rush Linux should see these not as competitors but as validation that the architectural direction is viable.

## 8. Evaluation Requirements

Rush Linux’s own benchmark discipline is one of its strengths, but the evaluation must be broadened in line with the orchestration thesis.

### 8.1 Responsiveness

Measure:
- wakeup latency percentiles,
- application launch delays,
- mixed foreground/background contention,
- frame-time quality,
- audio underruns.

### 8.2 Energy and idle quality

Measure:
- wall power or discharge rate,
- device and package residency,
- suspend drain,
- wakeups per second,
- dGPU runtime residency,
- media/display idle effectiveness.

### 8.3 Stability and correctness

Measure:
- rollback success,
- revert reliability,
- runtime PM failures,
- mode flapping,
- boot/update resilience,
- policy-owner conflicts.

### 8.4 Comparative baselines

Rush Linux should continue with the repo’s stated comparison ambition:
- Fedora,
- Ubuntu,
- Arch,
- and a minimal tuned baseline.

Only such comparisons can justify the claim that Rush Linux is not simply “different,” but **better on the dimensions it explicitly values**.

## 9. Conclusion

Rush Linux should be taken seriously because it has chosen the correct level of abstraction for the problem. It does not merely say “we need a smarter daemon.” It says, in effect, that a Linux system which wishes to close the vertical integration gap must make optimization part of the architecture of the distribution itself.

That is the right idea.

The project already gets several indispensable things right:

- one default runtime policy owner,
- safety and reversibility,
- explainability,
- benchmark-gated engineering,
- rollback-conscious system design.

These are genuine strengths, not mere aspirations.

Yet the project is not finished enough to claim success on the original research agenda. It still lacks broad observability and broad actuation across runtime PM, sleep quality, wakeup provenance, GPU/display/media, storage links, PM QoS contracts, and outer-loop budget allocation. Its current `optid` is a capable nucleus, but not yet the full orchestrator that the thesis demands.

The proper verdict is therefore balanced:

> Rush Linux already contains the **governance model** and **control-plane seed** required for a unified Linux power orchestrator.
> To truly bridge the vertical integration gap, it must now expand from adaptive CPU-and-slice policy into full platform power orchestration, and it must prove the value of that orchestration on real hardware with honest evidence.

That is a difficult program. But it is also a coherent and worthy one.
