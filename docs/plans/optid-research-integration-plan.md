# Optid Research Integration Plan

**Status:** draft execution plan

**Date:** 2026-07-28

**Baseline:** `c1ee086a0fdb7866d9990332ae7e19589d6fb6fa`

**Research synthesis:**
[`docs/research/0021-integrated-optid-research-disposition.md`](../research/0021-integrated-optid-research-disposition.md)

## 1. Goal

Convert the integrated research disposition into small, reviewable repository changes
without changing package truth, inventing hardware evidence, or making the research
mission itself a second implementation queue.

This plan preserves:

- F2 as the active general package;
- D0 as the active safety package;
- R1, R2, and R3 as parallel research work;
- architecture D2 as the accepted safety direction;
- EEVDF as the production scheduler;
- exact-hardware promotion gates;
- the rule that only a cold verifier may propose `completed`.

## 2. What this integration PR changes

The first PR is documentation-only and deliberately small:

1. add the integrated research disposition;
2. add this execution plan;
3. update the research index so future agents find the disposition before using an
   older WIP paper.

It does not:

- edit the Northstar;
- alter the current-work selector;
- change the package ledger;
- accept or reject an ADR on behalf of the maintainer;
- change production code or tests;
- generate evidence or signing keys.

## 3. Work lanes after this PR

### Lane A — Active general construction

#### A1. Complete F2 to candidate

Required result:

- add a production-surface integration test entering through the daemon or CLI;
- prove the injected kernel I/O seam is consumed through that production path;
- preserve current behavior;
- update only F2 in the package ledger;
- open a draft PR;
- cold verification remains separate.

#### A2. Refresh F1 cold verification

Required result:

- verify the current post-PR-337 runtime surface and real behavioral tests;
- commit a fresh receipt for the exact implementation commit;
- restore F1 to `completed` only through the independent verifier process.

#### A3. Finish F3

Required result:

- production-consumed versioned observation, decision, gate, action, readback,
  ownership/drift, restore, and recovery envelopes;
- shared reason and support-state vocabulary;
- optid, optctl, logs, and action outcomes consume the shared schema.

#### A4. Finish F4

Required result:

- complete desired-state reconciliation;
- target identity, original, last-confirmed, and ownership wired from production;
- restore on transitions rather than only shutdown;
- systemd/cgroup-property mutation either gains a complete contract or is disabled;
- shadow-versus-v1 parity gate.

### Lane B — Active safety construction

#### B1. Complete D0 proof

Required proof matrix:

- exact pre-opened sysfs descriptor after Landlock;
- no new hardware opens after sealing;
- inheritance across threads and child/exec;
- no-new-privileges and ABI-rights behavior;
- removed-object failure;
- dedicated exit code;
- `optid-recover` ordering before fresh process;
- feature-build CI;
- cold supported-kernel receipts.

Failure stops S4D, not unrelated observation or simulation work.

#### B2. S1D contract specification

One PR should add pure versioned contracts and fixtures only.

Required contract families:

- CPU DMA PM QoS owned request;
- per-device PM QoS shared request;
- CPU EPP policy;
- platform profile;
- runtime PM and autosuspend;
- SATA ALPM;
- per-link PCIe ASPM;
- backlight;
- VM sysctl composite transaction;
- powercap/PL1;
- dGPU runtime PM group;
- systemd/cgroup runtime properties, if owner-approved.

The PR must not migrate actuator writes.

#### B3. S2D–S5D

Separate PRs in dependency order:

1. persistent verified write-ahead transactions;
2. independent recovery, watchdog, and boot ordering;
3. sealed typed capability table;
4. domain circuit breakers and controlled canary re-entry.

### Lane C — Benchmark and evidence correction

#### C1. Benchmark decision PR

Amend ADR 0011 and the canonical workload/evidence documents together.

Required decisions:

- same-Rush causal control versus mainstream product comparator;
- paired full-cycle unit of analysis;
- genuinely concurrent `mixed-load-001` phases;
- phase-local PSI `total` deltas;
- whole-system energy versus package energy;
- minimum five valid pairs with predeclared extension;
- invalid-run preservation and typed causes;
- owner-ratified practical margins;
- generated machine-readable verdict.

No benchmark result or milestone claim belongs in this PR.

#### C2. Instrumentation implementation

Split into bounded PRs:

1. raw PSI and energy corrections;
2. campaign runner, balanced ordering, and provenance;
3. analysis/verdict generator;
4. host-runner migration to the canonical campaign;
5. independent physical campaigns.

### Lane D — R1, observability, events, and diagnostics

#### D1. R1 documentation correction

Amend research 0004 and 0018 and propose a replacement for ADR 0017.

Accepted direction:

- optid owns low-rate runtime observations;
- rushbench owns campaign instrumentation;
- E1 owns event delivery and scheduling;
- optctl consumes one canonical envelope;
- `rush_telemetry` remains excluded and is salvaged piecemeal;
- no steady-state BPF or remote telemetry requirement.

#### D2. O1 implementation

Implement:

- topology inventory separate from sampling;
- source adapters with freshness/support/error states;
- transaction-coupled readback;
- bounded source-specific cadence;
- current-state JSON and significant transition events;
- observer-effect accounting and calibration.

#### D3. E1 and I1

- E1 replaces fixed sleep with PSI, topology, power, config, and deadline events.
- I1 exposes the shared state through `optctl status`, `explain`, and bounded
  `doctor --capture`.

### Lane E — Contracts and depth domains

#### E1. C1

Add:

- `LatencyEstimateV1`;
- exact evidence fingerprint;
- strictest active ceiling;
- conservative complete wake-path composition;
- unknown/stale deny behavior;
- three-state live-use predicate;
- monotonicity tests.

#### E2. D1–D5

Each depth domain remains its own PR after its dependencies complete:

- D1 runtime PM;
- package D2 APST/ASPM/ALPM split;
- D3 backlight ownership and session bridge;
- D4 dGPU multi-function runtime PM, no MUX write;
- D5 correct memory ownership.

No allowlist promotion is bundled with construction.

### Lane F — Resource pull and session context

#### F1. O2

Implement cgroup-v2 identity, non-overlapping accounting frontier, counter deltas,
resource-pull vector, deterministic ranking, and observe-only production integration.

#### F2. X1

Freeze and implement:

- system-D-Bus bridge protocol;
- caller credentials and logind validation;
- polkit authorization;
- pidfd or PID/start-time target validation;
- server-side cgroup resolution;
- instance, sequence, boot, expiry, and disconnect rules;
- per-seat state.

#### F3. X2

One backend per PR:

- GNOME;
- KDE;
- first selected wlroots-family compositor;
- PipeWire media context;
- GameMode compatibility.

#### F4. PPD compatibility

Use a separate PR and current upstream ABI/hold semantics. Do not hide it inside X2.

### Lane G — Thermal and powercap

#### G1. T1 repair

Correct:

- thermal roles and full identity;
- alias-safe deduplication;
- no arbitrary hottest-sensor fallback;
- Tctl/Tdie distinction;
- source health and freshness;
- control versus emergency thresholds;
- state-transition hysteresis;
- fan stopped versus unavailable.

#### G2. T2 simulation

Compare:

- bounded direct follower;
- temperature/headroom PI with hard feed-forward ceiling.

Include dimensional gains, anti-windup, explicit `dt`, reset/freeze rules, asymmetric
slew, plant families, faults, and recorded trace replay.

#### G3. T3

Initial backend is only an exact named Intel long-term package constraint. Preserve
PL2 and time window, require a verified floor and emergency stabilizer, and promote
only exact hardware evidence.

### Lane H — R2 platform dispositions

One documentation PR should:

- amend research 0001;
- amend research 0014;
- propose rejection or supersession of ADR 0015;
- preserve the Northstar `sched_ext` block;
- record HFI/devfreq/DPTF/S0ix as bounded observation candidates;
- record the rejected/deferred primitives.

Optional read-only adapters then land individually. No R2 actuator package is created.

### Lane I — R3 display and ALS

One documentation PR should amend research 0007 and 0019.

Possible later work:

- optional Gamescope launch profiles, never automatic optid policy;
- one nested/wlroots research prototype if the maintainer wants it;
- hardware ALS through `iio-sensor-proxy` inside the session;
- one matched internal panel, calibration, explicit manual override;
- no camera path and no root ALS daemon.

### Lane J — Reproducible image and Secure Boot

This is outside the optid package ledger.

Create a separate release-engineering plan with bounded tasks:

#### IMG1 — Freeze inputs

- mkosi `Snapshot=`;
- repository DB/package/signature lock;
- Arch keyring provenance;
- Rush source and Cargo vendor bundle;
- pinned tools tree;
- deterministic seed and source epoch.

#### IMG2 — Hermetic unsigned image

- build Rust inside the pinned environment;
- `cargo --locked --offline`;
- networkless mkosi build;
- manifests, checksums, SBOM, provenance;
- two independent exact-byte builds;
- difference diagnosis.

#### SB1 — Development Secure Boot

- development-only certificate;
- OVMF custom variables;
- positive/negative/tamper VM matrix;
- embedded command-line and kernel-policy checks.

#### SB2 — Direct-DB release path

- HSM/signing service;
- release certificate and fingerprints;
- signing authorization after reproducibility PASS;
- versioned UKIs and known-good rollback;
- OEM enrollment, rotation, and recovery matrix.

#### SB3 — Optional Shim/MOK

No work starts until a maintainer decides to maintain a Rush Shim and pursue review and
Microsoft signing.

## 4. Pull-request map

| Order | PR | Scope | Ledger effect |
|---:|---|---|---|
| 0 | Integrated research disposition | This plan, synthesis, research index | None |
| 1 | F2 production-surface proof | Active general repair | F2 only, at most candidate |
| 2 | D0 proof completion | Active safety repair | D0 only, at most candidate |
| 3 | Benchmark methodology decision | ADR/workload/evidence docs | None |
| 4 | R1 telemetry corrections | Research/ADR disposition | R1 remains unchanged until verified |
| 5 | S1D pure contracts | Contract schemas and fixtures | S1D only, at most candidate |
| 6 | F3 production envelopes | Shared runtime truth | F3 only |
| 7 | F4 reconciliation | Complete desired state and handback | F4 only |
| 8 | O1 runtime observability | Read-only production path | O1 only |
| 9 | E1 event reactor | Event-driven reevaluation | E1 only |
| 10 | O2 resource pull | Observe-only cgroup path | O2 only |
| 11 | X1 secure bridge | Protocol/security boundary | X1 only |
| 12+ | X2 backends | One backend per PR | X2 only where applicable |
| later | C1, D1–D5 | One package per PR | Exactly one package each |
| later | T1 repair, T2, T3 | One package per PR | Exactly one package each |
| parallel | R2 and R3 document corrections | Research only | No false promotion |
| separate | IMG/SB release-engineering series | Image and trust chain | Outside optid ledger |

The table is an execution shape, not a command to serialize independent work. F2 and
D0 remain the active lanes; read-only research and specification work can continue in
parallel when it does not displace them.

## 5. Validation for the integration PR

Before this PR is ready for merge:

- verify links and document indexes;
- run documentation-sync validation;
- run repository checks appropriate to a docs-only change;
- confirm current-work selector and package ledger are byte-for-byte unchanged;
- confirm no ADR status became accepted/rejected/superseded;
- confirm no release or milestone file changed;
- confirm the branch contains no code, service, workflow, key, certificate, or binary.

## 6. Owner decision register

### Required before the corresponding specification can become binding

| Decision | Recommended default | Needed by |
|---|---|---|
| S1D includes systemd/cgroup property contract? | Yes; otherwise keep the mutation observe-only | S1D |
| Benchmark comparison and margins | Accept two-baseline paired model; ratify margins before physical campaign | ADR 0011 |
| `rush_telemetry` fate | Quarantine and salvage | R1/O1 |
| ADR 0015 | Reject or supersede; retain EEVDF and Northstar block | R2 |
| First Secure Boot production path | Direct UEFI `db`; no Shim/MOK claim | SB2 |

### Safe to defer

- exact latency ceilings;
- thermal targets and controller gains;
- resource-pull thresholds;
- compositor support order;
- ALS calibration UX;
- exact build snapshot and signing provider.

## 7. Final stop conditions

This integration is complete when:

- the synthesis and plan are discoverable from the research index;
- the draft PR contains no status promotion;
- the active work selector remains F2/D0;
- every research conclusion maps to a package or bounded release-engineering task;
- every unavoidable owner choice is named and deferred to the correct point;
- subsequent implementation can proceed without repeating the broad research mission.
