# Handover: Ecosystem Incorporation Strategy + First Hardware Evidence

| Field | Value |
|---|---|
| Doc | handover-2026-06-10-ecosystem-and-benchmarks, v1 |
| Date | 2026-06-10 |
| Produced by | Claude Code session (strategic assessment → ecosystem research → incorporation plan → host benchmark campaign) |
| Intended consumer | Any continuing agent (e.g. Antigravity CLI) or human maintainer |
| Working branch | `claude/practical-faraday-9hn0j8`, draft PR #22 |
| Governs | strategy + benchmark continuation; tactical execution conventions remain `docs/plans/action-plan-2026-06-10.md` and `AGENTS.md` |

This document is self-contained: it records every decision made, everything
built, every measurement taken, and the full forward plan, so work can
continue without access to the originating conversation.

---

## 1. Owner decisions (locked — do not relitigate without the owner)

Asked and answered by the project owner on 2026-06-10:

1. **Build base: adopt mkosi, image-composed.** Rush images will be built
   with mkosi from an upstream package base. `tools/rush-builder.py` and most
   of `recipes/` get retired once parity is proven (strangler-fig rule, §4.0).
2. **Base distro: Arch.** Rolling, newest kernels (best for sched_ext/MGLRU),
   CachyOS proves the scx-on-Arch path. Reproducibility via pinned Arch
   Archive snapshot dates.
3. **sched_ext: default-on centerpiece.** scx (`scx_bpfland`/`scx_lavd`,
   driven by `optid` through `scx_loader`) ships enabled on desktop/laptop
   editions, with verified automatic EEVDF fallback as the safety net.
4. Strategy docs were initially chat-only by owner choice; the owner later
   requested this exported handover, which is that export.

## 2. Strategic assessment (summary of findings)

Goal posed by owner: an OS comparable to macOS in **responsiveness, power
efficiency, reliability**.

- Architecture direction is sound: single policy owner (`optid`), modern
  boot/rollback stack (UKI + boot assessment + signed sysupdate), and
  explainability-first are the right macOS analogs.
- The core thesis ("an adaptive optimizer makes Linux measurably better")
  was unproven; the roadmap deferred proof (benchmarks, v0.8) until after the
  most expensive work. **Remedy applied: prove optid early on an existing
  distro.** This handover's §5 contains the first such proof.
- macOS mapping: reliability is reachable (sealed-volume analog = UKI +
  dm-verity + sysupdate); responsiveness is partially reachable and depends
  on scheduler-level work (sched_ext), NOT on EPP/cgroup tweaks (now
  empirically confirmed, §5); power efficiency is reachable only on a narrow
  validated hardware set.
- Effort profile risk: docs/process volume greatly exceeds code volume
  (~1.6k lines Rust). Cap meta-work until measured wins accumulate.

## 3. Ecosystem incorporation strategy

### 3.1 Admission rules (define "counterproductive")

1. **One owner per knob** — incorporate APIs/data/heuristics from other
   power/perf projects, never run their daemons alongside `optid`.
2. **One mechanism per problem** — one update path (sysupdate+UKI), one
   integrity path (dm-verity), one OOM handler (systemd-oomd), one scheduler
   framework (sched_ext w/ EEVDF fallback). A second mechanism for a solved
   problem is redundant even if good.
3. **Net-negative maintenance** — an incorporation must delete/obviate more
   than it adds.
4. **One novel-risk component per milestone** — sched_ext consumes its
   milestone's entire risk budget.
5. **Explainability gate** — every new actuation appears in `optctl explain`
   or stays dry-run.

### 3.2 Admitted projects

| Pattern | Projects |
|---|---|
| Adopt as dependency/tool | mkosi, scx suite (scx_lavd/scx_bpfland/scx_loader), zram-generator, systemd-oomd, MGLRU, Phoronix Test Suite, PowerTOP, Aya (Rust eBPF) |
| Implement their D-Bus API inside optid | power-profiles-daemon (`net.hadess.PowerProfiles`), GameMode (`com.feralinteractive.GameMode`) |
| Mine data/heuristics, never run | TLP (hardware quirk DB), system76-scheduler (foreground detection + process classes), intel-lpmd (E-core parking), uresourced (logind session protection) |
| Reference architecture only | ParticleOS (systemd upstream; mkosi profiles, user-key Secure Boot, verity layout) |

### 3.3 Rejected (the cut line)

nohang (redundant w/ systemd-oomd) · OSTree/bootc/composefs/ABRoot (second
update/integrity mechanism) · btrfs+snapper rollback (second rollback
mechanism) · running gamemoded/ppd/TLP/TuneD/auto-cpufreq/ananicy/uresourced/
system76-scheduler/intel-lpmd as daemons (knob conflicts) · custom Rush
package manager (obsolete under mkosi-on-Arch) · eBPF probes beyond one
budgeted Aya pilot.

### 3.4 Waves and gates

```
Wave 0 (config-only)   Wave 1 (foundation)        Wave 2 (optid surface)     Wave 3 (measurement)
zram-generator         mkosi/Arch image pivot     ppd D-Bus interface        PTS harness backend
systemd-oomd     ───►  scx + scx_loader + optid ► GameMode D-Bus interface ► PowerTOP calibration
MGLRU fragment         kernel fragment promote    foreground detection       Aya eBPF pilot (1 probe)
                                                  TLP quirk allowlist        intel-lpmd heuristics
                                                  vm.* actuation in optid
```

- **Wave 0 gate:** mkosi test image shows zram swap active, systemd-oomd
  running, MGLRU enabled.
- **Wave 1 gate:** mkosi image passes the EXISTING `tools/validate-uefi-boot.sh`
  and `tools/test-rollback.sh` unmodified, twice; scx soak passes with EEVDF
  fallback verified (`tools/test-scx-fallback.sh`, to be written).
- **Wave 2 gate:** GNOME/KDE power slider and a Steam game drive optid mode
  changes visible in `optctl explain`; zero writes outside the allowlist.
- **Wave 3 gate:** first published benchmark artifact vs Arch-stock per
  `benchmarks/manifest.toml`.

### 3.5 Roadmap remap

| Milestone | New meaning |
|---|---|
| v0.5.0-beta.1 | Image pivot: mkosi/Arch image passes all v0.3/v0.4 validation; Waves 0–1 done; bespoke builders retired |
| v0.6.0-beta.1 | Hardware-aware optid + Wave 2 (compat D-Bus interfaces, TLP allowlist, foreground detection, vm.* actuation) |
| v0.7.0-beta.1 | Editions become mkosi profiles + signed sysexts on one base image |
| v0.8.0-beta.1 | Benchmark lab backed by Phoronix Test Suite (Wave 3) |

### 3.6 Per-incorporation error handling (essentials)

- **Strangler-fig:** nothing deleted until its replacement passes the OLD
  validation twice; removal PR carries the transcript.
- **License quarantine:** repo is Apache-2.0. GameMode (BSD-3) interface XML
  may be copied with attribution; Aya (MIT/Apache) may be linked; scx/
  zram-generator/mkosi are external processes (no license interaction).
  **TLP (GPL-2.0) and system76-scheduler code must NEVER be copied** — only
  facts (device IDs, quirk values) and independently re-implemented behavior,
  provenance noted. Add this rule to CONTRIBUTING.md before Wave 2.
- **mkosi pivot:** pin mkosi version; pin Arch Archive snapshot date per
  release (reproducibility, ADR 0012); add an artifact-level image-policy
  test (boots image, asserts cgroup v2 unified, nftables loaded, no
  tlp.service, PSI active) to stop the Arch base silently reintroducing
  banned defaults. Abort criterion: if boot-validation parity is not reached
  in ~3 agent sessions, fall back to hybrid (mkosi assembly + current boot
  layout scripts).
- **sched_ext:** optid polls `/sys/kernel/sched_ext/state`; on eviction log a
  decision record, cooldown 10 min before reload, pin EEVDF for the session
  after 3 evictions/boot. Switch scx *profiles* on mode change; switch
  *schedulers* only on edition/explicit-mode boundaries. Never stack scx on
  the PREEMPT_RT edition.
- **ppd interface:** implement HoldProfile/ReleaseProfile cookies with
  NameOwnerChanged death-watch (auto-release on client crash) + main-loop
  expiry as defense in depth; ppd requests are INPUTS to optid's decision,
  guards (thermal/battery) win; mapping is lossy by design and `explain`
  says so. If the well-known name is taken, log loudly, skip the shim, keep
  the daemon running.
- **GameMode interface:** resolve PID→cgroup at registration (PID-reuse
  safety), clean stale registrations on process exit, cap registrations
  (e.g. 32). This also turns the `pin_application` stub in
  `crates/optid/src/main.rs` into real functionality.
- **vm.\* actuation:** every write through the allowlist; high swappiness
  gated on detected ZRAM (`high_swappiness_requires_zram` in
  `config/optid/policy.toml`); record old value in actions.log; intended
  state written to /run/optid before actuating, replayed/reverted on restart;
  failed sysctl write = log + continue, never daemon exit.
- **TLP quirk allowlist:** new `config/optid/hardware-allowlist.toml`,
  schema versioned from day one, entries carry provenance + `verified_on`;
  unverified entry = treated as absent.

## 4. What was built this session (all on PR #22)

| Commit | Artifact | Purpose |
|---|---|---|
| `686bc1d` | `tools/bench-optid-host.sh` (v1) + docs | First self-restoring host benchmark. Superseded for measurement (kept as reference); its confounds are documented in docs/testing-and-benchmarks.md. |
| `e3ab55e` | `tools/bench-optid-host-v2.sh` + docs | Isolating benchmark: load in background.slice vs probe in user.slice (makes the CPUWeight lever measurable); partial-load RAPL watts scenario (makes the EPP lever measurable); median of N iters. |
| `41a498e` | `tools/bench-optid-matrix.sh` + docs | Guided campaign runner: prompts plug/unplug and VERIFIES via /sys/class/power_supply; lever isolation (baseline / epp / weight / optid-performance / optid-battery); ambient-load recording from /proc/stat; stops+restarts tuned/ppd itself; battery floor (--min-batt 25); outputs results.csv + meta.txt + transcript.log. |

All three share a safety contract: refuse hosts optid cannot actuate
(no EPP and no platform_profile), refuse non-root, capture every knob optid
can mutate BEFORE applying, restore + verify on every exit path via shell
trap, `--apply` required to mutate (dry-run default). Slice properties are
set with `systemctl set-property --runtime` so a reboot also clears them.

Validation performed in-session: `cargo build --release` clean; optid
dry-run smoke test correct (plan emitted, no actions.log); `bash -n` passes;
preflight refuses a no-hardware container; `validate-doc-sync.py` green at
every commit.

## 5. Empirical findings (HP Victus, 24-CPU, Fedora Workstation 44)

Test hardware exposes EPP (24 CPUs) but NOT `/sys/firmware/acpi/platform_profile`;
RAPL package domain available. Ambient load during ALL runs: 4K video +
5-6 Chrome tabs + a CLI agent (recorded post-hoc; the matrix runner now
records ambient load automatically). Fedora 44's default power daemon is
**tuned** (not power-profiles-daemon).

### 5.1 Replicated, strong — the EPP/battery-mode power win

Partial load (nproc/4 busy threads), CPU package watts via RAPL, median of 3:

| Condition | AC run 1 | AC run 2 | Battery run |
|---|---|---|---|
| Baseline, no power daemon | 29.96 / 30.10 W | 30.18 / 30.09 W | 25.01 W |
| optid mode=performance | 30.12 W | 30.21 W | 25.04 W |
| optid mode=battery (EPP=power) | **16.49 W** | **16.89 W** | **16.46 W** |
| Fedora tuned active (accidental capture) | ~30 W (AC) | — | **16.99 W** |

Conclusions:
- `EPP=power` cuts package power **~45% on AC** (30→16.5 W) and **~34% on
  battery** (25→16.5 W) vs unmanaged defaults; replicated, non-overlapping
  iteration ranges.
- **optid battery mode matches and slightly edges Fedora's tuned on battery**
  (16.46 vs 16.99 W). This is first evidence toward the v0.6 exit criterion
  "battery behavior matches or improves mainstream defaults on at least one
  laptop" (n=1 machine, 1 workload — do not overclaim).
- Latency cost of battery mode was negligible in the last two runs (p95
  within ~0.005 ms of baseline).
- Firmware caps package power ~25 W on DC vs ~30 W on AC on this machine —
  record such facts in the future hardware allowlist DB.
- CAVEAT for any public claim: watts dropped but the busy-loop throughput
  also dropped (lower clocks). To claim an *efficiency* win (joules per unit
  work) the harness needs work-counting load. Not yet implemented.

### 5.2 Suggestive, unproven — cgroup-weight tail clipping

optid performance mode (user.slice CPUWeight=200) produced tight p99
iterations (0.066–0.081 ms) while baselines swung 0.060–0.796 ms — but one
baseline also posted 0.067 ms, so distributions overlap at iter=3.
**Open experiment:** `--levers baseline,weight --iter 9` in the matrix
runner settles it, and now isolates weight from EPP.

### 5.3 Null, three times replicated — the strategic datum

No mode moved p95 wakeup latency under load on this hardware. EPP and
cgroup weights are POWER levers, not felt-latency levers. This empirically
supports the strategy's central bet: **felt responsiveness must come from
scheduler-level work (sched_ext, Wave 1b), not from the current MVP's
knobs.**

## 6. Repo/infra state and conventions a continuing agent must know

- Branch `claude/practical-faraday-9hn0j8`, draft **PR #22**, three commits
  (see §4). Base `main` was `d11bb9c` at session time.
- The session container is ephemeral Ubuntu in the cloud — it has NO PSI,
  battery, thermal, EPP, or platform_profile. **Hardware benchmarks can only
  run on the owner's machines**; design every hardware task as a runbook the
  owner executes, with transcripts captured automatically.
- GitHub access from the cloud session was intermittent (403s); pushes
  eventually worked through the proxy. A PAT the owner pasted in-chat was
  used once for an emergency push and **must be revoked** (flagged to owner).
- Repo conventions that bind all work: docs updated in the same change
  (enforced by `tools/validate-doc-sync.py` + `docs/docmap.toml` — register
  new docs there); builder ≠ verifier per work package; no claim without a
  transcript (see `docs/templates/VERIFICATION.md`); evidence lives under
  `release/evidence/`; commit style and PR flow per `AGENTS.md` and
  `docs/plans/action-plan-2026-06-10.md`.
- Fedora hosts: stop `tuned` for clean A/Bs (the matrix runner does this
  itself, and restarts it).

## 7. Prioritized next actions

1. **Run the matrix campaign** on the Victus:
   `sudo ./tools/bench-optid-matrix.sh --apply` (≈25–30 min, one plug + one
   unplug prompt). Then analyze `results.csv`: attribute effects per lever;
   settle the p99/weight question (§5.2), ideally with a follow-up
   `--levers baseline,weight --iter 9`.
2. **Commit the evidence package** from §5 + the matrix output under
   `release/evidence/host-bench/2026-06-10-victus/` (raw transcripts +
   summary README with the tables and caveats above); update PR #22.
3. **Add perf-per-watt to the harness** (work-counting load → joules per
   work unit) so the 45% becomes a defensible efficiency claim (§5.1 caveat).
4. **Wave 0 PR** (small, config-only): MGLRU in
   `distro/kernel/default-adaptive.config`, `zram-generator.conf`,
   systemd-oomd enablement, optid zram detection to make
   `high_swappiness_requires_zram` functional.
5. **mkosi spike** (parallel, long pole): minimal Arch-based mkosi image;
   gate = existing `tools/validate-uefi-boot.sh` + `tools/test-rollback.sh`
   pass unmodified, twice. Abort to hybrid after ~3 failed sessions.
6. **scx integration design** per §3.6 (after Wave 0; consumes the risk
   budget of its milestone).
7. Wave 2 items in order: ppd D-Bus shim, GameMode shim, vm.* actuation,
   TLP-derived allowlist, logind foreground detection.
8. Write the two ADRs implied by owner decisions: "image composition on
   Arch via mkosi" (supersedes parts of ADR 0008) and "sched_ext default-on
   with EEVDF fallback" (amends ADR 0010 scope + non-goals wording).

## 8. Open questions for the owner (do not assume)

- Which 2–3 reference laptops define the supported-hardware deep-validation
  set? (Victus is de-facto #1.)
- When to flip the strategy from this handover doc into ADRs + roadmap
  edits on `main`? (Roadmap remap in §3.5 touches ROADMAP.md, which is
  currently untouched on the branch.)
- Public claim policy: hold the "45%" number until perf-per-watt
  measurement exists (recommended), or publish with the throughput caveat?
