# SPEC-northstar.md — Rush Linux Canonical Objective Specification

**Version:** 1.0
**Status:** CANON. This document defines what `optid` optimizes for. Every other
doc, roadmap, ADR, and agent task is *derived* from it. On any conflict, this
file wins. Changing the objective requires a human edit to this file and a
version bump — not a PR comment, not an agent proposal.

---

## 0. The single objective

> `optid` **minimizes avoidable platform energy** subject to a **per-workload-class
> responsiveness floor**, by holding each controllable domain in the **deepest
> power state its active latency contract permits**.

That is the whole project in one sentence. There is exactly one quantity to
minimize (energy) and exactly one constraint that may stop it (the floor).
Everything below is mechanism in service of this line.

### Why the old framing failed

The prior tenets were "zero waste + high responsiveness + optimal hardware use +
max vertical integration." Three of those are not objectives:

- **Zero waste** and **high responsiveness** are the *same axis* pulled in
  opposite directions. Deep idle saves energy and costs wake latency. You cannot
  maximize both; you arbitrate between them per context.
- **Optimal hardware use** is a tautology — optimal toward *what* is the question
  this spec answers.
- **Max vertical integration** is a *method*, not a goal. Integration is pursued
  only as far as it reduces avoidable energy without breaching a floor.

With no objective function there was nothing to orchestrate toward, so every
agent grabbed a different lever and called it strategy. This spec removes that
freedom: a lever is legitimate only if it serves the line in §0.

---

## 1. Definitions

**Avoidable energy.** Joules spent where no useful work is being done, or spent
holding a device readier than the active contract requires. This is what we
minimize. Measured as discharge rate (battery) or wall power (AC), plus device
and package residency, wakeups/sec, and suspend drain.

**Workload class.** The currently dominant kind of work, which selects the active
contract. Initial classes: `idle`, `light` (typing, reading), `interactive`
(scroll, UI), `latency-critical` (audio/DAW, video call, game), `throughput`
(build, batch). Detected from PSI, foreground app, fullscreen/audio/video state,
and explicit pins.

**Latency contract.** The responsiveness floor for the active class, expressed as
concrete budgets: CPU wakeup latency, per-device resume latency, frame-time
ceiling, audio-buffer safety. A contract is a *floor*, never a target to exceed.

**Controllable domain.** Anything with selectable power states: CPU, each device
(NVMe, PCIe links, SATA, USB, radios, audio, camera), GPU/display/media, memory
behavior, and the aggregate thermal/power budget.

---

## 2. The only three roles a lever may have

Every lever in the entire research corpus maps to exactly one role. If a proposed
lever does not fit one of these, it does not belong in `optid`.

1. **CONTRACT-SETTER** — reads context and *sets or raises a floor*. Decides how
   responsive a domain must stay. Floors compose by `max` (most-demanding class
   wins while active).
2. **DEPTH-ENABLER** — moves a domain to the *deepest power state allowed by the
   current floor*. This is where energy is actually saved.
3. **BUDGET-ARBITRATOR** — the outer loop. Caps aggregate power/thermal and sheds
   from lowest-priority domains first, but **never breaches a floor**.

Observability is not a role; it is the *input* that lets setters and enablers
act. Listed separately in §4.

---

## 3. The actuation rule (unifies contract + safety + reversibility)

A DEPTH-ENABLER may move domain `D` to power state `S` **iff all hold**:

1. **Contract gate:** `exit_latency(S) ≤ active_contract.floor(D)`.
2. **Safety gate:** `hwid ∈ allowlist(D, S)` **or** `S ∈ always-safe baseline`.
3. **Mutation gate:** `--apply` is set; otherwise log the intended action only
   (dry-run remains the default).
4. **Reversibility gate:** the write is recorded in the revert journal with a
   crash-safe restore path.

Every write emits a reason: `(domain, from→to, contract that permitted it,
allowlist entry)`. No silent actuation, ever.

This rule dissolves the old "safe optimizer vs aggressive optimizer" debate. NVMe
APST and PCIe ASPM exit latencies map *directly* onto PM QoS resume-latency
budgets — so "is this aggressive setting safe?" becomes "does its exit latency
fit the active floor, and is this HWID allowlisted?" Safety is no longer a
separate feature; it is two clauses of the actuation rule.

---

## 4. The lever ledger

Every lever from both papers, binned. `Status` is honest as of repo state
2026-06-08 (IMPLEMENTATION_STATUS.md): **A**=actuates, **O**=observes only,
**P**=parsed-not-applied, **—**=not implemented.

### 4.1 Observability inputs (read to know state + floor)

| Input | Kernel interface | Status |
|---|---|---|
| CPU/mem/IO pressure | `/proc/pressure/*` (PSI) | O |
| AC/battery + percentage | `/sys/class/power_supply` | O |
| Thermal zones | `/sys/class/thermal` | O |
| Load average | `/proc/loadavg` | O |
| zram swap activity | `/sys/block/zram*` | O |
| Wakeup-source / suspend blockers | `sysfs-class-wakeup` | — |
| Per-device runtime PM state + failures | `runtime_pm` sysfs | — |
| Package/C-state + sleep quality | residency counters, s2idle stats | — |
| GPU/display/media state | DRM, panel-self-refresh state | — |
| Storage/link power state | ALPM / APST current state | — |
| PM QoS / latency-contract state | `pm_qos` interfaces | — |
| Firmware/workload hints | DPTF workload hints, Intel/AMD HFI | — |

### 4.2 CONTRACT-SETTERS (set/raise the floor)

| Lever | Kernel interface | Role detail | Status |
|---|---|---|---|
| Workload-class detection | foreground/fullscreen/audio/video, PSI | Selects active contract | A |
| `optctl pin <app> <mode>` | optid state | Manual floor override | A |
| PM QoS CPU wakeup latency | `cpu_dma_latency` / `pm_qos` | CPU floor as a number | **A** |
| PM QoS per-device resume latency | per-device `pm_qos` | Device floor as a number | **A** |
| util_clamp (`uclamp_min`) | sched util-clamp | Per-task min performance floor | — |
| EPP (toward performance) | `energy_performance_preference` | Coarse CPU floor expression | A |
| `platform_profile` (performance) | `platform_profile` sysfs | Coarse platform floor | A |

*Note: The PM QoS enabler exit-latency check `fits_contract` is defined/available in the code, but is unconsumed until WP-N5/N6. Latency budget values are provisional pending WP-B1 validation.*

### 4.3 DEPTH-ENABLERS (go deeper within the floor)

| Lever | Kernel interface | Gated by §3 | Status |
|---|---|---|---|
| EPP (toward power-save) | `energy_performance_preference` | contract | A |
| `platform_profile` (low-power) | `platform_profile` | contract | A |
| systemd slice weights (background) | cgroup v2 | contract | A |
| `vm.*` sysctls (swappiness, dirty_*) | sysctl, zram-gated | contract | **A** |
| Runtime PM autosuspend (USB/PCI/audio/camera/radio) | `runtime_pm` | contract + allowlist | — |
| NVMe APST | nvme power states | contract + allowlist | — |
| PCIe ASPM (L0s/L1/L1.2) | ASPM policy | contract + allowlist | — |
| SATA link PM (ALPM) | `link_power_management_policy` | contract + allowlist | — |
| USB autosuspend / port power / wake | USB PM | contract + allowlist | — |
| Display: panel self-refresh, DPMS, backlight | DRM/i915 etc. | contract | — |
| dGPU runtime suspend | DRM runtime PM | contract + allowlist | — |
| devfreq downscale (non-CPU domains) | devfreq, interconnect | contract | — |
| Idle injection / powerclamp | idle injection | contract (outer-loop use) | — |

### 4.4 BUDGET-ARBITRATORS (outer loop)

| Lever | Kernel interface | Role detail | Status |
|---|---|---|---|
| DTPM / powercap | `powercap`, `dtpm` | Hierarchical shared power cap | — |
| Thermal governor / power allocator | thermal sysfs | Thermal budget across domains | — |
| Fan / acoustic | ACPI fan performance | Couples acoustic to thermal budget | — |
| HFI feedback | Intel/AMD HFI | Informs placement under changing capacity | — |
| `sched_ext` | BPF scheduler class | Scheduler specialization; safe fallback | — (experimental fragment only) |

---

## 5. What this spec forbids

- **No lever without a role.** If it isn't a setter, enabler, or arbitrator, it
  is out of scope.
- **No actuation outside the §3 rule.** No "aggressive mode" that ignores the
  floor or the allowlist.
- **No maximizing responsiveness.** The floor is *sufficient and invisible*, not
  "as high as possible." Spending energy to exceed a floor is, by definition,
  avoidable energy — the thing we minimize.
- **No "integration for its own sake."** A new domain is absorbed only when a
  benchmark shows it reduces avoidable energy at a fixed floor.
- **No agent redefining §0.** See §7.

---

## 6. Work-package decomposition (derived, not invented)

Each WP implements one ledger row (or tight group), behind the §3 gate, with one
verifier criterion. Sequence respects the dependency: you cannot enable depth
without first observing state and defining the contract.

| WP | Implements | Role | Verifier PASS criterion |
|---|---|---|---|
| WP-N0 | Finish `vm.*` actuation (status P→A) | enabler | vm keys applied only when zram-backed; revert journal entry present |
| WP-N1 | Workload-class detector | setter | Correct class on fixtures: idle/light/interactive/latency-critical/throughput |
| WP-N2 | PM QoS contract layer (CPU + per-device latency budgets) | setter | Floors expressed as numbers; max-composition correct under overlapping classes |
| WP-N3 | Wakeup-source + runtime-PM telemetry | observe | `optctl` reports what woke the machine and which devices never autosuspended |
| WP-N4 | Hardware allowlist DB (HWID → {domain,state} allow/deny) | safety | Default-deny for risky knobs; seeded safe baseline; denial logged with reason |
| WP-N5 | Runtime PM autosuspend policy | enabler | Device reaches deepest state whose exit latency ≤ floor; allowlist-gated; reverts on stop |
| WP-N6 | NVMe APST + PCIe ASPM + SATA ALPM | enabler | State selected by exit-latency-vs-floor; no panic on allowlisted set; off-list = denied |
| WP-N7 | Display/media depth (PSR, DPMS, backlight, dGPU runtime) | enabler | Idle display power drops at fixed interactive floor; resume within budget |
| WP-N8 | DTPM/powercap outer loop | arbitrator | Aggregate cap enforced; sheds lowest-priority first; never breaches a floor |
| WP-N9 | Thermal/fan budget coupling | arbitrator | Acoustic state tracks thermal headroom without floor breach |
| WP-B1 | Benchmark harness execution vs PPD/TLP/baseline | evidence | Real numbers published in `benchmarks/results/`; losses documented honestly |

*Note: WP-B1's first deliverable is the measurement rig (`rushbench`) and single-host evidence; the cross-distro PPD/TLP/baseline comparison is a follow-up under the same WP row.*

`sched_ext` stays an experimental fragment with no WP until a hypothesis grounded
in WP-B1 data justifies one.

**Gate:** WP-B1 evidence is required before claiming any enabler "works." An
enabler that doesn't lower avoidable energy at a fixed floor is reverted, not
shipped. The gate is satisfied by the benchmark results dataset (evidence) produced by the `rushbench` measurement rig, not by the existence of the rig itself. Gate first cleared by [benchmarks/results/2026-06-14/fedora/](file:///home/victus/Rush-linux/benchmarks/results/2026-06-14/fedora/).

---

## 7. Agent contract addendum (paste into AGENTS.md)

- Agents **may not** propose project direction, redefine the objective in §0, or
  offer "strategic pivots." Such output is auto-rejected on sight.
- A task = implement **one ledger row** from §4 as **one WP** from §6, behind the
  §3 actuation rule.
- Deliverable = code + verifier verdict (PASS/FAIL with evidence paths). Not a
  memo, not a roadmap.
- Humans own the objective and the tree. Agents implement leaves.
- Any agent claim of a created PR/branch/file must be verifiable
  (`gh pr view`, `git log`) or it is treated as fabricated.

---

## 8. One-line summary for the README

> Rush Linux minimizes avoidable platform energy subject to a per-workload-class
> responsiveness floor, holding every device in the deepest power state its active
> latency contract permits — observably, reversibly, and proven on real hardware.
