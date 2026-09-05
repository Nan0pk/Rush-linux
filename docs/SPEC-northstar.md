# SPEC-northstar.md — Rush Linux Canonical Objective Specification

**Version:** 1.1 — experimental amendment, 2026-09-05
**Status:** Proposed for review on the experimental branch. The owner authorized
the reassessment and isolated experiments; this does not assert acceptance of
every recommendation or change production defaults. The approved version on
`main` remains the production specification until the maintainer approves this
amendment. Objective changes require explicit human approval and a version bump;
agents may prepare and recommend them.

## Rush Linux's purpose

Build a dependable, responsive Linux OS that completes useful work efficiently,
adapts automatically to the user and machine, and requires little maintenance.
Mac-like responsiveness, battery behavior, and integration are comparison goals,
not existing capabilities or a promise of hardware-independent parity.

Optid's objective below is one part of that OS. Installation, application
compatibility, graphics/audio quality, accessibility, security, updates, recovery,
and daily usability have independent acceptance requirements. They do not need
to justify their existence as energy-saving levers. Source building and vertical
integration are available methods, not success criteria.

Evaluate improvement at equivalent useful work and service quality: completion
time and throughput, response-time tails, dropped frames/audio underruns,
brightness and resolution, battery/suspend behavior, and recovery. Record the
user's performance or battery preference. Do not hide a regression behind a
single aggregate score or quieter/slower behavior. Numerical experimental
margins must be fixed before confirmatory measurements and do not silently
replace approved release criteria.

---

## 0. The single objective

> `optid` **minimizes avoidable platform energy** subject to a **per-workload-class
> responsiveness floor**, by holding each controllable domain in the **deepest
> power state its active latency contract permits**.

This is the optimizer's objective, not the whole OS's definition. Energy is
measured over equivalent completed work, or an equal-duration idle/service
window with equivalent delivered service. Responsiveness, throughput and output
quality requirements bound the optimization; they are not expendable to obtain
a lower wattage. Faster completion may itself reduce total energy.

The deepest admissible state is a candidate, not an instruction to force every
idle interval into it. Account for transition energy, expected residency,
uncertainty and oscillation. Prefer native kernel/driver decisions when they
already own this timing. Linux CPUIdle explicitly considers both target residency
and exit latency ([primary documentation](https://docs.kernel.org/admin-guide/pm/cpuidle.html)).
This correction does not invent a new generic device-residency ABI or authorize
a hardware write; any new implementation must pass the existing actuation rule.

### Why the optimizer needs a precise objective

The prior tenets were "zero waste + high responsiveness + optimal hardware use +
max vertical integration." Three of those are not objectives:

- **Zero waste** and **high responsiveness** are the *same axis* pulled in
  opposite directions. Deep idle saves energy and costs wake latency. You cannot
  maximize both; you arbitrate between them per context.
- **Optimal hardware use** is a tautology — optimal toward *what* is the question
  this spec answers.
- **Max vertical integration** is a *method*, not a goal. Integration is pursued
  only as far as it reduces avoidable energy without breaching a floor.

These distinctions bound Optid's responsibilities. They do not forbid proposing
a better algorithm, testing a different build, or pursuing the wider OS goals.

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

**Latency contract.** The responsiveness requirement for the active class,
expressed as concrete budgets: CPU wakeup latency, per-device resume latency,
frame-time ceiling, audio-buffer safety. A latency ceiling is a maximum allowed
delay, not a measurement and not a reason to deliberately slow faster work.
Most-demanding composition means `min` for numeric maximum-delay budgets and
`max` for minimum-performance requirements.

**Controllable domain.** Anything with selectable power states: CPU, each device
(NVMe, PCIe links, SATA, USB, radios, audio, camera), GPU/display/media, memory
behavior, and the aggregate thermal/power budget.

---

## 2. The only three roles a lever may have

Every lever in the entire research corpus maps to exactly one role. If a proposed
lever does not fit one of these, it does not belong in `optid`.

1. **CONTRACT-SETTER** — reads context and *sets or raises a floor*. Decides how
   responsive a domain must stay. Compose by strictness as defined in §1;
   do not take the numeric maximum of maximum-delay budgets.
2. **DEPTH-ENABLER** — moves a domain to the *deepest power state allowed by the
   current floor*. This is where energy is actually saved.
3. **BUDGET-ARBITRATOR** — the outer loop. Caps aggregate power/thermal and sheds
   from lowest-priority domains first, but **never breaches a floor**.

These are admission requirements, not a guarantee that arbitrary simultaneous
demands are physically achievable. If a safe thermal/power envelope cannot meet
all contracts, retain native hardware protection, report the unmet requirement,
and use the explicitly defined degradation policy. Never override hardware
protection or record success merely to satisfy an impossible contract.

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

Historical design inventory from June 2026; the status cells below are **not a
current implementation or promotion ledger**. For current construction read
[`optid-package-status.toml`](plans/optid-package-status.toml). The historical
notation is **A**=actuates, **O**=observes only,
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
| Fan / acoustic observation | ACPI/hwmon | Informs thermal context; fan actuation excluded | — |
| HFI feedback | Intel/AMD HFI | Informs placement under changing capacity | — |
| `sched_ext` | BPF scheduler class | Scheduler specialization; safe fallback | — (experimental fragment only) |

---

## 5. What this spec forbids

- **No lever without a role.** If it isn't a setter, enabler, or arbitrator, it
  is out of scope.
- **No actuation outside the §3 rule.** No "aggressive mode" that ignores the
  floor or the allowlist.
- **No unaccounted trade-offs.** Responsiveness and throughput improvements are
  legitimate OS goals. Optid respects the active service requirements and user
  preference; a lower power reading alone is not proof of greater efficiency.
- **No "integration for its own sake."** A new domain is absorbed only when a
  benchmark shows it reduces avoidable energy at a fixed floor.
- **No agent redefining §0.** See §7.

---

## 6. Work-package decomposition (derived, not invented)

The table below is the historical decomposition, not the active task selector.
Use the current completion plan and package ledger for construction. Sequence
respects the dependency: you cannot enable depth without first observing state
and defining the contract. OS integration and isolated source-build experiments
may proceed independently of unfinished optional optimizer domains.

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
| WP-N9 | Thermal budget and fan observation | arbitrator | Thermal policy respects contracts; no fan actuation |
| WP-B1 | Benchmark harness execution vs PPD/TLP/baseline | evidence | Real numbers published in `benchmarks/results/`; losses documented honestly |

*Note: WP-B1's first deliverable is the measurement rig (`rushbench`) and single-host evidence; the cross-distro PPD/TLP/baseline comparison is a follow-up under the same WP row.*

`sched_ext` stays an experimental fragment with no WP until a hypothesis grounded
in WP-B1 data justifies one.

**Gate:** WP-B1 evidence is required before claiming any enabler "works." An
enabler that doesn't lower avoidable energy at a fixed floor is reverted, not
shipped. The gate is satisfied by the benchmark results dataset (evidence) produced by the `rushbench` measurement rig, not by the existence of the rig itself.

---

## 7. Relationship to agent work

- `AGENTS.md` owns execution and verification procedure; do not duplicate a
  competing workflow here.
- Agents may research, recommend direction, prepare amendments and implement
  authorized experiments. They must distinguish a proposal from human approval.
- Choose a coherent behavior or research question. Use the active package
  contract for Optid construction and the smallest risk-appropriate path for
  other work. Historical lever rows do not define every permitted task.
- Evidence must match the claim: source review, production-path tests, controlled
  hardware experiments and release acceptance prove different things.
- Human approval owns permanent project direction and release decisions. Reviewed
  repository integration is delegated under
  `docs/agent-protocol.md` and ADR 0027; do not require a human merge of every PR.
- Any agent claim of a created PR/branch/file must be verifiable
  (`gh pr view`, `git log`) or it is treated as fabricated.

---

## 8. One-line summary for the README

> Rush Linux minimizes avoidable platform energy subject to a per-workload-class
> responsiveness floor, holding every device in the deepest power state its active
> latency contract permits — observably, reversibly, and proven on real hardware.
