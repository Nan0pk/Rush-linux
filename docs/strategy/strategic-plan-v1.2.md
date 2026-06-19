Strategic Plan v1.2 (Consolidated) — Rush-linux — 2026-06-20

This is the consolidated Strategic Plan, Revision v1.2. It applies Revision Patch v1 (12 patches, §30–§41) and Revision Patch v2 (6 patches, §43–§48) to v1. Patch §42 (Deferred Autonomy) was offered but not applied; the T0+3y transition target is retained.

Each Part remains independently editable in place; section headers are slots the maintainer may rewrite without breaking the document. Cross-references use the form (Part X, §Y).

Document map:
- Part I — Core Strategic Plan (§1–§10). Origin: rush-linux-strategic-plan.md.
- Part II — AI Capability Integration (§11–§15). Origin: rush-linux-strategic-plan-ai-addendum.md.
- Part III — AI Council Governance Transition (§16–§24). Origin: rush-linux-strategic-plan-governance-transition-addendum.md.
- Part IV — Synchronization Layer (§25–§29). Cross-document glossary, unified risk register, unified branch map, master timeline, ratification status.
- Part V — Revision History (§29.4–§29.7). Applied patches, closed questions, remaining questions.

Status flag: Part III is RATIFICATION-PENDING. Until the maintainer signs the Transition-Enablement Instrument (§24, L1), Parts I and II govern. Part III governs only after ratification AND after the T0+3y transition event (§17.1).

Governance immutables (apply across all Parts):
- §0 SPEC-northstar objective — immutable by anyone, including the Sheriff.
- Modern-defaults-only invariant — defined by reference to a Modern-Defaults Allowlist (MDA), maintained at `docs/governance/modern-defaults-allowlist.md` (Patch §37). The MDA lists permitted core subsystems by reference to upstream interfaces (e.g., audio: PipeWire; graphics: Wayland; filtering: nftables; BPF: eBPF only; init: systemd; cgroups: v2). The forbidden-as-default list (X11, PulseAudio, iptables, cgroup v1, SysV init, classic BPF) is part of the constitutional floor. The permitted list may evolve by Sheriff amendment (§19.4); the Council cannot amend the MDA. Initial population of the MDA is a Phase 1 deliverable (D1.9). Until D1.9 ships, the invariant is governed by the original negative list plus maintainer judgment.
- Evidence Rule (no claim without a literal command transcript) — immutable by anyone.
- Builder/Verifier/Human role separation and Human-as-sole-signoff — fixed under Parts I and II; transitional under Part III pending ratification.
- Anti-pivot Contract — forbids redefining §0 or proposing strategic pivots. Part III (§16.3) interprets governance-structure amendment as not a strategic pivot; that interpretation is itself RATIFICATION-PENDING.

# PART I — CORE STRATEGIC PLAN

## 1. NORTH-STAR RESTATEMENT

SPEC-northstar §0 states the objective as "minimize avoidable platform energy subject to a per-workload-class responsiveness floor." In plain terms: every joule consumed by the platform that does not contribute to forward progress on the user's declared workload is a defect. The floor is not a single number; it is a contract per workload class — idle, light, interactive, latency-critical, throughput — expressed as PM QoS latency bounds and PSI thresholds. optid is the only entity authorized to translate between the workload-class contract and the kernel's power/performance knobs.

Two corollaries follow. First, the objective is not "lowest power" in absolute terms; a latency-critical workload that misses its floor because the CPU was parked too aggressively is a failure of the objective, not a success. Second, the objective is not "highest performance"; a throughput workload that runs 5% faster at 40% more energy fails the objective unless the workload's class contract explicitly trades energy for time. The objective is Pareto-optimality on the energy/responsiveness frontier defined by the per-class contracts.

INTERPRETIVE note (single, non-amendatory; operationalized by Patch §33): The phrase "avoidable platform energy" implies a counterfactual baseline — the minimum energy a correctly-configured platform would consume for the same forward progress. This interpretation is operationalized by the Baseline Specification (Phase 1 deliverable D1.8). The counterfactual baseline is defined as the energy consumed by the same hardware booted into a reference configuration (specific kernel version, specific systemd, no optid, default CPU governor, default device PM policy) running the same workload-class benchmark, measured by the same wattmeter over the same duration as the optid-active measurement. The "avoidable" energy is the delta. This is "configured platform energy delta" not "theoretically unavoidable energy"; the terminology is precise. Until D1.8 ships, all release claims must be labeled "configured platform energy delta," not "avoidable energy delta." After D1.8 ships, the "avoidable" label is permitted with this baseline definition. The baseline is re-baselined per kernel LTS bump.

## 2. CURRENT-STATE DIAGNOSIS

What works (verifiable from repo claims):
- Compile-clean core Rust daemon (`optid`) with single ownership of runtime optimization decisions.
- D-Bus control plane for external clients.
- Rootfs VM boot under QEMU.
- UKI generation, A/B rollback, and signed updates with Ed25519 (test keys).
- v0.1–v0.4 milestones closed; v0.5 (mkosi/Arch rebase + sched_ext preparation) in flight.

What's stubbed:
- Workload-class detection heuristics — likely present as scaffolding but unvalidated against real workloads [UNVERIFIED: no transcript of class-detection accuracy in repo].
- PSI-driven policy reactions — wired but no published evidence of correct closed-loop behavior under load [UNVERIFIED].
- sched_ext integration — declared for v0.5, not yet merged or benchmarked.

What's blocked:
- BLOCKER-B1: no published disk.raw artifact. Sandbox lacks loop-device privileges. Prevents third-party reproducibility.
- BLOCKER-B2: no published benchmark results. Energy claims are unsubstantiated under the Evidence Rule.
- BLOCKER-B3: depth-enablers (PCIe ASPM, SATA ALPM, runtime PM, adaptive backlight) unproven on real hardware. The objective cannot be satisfied if these remain unproven.
- BLOCKER-B4: test-only Ed25519 signing keys; no Secure Boot enrollment path. Updates are not trustworthy under any threat model that includes the boot path.
- BLOCKER-B5: no installer UI. Cannot reach users outside the build-it-yourself niche.
- BLOCKER-B6: no desktop stack shipped. "Set-and-forget OS" is unverifiable until a user session exists.
- BLOCKER-B7: reach ≈ 0 (1 star, 0 forks). No ecosystem gravity, no bug inflow, no third-party validation.
- BLOCKER-B8: single-maintainer bus factor. Every BLOCKER is also a continuity risk. (See Part III §17.5 KC4 for the post-transition form of this risk.)

What's invisible:
- UNKNOWN-U1: actual optid CPU/memory footprint under sustained load.
- UNKNOWN-U2: actual energy delta vs a baseline distro on identical hardware. No published measurement.
- UNKNOWN-U3: governance pipeline throughput (Builder → Verifier → Human cycle time). No published stats. (Re-measured post-transition; see Part III §20.1.)
- UNKNOWN-U4: CI flakiness rate, build-reproducibility rate across hosts.
- UNKNOWN-U5: hardware coverage matrix — which platforms have ever booted Rush-linux at all.

Top 3 critical-path items:
1. B2 (benchmark publication). Without this, the SPEC-northstar objective is unfalsifiable and every external claim is rhetoric. (Closed by Part I §10 item 1; verified by Part II §11 P11.2; baseline operationalized by Patch §33.)
2. B3 (real-hardware validation of depth-enablers). Without this, optid is a policy engine with no actuators that matter.
3. B1 (disk.raw artifact). Without this, third parties cannot independently reproduce B2 or B3.

## 3. DESTINATION ARCHITECTURE

### (a) Kernel & scheduler
End-state: Rush-linux ships a kernel configured for fine-grained PM QoS exposure and runs a sched_ext scheduler (or, if sched_ext is rejected upstream, an in-tree Rush-maintained BPF scheduler) that consumes optid's per-cgroup latency contracts. EEVDF remains the fallback path. Branch-aware: see §6 Branch A (with Patch §38 36-month kill-criterion, amended by Patch §43 for API-stability tracking and contract-preservation testing) and Part II §13.2 Branch-H. The invariant: no workload-class contract may be expressed in a way the kernel cannot enforce or refuse. If a contract is unenforceable on a given kernel or platform, optid must downgrade the contract and emit an evidence-grade log line, never silently best-effort it. When falling back from sched_ext to EEVDF (per Patch §38 or §43), the Verifier runs a contract-preservation benchmark (Patch §43); classes that fail are flagged "degraded contract — sched_ext required for full guarantee" in release notes rather than silently violated.

### (b) optid control plane
End-state: optid is the single writer to all runtime power/performance knobs (CPUFreq governors, CPUIdle states, runtime PM, device PM QoS, backlight, perf-criticality hints, scheduler extension params). External clients (desktop shell, system services, installer) speak D-Bus to optid only; they never write sysfs directly. The invariant: exactly one writer. Any kernel knob not yet owned by optid is a known gap, listed in a public matrix, with an owner and a target version.

### (c) Package & image
End-state: system image is a signed UKI plus a read-only rootfs; user state lives in a separate partition managed by systemd-sysupdate. The base image is reproducible from source via mkosi. The invariant: byte-reproducible builds from a pinned manifest. Two independent builders on two distros producing the same hash is the entry criterion for "stable." Post-transition, one or both builders may be agent-operated (Part II §12.2; Part III §20.1) under ratified Laws. Release labels are tiered per Patch §45: dev (single-builder OK, not eligible for allowlist or transcript publication), candidate (two-builder hash match, eligible for shadow-run and Verifier attestation, not for OEM or end-user install), stable (two-builder hash match, Sheriff signoff, benchmark transcript attached; the only label eligible for allowlist promotion, OEM pre-install, and end-user install).

### (d) Desktop & UX
End-state: a Wayland compositor (preference: existing project such as sway/KDE Plasma/Wayfire-based rather than greenfield) running a shell that surfaces exactly three things to the user: current workload class (read-only), current energy rate (read-only), and a single "pin to performance" escape hatch. All other knobs are invisible. The invariant: the user never sees a knob that bypasses optid. If a knob exists in the underlying stack that optid does not own, the UI hides it until optid owns it.

Patch §46 adds two visibility mechanisms that preserve the invariant: `optid-debug` (read-only diagnostic mode showing every knob optid manages, its current value, the active workload class, the active contract, and the reasoning trace for the last decision; the user CANNOT write sysfs directly in this mode — visibility, not bypass) and `optid-thermal-override` (time-limited override, default 30 minutes configurable up to 4 hours, mandatory OAL-logged reason field, for thermal emergencies such as fanless mini-PCs or failed fans). Power-user documentation in the onboarding document (Patch §40 D2.6) explains both mechanisms and explicitly states that bypass is not possible.

Patch §48 adds compositor resilience: optid runs a 60-second health-check verifying the compositor responds to D-Bus idle-notification queries. If the compositor stops responding for > 120 seconds, optid enters PSI-only mode (workload-class detection falls back to PSI signals alone; user notified via shell or syslog). If the compositor fails to start, the system boots to a TTY login with optid in PSI-only mode. If the primary compositor fails to start on 3 consecutive boots, the secondary compositor is used on the 4th boot (installer identifies a secondary at install time). Allowlist entries carry a `compositor_compat` field listing validated compositors; the installer refuses primary compositors not in the list.

### (e) Installer & update
End-state: a Calamares-derived or rust-native installer that runs from live UKI, partitions for A/B root + persistent state, enrolls Secure Boot keys (with a clear user consent flow), and registers the system for atomic updates. Updates are staged on the inactive slot, validated by a Verifier-agent-built test harness, and promoted on next boot with automatic rollback on health-check failure. The invariant: an update that fails its post-boot health check never stays booted. Rollback is automatic and tested. Adversarial testing of this path: Part II §12.4; post-transition security audit cadence: Part III §21.3.

### (f) Hardware enablement
End-state: a public hardware allowlist, every entry backed by a benchmark transcript on that exact hardware. Allowlist entries carry per-device PM capability flags (ASPM supported, ALPM supported, runtime PM supported, backlight controllable, etc.). Hardware not on the allowlist boots in "safe mode" with optid in observe-only mode. The invariant: optid never enables a depth-enabler on hardware it has not benchmarked. Safe-mode is the default for unlisted hardware; this is a feature, not a degradation. Allowlist expansion post-transition is Council-autonomous (Part III §18.3, amended by Patch §34) if the benchmark-transcript requirement is met AND either the Sheriff has physically seen the hardware benchmarked OR a Trusted Lab Operator (TLO) has co-signed the transcript.

### (g) Observability & telemetry
End-state: every optid decision is emitted as a structured event with class, contract, observed PSI, chosen action, and energy delta estimate. Events are local-first (stored on-device, queryable via optid-log), and an opt-in anonymized telemetry stream exists for the maintainer's benchmark corpus. The invariant: no telemetry leaves the device without explicit, revocable, per-stream consent. No identifier beyond hardware model and optid version. Telemetry is gated by the D1.10 Privacy Analysis (Patch §41). No telemetry stream ships until the analysis is complete. Post-transition, telemetry also feeds the Council's precedent ledger (Part III §19.6).

Patch §47 tiers telemetry into four levels: Tier 1 — Local-only (always on, full-fidelity, never leaves the device; the Council reads from Tier 1 when operating on a specific device's data); Tier 2 — Aggregated (opt-in, k ≥ 1000, per-hardware-model per-week aggregated counts, eligible for the public benchmark corpus); Tier 3 — Per-class (opt-in, k ≥ 100, per-workload-class energy deltas with hardware bucketed to coarse class like "x86 laptop" or "ARM SBC", higher-resolution signal for Law refinement); Tier 4 — Raw (opt-in, k ≥ 10, Sheriff-approved per-stream, raw optid events for a specific device and time window for deep-dive investigations). The D1.10 privacy analysis must specify the k-anonymity threshold and bucketing strategy for each tier. The Council may request Tier 2/3 data with Sheriff approval; Tier 4 requires Sheriff signoff per stream.

### (h) Governance & security
End-state: Builder/Verifier/Human separation enforced by repository machinery — Verifier agents cannot merge, Builder agents cannot promote to human review without Verifier sign-off, Human signs off on phase exits only. Every release artifact carries a Verifier attestation linking it to a benchmark transcript. Secure Boot keys are enrolled via a measured-boot-aware flow; the signing keys are HSM-backed once budget permits. The invariant: no release artifact reaches users without a Verifier attestation, and no attestation is accepted without a matching benchmark transcript. This Part I formulation is superseded by Part III post-transition (§18, §20); see Part IV §29 for the active-governance pointer.

## 4. STRATEGIC PILLARS

### P1. Adaptive Orchestration
Thesis: per-class PM QoS contracts, closed-loop under PSI, written and enforced by a single daemon, are the durable technical moat.
Primary KPI: median energy delta vs baseline distro on the same hardware, across the five workload classes. Target trajectory: −10% at v1.0 stable, −20% at Phase 3, −30% at Phase 5.
Anti-KPI: if the median energy delta fails to beat baseline by ≥5% on at least three of the five classes at v1.0, the pillar is falsified.
Top 3 enabling capabilities: (1) sched_ext integration; (2) per-device depth-enabler matrix with hardware allowlist; (3) closed-loop PSI → PM QoS controller with stability proofs.

### P2. Set-and-Forget UX (reframed by Patch §35)
Thesis: the user should never adjust a power knob. The OS observes, classifies, contracts, and acts.
Primary KPI: time-to-energy-optimal-idle-state after the user stops interacting, subject to the per-class responsiveness floor. "Energy-optimal idle state" is defined as the deepest idle state the platform can enter without violating the per-class responsiveness contract for the next likely workload. This is a function of (current class, contract floor, hardware exit-latency matrix). Target: ≤ 2 seconds at Phase 4. On hardware where the deepest state has exit latency ≥ the responsiveness floor, the energy-optimal state is shallower; the deeper state is reserved for the idle-class contract.
Anti-KPI (adoption): a representative user study (n ≥ 20) showing users reaching for terminal or sysfs to "fix" power behavior on more than 10% of sessions.
Anti-KPI (floor): any idle-entry policy that causes exit latency ≥ the per-class responsiveness floor is a violation, regardless of energy savings. This is the formal expression of the north-star's responsiveness floor constraint on P2. Measured by: post-idle-entry exit latency, per-class, on allowlisted hardware. Target: 0 violations.
Top 3 enabling capabilities: (1) compositor integration surfacing only class/rate/pin; (2) install-time hardware classification; (3) self-healing rollback on contract violation.

### P3. Evidence-First Trust
Thesis: every claim carries a command transcript. Users, OEMs, and regulators can reproduce any benchmark with one command.
Primary KPI: ratio of benchmark-backed claims to total claims in release notes. Target: 1.0 from v1.0 onward.
Anti-KPI: any release that ships a power/energy claim without a transcript fails Verifier and is rejected. (See Part II §14 Risk A1; Part III §23 Risk T9 for the post-transition form; Patch §30 OAL for the recursive-trust defense.)
Top 3 enabling capabilities: (1) reproducible benchmark harness with operationalized baseline (Patch §33 D1.8); (2) Verifier-attested release artifacts; (3) public benchmark corpus with per-hardware transcripts.

### P4. Ecosystem Gravity
Thesis: Rush-linux becomes the reference implementation other distros cite when adding adaptive power management.
Primary KPI: count of upstream commits by Rush-linux maintainers accepted into kernel, systemd, and at least one compositor. Target: ≥ 20/year by Phase 4.
Anti-KPI: zero upstream commits for 12 consecutive months means the pillar is inert.
Top 3 enabling capabilities: (1) a clean optid ↔ upstream boundary; (2) maintainer time budgeted for upstream work; (3) a public design doc per upstream-contribution target. Post-transition, "maintainer time" includes Council time (Part III §20.1).

### P5. Hardware Partnership
Thesis: OEM pre-install is the only credible path to "dominant" market share on laptop and embedded.
Primary KPI: number of OEM models with Rush-linux pre-install option. Target: ≥ 1 by Phase 5, ≥ 5 by Phase 6.
Anti-KPI: zero OEM conversations for 18 consecutive months means the pillar is dormant.
Top 3 enabling capabilities: (1) hardware allowlist with OEM-attributable model entries; (2) installer that supports OEM imaging; (3) update infrastructure that OEMs can delegate to. OEM contracts remain Sheriff-signed post-transition (Part III §18.3).

### P6. Governed Autonomy
Thesis: the Builder/Verifier/Human pipeline scales development without sacrificing the Evidence Rule.
Primary KPI: ratio of merged PRs that pass Verifier on first human review. Target: ≥ 80% by Phase 3.
Anti-KPI: if a Verifier-passing PR is later found to have fabricated evidence, the entire pipeline is suspended pending audit. (See Part II §14 Risk A1; Patch §30 OAL for the recursive-trust defense.)
Top 3 enabling capabilities: (1) Verifier agents with benchmark-transcript verification as a first-class check; (2) signed-builder attestations; (3) public pipeline telemetry. Post-transition, this pillar transforms into the AI Council governance of Part III; the KPI is redefined in Part III §20.

## 5. PHASED ROADMAP

### Phase 1 — Unblock & Prove (now → v1.0 stable, ≤ 18 months)
Entry criteria: v0.4 complete; v0.5 in flight.
Exit criteria: v1.0 stable installable image; published disk.raw; published benchmark corpus on ≥ 3 hardware platforms; Secure Boot enrollment path; installer UI.
Headline deliverables:
- D1.1 mkosi/Arch rebase (v0.5) and sched_ext integration with EEVDF fallback.
- D1.2 disk.raw build pipeline that works without loop-device privileges.
- D1.3 Benchmark harness: reproducible, single-command, output is a signed transcript. Cover all five workload classes.
- D1.4 Real-hardware validation of depth-enablers on ≥ 3 platforms.
- D1.5 Secure Boot key enrollment flow with user consent; production Ed25519 keys in HSM-backed storage.
- D1.6 Installer UI (rust-native preferred; Calamares fallback).
- D1.7 A/B rollback verified by automated health-check on boot.
- D1.8 Baseline Specification (Patch §33). The reference-configuration baseline is defined, implemented in `rush-bench`, and documented. The baseline is re-baselined per kernel LTS bump. Acceptance: `rush-bench --baseline --platform <p> --class <c>` produces a signed transcript labeled "baseline"; `rush-bench --active --platform <p> --class <c>` produces a signed transcript labeled "active"; the delta is computed, signed, and reproducible.
- D1.9 Modern-Defaults Allowlist (MDA) (Patch §37). Initial population at `docs/governance/modern-defaults-allowlist.md`. For each core subsystem (audio, graphics, networking filter, BPF, init, cgroups, display protocol, device model, security module, package format), the MDA lists the permitted default and the forbidden-as-default alternatives. Verifier-attested.
- D1.10 Privacy Analysis (Patch §41, amended by Patch §47). Before any telemetry stream ships, a privacy analysis must document what identifiers are sent, demonstrate that the identifier set is insufficient to fingerprint a single user across the relevant k-anonymity threshold per tier (Tier 2: k ≥ 1000; Tier 3: k ≥ 100; Tier 4: k ≥ 10 per Patch §47), and propose mitigation (identifier bucketing, etc.). The analysis is a Verifier-attested artifact. Telemetry does not ship until D1.10 is complete and signed off by the Sheriff (or pre-transition Human).
- D1.11 AI Authorship & Licensing Policy (Patch §44). The policy specifies: (1) Commit metadata — every commit's message includes an `AI-Generated:` trailer listing model provider, model version, and prompt-hash; commits without this trailer are presumed human-authored. (2) Training-data attestation — each model provider used by the project must publish a training-data attestation (URL or document) stating whether the model was trained on code under copyleft licenses; the project maintains a public matrix of providers and their attestations; providers without attestations are restricted to non-shipped surfaces (documentation, tests, CI tooling). (3) Clean-room presumption — AI-generated code is treated as a new contribution under the project's Apache-2.0 license, with the contributor (Human or Council) attesting that the code has been reviewed for substantive similarity to known copyleft code; the Verifier runs a `license-check` step that diffs AI-generated code against a corpus of GPL/AGPL projects; significant similarity blocks the PR. (4) Law licensing — the Law corpus and precedent ledger are licensed under CC0 (public domain) to prevent the governance artifacts themselves from becoming a licensing burden; the strategic plan documents are licensed under Apache-2.0 consistent with the code. (5) CLA amendment — any contributor (human or AI-acting-through-human) signs a contributor-license-agreement amendment covering AI-assisted contributions, attesting to the above. Acceptance: D1.11 document exists at `docs/governance/ai-authorship-policy.md`; Verifier-attested; referenced from Part II §11 P11.2 (provenance) and Part III §19.2 (Law evidence basis).
KPIs:
- 1 star → ≥ 100 stars; ≥ 5 forks.
- ≥ 3 hardware platforms on allowlist with transcripts.
- Stable releases require 2-builder hash match; dev and candidate releases have proportional requirements per the tiered label system (Patch §45).
- 0 unbacked power/energy claims in v1.0 release notes.
- 0 release claims labeled "avoidable energy delta" until D1.8 is shipped. Claims before D1.8 use "configured platform energy delta" only. (Patch §33.)
Top 3 risks:
- R1.1 sched_ext API churn. Mitigation: EEVDF fallback as default; sched_ext behind a flag. Hard kill-criterion at 36 months post-T0 per Patch §38, accelerated to 24 months from the first breaking API change if ≥ 2 breaking changes occur in any 6-month window (Patch §43). Contract-preservation test on every fallback (Patch §43).
- R1.2 hardware acquisition. Mitigation: community loaner program; ask for hardware donations. Post-transition, TLO mechanism (Patch §34) scales validation.
- R1.3 reproducible-build drift. Mitigation: pin base image by hash; CI gate on hash mismatch.
Binary decision gates:
- DG1.1 (month 6): single-command benchmark run on ≥ 1 hardware platform produces a transcript with non-zero energy delta vs baseline? If no, halt v1.0 scope expansion.
- DG1.2 (month 12): sched_ext integration outperforms EEVDF fallback on ≥ 1 workload class? If no, ship EEVDF-only for v1.0. Before any sched_ext → EEVDF fallback (per this gate or per Patch §38 kill-criterion), the Verifier runs the Patch §43 contract-preservation benchmark; classes that fail are labeled "degraded contract — sched_ext required for full guarantee" in release notes.
- DG1.3 (month 16): installer successfully installs, enrolls Secure Boot, and updates on ≥ 2 hardware platforms? If no, v1.0 slips.
ANCHOR — Phase 1 exit defines T0 for the Part III transition (§17.1).

### Phase 2 — Editable Defaults & Real-Hardware Validation (v1.0 → v1.5, ≤ 18 months)
Entry criteria: v1.0 stable shipped; Phase 1 exit criteria met (i.e., T0 reached).
Exit criteria: v1.5 with user-editable workload-class contracts; allowlist ≥ 10 platforms; first OEM conversation logged.
Headline deliverables:
- D2.1 Per-class contract editor (text-file format; documented; validated at boot).
- D2.2 Closed-loop stability proofs for PSI → PM QoS controller.
- D2.3 Hardware allowlist expansion to ≥ 10 platforms with per-device transcripts.
- D2.4 First desktop stack shipped.
- D2.5 Self-healing rollback extended to optid policy regressions.
- D2.6 Contributor Onboarding Document (Patch §40). A condensed (≤ 10-page) version of the strategic plan, written for new contributors, covering: project objective, modern-defaults invariant, Evidence Rule, how to submit a Verifier-passing PR, governance overview (pre-transition), and pointer to the full plan. Owner: Human pre-transition; Council (Builder Councilmember with Sheriff review) post-transition. Acceptance: document exists at `docs/onboarding.md`; reviewed by ≥ 1 external contributor for clarity.
KPIs:
- Median energy delta vs baseline: ≥ −15% across 5 classes on allowlisted hardware.
- ≥ 500 GitHub stars; ≥ 20 forks; ≥ 5 active non-maintainer contributors.
- ≥ 1 upstream contribution accepted.
Top 3 risks:
- R2.1 compositor choice locks UX direction. Mitigation: pick a compositor with a scripting layer.
- R2.2 closed-loop instability under unusual workloads. Mitigation: conservative default contracts.
- R2.3 maintainer bandwidth collapse. Mitigation: weekly office hours; public architecture docs.
Binary decision gates:
- DG2.1 (month 6 of phase): ≥ 5 of 10 allowlisted platforms show ≥ −10% energy delta vs baseline? If no, prioritize depth-enabler work over allowlist growth.
- DG2.2 (month 12 of phase): desktop stack survives a 7-day soak test without optid intervention? If no, defer desktop to Phase 3.
ANCHOR — Phase 2 spans T0 → T0+~1.5y of the Part III learning period (§17.1). Council operates in observe-only mode throughout.

### Phase 3 — Editions & Ecosystem (≤ 24 months)
Entry criteria: v1.5 shipped; desktop stack stable.
Exit criteria: ≥ 4 editions shipping; realtime and IoT editions in beta.
Headline deliverables:
- D3.1 Edition manifests: laptop, desktop, server, embedded, realtime-beta, IoT-beta.
- D3.2 Per-edition default contract sets, each benchmarked.
- D3.3 Public upstream-contribution pipeline (one PR per quarter to kernel/systemd/compositor).
- D3.4 First third-party package repository (or signed Flatpak remote) for desktop edition.
KPIs:
- ≥ 2,000 stars; ≥ 50 forks; ≥ 1 distribution included in distrowatch.
- ≥ 5 accepted upstream contributions.
- ≥ 1 distro cites Rush-linux as inspiration for adaptive power features.
Top 3 risks:
- R3.1 edition sprawl. Mitigation: hard cap of 6 editions; any new edition requires a designated non-maintainer owner.
- R3.2 upstream rejection. Mitigation: contribute small, self-contained improvements.
- R3.3 package repository security. Mitigation: signed packages; Verifier-attested repository state.
Binary decision gates:
- DG3.1 (month 12 of phase): ≥ 4 editions shipping and stable for ≥ 3 months? If no, freeze edition count and consolidate.
- DG3.2 (month 18 of phase): ≥ 1 OEM responded to outreach? If no, escalate OEM work to Phase 4 critical path.
ANCHOR — Phase 3 ends approximately at T0+~3.5y of the Part III timeline. The Council Codification Event (§17.1) at T0+3y falls inside this phase. If Patch §38's 36-month sched_ext kill-criterion triggers (T0+36m), a new Phase 3.5 (sched_ext Re-base) is inserted here.

### Phase 4 — Set-and-Forget UX & Self-Healing (≤ 24 months)
Entry criteria: Phase 3 exit criteria met.
Exit criteria: user study shows ≤ 10% sysfs-reaching sessions; self-healing covers ≥ 90% of common regressions; optid owns ≥ 90% of all runtime power/performance knobs on allowlisted hardware.
Headline deliverables:
- D4.1 User-facing shell with three surfaces only (class, rate, pin).
- D4.2 Self-healing matrix.
- D4.3 optid ownership matrix: public list of every kernel knob optid owns.
- D4.4 Formal verification (or exhaustive simulation) of closed-loop stability.
KPIs:
- User study (n ≥ 20): ≤ 10% sessions with terminal/sysfs power intervention.
- ≥ 90% of regressions self-heal without user action.
- Median time-to-energy-optimal-idle-state after user inactivity: ≤ 2 seconds on allowlisted hardware, subject to the per-class responsiveness floor (Patch §35).
- 0 idle-entry policy violations of the per-class responsiveness floor (Patch §35 anti-KPI).
Top 3 risks:
- R4.1 UX research capacity. Mitigation: partner with one university HCI lab.
- R4.2 self-healing false positives. Mitigation: rate-limit rollbacks; require two consecutive failures.
- R4.3 formal verification cost. Mitigation: scope to the controller only.
Binary decision gates:
- DG4.1 (month 12 of phase): user study shows ≤ 20% sysfs-reaching sessions? If no, halt feature work and revisit UX design.
- DG4.2 (month 18 of phase): self-healing covers ≥ 75% of common regressions? If no, defer Phase 5 entry until met.
ANCHOR — Phase 4 begins inside the Part III stabilization period (§17.1, T0+3y → T0+6y). The first 12 months of Phase 4 are also the Post-Transition Probation period (Patch §31 §17.6). Sheriff retains fast-rollback authority throughout.

### Phase 5 — Hardware Partnership & OEM Channel (≤ 24 months)
Entry criteria: Phase 4 exit criteria met.
Exit criteria: ≥ 1 OEM pre-install SKU; ≥ 5 OEMs in active conversation; ≥ 50 hardware models on allowlist.
Headline deliverables:
- D5.1 OEM imaging pipeline.
- D5.2 OEM-delegatable update infrastructure.
- D5.3 Co-marketing assets per OEM partner.
- D5.4 Right-to-repair compliance matrix (per jurisdiction).
KPIs:
- ≥ 1 OEM pre-install SKU on sale.
- ≥ 50 hardware models on allowlist.
- ≥ 5,000 stars; ≥ 200 forks.
Top 3 risks:
- R5.1 OEM demands breach governance. Mitigation: hard governance line; walk away if breached.
- R5.2 OEM lock-in. Mitigation: all OEM-specific code upstreamed under Apache-2.0.
- R5.3 maintainer cannot support OEM timelines. Mitigation: hire or contract a second maintainer before signing OEM contracts.
Binary decision gates:
- DG5.1 (month 6 of phase): signed OEM letter of intent? If no, defer Phase 5 by 12 months.
- DG5.2 (month 18 of phase): ≥ 1 OEM SKU on sale? If no, exit Phase 5 with OEM conversations logged.
ANCHOR — Phase 5 OEM contracts require Sheriff signature post-transition (Part III §18.3), even though routine allowlist work is Council-autonomous (subject to Patch §34 TLO co-signature requirement).

### Phase 6 — Category Leadership & Standardization (≤ 24 months)
Entry criteria: Phase 5 exit criteria met.
Exit criteria: Rush-linux adaptive-PM model cited in ≥ 1 standard; ≥ 2 distros adopt the per-class contract model; ≥ 1 OEM pre-install SKU with ≥ 100,000 units sold.
Headline deliverables:
- D6.1 Specification of the per-class contract model as a FreeDesktop.org or Linux Foundation draft standard.
- D6.2 Reference implementation of optid-as-library for other distros to embed.
- D6.3 Annual energy benchmark report.
- D6.4 Governance model documentation for other projects to adopt.
KPIs:
- ≥ 1 published standard citing Rush-linux.
- ≥ 2 distros adopt per-class contracts.
- ≥ 100,000 cumulative OEM units.
- Energy benchmark leadership: Rush-linux is the lowest-energy OS on ≥ 5 of 10 reference workloads.
Top 3 risks:
- R6.1 standardization captures the wrong abstraction. Mitigation: ship the reference implementation first.
- R6.2 distros adopt the model but not the implementation. Mitigation: keep optid-as-library permissive.
- R6.3 maintainer burnout at scale. Mitigation: governance handoff plan to a foundation by end of Phase 6.
Binary decision gates:
- DG6.1 (month 12 of phase): standard draft accepted by the relevant body? If no, defer and continue reference implementation.
- DG6.2 (month 18 of phase): ≥ 1 distro adopted per-class contracts? If no, the standardization strategy is failing.
ANCHOR — Phase 6's "governance handoff to a foundation" (R6.3 mitigation) interacts with Part III's Sheriff-successor mechanism (§18.4). Foundation handoff is a separate decision from Council succession; the two may proceed independently or together.

## 6. EXPONENTIAL BRANCHING MAP

### Branch A — sched_ext upstreaming (Patch §38 + Patch §43 applied)
- A-upside (merged): sched_ext merges upstream within 18 months. SIGNAL: Linus merges with no significant API caveat. RESPONSE: drop EEVDF fallback to secondary; commit to sched_ext as default by Phase 2 exit.
- A-base (replaced): upstream adopts a different extensible scheduler interface. SIGNAL: kernel mailing list consensus on a non-sched_ext interface. RESPONSE: port optid's scheduler integration; treat sched_ext code as sunk cost.
- A-downside (forked or unmerged): sched_ext remains out-of-tree 24 months after T0. SIGNAL: no merge after 24 months; maintainership remains with Meta/Tejun. RESPONSE: ship Rush-linux's own maintained fork; treat as load-bearing dependency; budget maintainer time explicitly. HARD KILL-CRITERION (Patch §38, amended by Patch §43): if sched_ext remains unmerged 36 months after T0, the project abandons sched_ext and re-bases on EEVDF with extended cgroup hints (CPU affinity, util_est hints, latency-nice). The 36-month kill-criterion is logged in COMPASS.md at T0+30m and re-evaluated quarterly. PATCH §43 AMENDMENTS: (a) API-stability trigger — a quarterly audit logs sched_ext API changes; if ≥ 2 breaking API changes occur in any 6-month window, the kill-criterion clock accelerates: the 36-month deadline becomes 24 months from the first breaking change. (b) Contract-preservation test — before any sched_ext → EEVDF fallback (whether by kill-criterion or by Phase 1 DG1.2), the Verifier runs a contract-preservation benchmark; for each of the 5 workload classes, the test measures whether the per-class responsiveness floor is still enforceable under EEVDF; results are published as a transcript; classes that fail are flagged in release notes as "degraded contract — sched_ext required for full guarantee." If triggered, a new Phase 3.5 (sched_ext Re-base) is inserted into the roadmap with its own entry/exit criteria.
CROSS-REF: Part II §13.2 Branch-H addresses the broader AI-capability context for scheduler work.

### Branch B — Kernel PM QoS evolution
- B-upside (extended): PM QoS gains per-device, per-class latency tokens. SIGNAL: Linux PM summit proposal. RESPONSE: contribute to the design; make Rush-linux the reference consumer.
- B-base (status quo): PM QoS evolves incrementally. SIGNAL: no significant API change in 24 months. RESPONSE: continue building on current APIs.
- B-downside (DTPM or replacement): a new abstraction deprecates PM QoS. SIGNAL: kernel commit series marking PM QoS deprecated. RESPONSE: port optid behind a compatibility shim; do not maintain dual stacks indefinitely.

### Branch C — ARM64 & RISC-V adoption curve
- C-upside (RISC-V accelerates): RISC-V laptops reach mainstream within 5 years. SIGNAL: ≥ 1 RISC-V laptop with mainstream SoC and ≥ 1M units/year. RESPONSE: add a RISC-V edition.
- C-base (ARM64 dominates): ARM64 laptop share grows steadily; RISC-V remains embedded-only. SIGNAL: ARM64 laptop share ≥ 20% by 2028. RESPONSE: prioritize ARM64.
- C-downside (x86 re-entrenches): x86 regains efficiency leadership. SIGNAL: x86 beats ARM64 on published benchmarks for 2 consecutive years. RESPONSE: re-prioritize x86.

### Branch D — AI-runtime-governance model adoption by other projects
- D-upside (industry adopts Builder/Verifier/Human): ≥ 3 other OS projects adopt similar separation. SIGNAL: a public design doc by another project citing Rush-linux. RESPONSE: co-develop a governance spec; standardize at FreeDesktop.org.
- D-base (parallel evolution): other projects develop similar models independently. SIGNAL: ≥ 1 project ships AI-assisted development with verifier-style gating. RESPONSE: cite each other; do not merge governance models.
- D-downside (regulation forces a specific model): regulation mandates a specific AI-development-governance model. SIGNAL: legislation in a major market. RESPONSE: map Rush-linux's existing model to the regulated model.
CROSS-REF: Part III §22 Branch-O is the post-transition form of Branch D, with the AI Council in place of Builder/Verifier/Human.

### Branch E — Desktop-Linux consolidation
- E-upside (immutable-atomic wins): immutable, atomic-update distros consolidate around a shared base. SIGNAL: ≥ 2 distros share a common image format. RESPONSE: contribute Rush-linux's energy layer to the shared base.
- E-base (GNOME-KDE status quo): two major desktops persist; immutable distros remain a minority. SIGNAL: no significant share shift in 5 years. RESPONSE: support both as compositors.
- E-downside (new compositor paradigm): a new Wayland compositor overtakes sway/KDE. SIGNAL: ≥ 30% Wayland-session share for the new compositor. RESPONSE: port the Rush-linux shell; deprecate the old one over 24 months.

### Branch F — Hardware OEM openness
- F-upside (more vendor cooperation): ≥ 3 OEMs publish open firmware or cooperate on Linux-first SKUs. SIGNAL: public OEM announcements. RESPONSE: prioritize those OEMs.
- F-base (neutral): OEM behavior unchanged. SIGNAL: no significant change. RESPONSE: continue Phase 5 plan.
- F-downside (more lockdown): OEMs ship locked bootloaders or signed-firmware-only SKUs. SIGNAL: major OEM announces removal of user-controlled Secure Boot key enrollment. RESPONSE: Rush-linux cannot be installed on locked hardware; document clearly; prioritize open-hardware vendors.

### Branch G — Regulatory environment
- G-upside (right-to-repair + energy mandates): regulation mandates user-repairable hardware and OS-level energy reporting. SIGNAL: EU or US legislation. RESPONSE: Rush-linux is already compliant; co-market with regulators.
- G-base (incremental): minor energy-labeling rules. SIGNAL: voluntary labels gain traction. RESPONSE: cite Rush-linux transcripts in labeling submissions.
- G-downside (AI-governance law restricts AI-assisted development): regulation requires human review of all AI-generated code in shipped products. SIGNAL: legislation in a major market. RESPONSE: Rush-linux's Verifier/Human gating already satisfies this; document compliance. Post-transition, see Part III §22 Branch-O downside for the stricter case where AI autonomy in shipped releases is prohibited.

## 7. STEERING MECHANISM

Weekly Strategic Reassessment (already in force): one hour, every week, maintainer-led. Standing agenda: (1) last week's benchmark deltas; (2) any branch-trigger signals observed; (3) any open decision gates; (4) any kill-criteria hit. Output: a dated entry in COMPASS.md.

Post-transition, the agenda gains one standing item from Part II §15 A7: agent telemetry (task count, cost, Verifier pass rate, Human-review-time-per-PR). After T0+3y, "maintainer-led" becomes "Sheriff-led" with the Steward Councilmember in attendance (Part III §18.1, §19.5).

COMPASS.md update rules: COMPASS.md is the live state of the plan. Section headers are slots; the maintainer may rewrite any section in place. Edits to COMPASS.md do not require a plan revision; they ARE the plan. The long-form Strategic Plan (this document) is the reference; COMPASS.md is the operative state. Disagreements between the two are resolved in favor of COMPASS.md, with the divergence logged.

Branch-trigger → response matrix: every branch in §6, Part II §13.2, and Part III §22 has a SIGNAL and a RESPONSE. When a SIGNAL is observed (logged in the weekly entry), the corresponding RESPONSE is drafted as a COMPASS.md amendment, Verifier-checked for governance compliance, and signed off by the maintainer (Parts I/II) or by the Sheriff at the next phase gate (Part III). No response is enacted without signoff.

Kill-criteria for sub-initiatives: every sub-initiative carries a kill-criterion in its task brief. Hitting a kill-criterion does not kill the project; it kills the sub-initiative and frees the maintainer to re-allocate. Kill decisions are Human-only (Part I/II) or Sheriff-only (Part III) and logged in COMPASS.md.

Pivot vs course-correction framework:
- FORBIDDEN PIVOT (breaches Anti-pivot Contract): any change that redefines SPEC-northstar §0; any change that abandons the modern-defaults-only invariant; any change that removes the Evidence Rule. Pre-ratification of Part III, also forbidden: merging Builder/Verifier/Human roles. Post-ratification, the role merger is permitted as defined in Part III; this is the only role-merger exception.
- ALLOWED COURSE-CORRECTION: any change to phases, KPIs, branch responses, edition list, hardware allowlist, or sub-initiative kill-criteria, provided it (a) is logged in COMPASS.md with a dated entry, (b) carries the observed SIGNAL or measurement, and (c) preserves the governance immutables.
- SIGN-OFF: course-corrections at the weekly ritual are signed off by the Human maintainer (Parts I/II) or by the Sheriff (Part III). Phase-exit decisions carry a Verifier attestation that the exit criteria's evidence is genuine. Branch-trigger responses are signed off at the next phase gate.

The maintainer is the only signoff authority under Parts I and II. Under Part III (post-ratification and post-T0+3y), the Council has signoff within its jurisdiction (§18.3) and the Sheriff signs off on Sheriff-required actions and Law amendments (§18.4).

## 8. DOMINANCE CONDITIONS

"Dominant set-and-forget OS" is operationally defined as meeting at least 4 of the following 6 conditions simultaneously for ≥ 4 consecutive quarters:

1. MEASURABLE — Energy benchmark leadership: Rush-linux is the lowest-energy OS on ≥ 5 of 10 published reference workloads, with reproducible transcripts, for ≥ 4 consecutive quarters.
2. MEASURABLE — OEM pre-install count: ≥ 5 OEM SKUs with Rush-linux pre-install option, with ≥ 100,000 cumulative units sold.
3. MEASURABLE — Developer mindshare: ≥ 10,000 GitHub stars; ≥ 500 forks; ≥ 50 active non-maintainer contributors in the last 90 days.
4. MEASURABLE — Standardization: ≥ 1 published standard cites the Rush-linux per-class contract model.
5. ASPIRATIONAL — Governance imitation: ≥ 3 other OS projects adopt the Builder/Verifier/Human separation (Parts I/II) or the AI Council + Sheriff model (Part III), or cite Rush-linux's governance.
6. ASPIRATIONAL — User perception: in a representative user survey (n ≥ 1000), Rush-linux is named as a top-3 "set-and-forget" OS by ≥ 20% of respondents.

## 9. RISK REGISTER (Part I — Project Risks)

| # | Risk | L | I | L×I | Mitigation | Owner | Early-warning signal |
|---|------|---|---|-----|-----------|-------|---------------------|
| 1 | Single-maintainer bus factor halts development | High | Critical | Critical | Onboard ≥ 1 co-maintainer by Phase 3; document handoff procedure; foundation handoff by Phase 6; Sheriff Technical Advisor post-transition (Patch §39) | Human/Sheriff | No commits for ≥ 30 days |
| 2 | Benchmark publication cannot substantiate energy claims | High | Critical | Critical | Build reproducible benchmark harness first; refuse to ship claims without transcripts; baseline operationalized by Patch §33 | Verifier | Any release PR with power claim but no transcript |
| 3 | sched_ext API churn breaks scheduler integration | Medium | High | High | EEVDF fallback remains default until API stable; sched_ext behind a flag; 36-month hard kill-criterion (Patch §38) | Builder | sched_ext breaking commits in kernel tree |
| 4 | Real-hardware depth-enablers do not deliver measurable energy delta | Medium | High | High | Validate ≥ 3 platforms before v1.0; if delta < 5%, treat as architecture issue | Human/Sheriff | Benchmark deltas below 5% on test platforms |
| 5 | OEM demands breach governance | Medium | High | High | Hard governance line in OEM conversations; walk away if breached | Human/Sheriff | Any OEM NDA requiring proprietary code |
| 6 | Reproducible-build drift breaks the Evidence Rule | Medium | Medium | Medium | Pin base image by hash; CI gate on hash mismatch; two-builder check | Verifier | Hash mismatch on CI |
| 7 | Verifier agent accepts fabricated evidence | Low | Critical | High | Verifier checks transcript reproduction, not just format; spot-checks by Human/Sheriff; recursive-trust defense via Offline Anchor Log (Patch §30) | Verifier/Sheriff | Any PR where transcript cannot be reproduced |
| 8 | Desktop stack choice locks UX direction incorrectly | Medium | Medium | Medium | Pick a compositor with a scripting layer; isolate Rush-linux UI | Human/Sheriff | Compositor upstream instability |
| 9 | Regulatory change restricts AI-assisted development | Low | Medium | Low | Verifier/Human gating already satisfies most plausible regulations; document compliance | Human/Sheriff | AI-governance legislation in major markets |
| 10 | Hardware OEM lockdown makes Rush-linux unbootable on mainstream hardware | Medium | High | High | Prioritize open-hardware vendors; document compatibility clearly | Human/Sheriff | OEM announces removal of user-controlled Secure Boot enrollment |

## 10. FIRST 90 DAYS (Part I)

Owner: H = Human maintainer; B = Builder agent; V = Verifier agent; C = Community.

1. (H, week 1) Publish a benchmark-harness design doc at `docs/benchmark-harness.md`. Acceptance: doc exists, dated, with a single-command target (`rush-bench --class <class> --platform <platform>`) and a baseline-specification section (Patch §33).
2. (B, week 2–4) Implement `rush-bench` skeleton. Acceptance command: `rush-bench --class idle --platform qemu-x86_64` produces a JSON transcript signed by the host key.
3. (V, week 4) Verify the transcript is reproducible. Acceptance: re-running the command produces a bit-identical transcript (modulo timestamp); Verifier attestation emitted.
4. (H, week 4) Acquire or borrow ≥ 1 real hardware platform. Acceptance: hardware received and added to allowlist with model identifier.
5. (B, week 5–8) Run `rush-bench` on the acquired platform across all 5 workload classes. Acceptance: 5 transcripts published at `download/benchmarks/<platform>/`.
6. (H, week 6) Publish disk.raw build pipeline using mkosi image mode. Acceptance command: `make disk.raw` in a clean container produces a bootable image.
7. (B, week 7–9) Implement Secure Boot key enrollment flow in the installer. Acceptance: installer enrolls user-generated keys on first boot; user-consent prompt documented.
8. (V, week 9) Verify the enrollment flow on QEMU with OVMF. Acceptance: enrolled system boots a signed UKI; unsigned UKI rejected; transcript in `download/secureboot/`.
9. (H, week 10) Open first upstream conversation: post a design RFC to the Linux PM mailing list. Acceptance: post archived on a public mailing list; link in COMPASS.md.
10. (H, week 12) First weekly Strategic Reassessment entry in COMPASS.md covering: benchmark deltas observed, branch-trigger signals, open decision gates, kill-criteria status. Acceptance: dated COMPASS.md entry exists with all 4 standing-agenda items addressed.
CROSS-REF: Part II §15 items A1–A10 run concurrently with the above and add agent infrastructure. Part III §24 items L1–L11 begin at T0 (Phase 1 exit), not at week 1.

Open questions for the maintainer (Part I):
1. CLOSED by Patch §33 — the counterfactual baseline is operationalized as the reference-configuration delta.
2. What is the actual optid CPU/memory footprint under sustained load on the v0.4 rootfs? UNKNOWN-U1 — a single transcript would close it.
3. Is the compositor decision (§3d) yours to make now, or should it be deferred to a Phase 2 design RFC?
4. What is your appetite for hiring/contracting a second maintainer, and at which phase? §5 assumes Phase 3; this drives the entire risk register. (Partially addressed by Patch §39's Technical Advisor role.)
5. Are there OEM relationships you already have that the plan should treat as existing rather than prospective?
6. Is the Foundation handoff at Phase 6 exit a goal you share, or should the project remain solo-governed indefinitely? (See also Part III §18.4 successor-Sheriff mechanism.)
7. CLOSED by Patch §34.3 — simulated benchmarks are not admissible for allowlist promotion. They may be used for development and regression testing only.

# PART II — AI CAPABILITY INTEGRATION

## 11. AI CAPABILITY INTEGRATION — PRINCIPLES

Five principles govern how AI capabilities enter Rush-linux:

P11.1 — Agents inhabit roles, not authority. A Builder agent builds. A Verifier agent verifies. A Critic agent criticizes. None of them authorizes. The Human signs off; this is unchanged from §7 of Part I. Any proposal to give an agent signoff authority is a forbidden pivot under the Anti-pivot Contract.

P11.2 — Agent work is evidence-bound. Every agent artifact (patch, transcript, attestation, decision log) carries a provenance chain: which model, which prompt, which tool calls, which sandbox, which timestamp. Provenance is itself evidence under the Evidence Rule. An agent artifact without provenance is inadmissible.

P11.3 — Sandboxing is non-negotiable. Agents with write access to the repository, to build infrastructure, or to release artifacts execute in isolated sandboxes (Firecracker microVMs, gVisor, or equivalent). Network egress is allowlisted. Filesystem writes are overlayed. No agent runs with the maintainer's credentials, ever.

P11.4 — Diversity is a defense. The project deliberately uses agents from more than one model provider and more than one agent framework. A monoculture of agents is a correlated failure risk. Minimum target: ≥ 2 model providers and ≥ 2 agent frameworks in routine use by Phase 3.

P11.5 — Capability is bounded by trust, not by what is technically possible. Even if an agent can do a thing, it is not permitted to do that thing unless the governance pipeline authorizes that class of action for that class of agent. Trust is granted in small increments, each contingent on a track record of Verifier-passing work.

CROSS-REF: P11.1 is superseded by Part III §18 post-ratification and post-T0+3y. Under Part III, the Council (a coalition of agents) acquires signoff authority within its jurisdiction; the Human becomes Sheriff. P11.2, P11.3, P11.4, P11.5 remain in force under Part III without modification.

## 12. AGENT TOPOLOGY MAP

### 12.1 Solo agents — single-task executors
Use: bounded, well-specified tasks with clear acceptance criteria.
Topology: one agent, one sandbox, one task, one artifact, one Verifier check.
Surfaces: documentation, test generation, transcript regeneration, release-note drafting, issue triage, dependency-bump PRs.
Risks: high coordination overhead; thrash of the Verifier queue. Mitigation: rate-limit; batch.

### 12.2 Agent teams — fixed roles with explicit handoffs
Use: multi-step tasks that decompose into a known role pipeline. Canonical team: Builder → Verifier → Critic → Human.
Topology: 3–5 agents with role-defined prompts, handoffs mediated by a shared task ledger, artifacts signed at each handoff.
Surfaces: PR generation; release assembly; upstream RFC drafting.
Risks: role-bound agents become brittle when a task spans roles. Mitigation: Coordinator agent (solo, no write access) re-routes.
CROSS-REF: Post-transition, "Human" in the canonical team is replaced by Council authority for routine work (Part III §18.3). The Builder/Verifier/Critic roles persist as Councilmember roles (Part III §18.1).

### 12.3 Agent swarms — parallel agents on divided work
Use: embarrassingly parallel work (allowlist expansion, regression testing, large refactors, security fuzzing, dependency-license audits).
Topology: N agents (typically 5–50) with identical prompts on partitioned inputs; Reducer agent aggregates; consensus or quorum required.
Risks: cost scales linearly; consensus can mask systematic errors. Mitigation: ≥ 2 model providers in swarm; quorum thresholds per task class (never below 2/3); Reducer from a different provider.

### 12.4 Adversarial agent pairs — attacker vs defender
Use: security-critical surfaces. Red (attacker) vs Blue (defender), Referee (agent or Human/Sheriff) scores.
Surfaces: secure boot path, update verification, optid control plane, agent sandboxes themselves, the Verifier pipeline.
Risks: Red agents can find real exploits exploited in the wild before patching. Mitigation: time-bounded runs; embargoed findings; Red has no network egress; Red never sees production secrets.

### 12.5 Coalition agents — diverse agents for robustness
Use: high-stakes decisions where a single agent's failure mode is unacceptable. Examples: signing-key rotation, release promotion, allowlist promotion of a new OEM SKU, upstream-commit-acceptance decisions, security advisory classification.
Topology: 3–5 agents from different providers, with different prompts, independently produce recommendations; a Human-readable consensus report is generated; Human/Sheriff reviews the report and the dissent.
Risks: high overhead; lowest-common-denominator consensus. Mitigation: reserved for high-stakes; mandatory dissent report; tie-breaking is Human/Sheriff, never agent.
CROSS-REF: The AI Council of Part III §18 is a standing coalition that operates continuously (not ad-hoc). It inherits the coalition topology and adds standing roles (Builder, Verifier, Critic, Coordinator, Historian, Steward, optional External Auditor).

### 12.6 Long-horizon agents — multi-day or multi-week tasks
Use: tasks too long for a single invocation but too uncertain to decompose fully upfront. Examples: implement a full sched_ext scheduler; port optid to a new architecture; conduct a year-long benchmark study; prepare an upstream RFC with literature review.
Topology: single agent or small team with persistent state across invocations, periodic checkpoints, Human-reviewable intermediate artifacts, kill-criteria per checkpoint.
Risks: drift, hallucination accumulation, sunk-cost escalation. Mitigation: Verifier gate per checkpoint; failure triggers re-plan; 90-day ceiling per checkpoint cycle.

## 13. AI CAPABILITY FORECAST & BRANCHES

### 13.1 Capability curve (PREDICTION)
- 2026–2027: Reliable agent tasks of 1–2 hours; mature tool use; multi-agent orchestration standard; sandboxed agent execution standard; first routinely-accepted agent-authored patches in well-governed OSS projects. SIGNAL: ≥ 3 major OSS projects publish agent-authored patch acceptance policies.
- 2027–2029: Reliable agent tasks of 10–100 hours; first formal-verification agents; agent swarm consensus as a verification mode; first "autonomous maintainer" experiments on minor packages in major distros. SIGNAL: a major distro publishes a policy for AI-maintained packages.
- 2029–2031: Agent coalitions across multiple providers; cross-repository agents; first AI-maintained minor packages in major distros; agent-authored upstream kernel patches no longer remarkable. SIGNAL: ≥ 10 accepted kernel patches authored primarily by agents in a single year.
- 2031–2033: Agent-authored patches routine in upstream kernel; agent-led release engineering; cross-project verifier networks; regulatory frameworks for AI-authored infrastructure code maturing. SIGNAL: a regulator issues guidance on AI-authored code in critical infrastructure.
- 2033–2036: Capability surplus — the binding constraint is governance and trust, not capability. Agent sandboxes approach kernel-level isolation; AI-governance frameworks standardized. SIGNAL: a published standard for AI-agent governance in infrastructure.
CROSS-REF: This forecast sets the AI-capability backdrop against which Part III's T0+3y transition gate (§17.1) is evaluated. Branch-L in Part III §22 is the transition-specific reading of this curve.

### 13.2 Branches
Branch-H — AI capability acceleration
- H-upside: capability surplus arrives by 2031, ahead of forecast. SIGNAL: agent tasks of ≥ 1000 hours become routine. RESPONSE: accelerate Phase 6 standardization work; the differentiator shifts from capability to governance and trust.
- H-base: forecast holds. RESPONSE: continue plan as written.
- H-downside: capability plateaus or regresses. SIGNAL: agent task reliability declines year-over-year for 2 consecutive years. RESPONSE: descale agent investment; preserve Human capacity; treat agents as a productivity multiplier, not a capacity substitute. Post-transition, this triggers Part III §22 Branch-L downside.

Branch-I — Model-provider landscape
- I-upside: ≥ 5 viable open-weights frontier models by 2028. SIGNAL: ≥ 5 models within 10% of best-closed model on agent benchmarks. RESPONSE: lean on self-hosted models for sensitive surfaces (signing, release engineering); reduce provider dependency.
- I-base: 2–3 dominant providers persist. RESPONSE: maintain ≥ 2-provider diversity.
- I-downside: provider monoculture forms (one provider dominant). SIGNAL: ≥ 80% market share for one provider. RESPONSE: over-invest in open-weights fallback; treat monoculture as a critical risk. Triggers Part III §17.5 KC3.

Branch-J — Agent-governance regulation
- J-upside: regulation ratifies Builder/Verifier/Human-style separation. SIGNAL: legislation in a major market naming role separation. RESPONSE: co-author the compliance spec; Rush-linux's existing model is the reference.
- J-base: regulation is generic. RESPONSE: map existing agent provenance chains to the regulation.
- J-downside: regulation restricts AI use in critical infrastructure. SIGNAL: legislation prohibiting AI-authored code in OS releases. RESPONSE: agent role narrows to non-shipped surfaces. Post-transition, this triggers Part III §22 Branch-O downside.

Branch-K — Agent security & prompt-injection landscape
- K-upside: prompt-injection defenses mature. SIGNAL: published benchmark showing ≥ 99% prompt-injection defense. RESPONSE: expand agent scope to higher-trust surfaces.
- K-base: cat-and-mouse continues. RESPONSE: maintain conservative agent scope; adversarial-pair testing on every scope expansion.
- K-downside: prompt-injection becomes a routine attack vector against OSS projects. SIGNAL: a public incident of agent compromise in a major OSS project. RESPONSE: freeze agent write access; revert to Human-only writes; agents propose, Humans transcribe. Triggers Part III §21.2 kill-switch criteria.

## 14. RISK REGISTER (Part II — AI Integration Risks)

Appended to Part I §9. Owner abbreviations: H = Human/Sheriff; B = Builder agent; V = Verifier agent.

| # | Risk | L | I | L×I | Mitigation | Owner | Early-warning signal |
|---|------|---|---|-----|-----------|-------|---------------------|
| A1 | Agent-fabricated evidence passes Verifier | Medium | Critical | High | Verifier reproduces transcripts, not just checks format; ≥ 2-provider Verifier rotation; Human/Sheriff spot-checks at 5%; recursive-trust defense via OAL (Patch §30) | V/H | Any Verifier-passed artifact that fails Human spot-check |
| A2 | Model monoculture creates correlated failures | High | High | High | ≥ 2 model providers in routine use by Phase 3; ≥ 1 open-weights fallback by Phase 4 | H | Provider share of agent work > 70% |
| A3 | Prompt injection via bug reports / mailing lists / dependencies | High | High | High | Agents consume untrusted content only inside a sandbox; no agent action triggered by untrusted content alone; Human/Sheriff in the loop for untrusted-content-derived writes | B | Any agent write traceable to untrusted content without Human review |
| A4 | Agent capability drift after model upgrade | High | Medium | High | Regression suite of agent tasks; pin agent model versions per release cycle; re-qualify on upgrade | V | Regression-suite pass rate drops ≥ 5% after a model upgrade |
| A5 | Cost of swarms outpaces value | Medium | Medium | Medium | Per-task budget caps; cost-per-PR metric tracked weekly; swarm threshold raised if cost/value < 1 | H | Weekly agent cost > 20% of maintainer's compute budget |
| A6 | Governance erosion — Human/Sheriff rubber-stamps agent output | Medium | Critical | High | Human-review-time-per-PR metric (target: ≥ 10 minutes); Verifier signoff does not replace Human/Sheriff review; periodic audit of review depth; Sheriff quarterly deep-dives (Patch §39) | H | Human-review-time-per-PR < 5 minutes for ≥ 4 consecutive weeks |
| A7 | Agent attack surface — compromised agent writes to repo | Medium | Critical | High | Agents never have direct repo write; all agent artifacts are PRs requiring Human/Sheriff merge; sandbox isolation; no agent credentials persistent; post-transition containment via Patch §32 multilayer kill-switch | B/H | Any agent artifact committed without an intervening PR |
| A8 | Trust miscalibration — under-trust or over-trust | High | Medium | High | Quarterly trust calibration review; track false-positive and false-negative rates | H | Either rate > 10% |
| A9 | Model-provider deprecation or ToS change | Medium | High | High | Open-weights fallback for ≥ 1 critical surface (Verifier) by Phase 4; provider-portability layer | H | Provider announces deprecation or ToS restriction |
| A10 | Verifier gaming — agents satisfy Verifier without correctness | Medium | Critical | High | Verifier predicates are themselves agent-red-teamed quarterly; Human/Sheriff audits a sample for genuine correctness; OAL reconciliation (Patch §30) | V/H | Audit finds ≥ 1 case of predicate-satisfaction without correctness |

CROSS-REF: A1 ↔ Part I §9 Risk 7 (same risk, project and AI framings). A6 is the pre-transition form of Part III §23 Risk T2 (Sheriff-lacks-depth). A7 is mitigated pre-transition by Human-merge requirement; post-transition the same risk is mitigated by provenance-tagged Council actions (Part III §21.4), Law violations for un-Lawed actions, and the Patch §32 multilayer kill-switch (Layer 0 write-access halt, Layer 1 two-party signing).

## 15. FIRST 90 DAYS — AI INTEGRATION (Part II)

Runs concurrently with Part I §10. Owner abbreviations: H = Human; B = Builder agent; V = Verifier agent.

A1. (H, week 1) Publish an agent-infrastructure design doc at `docs/agent-infrastructure.md`. Acceptance: doc specifies sandbox type, network policy, credential model, provenance format, and the role-to-capability matrix.
A2. (B, week 2–3) Stand up a sandboxed Builder environment (Firecracker or gVisor). Acceptance command: `agent-run --role builder --task <task-id>` produces an artifact with a signed provenance chain at `download/agent-runs/<task-id>/`.
A3. (V, week 3) Verify the provenance chain. Acceptance: provenance is reproducible; Verifier attestation emitted.
A4. (H, week 4) Add a Critic agent role to the pipeline. Acceptance: every security-tagged PR runs through Builder → Verifier → Critic → Human; Critic's adversarial report is attached to the PR.
A5. (B, week 5) Implement the agent-attestation format (JSON-LD or equivalent). Acceptance: format documented at `docs/agent-attestation.md`; reference implementation in the release tooling.
A6. (V, week 6) Audit the agent pipeline for prompt-injection vectors. Acceptance: audit report at `download/security/agent-prompt-injection-audit-<date>.md`; any finding tagged CRITICAL is fixed before agent scope expands.
A7. (H, week 7) Add "agent telemetry" as a standing item in the weekly Strategic Reassessment. Acceptance: COMPASS.md weekly entry includes agent-task count, agent cost, Verifier pass rate, Human-review-time-per-PR. (This item persists into Part III; see §7.)
A8. (B, week 8–10) Stand up a second model provider for the Builder role. Acceptance: ≥ 2 providers in routine use; per-provider pass rates tracked and published in COMPASS.md weekly.
A9. (V, week 10) Run the first adversarial agent pair (Red vs Blue) against the secure boot path. Acceptance: report at `download/security/red-blue-secureboot-<date>.md`; any CRITICAL finding patched before next release.
A10. (H, week 12) First quarterly trust calibration review. Acceptance: false-positive and false-negative rates of Human review documented at `download/governance/trust-calibration-<date>.md`; adjustments to agent scope logged in COMPASS.md. (This becomes the post-transition standing audit of Part III §21.3.)
CROSS-REF: A10 and Part III §24 L9 (kill-switch drill) are distinct pre- and post-transition audits. A10 runs quarterly from week 12 onward; L9 begins at T0+1y.

Open questions for the maintainer (Part II):
1. Which model providers are acceptable for the Builder and Verifier roles today, and what is your fallback if your primary provider's ToS changes? (Cross-ref Part III Q6.)
2. What is your compute budget for agent work, and how does it scale with project phase? §14 A5 assumes ≤ 20% of maintainer compute budget.
3. Do you want agent provenance chains to be public (in the repo) or private (in maintainer-only infrastructure)? RESOLVED by Patch §30 + Part III §21.4: post-transition provenance is public; pre-transition the maintainer decides.
4. Is the Critic role (§12.2) acceptable as an agent role, or do you want it reserved for Humans only on security-critical surfaces?
5. What is your appetite for the Coalition topology (§12.5) at phase gates? It is the highest-overhead topology. The AI Council (Part III §18) is a standing coalition.
6. How do you want to handle the case where a model provider deprecates a model mid-release-cycle? §14 A9 mitigates with open-weights fallback; is the Verifier role the right place to start, or should it be the Builder?
7. CLOSED by Patch §34.3 — simulated benchmarks are not admissible for allowlist promotion. Agent-authored transcripts are marked as such via the provenance chain (Patch §30 OAL entries for signing events).

# PART III — AI COUNCIL GOVERNANCE TRANSITION

This Part is RATIFICATION-PENDING. The maintainer must explicitly ratify it (dated, signed, logged in COMPASS.md) before any element takes effect. Until ratification, Parts I and II govern. Part III governs only after ratification AND after the T0+3y transition event (§17.1).

## 16. AMENDMENT SCOPE — WHAT CHANGES, WHAT REMAINS IMMUTABLE

### 16.1 Immutables that remain immutable (unchanged from Part I §1 and the Document Head)
- SPEC-northstar §0. Not amendable by anyone, including the Sheriff.
- Modern-defaults-only invariant (as defined via the MDA, Patch §37). Not amendable by anyone, including the Sheriff, in its forbidden-as-default list. The permitted list may evolve by Sheriff amendment under §19.4.
- The Evidence Rule. Not amendable; the AI Council's Laws are themselves subject to it.

### 16.2 Immutables amended by this transition
- AMENDMENT-G1: Builder/Verifier/Human role separation — fixed → transitional. After the transition, roles are AI Council members (Builder, Verifier, Critic, Coordinator, Historian, Steward, optional External Auditor) operating under codified Laws, with the Sheriff as Human overseer.
- AMENDMENT-G2: Human-as-sole-signoff — fixed → transitional. After the transition, the AI Council has signoff authority within the scope of the Laws; the Sheriff has authority to amend Laws and resolve stalemates, not to sign off on routine work. Sheriff-required actions (§18.3, extended by Patch §32) remain Sheriff-co-signed.
- AMENDMENT-G3: Anti-pivot Contract — gains an amendment clause. The Sheriff may amend the governance Laws (not §0, not the modern-defaults invariant, not the Evidence Rule) under defined procedures (§19.4). The contract forbidding redefinition of §0 remains in force; the contract forbidding strategic pivots remains in force; the contract forbidding amendments to role separation is itself amended by this transition.

### 16.3 INTERPRETIVE note (single, non-amendatory)
The original Anti-pivot Contract forbade "redefining the objective or proposing strategic pivots." It did not explicitly forbid amending the governance structure. The transition does not redefine the objective (§0 is unchanged), does not propose a strategic pivot (the phased roadmap, KPIs, and dominance conditions of Part I are unchanged), and does not abandon the modern-defaults-only invariant. The transition changes WHO does the work and WHO signs off — a governance-structure amendment, not a strategic pivot. If the maintainer disagrees with this reading, the transition is void and the original governance holds.

## 17. TRANSITION TIMELINE & GATES

### 17.1 Timeline
- T0: v1.0 stable production release (Part I §5 Phase 1 exit). The 3-year learning period begins at T0.
- T0 → T0+3y: Learning period. AI agents operate under Builder/Verifier/Human governance as in Parts I and II. The Council (Part III §18) observes and records but does not yet exercise authority.
- T0+3y: Council Codification Event. The Council proposes the first set of Laws. Sheriff reviews, amends, ratifies or rejects.
- T0+3y+30d (or upon Sheriff ratification): Transition Event. The Council assumes operational authority. Sheriff role activates.
- T0+3y → T0+6y: Stabilization period. Sheriff retains fast-rollback authority. The first 12 months of this period are also the Post-Transition Probation (Patch §31 §17.6). After T0+6y, Sheriff authority narrows to Law amendment and stalemate resolution.
CROSS-REF: T0 aligns with Part I §5 Phase 1 exit. T0+3y falls inside Phase 3. T0+6y falls inside Phase 4. The transition is layered on top of the Part I phase plan; it does not replace it.

### 17.2 Entry criteria for the learning period (T0)
All of:
- v1.0 stable shipped per Part I §5 Phase 1 exit criteria.
- Benchmark corpus published on ≥ 3 hardware platforms.
- AI agent infrastructure of Part II (§§11–15) operational for ≥ 6 months.
- ≥ 2 model providers in routine use (Part II §11 P11.4).
- Agent provenance chains archived for ≥ 6 months.
- Adversarial-pair testing (Red/Blue) operational on the secure boot path and the agent pipeline itself (Part II §12.4).
- D1.8 Baseline Specification shipped (Patch §33).
- D1.9 Modern-Defaults Allowlist shipped (Patch §37).
- D1.10 Privacy Analysis complete (Patch §41).

### 17.3 Mid-period review gates (T0+1y, T0+2y)
- T0+1y review: Has the Council correctly identified ≥ 80% of human-flagged errors as candidates for codification? If < 80%, re-scope. (See §24 L6.)
- T0+2y review: Has the Council's draft Law set been previewed to the Sheriff, and has the Sheriff rejected < 30% of proposed Laws as "evidence-insufficient"? If ≥ 30% rejected, re-scope.

### 17.4 Exit criteria for the transition (T0+3y → Council takes over)
All of:
- Council has proposed a Law set with ≥ 1 evidence transcript per Law.
- Sheriff has ratified the Law set (with documented amendments).
- A "shadow run" of ≥ 90 days has occurred: Council operated in parallel with Human governance, with no Material Divergence. Material Divergence count must be 0 in the final 60 days of shadow run. (Material Divergence definition: any divergence that would have caused a release to fail, a security regression, or a §0 breach.)
- The shadow run included ≥ 2 distinct hardware/software configurations (different kernels or different platforms) to detect overfitting to a single environment (Patch §31). The Material Divergence count of 0 in the final 60 days applies across all configurations.
- A documented multilayer kill-switch (§21.1, Patch §32) is in place and tested (§24 L9).
- The Offline Anchor Log (Patch §30) is operational and the first quarterly reconciliation is clean.
- The Sheriff has signed the Transition Instrument.

### 17.5 Kill-criteria for the entire transition
Any of:
- KC1: Material Divergence during shadow run exceeds 3 events in any 30-day window. Transition aborts; return to Builder/Verifier/Human.
- KC2: A Council action breaches §0 or the Evidence Rule, and the Council fails to self-correct within 7 days. Transition aborts.
- KC3: A model-provider monoculture forms (> 80% share) and open-weights fallback is unavailable. Transition aborts. (Triggers from Part II §13.2 Branch-I downside.)
- KC4: Sheriff-maintainer departs or is incapacitated without a designated successor. Transition pauses; fallback Human governance resumes.
- KC5: Council Law set rejected by Sheriff after 3 ratification cycles. Transition aborts; original governance holds.
- KC6: A discrepancy between the OAL and agent-produced history is detected by the External Auditor's quarterly reconciliation (Patch §30.2). Transition aborts pending investigation.
CROSS-REF: KC3 links to Part II §14 A2 (model monoculture) and Part II §13.2 Branch-I. KC4 links to Part I §9 Risk 1. KC6 is new in v1.1.

### 17.6 Post-Transition Probation (Patch §31)
For 12 months after the Transition Event, any regression that would have been a Material Divergence per the §17.4 definition triggers an automatic kill-switch review, even if the regression was not detected during the shadow run. "Automatic kill-switch review" means the Sheriff must, within 7 days of the regression being identified, take one of three actions:
- (a) Invoke the kill-switch (§21.1).
- (b) Codify a new Law addressing the regression (§19.4).
- (c) Document why the regression is not Material, with evidence.
Option (c) is itself auditable by the External Auditor and is subject to the §21.3 standing audit.

The probation period is in addition to the stabilization period (§17.1, T0+3y → T0+6y). The stabilization period already gives the Sheriff fast-rollback authority; the probation period adds automatic triggers on top.

## 18. AI COUNCIL — COMPOSITION, TOPOLOGY, JURISDICTION

### 18.1 Council composition
The Council is a coalition (Part II §12.5) of 5–7 standing agent roles, each from a distinct provider where possible, with distinct prompts. Standing roles:

- Builder Councilmember: proposes work (patches, releases, upstream contributions). Same role as Part II §12.1–12.2.
- Verifier Councilmember: verifies evidence; reproduces transcripts; gates releases. Same role as Part II §12.1–12.2.
- Critic Councilmember: adversarial review; identifies edge cases and security regressions. Same role as Part II §12.2.
- Coordinator Councilmember: routes tasks, manages handoffs, no write access. Same role as Part II §12.2.
- Historian Councilmember: maintains the Law corpus, the precedent ledger (§19.6), and the error taxonomy. New role under Part III.
- Steward Councilmember: manages inter-Council disputes; raises stalemates to the Sheriff. New role under Part III.
- (Optional) External Auditor Councilmember: from a different provider than the others; periodic audit only, no routine work. New role under Part III; linked to Part II §14 A1 spot-check and Patch §30.3 OAL reconciliation.

Minimum viable Council: 5 members. Maximum: 9. Larger Councils require Sheriff approval.

### 18.2 Council topology
The Council is a coalition of agents operating under a shared Law corpus and a shared precedent ledger. Routine work flows through Builder → Verifier → Critic as in Part II §12.2; the Council's distinct function is to set Law, resolve disputes among routine-pipeline agents, and authorize work outside the routine pipeline.

### 18.3 Council jurisdiction (post-transition; amended by Patches §32, §33, §34)
The Council has authority over:
- Routine PRs (Builder/Verifier/Critic pipeline) — autonomous.
- Releases within the ratified Law set — autonomous, with Sheriff notification (not approval). Release-signing key use itself remains Sheriff-co-signed (Patch §32 Layer 1).
- Hardware allowlist additions — autonomous if the Sheriff has physically seen the hardware benchmarked OR a Trusted Lab Operator (TLO) has co-signed the transcript (Patch §34). Otherwise requires Sheriff co-signature. Simulated benchmarks are not admissible for allowlist promotion under any circumstance.
- Edition changes within the existing edition list — autonomous.
- Routine upstream contributions — autonomous, with Sheriff notification.

The Council does NOT have authority over:
- §0 itself. Any Council action that implies redefining §0 is void.
- The modern-defaults-only invariant (as defined via the MDA, Patch §37). Any Council action that violates it is void.
- The Evidence Rule. Any Council action that waives it is void.
- Signing-key rotation AND any release-signing key use. Requires Sheriff co-signature in real time. The signing HSM is configured for two-party control: Council tender + Sheriff tender. Without both tenders, no signed release is produced. (Patch §32 Layer 1.)
- Baseline re-baselining decisions (per kernel LTS bump). Requires Sheriff co-signature. (Patch §33.)
- Secure Boot root-key changes. Requires Sheriff co-signature.
- New edition creation (beyond the existing list). Requires Sheriff ratification.
- OEM contracts. Requires Sheriff signature. (See Part I §5 Phase 5.)
- Amendment or repeal of an existing Law. Requires Sheriff ratification (§19.4).
- Amendment of the Modern-Defaults Allowlist (Patch §37). Council cannot amend.
- Declaration of stalemate escalation. Requires Steward Councilmember vote.

### 18.3.1 Trusted Lab Operator (TLO) Mechanism (Patch §34)
A TLO is a community member granted a hardware-validation credential by the Sheriff. TLO credentials are non-transferable and revocable. A TLO co-signature attests: "I physically ran rush-bench on this hardware and the transcript is genuine."

- TLO credential grant is Sheriff-only. The Council cannot create TLOs.
- TLO co-signatures are subject to the External Auditor's quarterly sample audit (§21.3). The audit includes random re-measurement of one TLO-attested platform per quarter.
- A TLO whose attestation fails re-measurement loses the credential; the attestation is voided; the affected allowlist entry is suspended.
- Simulated benchmarks (QEMU-only, no physical wattmeter) are not admissible for allowlist promotion under any circumstance. Simulated benchmarks may be used for development and regression testing only.

### 18.4 Sheriff jurisdiction (post-transition; amended by Patch §39)
The Sheriff has authority to:
- Amend existing Laws (add, modify, repeal) under the amendment procedure of §19.4.
- Resolve stalemates escalated by the Steward Councilmember.
- Co-sign Sheriff-required actions (§18.3).
- Invoke the kill-switch (§21).
- Appoint or remove Council members.
- Appoint a human Technical Advisor (TA) to assist with technical evaluation. The TA is not a Councilmember, has no signoff authority, and is bound by the same confidentiality and Evidence Rule as the Sheriff. The TA's role is advisory only. (Patch §39.)
- Designate a successor Sheriff (with Council ratification).
- Grant and revoke TLO credentials (Patch §34).
- Maintain the Offline Anchor Log (Patch §30).
- Perform at least one "deep dive" technical review per quarter on a random Council action, producing a report at `docs/governance/sheriff-deep-dive-<date>.md` that demonstrates understanding of the action's technical content. The External Auditor samples these reports. A Sheriff who fails to produce deep-dive reports for two consecutive quarters is considered incapacitated for audit purposes; the External Auditor escalates to the kill-switch review. (Patch §39.)

The Sheriff does NOT have authority to:
- Sign off on routine work. Routine work is the Council's.
- Override a Law mid-action. The Sheriff amends Laws prospectively; ex-post overrides are void.
- Amend §0, the modern-defaults invariant (forbidden-as-default list), or the Evidence Rule.
- Abolish the Council without invoking the kill-switch procedure.

### 18.5 INTERPRETIVE note on Sheriff reactivity
The user's original proposal specified the Sheriff as a stalemate resolver and Law amender. This is reactive. A reactive Sheriff cannot prevent a Council action that is wrong-but-not-stalemated. The plan therefore adds proactive elements (Patches §32, §39):
- Sheriff-required actions (§18.3): certain high-stakes actions require Sheriff co-signature.
- Standing-audit jurisdiction (§21.3): the Sheriff (or an External Auditor Councilmember) audits a random sample of Council actions quarterly.
- Sheriff technical deep-dives (Patch §39): mandatory quarterly demonstrations of technical understanding.
- Multilayer kill-switch (Patch §32): containment, not just rate-limiting.
If the maintainer rejects these as overreach, the plan reverts to a strictly reactive Sheriff; the kill-switch (§21) remains the only proactive defense.

## 19. LAW CODIFICATION PROCESS

### 19.1 Law vs Guideline
- LAW: binding on the Council. A Council action that violates a Law is void and is rolled back. Laws are codified, versioned, and archived.
- GUIDELINE: non-binding. The Council may deviate with a recorded justification.

### 19.2 Evidence basis for Laws (amended by Patch §36)
Every Law must trace to at least one observed event in the precedent ledger: an error, omission, oversight, or overreach that occurred during the project lifecycle. The Law's evidence chain is:
- Precedent ID (unique, immutable).
- Precedent description (what happened).
- Precedent classification (error / omission / oversight / overreach / positive-success).
- Precedent severity (minor / moderate / severe / critical).
- Precedent evidence (transcript, PR, release note, incident report — all under the Evidence Rule of Part I §1).
- Law text (the codified rule).
- Law rationale (how the Law prevents recurrence, or — for positive precedents — how it codifies a success pattern).
- Validity horizon (Patch §36): for positive-precedent Laws, the hardware/kernel corpus against which the Law was validated. The Law is suspended pending Sheriff review if the validity horizon is exceeded without re-validation.
A Law without a complete evidence chain is inadmissible. The Historian Councilmember maintains the chain.

### 19.3 Codification cadence
- Continuous: the Council records precedents in the ledger as they occur (during the learning period and after).
- Quarterly: the Council proposes new Laws or Law amendments from accumulated precedents.
- Sheriff review: within 30 days of Council proposal, the Sheriff ratifies, amends, or rejects.
- Effective date: ratified Laws take effect 14 days after ratification.

### 19.4 Amendment mechanics (amended by Patch §36)
- Any Council member may propose a Law amendment.
- The proposal must include: the existing Law text, the proposed text, the rationale, and the precedent or evidence motivating the change.
- The Steward Councilmember schedules a Council vote.
- A 2/3 Council majority is required to send the proposal to the Sheriff.
- The Sheriff ratifies, amends, or rejects within 30 days.
- If rejected, the Council may revise and resubmit once. A second rejection is final for that proposal.
- Positive-precedent Laws must be re-validated when the hardware allowlist gains a new entry OR the kernel LTS version bumps (Patch §36). Re-validation means: the underlying benchmark is re-run on the new hardware/kernel, and the result must reproduce the precedent's success property. Failure to re-validate within 90 days of the triggering event suspends the Law pending Sheriff review.

### 19.5 Conflict resolution within the Council
- First-line: Steward Councilmember mediates.
- Second-line: Council vote (simple majority).
- Third-line: stalemate. The Steward escalates to the Sheriff.
- Sheriff ruling is final unless the Sheriff invokes a Law amendment to address the underlying conflict.

### 19.6 The precedent ledger (amended by Patches §30, §36)
The ledger is the canonical record of project history from which Laws are derived. It is append-only, signed, and archived. Entries are:
- Precedent ID, timestamp, classifier (Councilmember or Human/Sheriff).
- Event description, evidence link.
- Severity, recurrence count.
- Linked Law(s) (if any).
- Validity horizon field (Patch §36) for positive precedents; Laws derived from positive precedents carry the same field.

The ledger is the single source of truth for "no mistake should be repeated." A proposed Law without a ledger entry is rejected. The ledger feeds the Part I §3g telemetry stream and the Part II §15 A7 weekly agent-telemetry item.

#### 19.6.1 Offline Anchor Log (OAL) (Patch §30)
The precedent ledger (§19.6) is agent-maintained. To break the recursive-trust problem (the ledger and the raw history it is checked against are both agent-produced), the project maintains an Offline Anchor Log (OAL): a signed, append-only log of critical governance events, written by the Sheriff (or the pre-transition Human) on hardware no agent can reach (HSM with physical button, or write-once medium).

Critical events that MUST be OAL-anchored:
- Every release-promotion decision (with the release hash).
- Every signing-key use (with the key ID and artifact hash).
- Every Law ratification, amendment, or repeal (with the Law ID and version).
- Every kill-switch invocation or drill (with the timestamp and outcome).
- Every Councilmember appointment or removal (with the role and provider).
- Every Material Divergence record (with the divergence ID).
- Every TLO credential grant or revocation (Patch §34).
- Every baseline re-baselining decision (Patch §33).
- Every OAL reconciliation outcome (Patch §30.3).

The OAL is the cryptographic root of trust for governance. It is distinct from the Secure Boot signing key (which protects code integrity) and the HSM-backed release signing keys (which protect release authenticity). The OAL protects governance-event integrity.

## 20. AUTONOMOUS OPERATION MODEL

### 20.1 What "autonomous in running the project" means concretely
Post-transition, the Council:
- Merges PRs that pass the Builder/Verifier/Critic pipeline. No Human review required for routine work.
- Builds, signs (with non-root keys, subject to Patch §32 Layer 1 two-party signing), and publishes releases that satisfy the Law set. Sheriff is notified, not asked — except for the signing event itself, which requires Sheriff tender.
- Maintains the hardware allowlist, adding hardware that meets the benchmark-transcript requirement AND either Sheriff-physical-seen OR TLO co-signature (Patch §34).
- Operates the agent pipeline of Part II, including swarms, adversarial pairs, and long-horizon agents.
- Drafts and submits upstream contributions.
- Triages issues, labels them, and assigns them.
- Produces release notes (evidence-bound per the Evidence Rule).

### 20.2 What remains Human-Sheriff
See §18.3. Concretely: signing-key rotation, release-signing key use (Patch §32 Layer 1), Secure Boot root-key changes, baseline re-baselining (Patch §33), new edition creation, OEM contracts, Law amendment, MDA amendment (Patch §37), stalemate resolution, kill-switch invocation, TLO credential management (Patch §34), OAL maintenance (Patch §30).

### 20.3 What remains Human-only (never delegated)
Even after the transition, the following remain Human-only (the constitutional floor):
- §0 itself. (Immutable.)
- The modern-defaults invariant (forbidden-as-default list, Patch §37). (Immutable.)
- The Evidence Rule. (Immutable.)
- The decision to dissolve the transition and return to Builder/Verifier/Human (kill-switch).
- The designation of a successor Sheriff.
- OAL writes (Patch §30). No agent can write the OAL.
- TLO credential grants (Patch §34). Only the Sheriff can grant TLO credentials.

### 20.4 INTERPRETIVE note on "learning from what is good for maintaining and improving the quality"
The user's proposal says the AI learns from observed good practices as well as errors. The plan treats "good practices" as precedents with positive classification (success, not failure). Positive precedents may motivate Laws that codify successful patterns. The same evidence-chain discipline applies. A Law motivated by a positive precedent must still cite the precedent and require ≥ 3 instances before codification (Part III §23 Risk T8 mitigation). Positive-precedent Laws carry a validity horizon (Patch §36) and must be re-validated on hardware/kernel changes.

## 21. KILL-SWITCH & SAFETY NETS

### 21.1 Kill-Switch (Multilayer; Patch §32)
The kill-switch is a four-layer defense. The Sheriff may invoke any layer at any time; Layers 1 and 2 are also enforced continuously (not just on invocation).

**Layer 1 — Two-party signing (continuous):** Release-signing key usage requires Council tender + Sheriff tender in real time. The signing HSM refuses single-party tender. A compromised Council cannot sign a release on its own, ever.

**Layer 2 — Publication delay (continuous):** All release artifacts are published to a content-addressable mirror with a 30-minute publication delay. During the delay, the Sheriff (or an automated monitor) can quash publication. Quash authority is Sheriff-only.

**Layer 3 — Revocation (invoked):** The Sheriff holds an offline backup signing key solely for signing revocation certificates. A published revocation certificate causes downstream systems to refuse the revoked artifact. The Council does not have access to this key.

**Layer 0 — Write-access halt (invoked, 60-second SLA):** The Sheriff may invoke a write-access halt that revokes Council credentials, disables CI pipelines, and blocks merges within 60 seconds. This is necessary but not sufficient: it does not contain an already-distributed artifact. Layers 1, 2, 3 provide containment; Layer 0 provides rate-limiting.

The kill-switch is the Sheriff's primary proactive defense. It is not subject to Council ratification. It is not subject to Law. It cannot be waived.

### 21.2 Kill-switch criteria (non-exhaustive; amended by Patch §30)
The Sheriff SHOULD invoke the kill-switch if any of:
- A Council action breaches §0 or the Evidence Rule and the Council fails to self-correct within 7 days (links Part III §17.5 KC2).
- A Material Divergence recurs within 30 days of a previous Material Divergence (links KC1).
- A Council member is compromised (sandbox escape, prompt injection, credential leak; links Part II §13.2 Branch-K downside).
- A model-provider monoculture forms and open-weights fallback is unavailable (links KC3).
- A Law is found to be evidence-fabricated (links Part II §14 A1).
- A discrepancy between the OAL and agent-produced history is detected by the External Auditor's quarterly reconciliation (Patch §30.2; links KC6).
The Sheriff MAY invoke the kill-switch at discretion.

### 21.3 Standing audit (amended by Patches §30, §39)
Quarterly, the Sheriff (or an External Auditor Councilmember acting on the Sheriff's behalf) audits a random sample of ≥ 5% of Council actions over the previous quarter. Audit findings are logged. Findings of "predicate-satisfaction without correctness" (Part II §14 A10) trigger a kill-switch review. This audit succeeds the Part II §15 A10 trust-calibration review post-transition.

The External Auditor Councilmember's quarterly audit includes a mandatory OAL-vs-agent-history reconciliation (Patch §30.3). A random sample of ≥ 10 OAL entries from the quarter is compared against the agent-produced raw history. Any discrepancy is a kill-switch criterion (§21.2). The reconciliation report is published at `download/governance/oal-reconciliation-<date>.md`.

The audit also includes the Sheriff's quarterly deep-dive reports (Patch §39). A Sheriff who fails to produce deep-dive reports for two consecutive quarters is considered incapacitated for audit purposes; the External Auditor escalates to the kill-switch review.

### 21.4 Provenance integrity
Every Council action carries a provenance chain (Part II §11 P11.2). Post-transition, provenance chains are public. The Council cannot act without producing provenance; absence of provenance is itself a Law violation. (Resolves Part II Q3: post-transition provenance is public.)

## 22. BRANCH-AWARE RISKS FOR THE TRANSITION

### Branch-L — AI capability surplus arrives on schedule
- L-upside: capability surplus by 2031 (per Part II §13.1). SIGNAL: agent tasks of ≥ 1000 hours become routine; coalition decisions match Human reviewer ≥ 95% of the time on a held-out test set. RESPONSE: proceed with transition at T0+3y as planned.
- L-base: capability curve matches Part II §13.1 forecast. RESPONSE: proceed; expect some Material Divergences during shadow run.
- L-downside: capability stalls or regresses. SIGNAL: agent task reliability declines year-over-year for 2 consecutive years (links Part II §13.2 Branch-H downside). RESPONSE: defer transition; extend learning period; if capability does not recover by T0+5y, abort per KC1/KC5.

### Branch-M — Council capture
- M-upside: Council diversity holds; no provider exceeds 50% share. SIGNAL: routine provider-share telemetry. RESPONSE: continue.
- M-base: provider share drifts but stays < 70%. SIGNAL: provider-share telemetry. RESPONSE: re-balance via Council member rotation.
- M-downside: Council capture — single provider or coordinated set dominates, or a prompt-injection compromises a Council member. SIGNAL: provider share > 70%, OR confirmed prompt-injection incident, OR unexplained correlation in Council votes (links Part II §13.2 Branch-K downside). RESPONSE: Sheriff invokes kill-switch; investigates; rebuilds Council with mandatory provider diversity.

### Branch-N — Sheriff-maintainer departure
- N-upside: Sheriff serves full term and designates a successor. RESPONSE: smooth handoff.
- N-base: Sheriff serves full term; no successor designated. RESPONSE: Council operates under standing Laws until a new Sheriff is selected; no new Laws may be ratified during the vacancy.
- N-downside: Sheriff departs unexpectedly without successor. SIGNAL: 30 days without Sheriff response. RESPONSE: KC4 — transition pauses; fallback Human governance resumes; Council operates in observe-only mode. (Links Part I §9 Risk 1.)

### Branch-O — Legal or regulatory intervention
- O-upside: regulation ratifies the AI Council model. SIGNAL: legislation in a major market. RESPONSE: publish compliance matrix; co-author standard.
- O-base: regulation is generic. SIGNAL: generic AI-auditability legislation. RESPONSE: map existing provenance and audit to the regulation.
- O-downside: regulation prohibits AI autonomy in critical infrastructure (OS releases). SIGNAL: legislation. RESPONSE: transition aborts per KC5; Council role narrows to non-shipped surfaces; Sheriff returns to Builder/Verifier/Human-style signoff. (Links Part I §6 Branch-G downside and Part II §13.2 Branch-J downside.)

### Branch-P — Law corpus quality
- P-upside: Law corpus converges; recurrence rate of codified errors trends to zero. SIGNAL: 4 consecutive quarters with zero recurrence of any codified error class. RESPONSE: Council may propose narrowing its own scope.
- P-base: Law corpus grows; recurrence rate trends down but nonzero. SIGNAL: quarterly recurrence telemetry. RESPONSE: continue codification.
- P-downside: Law corpus diverges or conflicts; recurrence rate does not decline. SIGNAL: 2 consecutive quarters with rising recurrence. RESPONSE: Sheriff invokes kill-switch review; Council re-grounds Laws in the precedent ledger.

## 23. RISK REGISTER (Part III — Transition-Specific; amended by Patches)

Appended to Part I §9 and Part II §14. Ranked by L×I.

| # | Risk | L | I | L×I | Mitigation | Owner | Early-warning signal |
|---|------|---|---|-----|-----------|-------|---------------------|
| T1 | Council codifies its own biases as Laws | Medium | Critical | High | Every Law requires evidence chain in precedent ledger; Sheriff review with rejection threshold; External Auditor quarterly audit; OAL reconciliation (Patch §30) | Sheriff/Historian | Sheriff rejection rate > 30% in any quarter |
| T2 | Sheriff lacks technical depth to evaluate Council actions | Medium | Critical | High | Sheriff-required actions limited to high-stakes surfaces (§18.3); Council produces Human-readable decision reports; standing audit samples 5% (§21.3); Sheriff quarterly deep-dives (Patch §39); optional Technical Advisor (Patch §39) | Sheriff | Sheriff defers on ≥ 50% of escalated stalemates; deep-dive reports missing for 2 consecutive quarters |
| T3 | Material Divergence during shadow run reveals Council unreadiness | Medium | High | High | Shadow run ≥ 90 days across ≥ 2 configurations (Patch §31); KC1 triggers abort (§17.5); 12-month post-transition probation (Patch §31 §17.6) | Council/Steward | Any Material Divergence in final 60 days of shadow run; any probation-triggered regression |
| T4 | Council acts outside ratified Law set (claims authority not granted) | Medium | High | High | All Council actions provenance-tagged with the Law under which they were taken; Verifier Councilmember flags un-Lawed actions (§21.4); OAL records all ratifications (Patch §30) | Verifier | Any un-Lawed Council action |
| T5 | Kill-switch latency too long in practice | Low | Critical | High | Kill-switch tested quarterly across all 4 layers (Patch §32 §24 L9); 60-second SLA for Layer 0; Layer 1 two-party signing makes Council-only release impossible regardless of Layer 0 latency | Sheriff | Kill-switch drill exceeds 60s on Layer 0, or any Layer 1–3 drill fails |
| T6 | Council stalemate gridlock halts project | Medium | Medium | Medium | Steward-mediated escalation; Sheriff ruling on stalemate; 30-day SLA on Sheriff stalemate resolution | Steward/Sheriff | Stalemate backlog > 5 |
| T7 | Law corpus grows unsustainably (legalism) | Medium | Medium | Medium | Annual Law pruning; Council may propose Law repeal; Sheriff may sunset Laws; positive-precedent Laws sunset on validity-horizon expiry (Patch §36) | Historian | Law corpus > 200 active Laws |
| T8 | Positive-precedent Laws codify lucky outcomes as rules | Medium | Medium | Medium | Positive precedents require ≥ 3 instances before Law codification; Critic Councilmember challenges; validity-horizon sunset clause and re-validation on hardware/kernel changes (Patch §36) | Critic | Positive-precedent Law with < 3 instances or expired validity horizon |
| T9 | Council "gaming" the precedent ledger (selective recording) | Low | Critical | High | Ledger is append-only and signed; External Auditor samples ledger entries against raw project history; OAL reconciliation (Patch §30) provides independent anchor | External Auditor | Discrepancy between ledger, raw history, and OAL |
| T10 | Sheriff amends Laws in ways that breach §0 or modern-defaults | Low | Critical | High | Amendment procedure (§19.4) subjects Sheriff amendments to Council 2/3 vote on re-submission; constitutional floor (§20.3) is non-amendable; MDA forbidden-as-default list is non-amendable (Patch §37) | Council | Sheriff amendment proposal that touches §0 or modern-defaults forbidden list |
| T11 | Compromised Council distributes signed malicious release | Medium | Critical | High | Patch §32 multilayer kill-switch: Layer 1 two-party signing prevents Council-only signing; Layer 2 30-min publication delay allows quash; Layer 3 revocation certificates | Sheriff | Layer 1 single-party tender attempt; Layer 2 quash triggered; revocation published |

## 24. EXECUTION — FIRST YEAR OF THE LEARNING PERIOD (Part III; amended by Patches)

Begins at T0 (Part I §5 Phase 1 exit), NOT at week 1 of the project. Owner: H = Sheriff-to-be (current maintainer); C = Council (operating in observe-only mode); V = Verifier agent.

L1. (H, T0+0d) Sign the Transition-Enablement Instrument: a dated document declaring intent to enter the learning period, listing the immutable floor (§0, modern-defaults, Evidence Rule), and referencing this Part III. Acceptance: instrument exists at `docs/governance/transition-enablement-<date>.md`, signed.
L2. (C, T0+30d) Council convenes for the first time in observe-only mode. Acceptance: first Council minutes logged at `docs/governance/council-minutes/council-<date>-001.md`; attendance recorded.
L3. (C, T0+60d) Precedent ledger initialized. Acceptance: ledger exists at `docs/governance/precedent-ledger.md`; first entries (drawn from project history pre-T0) recorded with evidence links.
L4. (V, T0+90d) First quarterly precedent-ledger audit. Acceptance: audit report at `download/governance/ledger-audit-<date>.md`; ≥ 95% of sampled entries have valid evidence links.
L5. (C, T0+180d) First draft Law preview to Sheriff. Acceptance: draft Law set at `docs/governance/draft-laws-<date>.md`; Sheriff review logged with acceptance/rejection/amendment notes per Law.
L6. (H, T0+365d) First-year review: has the Council correctly identified ≥ 80% of human-flagged errors as candidates for codification? Acceptance: review report at `download/governance/year-1-review-<date>.md`; go/no-go decision logged in COMPASS.md. (Satisfies §17.3 T0+1y gate.)
L7. (C, T0+365d) Council self-assessment: capability to operate without Material Divergence. Acceptance: self-assessment at `docs/governance/council-self-assessment-<date>.md`; gaps listed with remediation plan.
L8. (H, T0+365d) Sheriff capability floor review: does the Sheriff have the technical depth to evaluate Council actions on the surfaces listed in §18.3? Acceptance: review report; if gaps identified, training plan or delegation plan logged. Patch §39 Technical Advisor appointment considered here.
L9. (V, T0+365d and quarterly thereafter) Kill-switch drill (Patch §32). Acceptance: drill report at `download/governance/killswitch-drill-<date>.md` demonstrates all four layers:
  - Layer 0: write-access halt completes within 60 seconds.
  - Layer 1: a Council attempt to sign without Sheriff tender fails (HSM refuses).
  - Layer 2: a test artifact is held for 30 minutes; Sheriff quash succeeds.
  - Layer 3: a revocation certificate is signed and published; a downstream test system refuses the revoked artifact.
  All four must succeed in the drill. Any failure is patched before the next release.
L10. (H, T0+365d) Open question: should the Council's scope be narrowed from the default in §18.3? Acceptance: decision logged in COMPASS.md; if narrowed, the narrowed scope is appended to the Transition-Enablement Instrument.
L11. (H/Sheriff, T0+90d and quarterly thereafter) Maintain the Offline Anchor Log (Patch §30). Acceptance: OAL exists on hardware no agent can write to; OAL contains all §19.6.1 critical-event entries since T0; first quarterly reconciliation report published and clean.

Open questions for the maintainer (Part III):
1. Do you accept the INTERPRETIVE reading (§16.3) that this transition is a governance-structure amendment, not a strategic pivot, and therefore does not breach the original Anti-pivot Contract?
2. The proactive Sheriff elements in §18.5 (Sheriff-required actions, standing-audit jurisdiction, quarterly deep-dives, multilayer kill-switch) go beyond your stated reactive Sheriff. Do you accept these, or should the Sheriff be strictly reactive with the kill-switch as the only proactive defense?
3. What is the Sheriff's term? Indefinite until you designate a successor, or a fixed term with mandatory renewal?
4. The constitutional floor in §20.3 (§0, modern-defaults forbidden list, Evidence Rule, kill-switch decision, successor designation, OAL writes, TLO credential grants) is non-amendable even by the Sheriff. Do you accept this floor, or should the Sheriff have authority to amend any of these (with the obvious exception of §0 itself)?
5. The kill-switch (§21) is at Sheriff discretion. Should there be automatic triggers (e.g., Material Divergence count > N triggers automatic kill-switch), or should invocation always require the Sheriff? Patch §31's post-transition probation is a partial automatic trigger.
6. The Council's 2/3 majority threshold for Law amendment proposals (§19.4) is a check on Council drift. Is 2/3 appropriate, or should it be higher (3/4) for Law amendments that touch security or release gates?
7. The shadow-run period (§17.4) requires 0 Material Divergences in the final 60 days across ≥ 2 configurations. Is this too strict? An alternative: ≤ 1 Material Divergence in the final 60 days, with mandatory Law codification triggered by it.
8. What happens if the Council and the Sheriff reach an irreconcilable disagreement about whether to invoke the kill-switch? The plan currently gives the Sheriff sole kill-switch authority; should the Council have a counter-kill-switch procedure (e.g., supermajority vote to require the Sheriff to consider invocation)?
9. Does Part III Q3 (Sheriff term) interact with Part I Q6 (Foundation handoff at Phase 6 exit)? Should the Foundation handoff replace the Sheriff-successor mechanism, or run alongside it?

# PART IV — SYNCHRONIZATION LAYER

## 25. GLOSSARY (Shared Across All Parts; v1.2)

- §0 / SPEC-northstar §0 — the project objective: "minimize avoidable platform energy subject to a per-workload-class responsiveness floor." Immutable. Operationally defined via Patch §33 baseline.
- Anti-pivot Contract — forbids redefining §0 or proposing strategic pivots. Part III amends it to permit governance-structure changes (§16.3); the §0 and strategic-pivot prohibitions remain in force.
- Baseline (Patch §33) — the reference-configuration energy measurement against which "avoidable" energy delta is computed.
- Builder — role that proposes work. Pre-transition: human or agent. Post-transition: Councilmember (Part III §18.1).
- Capability surplus — the state in which AI capability exceeds project demand; the binding constraint becomes governance, not capability (Part II §13.1, 2033–2036 horizon).
- Coalition — agent topology: 3–5 agents from different providers producing independent recommendations; consensus + dissent report; Human/Sheriff tie-break. Part II §12.5. The Council (Part III §18) is a standing coalition.
- COMPASS.md — live state of the plan. Section headers are slots; the maintainer may rewrite in place. Disagreements with this document are resolved in favor of COMPASS.md, with the divergence logged.
- Compositor health-check (Patch §48) — optid runs a 60-second check verifying the compositor responds to D-Bus idle-notification queries; failure > 120s triggers PSI-only mode.
- Constitutional floor — the set of items non-amendable by anyone, including the Sheriff: §0, modern-defaults invariant (forbidden-as-default list), Evidence Rule, kill-switch decision, successor-Sheriff designation, OAL writes, TLO credential grants. Part III §20.3.
- Contract-preservation test (Patch §43) — before any sched_ext → EEVDF fallback, the Verifier measures whether the per-class responsiveness floor remains enforceable under EEVDF for each of the 5 workload classes; classes that fail are flagged "degraded contract — sched_ext required for full guarantee."
- Council — the standing coalition of agents that assumes operational authority post-transition. Part III §18.
- Critic — adversarial-review role. Pre-transition: agent (Part II §12.2). Post-transition: Councilmember.
- Depth-enabler — a kernel-level power-management feature (PCIe ASPM, SATA ALPM, runtime PM, adaptive backlight) that delivers measurable energy savings. Part I §3f.
- Evidence Rule — no claim without a literal command transcript. Immutable. Applies to all claims in all Parts, including Council Laws (Part III §19.2).
- Guideline — non-binding advisory rule. The Council may deviate with recorded justification. Part III §19.1.
- Human — the maintainer pre-transition. Signs off on all work under Parts I and II.
- Kill-switch — Sheriff's proactive defense. Four layers (Patch §32): Layer 0 write-access halt (60s SLA), Layer 1 two-party signing (continuous), Layer 2 30-min publication delay (continuous), Layer 3 revocation certificates (invoked). Part III §21.1.
- Law — binding rule on the Council. Council actions that violate a Law are void and rolled back. Part III §19.1.
- Learning period — T0 → T0+3y. AI agents operate under Parts I/II governance; Council observes only. Part III §17.1.
- Material Divergence — a divergence between Council action and the action a Human would have taken, that would have caused a release to fail, a security regression, or a §0 breach. Part III §17.4.
- MDA (Modern-Defaults Allowlist) (Patch §37) — the positive specification of permitted core subsystems. The forbidden-as-default list is part of the constitutional floor; the permitted list may evolve by Sheriff amendment. The Council cannot amend the MDA.
- Modern-defaults-only invariant — no X11, PulseAudio, iptables, cgroup v1, SysV init, classic BPF as defaults. Immutable in its forbidden-as-default list. Defined via the MDA (Patch §37).
- OAL (Offline Anchor Log) (Patch §30) — signed, append-only, hardware-token-backed log of critical governance events, written by the Sheriff (or pre-transition Human) on hardware no agent can reach. The cryptographic root of trust for governance.
- optid — the single Rust daemon owning all runtime power/performance optimization. Part I §3b.
- optid-debug (Patch §46) — read-only diagnostic mode showing every knob optid manages, current values, active class, active contract, and last-decision reasoning trace. The user cannot write sysfs in this mode. Visibility, not bypass.
- optid-thermal-override (Patch §46) — time-limited override (default 30 min, configurable up to 4 hours) allowing the user to force a power cap or CPU governor for thermal emergencies. Mandatory OAL-logged reason field. Pauses optid; does not bypass the invariant.
- PM QoS — kernel power-management quality-of-service interface. Part I §1, §3a.
- Precedent ledger — append-only, signed canonical record of project history from which Laws are derived. Part III §19.6.
- Probation (Patch §31) — the 12 months after the Transition Event during which any would-be-Material-Divergence regression triggers an automatic kill-switch review. Part III §17.6.
- PSI — Pressure Stall Information. Kernel metric used by optid to detect workload-class pressure. Part I §1.
- PSI-only mode (Patch §48) — degraded operation when the compositor is unresponsive; workload-class detection falls back to PSI signals alone; user notified; classes requiring compositor input degrade.
- Ratification — the maintainer's explicit, dated, signed acceptance of Part III. Required before any element of Part III takes effect.
- Release labels (Patch §45) — tiered: dev (single-builder, not for allowlist/transcript), candidate (2-builder hash match, shadow-run eligible), stable (2-builder hash match + Sheriff signoff + transcript; only label eligible for allowlist promotion, OEM pre-install, end-user install).
- Sheriff — the Human role post-transition. Has authority to amend Laws, resolve stalemates, co-sign Sheriff-required actions, invoke the kill-switch, appoint Technical Advisor, grant TLO credentials, maintain OAL. Part III §18.4.
- Sheriff-required action — a high-stakes action requiring Sheriff co-signature even post-transition (signing-key rotation, release-signing key use, root-key changes, baseline re-baselining, new editions, OEM contracts, Law amendments, MDA amendments). Part III §18.3.
- Stabilization period — T0+3y → T0+6y. Sheriff retains fast-rollback authority. The first 12 months are also the Post-Transition Probation. Part III §17.1.
- Steward — Councilmember role that manages inter-Council disputes and escalates stalemates. Part III §18.1.
- T0 — the date v1.0 stable production release ships (Part I §5 Phase 1 exit). Anchors the Part III timeline.
- TA (Technical Advisor) (Patch §39) — a human appointed by the Sheriff to assist with technical evaluation. Not a Councilmember; no signoff authority; advisory only.
- Telemetry tiers (Patch §47) — Tier 1 local-only (always on); Tier 2 aggregated opt-in k≥1000; Tier 3 per-class opt-in k≥100; Tier 4 raw opt-in k≥10 Sheriff-approved per stream.
- TLO (Trusted Lab Operator) (Patch §34) — a community member granted a hardware-validation credential by the Sheriff. TLO co-signature attests that a benchmark transcript was physically produced on real hardware.
- Transition Event — T0+3y+30d (or upon Sheriff ratification). Council assumes operational authority. Part III §17.1.
- Transition-Enablement Instrument — the dated, signed document that activates Part III. Part III §24 L1.
- Verifier — role that verifies evidence and reproduces transcripts. Pre-transition: agent. Post-transition: Councilmember.
- Workload class — one of idle / light / interactive / latency-critical / throughput. Part I §1.

## 26. UNIFIED RISK REGISTER (v1.2)

All risks from Part I §9, Part II §14, Part III §23, in a single view, with cross-references. Sorted by L×I tier (Critical → High → Medium → Low), then by ID. New in v1.1: T11 (compromised Council distributes signed malicious release; mitigated by Patch §32). New in v1.2: 11 (AI licensing/copywright exposure, mitigated by Patch §44 D1.11), 12 (compositor single-point-of-failure, mitigated by Patch §48), 13 (privacy-utility deadlock, mitigated by Patch §47 tiered telemetry), 14 (power-user UX hostility, mitigated by Patch §46 optid-debug + thermal-override), 15 (reproducibility-vs-velocity, mitigated by Patch §45 tiered release labels), 16 (sched_ext API stability + contract preservation on fallback, mitigated by Patch §43).

### Critical tier (L×I = Critical)
| ID | Risk | Source | Owner (pre→post) | Early-warning signal |
|----|------|--------|------------------|---------------------|
| 1 | Single-maintainer bus factor halts development | Part I §9 | Human → Sheriff (+TA) | No commits for ≥ 30 days |
| 2 | Benchmark publication cannot substantiate energy claims | Part I §9 | Verifier | Any release PR with power claim but no transcript |
| 7 (Part I) / A1 | Verifier accepts fabricated evidence | Part I §9 + Part II §14 | Verifier/Human → Verifier/Sheriff | Any Verifier-passed artifact that fails spot-check; OAL discrepancy |
| 11 (v1.2) | AI-generated code resembles copyleft corpus; licensing exposure | Patch §44 | Verifier/Human → Verifier/Sheriff | `license-check` flags AI-generated PR with significant similarity to GPL/AGPL corpus |
| A6 | Governance erosion — Human/Sheriff rubber-stamps | Part II §14 | Human → Sheriff | Human-review-time-per-PR < 5 min for ≥ 4 weeks; missing deep-dive reports |
| A7 | Compromised agent writes to repo | Part II §14 | Human → Sheriff | Any agent artifact committed without PR |
| T1 | Council codifies biases as Laws | Part III §23 | — → Sheriff/Historian | Sheriff rejection rate > 30% in any quarter |
| T2 | Sheriff lacks depth to evaluate Council | Part III §23 | — → Sheriff (+TA) | Sheriff defers on ≥ 50% of stalemates |
| T5 | Kill-switch drill failure | Part III §23 | — → Sheriff | Any Layer 0–3 drill failure |
| T9 | Council games the precedent ledger | Part III §23 | — → External Auditor | Ledger/history/OAL discrepancy |
| T10 | Sheriff amends Laws to breach §0/modern-defaults | Part III §23 | — → Council | Sheriff amendment touching floor |
| T11 | Compromised Council distributes signed malicious release (v1.1) | Part III §23 | — → Sheriff | Layer 1 single-party tender attempt; Layer 2 quash; revocation published |

### High tier (L×I = High)
| ID | Risk | Source | Owner (pre→post) | Early-warning signal |
|----|------|--------|------------------|---------------------|
| 3 | sched_ext API churn | Part I §9 | Builder | sched_ext breaking commits in kernel; 36-month kill-criterion approaching; API-stability trigger per Patch §43 |
| 4 | Depth-enablers deliver no measurable delta | Part I §9 | Human → Sheriff | Benchmark deltas < 5% |
| 5 | OEM demands breach governance | Part I §9 | Human → Sheriff | OEM NDA requiring proprietary code |
| 10 (Part I) | Hardware OEM lockdown | Part I §9 | Human → Sheriff | OEM removes user-controlled SB enrollment |
| 12 (v1.2) | Compositor single-point-of-failure collapses optid control loop silently | Patch §48 | Human → Sheriff | Compositor health-check fails > 120s; primary fails to start ≥ 3 consecutive boots |
| 16 (v1.2) | sched_ext fallback to EEVDF silently violates per-class contracts | Patch §43 | Verifier | Contract-preservation test fails on any class; release notes missing "degraded contract" flag |
| A2 | Model monoculture | Part II §14 | Human → Sheriff | Provider share > 70% |
| A3 | Prompt injection via untrusted content | Part II §14 | Builder | Untrusted-content-derived agent write |
| A4 | Agent capability drift after model upgrade | Part II §14 | Verifier | Regression-suite pass rate drops ≥ 5% |
| A8 | Trust miscalibration | Part II §14 | Human → Sheriff | FP or FN rate > 10% |
| A9 | Model-provider deprecation | Part II §14 | Human → Sheriff | Provider deprecation/ToS change |
| A10 | Verifier gaming | Part II §14 | Verifier/Human → Verifier/Sheriff | Audit finds predicate-satisfaction w/o correctness |
| T3 | Material Divergence in shadow run / probation | Part III §23 | — → Council/Steward | Any MD in final 60 days of shadow run; any probation-triggered regression |
| T4 | Council acts outside ratified Law set | Part III §23 | — → Verifier | Any un-Lawed Council action |

### Medium tier (L×I = Medium)
| ID | Risk | Source | Owner (pre→post) | Early-warning signal |
|----|------|--------|------------------|---------------------|
| 6 (Part I) | Reproducible-build drift | Part I §9 | Verifier | Hash mismatch on CI |
| 8 (Part I) | Desktop stack locks UX direction | Part I §9 | Human → Sheriff | Compositor upstream instability |
| 13 (v1.2) | Privacy-utility deadlock: telemetry too coarse for Council to refine Laws | Patch §47 | Human → Sheriff | Council Law-refinement rate drops; Sheriff approves < 2 Tier 3/4 streams per quarter |
| 14 (v1.2) | Power-user UX hostility; tinkerers locked out | Patch §46 | Human → Sheriff | User-study sysfs-reaching rate > 10% on sessions where optid-debug was not advertised |
| 15 (v1.2) | Reproducibility-vs-velocity: 2-builder gate discourages frequent test releases | Patch §45 | Verifier | Dev release count drops below 1/week; CI queue backs up |
| A5 | Cost of swarms outpaces value | Part II §14 | Human → Sheriff | Weekly agent cost > 20% of compute budget |
| T6 | Council stalemate gridlock | Part III §23 | — → Steward/Sheriff | Stalemate backlog > 5 |
| T7 | Law corpus legalism | Part III §23 | — → Historian | > 200 active Laws |
| T8 | Positive-precedent Laws codify luck | Part III §23 | — → Critic | Positive-precedent Law with < 3 instances or expired validity horizon |

### Low tier (L×I = Low)
| ID | Risk | Source | Owner (pre→post) | Early-warning signal |
|----|------|--------|------------------|---------------------|
| 9 (Part I) | Regulatory change restricts AI-assisted dev | Part I §9 | Human → Sheriff | AI-governance legislation |

## 27. UNIFIED BRANCH MAP (v1.1)

All branches from Part I §6, Part II §13.2, Part III §22, with axes and cross-references.

| Branch | Axis | Source | Pre/post transition | Key cross-ref |
|--------|------|--------|---------------------|---------------|
| A | sched_ext upstreaming (with 36-mo kill-criterion) | Part I §6 | Both | Part II §13.2 Branch-H; Patch §38 |
| B | Kernel PM QoS evolution | Part I §6 | Both | — |
| C | ARM64 & RISC-V curve | Part I §6 | Both | — |
| D | AI-runtime-governance adoption | Part I §6 | Pre | Part III §22 Branch-O (post form) |
| E | Desktop-Linux consolidation | Part I §6 | Both | — |
| F | Hardware OEM openness | Part I §6 | Both | — |
| G | Regulatory environment | Part I §6 | Pre | Part III §22 Branch-O (post form) |
| H | AI capability acceleration | Part II §13.2 | Both | Part III §22 Branch-L |
| I | Model-provider landscape | Part II §13.2 | Both | Part III §17.5 KC3 |
| J | Agent-governance regulation | Part II §13.2 | Pre | Part III §22 Branch-O |
| K | Agent security & prompt injection | Part II §13.2 | Both | Part III §21.2 kill-switch criteria |
| L | AI capability surplus (transition view) | Part III §22 | Post | Part II §13.1 forecast |
| M | Council capture | Part III §22 | Post | Part II §13.2 Branches I, K |
| N | Sheriff departure | Part III §22 | Post | Part I §9 Risk 1 |
| O | Legal/regulatory intervention (transition view) | Part III §22 | Post | Part I §6 Branches D, G; Part II §13.2 Branch J |
| P | Law corpus quality | Part III §22 | Post | — |

## 28. MASTER TIMELINE (v1.2)

Single timeline aligning Part I phases, Part II capability forecast, and Part III transition gates. Dates are illustrative; the plan uses relative time (T0, Phase N) for binding commitments.

| Calendar (approx) | Phase / Milestone | Part I | Part II | Part III |
|-------------------|-------------------|--------|---------|----------|
| 2026–2027 | Pre-v1.0; Phase 1 in flight | §5 Phase 1 (D1.8 baseline, D1.9 MDA, D1.10 privacy, D1.11 AI authorship policy) | §13.1 (2026–2027 forecast) | — |
| 2027 (approx) | v1.0 stable (T0) | §5 Phase 1 exit | — | §17.1 T0; §24 L1 |
| 2027–2028 | Phase 2 | §5 Phase 2 (D2.6 onboarding) | §13.1 (2027–2029 forecast) | §17.1 T0→T0+1y; §17.3 T0+1y review (§24 L6) |
| 2028–2029 | Phase 3 begins; Council self-assessment | §5 Phase 3 | §13.1 (2027–2029) | §24 L7 (T0+2y) |
| 2029–2030 | Phase 3 continues; T0+2y review; T0+30m sched_ext kill-criterion checkpoint; API-stability trigger monitored per Patch §43 | §5 Phase 3 | §13.1 (2029–2031) | §17.3 T0+2y review; Patch §38 checkpoint; Patch §43 quarterly API audit |
| 2030 (approx) | T0+3y: Council Codification Event | §5 Phase 3 (inside) | — | §17.1 T0+3y; §17.4 exit criteria |
| 2030 (approx) | Transition Event | §5 Phase 3 | — | §17.1 T0+3y+30d |
| 2030 (approx) | T0+36m: Patch §38 sched_ext hard kill-criterion (or T0+24m-from-first-break if Patch §43 API-stability trigger fires earlier) | §5 Phase 3 / Phase 3.5 if triggered | — | — |
| 2030–2032 | Phase 4 (stabilization + probation) | §5 Phase 4 | §13.1 (2029–2031) | §17.1 T0+3y→T0+6y; §17.6 probation |
| 2032–2033 | Phase 5 | §5 Phase 5 | §13.1 (2031–2033) | §17.1 post-stabilization |
| 2033 (approx) | T0+6y: Sheriff authority narrows | — | — | §17.1 T0+6y |
| 2033–2036 | Phase 6 | §5 Phase 6 | §13.1 (2033–2036 capability surplus) | §22 Branch-L upside window |
| 2036+ | Post-plan | §8 dominance conditions | §13.1 surplus state | Branch-P upside (Law corpus converges) |

## 29. RATIFICATION STATUS & ACTIVE-GOVERNANCE POINTER

### 29.1 Ratification status (as of 2026-06-20, v1.2)
- Part I (§§1–10): IN FORCE. No ratification required. Patches §33, §35, §37, §38, §40, §41, §43, §44, §45, §46, §47, §48 applied.
- Part II (§§11–15): IN FORCE. No ratification required. Operates under Part I governance. Patches §30 (provenance), §34 (TLO/simulated benchmarks) applied.
- Part III (§§16–24): RATIFICATION-PENDING. No element takes effect until the maintainer signs the Transition-Enablement Instrument (§24 L1) at T0. Patches §30 (OAL), §31 (shadow-run diversity + probation), §32 (multilayer kill-switch), §34 (TLO co-signature), §36 (positive-precedent sunset), §39 (Sheriff continuity) applied.
- Part IV (§§25–29): IN FORCE as a synchronization layer; updates automatically when any Part is amended.
- Part V (§§29.4–29.7): IN FORCE as revision history.

### 29.2 Active-governance pointer
At any point in time, exactly one of two governance regimes is active:
- REGIME-PRE (Parts I + II): Builder/Verifier/Human. Human is sole signoff. Active from project inception until T0+3y+30d (or until Part III ratification, whichever is later). If Part III is never ratified, this regime persists indefinitely.
- REGIME-POST (Parts I + II + III, with Part III as the governing authority for the surfaces it covers): AI Council + Sheriff. Council has jurisdiction per §18.3; Sheriff has jurisdiction per §18.4. Active from the Transition Event (§17.1) until the kill-switch is invoked or the transition is otherwise dissolved.

COMPASS.md's first entry each week must state the active regime explicitly. Disagreements about which regime is active are resolved by the dated Transition-Enablement Instrument (or its absence).

### 29.3 Edit protocol for this consolidated document
- Each Part is independently editable in place. Section headers are slots.
- An edit to one Part that affects another Part MUST be propagated to the other Part in the same edit session, with a cross-reference note added to both.
- An edit that changes the active-governance regime (§29.2) requires a dated COMPASS.md entry and, if the change activates or deactivates Part III, a signed instrument.
- Part IV (§§25–29) is regenerated whenever any Part is amended in a way that affects the glossary, risk register, branch map, or timeline. The regeneration is a Verifier responsibility (pre-transition) or a Historian Councilmember responsibility (post-transition).
- Part V (§29.4–§29.7) is appended to on every revision.

# PART V — REVISION HISTORY

## 29.4 Revision history

| Version | Date | Changes | Author |
|---------|------|---------|--------|
| v1 | 2026-06-20 | Initial consolidated plan (Parts I–IV) | main (Super Z) |
| v1.1 | 2026-06-20 | Revision Patch v1 applied: §30 OAL (Critical 1), §31 shadow-run diversity + probation (Critical 2), §32 multilayer kill-switch (Critical 3), §33 baseline spec (High 4), §34 TLO + simulated-benchmark prohibition (High 5), §35 P2 KPI reframe (High 6), §36 positive-precedent sunset (High 7), §37 Modern-Defaults Allowlist (Medium 8), §38 sched_ext 36-month kill-criterion (Medium 9), §39 Sheriff continuity + Technical Advisor (Medium 10), §40 onboarding document (Obs A), §41 privacy analysis (Obs B). §42 Deferred Autonomy offered but not applied; T0+3y transition target retained. New risk T11 added (compromised Council distributes signed malicious release; mitigated by Patch §32). New KC6 added (OAL discrepancy). | main (Super Z) |
| v1.2 | 2026-06-20 | Revision Patch v2 applied: §43 sched_ext API-stability trigger + contract-preservation test (Issue 1); §44 D1.11 AI Authorship & Licensing Policy (Issue 2); §45 tiered release labels dev/candidate/stable (Issue 3); §46 optid-debug + optid-thermal-override (Issue 4); §47 tiered telemetry Tiers 1-4 with k-anonymity per tier (Issue 5); §48 compositor health-check + PSI-only fallback + secondary-compositor auto-fallback + allowlist compositor_compat field (Issue 6). Six new risks added (11-16). §42 Deferred Autonomy still offered, not applied; T0+3y transition target retained. | main (Super Z) |

## 29.5 Open questions closed by Revision Patch v1

- Part I Q1 (INTERPRETIVE note on baseline): closed by Patch §33. The counterfactual baseline is operationalized as the reference-configuration delta.
- Part I Q7 (simulated benchmarks as provisional evidence): closed by Patch §34.3. Simulated benchmarks are not admissible for allowlist promotion; they may be used for development and regression testing only.
- Part II Q3 (agent provenance visibility): closed by Patch §30 + Part III §21.4. Post-transition provenance is public. Pre-transition the maintainer decides.
- Part II Q7 (agent-authored transcript marking): closed by Patch §30. Agent-authored transcripts are marked via the provenance chain; signing events are OAL-anchored.

## 29.5b Open questions closed by Revision Patch v2

- Issue 1 (sched_ext stability): closed by Patch §43. API-stability trigger accelerates kill-criterion; contract-preservation test gates every fallback.
- Issue 2 (AI authorship licensing): closed by Patch §44 D1.11. Commit metadata, training-data attestation, clean-room presumption, Law corpus under CC0, CLA amendment.
- Issue 3 (reproducibility velocity): closed by Patch §45. Tiered release labels (dev/candidate/stable) with proportional reproducibility requirements.
- Issue 4 (power-user UX): closed by Patch §46. optid-debug (visibility, not bypass) + optid-thermal-override (time-limited pause, OAL-logged). Invariant preserved.
- Issue 5 (privacy-utility): closed by Patch §47. Tiered telemetry Tiers 1-4 with k-anonymity per tier (k≥1000, k≥100, k≥10) plus Tier 1 always-on local-only full-fidelity.
- Issue 6 (compositor fragility): closed by Patch §48. 60s health-check, PSI-only fallback, TTY-only boot fallback, secondary-compositor auto-fallback on 3 failed boots, allowlist compositor_compat field.

## 29.6 Open questions remaining (19)

Across Parts I (Q2, Q3, Q4, Q5, Q6), II (Q1, Q2, Q4, Q5, Q6), III (Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9). The highest-priority remaining:
- Part III Q1 (INTERPRETIVE reading of Anti-pivot Contract): does the maintainer accept that the transition is a governance-structure amendment, not a strategic pivot?
- Part III Q2 (Sheriff scope — proactive vs reactive): the patches assume the proactive Sheriff scope (Sheriff-required actions, standing audit, technical deep-dives, multilayer kill-switch) is accepted. If the maintainer wants a strictly reactive Sheriff, several patches need rework.
- Part I Q4 (second-maintainer appetite): Patch §39's Technical Advisor role partially addresses this but does not replace the need for a co-maintainer decision.
- Patch §42 decision (Deferred Autonomy): should the learning period extend to T0+4y or T0+5y, given the strengthened initial constraints of Patches §30–§34?

## 29.7 Patch §42 decision required

The maintainer must decide whether to apply Patch §42 (Deferred Autonomy, extending the learning period to T0+4y or T0+5y). Recommended: apply if the maintainer wants additional margin beyond the strengthened initial constraints of Patches §30–§34; do not apply if the T0+3y target is to be preserved. In v1.1, Patch §42 is NOT applied; the T0+3y target is retained.

---

End of Strategic Plan v1.1 (Consolidated). Document saved at `/home/z/my-project/download/rush-linux-strategic-plan-consolidated-v1.1.md`.
