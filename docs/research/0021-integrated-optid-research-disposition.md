# 0021 — Integrated optid research disposition

**Status:** validated synthesis; non-binding until an accepted decision or active
plan adopts a recommendation

**Date:** 2026-07-28

**Baseline:** `c1ee086a0fdb7866d9990332ae7e19589d6fb6fa`

**Scope:** integration of the staged research mission covering capability sealing,
benchmark methodology, rollback contracts, observability, latency, thermal control,
resource context, remaining platform primitives, display/ALS feasibility, and
reproducible-image/Secure Boot work.

## 1. Purpose and authority

This paper is the current disposition map for the broad research set. It exists to
stop future work from selecting an attractive sentence from an older WIP paper while
ignoring later repository evidence or a higher-authority decision.

It does **not**:

- modify the Northstar;
- ratify a proposed ADR;
- complete a package;
- promote hardware;
- authorize a privileged write;
- make a release, performance, reproducibility, or Secure Boot claim.

Authority remains, in order, the maintainer's latest direction, `AGENTS.md`, the
Northstar, accepted decisions, current strategy, validated research, unfinished
research, active plans, committed evidence, and code.

When this synthesis conflicts with an accepted decision, the accepted decision wins.
When it conflicts with an unfinished research paper, use this synthesis as the newer
research disposition and amend the older paper before implementation.

## 2. Repository truth that remains unchanged

At the baseline commit:

- `active_general = "F2"`;
- `active_safety = "D0"`;
- `ready_parallel = ["R1", "R2", "R3"]`;
- F1, F2, F3, F4, D0, and T1 remain incomplete in the package ledger;
- dependencies unlock only from `completed`;
- architecture D2 remains the accepted fail-passive safety direction;
- `sched_ext` remains `spec_blocked`;
- all seeded hardware allowlist entries remain unpromoted;
- no research result in this paper changes a package status.

## 3. Integrated dispositions

| Area | Final research disposition | Existing owner/package |
|---|---|---|
| Capability sealing | Continue architecture D2. Prove pre-opened descriptor behavior, Landlock inheritance/irreversibility, removed-object behavior, child/exec behavior, no-new-privileges, exit-75 recovery ordering, and cold restart before S4D. | D0, then S1D–S5D |
| Benchmark methodology | Keep `mixed-load-001`, but make it genuinely concurrent, paired, provenance-locked, and statistically explicit. Separate same-Rush causal control from mainstream product comparison. | ADR 0011, rushbench, benchmark/evidence docs |
| Rollback and stabilization | Freeze a versioned per-lever contract. Exact rollback is allowed only while the current value still equals optid's last confirmed value; external drift normally relinquishes ownership. Stabilization must be named separately. | S1D, then S2D–S5D |
| Runtime observability | Put low-rate control observability inside optid and campaign instrumentation inside rushbench. Quarantine and salvage `rush_telemetry`; do not re-include it wholesale. | R1, F3, O1, I1 |
| Latency contracts | Separate workload constraints, advertised state latency, measured wake latency, and policy timers. Compose the complete wake path and deny depth when any required component or live-use predicate is unknown. | C1, O1, D1, package D2, D4 |
| Thermal/powercap | Repair T1 sensor identity and role semantics. Compare a bounded direct follower with a temperature/headroom controller; do not drive measured power toward a power budget. T3 v1 is an exact Intel long-term package constraint only. | T1, T2, T3, S1D |
| Resource pull and session context | Resource pull is primary machine-demand evidence. Focus, GameMode, fullscreen, and media signals are authenticated expiring latency-floor context, not one global importance rank. | O2, X1, X2, C1 |
| PPD/GameMode compatibility | Current shims are not production-compatible. GameMode belongs on the user bus with caller/target lifetime validation. PPD must implement the current upstream interface and hold semantics. | X2 and a separate PPD compatibility change |
| Remaining platform primitives | Accept HFI, devfreq, DPTF bounds/hints, and S0ix as bounded observations where supported. Reject or defer generic memory-controller writes, LTR overrides, adaptive IRQ steering, LPMD coexistence, generic ARM MPMM control, and broad idle injection. | R2, O1, I1, T2/T3 |
| `sched_ext` | Keep EEVDF as production scheduler. No work package until WP-B1 physical evidence and explicit authority change. First experiment, if ever authorized, uses one pinned scheduler and no automatic relaunch or dynamic class switching. | Remains excluded/spec-blocked |
| Render scaling | Gamescope is optional application-scoped tooling. Desktop-wide transparent scaling remains a compositor-core experiment and has no generic package. | R3; optional packaging only |
| Ambient light | Hardware ALS through `iio-sensor-proxy` is feasible in the user session. Camera-based sensing and a root ALS daemon are rejected. | X2/D3; desktop/session owner |
| Reproducible images | Use mkosi `Snapshot=`, a complete content lock, pinned tools, offline builds, deterministic image inputs, two independent unsigned builds, provenance, and SBOMs. | New release-engineering work, not optid ledger |
| Secure Boot | First owner-controlled production path is direct UEFI `db` enrollment of a Rush release certificate. Shim/MOK remains blocked until Rush has a reviewed and Microsoft-signed Shim. | New release-engineering/security work |

## 4. Cross-cutting contracts

### 4.1 Unknown is not a value

Missing, stale, malformed, permission-denied, unsupported, and identity-changed states
must remain distinguishable. They do not become zero, `on`, `balanced`, an arbitrary
fallback, or permission to actuate.

### 4.2 Identity precedes paths

A sysfs path or path hash is not stable hardware identity. Every dynamic target and
recovery record needs canonical identity, driver/firmware scope where relevant, and a
topology generation. Path reuse with identity mismatch forbids a write.

### 4.3 Readback is semantic

A successful `write()` and textual equality are not always proof. Verification can
also require device health, runtime state, complete composite-member state, link
health, actual brightness, accepted powercap quantization, or complete multi-function
group state.

### 4.4 Composite operations share ownership

Runtime-PM control and delay, VM dirty bytes/ratios, power plus time window, dGPU
function groups, and multi-property systemd changes are one transaction. Partial
external drift relinquishes the whole transaction unless a contract explicitly says
otherwise.

### 4.5 Evidence promotes exact scope

Mocks prove parsing, ordering, invariants, and failure logic. They do not prove a
backlight floor, autosuspend delay, link power state, PL1 floor, dGPU depth, benchmark
improvement, or firmware-specific safety. Physical evidence promotes only the
recorded hardware/firmware/kernel envelope.

### 4.6 Observation and mutation have different owners

Kernel and firmware policy outcomes should normally be observed rather than duplicated.
Session-owned signals and preferences remain in the session. Benchmark tracing remains
campaign-scoped. Release signing remains outside the ordinary image builder.

## 5. Required corrections to older research and proposed decisions

The following sources remain useful but contain stale or contradicted statements that
must be amended before implementation:

| Source | Required correction |
|---|---|
| `docs/decisions/0011-benchmark-methodology.md` | Name the two distinct baselines, paired unit of analysis, confidence procedure, invalid-run rules, evidence schema, and owner-ratified margins. |
| `docs/decisions/0015-sched-ext-default-on.md` | Do not accept as written. It conflicts with the Northstar and lacks WP-B1 evidence. Reject or supersede only through maintainer action. |
| `docs/decisions/0017-rush-telemetry-fate.md` | Replace wholesale re-inclusion with quarantine-and-salvage. |
| `docs/research/0001-apple-power-stack-analysis.md` | Convert the broad inventory into per-primitive observe/map/defer/reject dispositions. |
| `docs/research/0004-telemetry-fidelity-rca-and-architecture.md` | Remove zero-cost/eBPF and vDSO overclaims; separate runtime O1 from benchmark instrumentation. |
| `docs/research/0005-focus-vs-resource-pull.md` | Remove the nonexistent current detector, global max-class model, trusted cgroup path, app-ID-only selection, and global multi-seat focus. |
| `docs/research/0007-display-panel-backlight-psr-vrr-dpms.md` | Correct user ownership, safety-floor versus brightness-ceiling semantics, and session/compositor boundaries. |
| `docs/research/0008-nvme-apst-pcie-aspm-sata-alpm.md` | Separate APST, ASPM, ALPM, and runtime D3; require complete wake-path latency evidence. |
| `docs/research/0009-runtime-pm-autosuspend-policy.md` | Replace generic delays and warning-only activity risks with class-specific live-use contracts and unknown-deny behavior. |
| `docs/research/0010-ppd-gamemode-dbus-shim.md` | Update to current PPD ABI/hold semantics and move GameMode to a per-user compatibility bridge. |
| `docs/research/0011-dgpu-runtime-pm-and-mux.md` | Separate D3hot, D3cold, driver-internal, and VRAM states; preserve no-MUX-actuation. |
| `docs/research/0012-dtpm-powercap-outer-loop.md` | Replace measured-power tracking, fixed indices, dimensionless gains, generic floors, and AMD Mobile write assumptions. |
| `docs/research/0013-thermal-fan-budget-coupling.md` | Correct Tctl/Tdie, sensor-role identity, fan-proxy, skin, and critical-threshold assumptions. |
| `docs/research/0014-sched-ext-selector-per-workload-class.md` | Remove default scheduler, dynamic class switching, stale ABI, optid child ownership, and automatic relaunch claims. |
| `docs/research/0016-mkosi-ala-snapshot-pinning.md` | Use `Snapshot=`, complete content/tool locks, networkless independent builds, and a security-triggered update process. |
| `docs/research/0017-uki-signing-secure-boot-enrollment.md` | Correct the Shim/MOK chain, SBAT scope, key roles, direct-db first path, and mutable-rootfs limitation. |
| `docs/research/0018-telemetry-runtime-state-observability.md` | Correct wakeup ABI status/names, cadence, descriptor lifetime, package-state claims, and freshness model. |
| `docs/research/0019-gpu-upscaling-resolution-scaling-als.md` | Separate app-scoped Gamescope from compositor-core scaling; record the webcam audit and reject camera ALS. |

## 6. Research-question register

| ID | Resolution |
|---|---|
| INT-001 | No new generic optid architecture is needed; architecture D2 and the active package plan remain the integration backbone. |
| INT-002 | Benchmark proof requires paired, exact-work, provenance-locked campaigns; five pairs are a minimum collection rule, not automatic proof. |
| INT-003 | S1D needs a shared per-lever schema and must disposition the omitted `SystemdSetProperty` mutation. |
| INT-004 | Runtime observability and benchmark telemetry are separate systems with separate owners. |
| INT-005 | Unknown latency or unknown live use denies depth actuation. |
| INT-006 | Thermal control targets temperature/headroom and emits a bounded PL1 ceiling; it does not force measured power toward a budget. |
| INT-007 | Resource pull, latency floors, and global user power preference remain separate policy dimensions. |
| INT-008 | Most broad platform primitives are observations or kernel-owned policies; none creates a new v1 actuator package. |
| INT-009 | `sched_ext` remains blocked until physical evidence and authority change. |
| INT-010 | Desktop-wide render scaling has no accepted portable product interface; hardware ALS is session-owned and camera-free. |
| INT-011 | Reproducible-image and Secure Boot work form a separate release-engineering track outside the optid package ledger. |

## 7. Immediate owner decisions versus deferred decisions

### Immediate for specification PRs

1. **S1D scope:** add a twelfth systemd/cgroup-property contract or keep that
   production-reachable mutation observe-only until a dedicated contract exists.
2. **Benchmark decision:** accept the paired two-baseline methodology and choose
   numerical non-regression/improvement margins before confirmatory physical runs.
3. **Telemetry decision:** supersede wholesale `rush_telemetry` re-inclusion with
   quarantine-and-salvage.
4. **Scheduler decision:** reject or supersede ADR 0015 while retaining the
   Northstar evidence gate.
5. **Secure Boot product path:** use direct UEFI `db` as the first supported
   owner-controlled path and leave Shim/MOK unsupported until a Rush-specific
   trusted Shim exists.

### Deferred until their package reaches implementation

- exact latency ceilings and guard margins;
- thermal targets, controller choice and gains;
- cgroup ranking thresholds;
- session-context lease periods and desktop support matrix;
- ALS calibration UX;
- exact release snapshot, signing service, certificate policy, and OEM matrix.

## 8. Stop rules

- Do not update `active_general`, `active_safety`, or package statuses from this paper.
- Do not expand an existing hardware actuator before D0 and the required S-lane
  dependencies are complete.
- Do not create a `sched_ext` implementation package.
- Do not package camera-based ALS.
- Do not claim image reproducibility until two clean unsigned artifacts match.
- Do not claim production Secure Boot until a tested trust path and recovery matrix exist.
- Do not mark this synthesis as an accepted decision; owner choices belong in
  separately ratified ADRs or accepted plans.
