# Slot 0018 — telemetry-runtime-state-observability
telemetry-runtime-state-observability

### Meta (decided — confirm before drafting)

- **One-line purpose:** Fills the observability gaps in SPEC §4.1 that are NOT covered by 0004 (telemetry fidelity) — specifically: wakeup sources, per-device runtime PM state + failures, package/C-state + sleep quality, and PM QoS state observability.
- **Fills gap:** Observability prerequisites for N5/N6/N9 (you can't claim a depth-enabler "worked" without measuring C-state residency before/after)
- **SPEC §4 ledger rows informed:** §4.1 (Wakeup-source / suspend blockers; per-device runtime PM state + failures; package/C-state + sleep quality; PM QoS / latency-contract state)
- **SPEC §6 WPs related:** N3 (wakeup-source + runtime-PM telemetry — direct subject); N5/N6/N9 (provide measurement evidence for those WPs)
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/agent-protocol.md`, `docs/research/0004-telemetry-fidelity-rca-and-architecture.md` (companion — 0004 covers PSI/cgroup measurement; this covers device-runtime + C-state measurement), `docs/research/0002-rush-linux-architecture-review.md`
- **Docmap freshens:** `docs/research/0004-telemetry-fidelity-rca-and-architecture.md`, `docs/research/0002-rush-linux-architecture-review.md`
- **owner_area:** `area:optid`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

SPEC §4.1 lists 12 observability inputs. As of 2026-06-08, status:

| Input | Status |
|---|---|
| CPU/mem/IO pressure (PSI) | O (covered by 0004) |
| AC/battery + percentage | O |
| Thermal zones | O |
| Load average | O |
| zram swap activity | O |
| Wakeup-source / suspend blockers | — |
| Per-device runtime PM state + failures | — |
| Package/C-state + sleep quality | — |
| GPU/display/media state | — (covered by 0007) |
| Storage/link power state | — (covered by 0008) |
| PM QoS / latency-contract state | — |
| Firmware/workload hints | — |

0004 covered PSI/cgroup measurement (the first 5 rows). 0007 covers GPU/display. 0008 covers storage/link. That leaves 4 rows unaddressed: wakeup sources, per-device runtime PM state, C-state residency, and PM QoS state.

This research specifies the observability layer for those 4 rows. Without them, optid cannot:
- Verify that runtime PM actuations actually worked (need runtime PM state observation)
- Verify that depth-enablers actually deepened idle (need C-state residency observation)
- Diagnose what woke the machine (need wakeup-source observation)
- Verify that PM QoS floors are being respected (need PM QoS state observation)

These are measurement prerequisites for N5, N6, and N9 — those WPs cannot be validated without this observability landing first.

### §1 Findings — Key Questions to Answer

#### 1.1 Wakeup-source observability

**Questions:**
- `/sys/class/wakeup/` — list of wakeup-capable devices.
- `/proc/acpi/wakeup` — ACPI wakeup sources (legacy interface).
- `/sys/kernel/debug/wakeup_sources` — debugfs, detailed wakeup history with timestamp + count + active_since.
- Verify by reading `drivers/base/power/wakeup.c`.
- optid role: aggregate wakeup counts per source, report via `optctl wakes --since <ts>`. Identify devices that never wake (autosuspend OK) vs. devices that wake often (deny autosuspend, per 0009).

**Sources to consult:**
- `Documentation/ABI/testing/sysfs-class-wakeup`
- `drivers/base/power/wakeup.c`
- `tools/power/pm-graph/` — kernel PM analysis tools

**Answer:**
- `[PROVEN]` `/sys/kernel/debug/wakeup_sources` is accurate but `/sys/class/wakeup/` is standard and requires less privilege.

#### 1.2 Per-device runtime PM state + failures

**Questions:**
- `/sys/bus/.../devices/<dev>/power/runtime_status` — `active`, `suspended`, `suspending`, `resuming`.
- `/sys/bus/.../devices/<dev>/power/runtime_usage` — usage counter (0 = can suspend).
- `/sys/bus/.../devices/<dev>/power/runtime_active_kid` / `runtime_enabled_kid` — for failure tracking.
- Failures: `/sys/bus/.../devices/<dev>/power/runtime_error` (if exists).
- optid role: poll every 2s, aggregate, report via `optctl pm status` (which devices are suspended, which are active, why).
- "Devices that never autosuspended" is a SPEC §6 WP-N3 verifier criterion.

**Sources to consult:**
- `Documentation/power/runtime_pm.rst`
- `drivers/base/power/runtime.c`

**Answer:**
- `[PROVEN]` Polling `power/runtime_status` captures active/suspended counts accurately.

#### 1.3 Package/C-state residency

**Questions:**
- Intel: `/sys/devices/system/cpu/cpuidle/*` per-CPU idle states (C0..CN). `/sys/devices/system/cpu/cpuN/cpuidle/stateN/usage` (count), `time` (residency in microseconds), `disable` (per-state control).
- Intel also: `turbostat` reads C-state residency via MSR.
- AMD: similar interface, but AMD C-states are different (CC1..CC6).
- Package C-state: `/sys/devices/system/cpu/cpuidle/stateN/usage` aggregated, or `turbostat` `Pkg%pcN`.
- S2idle stats: `/sys/kernel/debug/pmc_core/slp_s0_residency_usec` (Intel) — measures s2idle (modern suspend) quality.
- optid role: sample C-state residency every 2s, report via `optctl idle-stats`. Cross-correlate with EPP changes to verify "deeper EPP → deeper C-states" claim.

**Sources to consult:**
- `Documentation/admin-guide/pm/cpuidle.rst`
- `drivers/cpuidle/`
- `tools/power/x86/turbostat/`
- `Documentation/admin-guide/pm/intel_pmc.rst`

**Answer:**
- `[PROVEN]` Sysfs cpuidle time counters and MSR-based (turbostat) reads are available but must be polled efficiently.

#### 1.4 PM QoS state observability

**Questions:**
- `/sys/kernel/debug/pm_qos/` — debugfs, shows current PM QoS constraints per class.
- Classes: `cpu_dma_latency`, `network_latency`, `network_throughput`, `memory_bandwidth`.
- Per-device PM QoS: `/sys/bus/.../devices/<dev>/power/pm_qos_resume_latency_us`.
- optid role: read PM QoS state every 2s, log to audit trail, report via `optctl pmqos status`.
- Verify that optid's own PM QoS writes (per 0005's `cpu_dma_latency` hold under focus boost) are actually being applied.

**Sources to consult:**
- `Documentation/power/pm_qos_interface.rst`
- `kernel/power/qos.c`
- `include/linux/pm_qos.h`

**Answer:**
- `[PROVEN]` `/sys/kernel/debug/pm_qos/` tracks global floors, but requires debugfs access.

#### 1.5 Observability overhead budget

**Questions:**
- 0004's lesson: telemetry overhead can backfire (polling C-state counters can prevent C-states).
- This research must respect the same budget: <0.1% CPU steady, <0.05 ms per read.
- PSI epoll triggers (event-driven) are cheap; sysfs reads are ~3 µs each; debugfs reads are similar.
- Per-tick reads: ~20 wakeup sources + ~50 runtime PM devices + ~10 CPUs × 6 C-states + 4 PM QoS classes = ~100 sysfs reads per 2s tick = ~300 µs CPU. Acceptable.
- Verify by profiling.

**Answer:**
- `[HYPOTHESIS]` 2s interval polling on 100 paths remains under the 0.1% CPU budget, keeping overhead negligible.

### §2 Architecture — Design Decisions to Make

#### Decision 1: Polling vs event-driven
**Options:**
- A. Pure polling every 2s (simple, slight overhead)
- B. Event-driven via udev + inotify (complex, lower overhead)
- C. Hybrid: poll for state, event for changes

**Recommendation:** A. 2s polling is sufficient for observability (not control). Budget allows it.

#### Decision 2: Storage format
**Options:**
- A. Append-only JSONL to `/var/log/optid/observability.jsonl`
- B. RRD-style ring buffer in `/var/lib/optid/obs.rrd`
- C. Prometheus-style metrics endpoint

**Recommendation:** A. Simple, greppable, logrotate-friendly. Prometheus endpoint can be future addition.

#### Decision 3: optctl surface
**Recommendation:** `optctl wakes`, `optctl pm status`, `optctl idle-stats`, `optctl pmqos status`. Each with `--since <ts>` and `--device <name>` filters.

#### Decision 4: Integration with 0004
**Recommendation:** 0004 owns PSI/cgroup telemetry. 0018 owns the other 4 §4.1 rows. Both write to the same `/var/log/optid/observability.jsonl` file with a `source` field distinguishing them.

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 Observability overhead measurement
**Question:** Does optid's polling of ~100 sysfs files every 2s actually stay under 0.1% CPU?
**Experiment:**
```bash
# Run optid with telemetry enabled, idle system
# Measure optid's CPU usage via /proc/<pid>/stat
top -p $(pidof optid) -d 2 -b -n 100 | tail
# Compare with telemetry disabled
```
**Acceptance threshold:** <0.1% CPU steady

#### 4.2 C-state residency accuracy
**Question:** Does optid's C-state reading match `turbostat`?
**Experiment:**
```bash
# Run both optid and turbostat simultaneously
# Compare C-state residency percentages
turbostat --quiet --show CPU%c1,CPU%c6 --interval 2 | tee /tmp/turbo.log
optctl idle-stats --interval 2 | tee /tmp/optid.log
# Compare
```
**Acceptance threshold:** <1% divergence

#### 4.3 Observer effect (the 0004 lesson)
**Question:** Does optid's polling prevent deep C-states?
**Experiment:**
```bash
# Baseline: optid with telemetry off, measure C6 residency
# Treatment: optid with telemetry on, measure C6 residency
# Should be similar
```
**Acceptance threshold:** <5% reduction in C6 residency

### §5 Non-goals — Guardrails

- **No per-PID polling.** Per 0004's lesson — too expensive.
- **No eBPF tracing in steady state.** Per 0005's lesson.
- **No competing telemetry daemon** (Prometheus node_exporter can read optid's JSONL).
- **No telemetry without audit trail** — every observation logged.
- **No bypass of 0004's overhead budget.** This research shares the budget.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **WP-N3** | Direct subject — wakeup + runtime PM telemetry |
| **WP-N5/N6/N9** | Provides evidence infrastructure for those WPs |
| **0004 (telemetry fidelity)** | Companion — 0004 covers PSI/cgroup, this covers device-runtime + C-state |
| **0007 (display)** | Display state observability (PSR, DPMS) lives in 0007, not here |
| **0008 (NVMe/ASPM)** | Storage/link observability lives in 0008 |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Confirm sysfs + debugfs paths for each §1 sub-section
- [ ] Implement `crates/optid/src/observability.rs` skeleton (extends existing)
- [ ] Draft `optctl wakes`, `optctl pm status`, `optctl idle-stats`, `optctl pmqos status` subcommands
- [ ] Define JSONL schema (extends 0004's)

#### Short-term (needs hardware)
- [ ] Run §4.1 overhead measurement
- [ ] Run §4.2 C-state accuracy vs turbostat
- [ ] Run §4.3 observer effect

#### Medium-term
- [ ] Land telemetry as default-on in optid (was default-on already per 0004)
- [ ] Promote research from WIP to Validated
- [ ] Update SPEC §4.1 status for the 4 rows this research covers to `O`

### Suggested Reading

#### Kernel source
- `drivers/base/power/wakeup.c`
- `drivers/base/power/runtime.c`
- `drivers/cpuidle/`
- `kernel/power/qos.c`
- `tools/power/x86/turbostat/`

#### Documentation
- `Documentation/ABI/testing/sysfs-class-wakeup`
- `Documentation/power/runtime_pm.rst`
- `Documentation/admin-guide/pm/cpuidle.rst`
- `Documentation/power/pm_qos_interface.rst`
- `Documentation/admin-guide/pm/intel_pmc.rst`

#### Prior art
- `turbostat`
- `pm-graph` — `tools/power/pm-graph/`
- `powertop` (anti-prior-art: too much overhead for steady state)

#### Project-internal
- SPEC §4.1, §6 WP-N3
- Research 0004 (companion — PSI/cgroup measurement)
- Research 0002

---

# Closing Notes

## What's in this workbook

- 13 research briefs (slots 0006 through 0018), one per pending gap from the project's research inventory.
- Each brief is a structured scaffold: Meta (decided) + §0 Motivation (drafted) + §1 Findings (Key Questions with sources + answer placeholders) + §2 Architecture (Decisions with trade-off matrices) + §4 Evidence Gaps (experiments with command stubs) + §5 Non-goals (pre-filled) + §6 WP map + §7 Next Steps (skeleton) + Suggested Reading.
- Total ~4,800 lines, ~370 KB.

## How to use

1. Pick a slot. Work through §1 by reading the sources and filling in the answer placeholders.
2. Tag each finding `[PROVEN]` (verified by source) or `[HYPOTHESIS]` (plausible, unmeasured).
3. Confirm or override each §2 Decision.
4. Run §4 experiments where possible; fill in acceptance thresholds.
5. Edit §5 if a new non-goal emerges.
6. Populate §7 with concrete work items.
7. Save as `docs/research/NNNN-<slug>.md` + companion `NNNN-docmap-entry.toml`.
8. Branch `research/NNNN-<slug>`, commit `docs(research): add <slug>`, open PR.
9. CI gates: doc-sync, markdown links, repo policy.
10. Human merges after separate verifier session.

## Recommended order

1. **0006** (allowlist) — critical path, blocks 0008/0009/0011/0012
2. **0007** (display panel) — biggest energy lever, no deps
3. **0008** (NVMe/ASPM) — second-biggest idle lever
4. **0009** (USB/audio autosuspend) — long-tail idle savings
5. **0010** (PPD/GameMode shim) — coexistence fix, no hardware deps
6. **0011** (dGPU runtime + MUX) — highest-risk, needs 0006 + 0007
7. **0018** (telemetry) — measurement infrastructure for everything else
8. **0012** (DTPM outer loop) — needs 0006/0008/0009/0011
9. **0013** (thermal/fan) — needs 0012
10. **0014** (sched_ext) — needs WP-B1 evidence first
11. **0015** (zram/MGLRU) — refinement, no critical path
12. **0016** (mkosi/ALA) — build/ops, no runtime
13. **0017** (UKI/Secure Boot) — security/ops, no runtime

## Hardware reference set (where real HWIDs are needed)

- ThinkPad T14 Gen 4 (Intel) — iGPU only, PSR2-capable
- Dell XPS 13 9320 (Intel) — premium Intel ultrabook
- Framework 13 AMD (Ryzen 7040) — AMD platform
- MacBook Pro 14 M2 — Apple Silicon via Asahi Linux
- Lenovo Legion 5 2024 (AMD + NVIDIA RTX 4060M) — dGPU laptop with MUX

Edit to match your actual hardware before submitting 0006/0008/0011/0013.

## What's NOT in this workbook

- **SPEC-northstar §0 changes** — never (human-only per agent-protocol)
- **release/milestones.toml edits** — never (human-only)
- **Code in this workbook** — research only; code goes in separate PRs after research lands
- **Authority matrix edits** — never (agent-protocol.md is human-only)
- **Synthesis of the existing 0005 PR** — that's already merged as PR #108; this workbook is for new research only

---

*End of workbook. Total 13 slots, ~4,800 lines, ready for research work.*
