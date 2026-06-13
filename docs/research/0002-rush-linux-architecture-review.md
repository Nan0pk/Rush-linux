# Rush Linux Architecture and Readiness Review
## What It Already Gets Right, What It Must Do, and What Requires Further Testing

## Executive Summary

**Overall judgment:** Rush Linux is **architecturally strong, operationally promising, and implementation-incomplete**.

It already does several unusually important things right:

- establishes a single default runtime optimization owner,
- separates dry-run from mutation,
- treats evidence as a release gate,
- integrates rollback and update integrity into system design,
- and avoids presenting itself as a mere tweak script.

But the current implementation is still too narrow to fulfill the full “unified power orchestrator” vision. Today, `optid` is best described as:

- a **pressure-aware mode resolver and guarded policy applier**,

not yet:

- a **complete platform power orchestrator**.

That gap is bridgeable, but only if the next engineering wave focuses on:

1. control-plane hardening,
2. observability expansion,
3. actuator expansion,
4. honest comparative benchmarking.

## 1. What Rush Linux Already Gets Right

### 1.1 Architectural ownership

**Strong.**

`optid` is explicitly intended to be the only default owner of runtime optimization policy. This is the correct answer to Linux power-policy fragmentation and aligns with Rush Linux’s own accepted policy-ownership ADR.

### 1.2 Safety posture

**Strong.**

Dry-run by default and apply-by-explicit-service is one of the best current design decisions in the repo. It lowers the risk of privileged policy mistakes and keeps the project honest about which behaviors are simulated versus enacted.

### 1.3 Explainability

**Strong.**

Decision logging, status reporting, and the repo-wide evidence culture are major assets. A privileged optimization daemon that cannot explain itself is not a trustworthy systems component; Rush Linux already understands this.

### 1.4 Build/update/rollback integration

**Strong.**

Treating rollback and update signing as part of the same architecture as optimization is exactly the right systems-level instinct. This makes adaptive policy a governed part of the product, not an unsafe sidecar.

### 1.5 Early kernel/userspace alignment

**Good.**

The project already thinks in terms of:

- PSI,
- cgroup v2,
- systemd slices,
- EPP,
- `platform_profile`,
- future uclamp/sched_ext integration.

That is substantially better than ad hoc post-install tuning.

## 2. What the Repo Must Still Do

### 2.1 Make runtime PM first-class

**Priority: Critical**

Rush Linux currently does not treat device runtime PM as a central optimization domain. That must change.

#### Needed

- runtime PM telemetry collection,
- autosuspend state visibility,
- per-device eligibility and failure logging,
- policy integration for USB, PCI, storage, radios, audio, and cameras.

#### Why

A large share of avoidable idle-power waste comes from devices that never enter low-power states or wake too aggressively.

### 2.2 Make sleep quality and wake attribution first-class

**Priority: Critical**

#### Needed

- wakeup-source ingestion,
- suspend blocker reporting,
- s2idle/suspend quality metrics,
- root-cause attribution for failed or degraded sleep,
- policy reactions to known wake blockers.

#### Why

This is one of the biggest practical laptop-quality gaps and one of the strongest opportunities for differentiation.

### 2.3 Add PM QoS and latency budgeting

**Priority: Critical**

Rush Linux speaks convincingly about responsiveness, but it does not yet fully ground that in explicit kernel-native latency contracts.

#### Needed

- PM QoS abstraction in `optid`,
- per-mode and per-workload wake/resume latency policy,
- device-level latency tolerance management,
- linkage between latency intent and low-power-state eligibility.

#### Why

Without this, responsiveness remains heuristic rather than contractual.

### 2.4 Expand actuator coverage beyond CPU + slices + VM knobs

**Priority: Critical**

#### Missing high-impact actuator domains

- runtime PM,
- PM QoS,
- GPU/display/media power,
- storage link policy,
- USB wake and port power,
- DTPM/powercap budgeting,
- fan/acoustic integration,
- util-clamp/task-class shaping,
- eventually scheduler specialization.

#### Why

The current actuator surface is too small to justify the full orchestration claim.

### 2.5 Harden the control plane

**Priority: Critical**

#### Needed

- split `optid` into modules,
- remove or quarantine stubs,
- strong D-Bus authorization,
- explicit write journal and revert model,
- dwell/hysteresis and flap resistance,
- atomic config reload,
- crash-safe restore and service-stop cleanup.

#### Why

A privileged orchestrator must be extremely predictable.

### 2.6 Convert planned compatibility into controlled translation

**Priority: High**

The long-term goal should not be “conflict with everything else forever.” It should be:

- absorb user intent from desktop power UIs,
- optionally present compatible surfaces where needed,
- and translate those intents into one coherent policy engine.

That is more powerful than merely winning the daemon conflict.

## 3. What Requires Further Testing

### 3.1 Immediate functional tests

**Must have before broader claims**

- policy decision matrix from fixtures,
- malformed telemetry handling,
- allowlist denial behavior,
- noop detection,
- revert and cleanup behavior,
- config reload correctness.

### 3.2 Flapping and hysteresis tests

**Must have before benchmark publication**

- PSI threshold oscillation,
- thermal jitter,
- AC/DC transition stability,
- battery boundary transitions,
- repeated rapid mode-change suppression.

### 3.3 Suspend and wake tests

**Must have before laptop claims**

- repeated suspend/resume cycles,
- suspend drain,
- wakeup-source attribution correctness,
- dock and external-display cases,
- resume stability after mode changes.

### 3.4 Runtime PM tests

**Must have before “zero avoidable waste” claims**

- USB autosuspend compatibility,
- NVMe APST safety,
- Wi-Fi and Bluetooth runtime behavior,
- audio device runtime suspension,
- dGPU runtime transitions,
- package residency before and after optimization.

### 3.5 Benchmark tests

**Must have before release-quality performance claims**

- baseline vs PPD vs Rush,
- AC and battery,
- laptop and desktop,
- latency and power together,
- work-per-joule,
- regression thresholds.

### 3.6 Security and privilege tests

**Must have before enabling broad actuation**

- D-Bus permission failures,
- write attempts outside allowlist,
- service sandbox validation,
- malicious config and malformed inputs,
- update signing and rollback integrity.

### 3.7 Documentation-to-code integrity tests

Rush Linux already values doc sync. That should continue.

#### Must test

- roadmap/version consistency,
- implemented vs planned surfaces,
- CLI/help truthfulness,
- service-unit behavior matching docs,
- benchmark report reproducibility.

## 4. Recommended Priority Order

### Phase 1 — Truth and safety

1. status honesty across docs and code,
2. `optid` modular split,
3. D-Bus hardening,
4. revert journal and crash-safe cleanup,
5. hysteresis and cooldown.

### Phase 2 — Observability

6. wakeup-source support,
7. runtime PM telemetry,
8. sleep-quality metrics,
9. device residency visibility,
10. GPU/display/media state visibility.

### Phase 3 — High-value actuator expansion

11. PM QoS,
12. runtime PM policy,
13. storage/link power policy,
14. GPU/display/media policy,
15. util-clamp/task-class shaping.

### Phase 4 — Outer-loop integration

16. DTPM/powercap,
17. fan/acoustic logic,
18. thermal budget sharing,
19. controlled idle injection where justified.

### Phase 5 — Evidence

20. full benchmark harness,
21. public A/B comparisons,
22. losses documented honestly,
23. policy tuning from data, not intuition.

## 5. Bottom-Line Repo Verdict

### If asked: “Is Rush Linux already the orchestrator the paper calls for?”

**No — not yet.**

### If asked: “Is it already pointed in the right direction?”

**Yes — more clearly than most comparable projects.**

### If asked: “What is its most important current success?”

**It has chosen the correct architectural owner and safety model.**

### If asked: “What is its most important current omission?”

**Runtime PM, sleep quality, and wakeup attribution are not yet first-class.**

### If asked: “What would most increase its credibility quickly?”

**Honest, comparative benchmark results plus first-class sleep/runtime-PM observability.**

## 6. Final Assessment

Rush Linux is one of the more interesting responses to the Linux power-management problem precisely because it understands that this is **not just a daemon problem**.

It gets several foundational things right already:

- one runtime policy owner,
- explicit safety boundaries,
- dry-run versus apply,
- explainability,
- benchmark discipline,
- rollback and update integrity,
- and a broad enough system scope to matter.

Those are substantial strengths.

But this review must also be sober: today’s Rush Linux does **not yet** implement the full architecture implied by the original research. It still lacks major domains of observability and actuation, especially around runtime PM, sleep quality, wakeup attribution, GPU/display/storage power, latency contracts, and thermal/power budget arbitration. Its current `optid` is a promising nucleus, not yet the finished orchestrator.

So the right conclusion is this:

> Rush Linux already embodies the right **governance model** and a credible **control-plane starting point** for a unified Linux power orchestrator. What it must do next is broaden that control plane from CPU-and-slice policy into full platform power orchestration, and then prove—honestly, repeatedly, and on real machines—that it reduces avoidable waste without harming responsiveness.
