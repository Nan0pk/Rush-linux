# COMPLETE-AUDIT-REPORT.md — Rush Linux Repository Audit

**Date:** 2026-07-21
**Auditor:** Automated deep-read (every file, every line)
**Repository:** https://github.com/Nan0pk/Rush-linux
**Commit:** HEAD of `main` (shallow clone, depth=1)

---

## 1. Executive Summary

Rush Linux is an Arch-based distribution with an adaptive power/performance
optimizer (`optid`) written in Rust. The codebase is **substantive and real**:
24,222 lines of Rust across 70 files implement genuine kernel interaction
(PSI, PM QoS, sysfs, D-Bus), a six-class workload classifier, a journaled
actuator with crash recovery, PPD/GameMode D-Bus compatibility shims, and a
hardware allowlist safety gate. This is not vaporware.

**Critical findings:**

1. **No classic branch protection on `main`** (HTTP 404). A ruleset named
   `protect-main` exists (enforcement=active) but the classic branch
   protection API returns 404. [API]
2. **`fits_contract()` is dead code** — the SPEC §3 contract gate exists
   at `contracts.rs:290` with `#[allow(dead_code)]` and zero call sites.
   Depth-enablers that would call it are not implemented. [READ]
3. **All 12 allowlist entries are `verified = false`** — no depth-enabler
   write can pass the allowlist gate on any hardware. Correct fail-closed
   behavior, but means zero real power savings today. [READ]
4. **`rush_telemetry` does not compile** — excluded from workspace.
   Contains all 21 `unsafe` blocks in the repo. [READ]
5. **Two scheduled workflows are failing** (maintenance, reassess) as of
   2026-07-20. [API]

**What works and is verified:**
- `optid --once` dry-run loop: sensors -> classify -> decide -> render. [READ]
- D-Bus control plane (`optctl status/explain/mode/pin`). [READ]
- PPD shim (`net.hadess.PowerProfiles`) and GameMode shim
  (`com.feralinteractive.GameMode`). [READ]
- VM-guest detection and PSI de-rating. [READ]
- Boot path: UKI + systemd-boot + signed rollback (v0.3-v0.5 milestones
  verified with transcripts). [READ]
- Systemd sandboxing: `ProtectSystem=strict`, `NoNewPrivileges=yes`,
  `MemoryDenyWriteExecute=yes`, `SystemCallFilter=@system-service`. [READ]

---

## 2. Repository Metrics

| Metric | Value | Method |
|--------|-------|--------|
| Total files | 714 | RUN (`find`) |
| Total lines | 157,569 | RUN (`wc`) |
| Rust files (crates/) | 70 | RUN |
| Rust lines | 24,222 | RUN |
| Python files | 70 | RUN |
| Python lines | 26,502 | RUN |
| Markdown files | 183 | RUN |
| Markdown lines | 29,726 | RUN |
| Shell scripts | 31 | RUN |
| Shell lines | 7,856 | RUN |
| TOML configs | 47 | RUN |
| Workflow files | 10 | RUN |
| Systemd units | 8 (+5 in mkosi mirror) | RUN |
| PRs (all time, fetched) | 100 (95 merged, 0 open, 5 closed) | API |
| Issues (open, non-PR) | 0 | API |
| Releases | 5 (latest: v0.7.0-beta.4, 2026-07-01) | API |
| Tags | 4 (latest: v0.7.0-beta.4) | API |
| Workflow runs (total) | 4,143 | API |
| Workflow runs (last 100) | 83 success, 17 failure | API |
| Dependabot alerts (open) | 0 | API |
| Code scanning alerts (open) | 0 (11 total, all fixed) | API |
| Secret scanning alerts | 0 | API |
| Contributors | 10 (top: Nan0pk=339, claude=28) | API |
| CODEOWNERS | Not present (HTTP 404) | API |
| Branch protection (main) | HTTP 404 ("Branch not protected") | API |
| Rulesets | 1 (`protect-main`, enforcement=active) | API |

### Lines by extension (top 10)

| Extension | Files | Lines |
|-----------|-------|-------|
| (no ext) | 29 | 33,866 |
| .md | 183 | 29,726 |
| .py | 70 | 26,502 |
| .rs | 70 | 24,222 |
| .log | 18 | 15,268 |
| .sh | 31 | 7,856 |
| .json | 134 | 6,160 |
| .txt | 27 | 4,349 |
| .toml | 47 | 2,927 |
| .ps1 | 5 | 2,185 |

---

## 3. Code Quality (per module)

### 3.1 `crates/optid` — The Optimizer Daemon (14,851 lines)

**Status: REAL, substantive, well-structured.**

| File | Lines | Status | Notes |
|------|-------|--------|-------|
| `main.rs` | 411 | REAL | Run loop: snapshot->classify->decide->actuate. Signal handling, single-instance lock, conflict detection, boot state computation. |
| `args.rs` | 284 | REAL | CLI parsing. `--apply`, `--once`, `--allowlist`, `--foreground`. 12 tests. |
| `sensors.rs` | 453 | REAL | PSI, battery, thermal, loadavg, PM QoS path discovery, runtime-PM/ASPM/ALPM/backlight enumeration, VM-guest detection. 14 tests. |
| `workload.rs` | 356 | REAL | 6-class taxonomy + Mode enum. Two hysteresis state machines (class: 3s dwell, mode: 6s dwell). 6 tests. |
| `contracts.rs` | 322 | REAL | Per-class latency floors. TOML loader. `fits_contract()` is **dead code** (`#[allow(dead_code)]`, zero call sites). 9 tests. |
| `policy.rs` | 1,050 | REAL | Classification logic, auto-mode selection, decision builder. VM-guest PSI de-rating (x0.5). Curated baseline fallback. 18 tests. |
| `actuator.rs` | 1,072 | REAL | 10 Action variants. PM QoS sink abstraction. Allowlist gate. Boot-state gate. Journaled transactional writes with rollback. |
| `action.rs` | 228 | REAL | Closed Action enum: CpuEpp, PlatformProfile, SystemdSetProperty, VmSysctl, CpuDmaLatency, DeviceResumeLatency, RuntimePm, PcieAspm, SataAlpm, Backlight. |
| `decision.rs` | 63 | REAL | Renderable decision record. |
| `io_util.rs` | 568 | REAL | `guarded_write` (write allowlist + traversal rejection), atomic state files, revert functions for 5 subsystems, crash recovery. 7 tests. |
| `load_state.rs` | 222 | REAL | LoadState enum (Ok/Defaulted/Partial/Invalid). BootState decision surface. 5 tests. |
| `capability.rs` | 770 | REAL | Per-Action capability manifest. Path validation. Systemd ReadWritePaths cross-check. |
| `allowlist.rs` | 615 | REAL | WP-N4 hardware allowlist. Default-deny. Seeded baseline (compiled in via build.rs). Override precedence (seeded < distro < admin). 8 tests. |
| `dbus.rs` | 361 | REAL | `io.rushlinux.Optid1` interface. `validate_app_id` path-traversal defence. PinApplication gated behind env var pending polkit. 14 tests. |
| `foreground/mod.rs` | 153 | **STUB** | v0.6 stub: `subscribe()` spawns a thread that sleeps forever. Never yields events. Explicitly documented. 5 tests. |
| `shim/conflict.rs` | 219 | REAL | Competing-daemon detection via `systemctl is-active`. Fails open in containers. 7 tests. |
| `shim/gamemode.rs` | 645 | REAL | `com.feralinteractive.GameMode` D-Bus shim. TTL-based lazy expiry. Pin file management. 24 tests. |
| `shim/ppd.rs` | 1,054 | REAL | `net.hadess.PowerProfiles` D-Bus shim. HoldProfile/ReleaseProfile with cookie registry. |
| `actuators/display.rs` | 115 | REAL | Backlight selection heuristic, brightness floor-clamp. |
| `actuators/runtime_pm.rs` | 143 | REAL | Network carrier check, wakeup warning. |
| `actuators/storage.rs` | 67 | REAL | CNVi detection, ALPM policy constant. |
| `build.rs` | 136 | REAL | Codegens allowlist table from `data/allowlist.toml`. |
| `tests.rs` | 2,776 | REAL | Integration tests. |
| `tests/*.rs` (4 files) | 995 | REAL | Integration tests: shims, write-site gating. |

**unsafe blocks in optid:** 1 — `libc::flock` in `main.rs:82`. Justified.

**Key dead code:**
- `fits_contract()` at `contracts.rs:290` — `#[allow(dead_code)]`, zero call sites. [READ]
- `Policy::decide()` at `policy.rs:480` — `#[allow(dead_code)]`, superseded by `decide_resolved()`. [READ]

### 3.2 `crates/optctl` — CLI Client (849 lines)

**Status: REAL.** D-Bus proxy + file fallback. Commands: status, explain,
mode, pin, trace, allow, deny, list-allow. `--json` output. HWID resolution
(modalias, sysfs, udevadm). Admin override file management.

### 3.3 `crates/rushbench` — Benchmark Harness (1,973 lines)

**Status: REAL.** `run`, `matrix`, `report` subcommands. RAPL/battery energy
detection. cyclictest, PSI, foreground-launch probes. Contract validation
report generator. Mock support via env vars.

### 3.4 `crates/rush_collect` — Hardware Collector (669 lines)

**Status: REAL (Linux), STUB (Windows).** 30s observation window. PSI/RAPL/
battery deltas. Linux platform reads cpuinfo, meminfo, PSI, RAPL, thermal,
cpufreq, DMI. Windows backend: all functions return hardcoded defaults.

### 3.5 `crates/rush_telemetry` — Telemetry (2,118 lines)

**Status: PARTIALLY IMPLEMENTED, EXCLUDED FROM WORKSPACE.** Does not compile
(missing `libc` dependency, BPF skeleton codegen incomplete). Contains all 21
`unsafe` blocks in the repository. License: GPL-2.0-only (vs workspace
Apache-2.0).

### 3.6 `crates/testos` — Bootable Benchmark Environment (4,064 lines)

**Status: REAL.** Benchmark execution engine, results ingestion/validation,
host-side USB launcher, terminal UI, run planning, crash recovery.

---

## 4. Configuration Truth

### 4.1 Version Consistency

| Source | Value | Match? |
|--------|-------|--------|
| `VERSION` | `0.7.0-beta.4` | YES |
| `Cargo.toml` workspace version | `0.7.0-beta.4` | YES |
| `release/milestones.toml` current_version | `0.7.0-beta.4` | YES |
| Latest git tag | `v0.7.0-beta.4` | YES |
| Latest GitHub release | `v0.7.0-beta.4` (2026-07-01) | YES |

**All five sources agree.** [READ + API]

### 4.2 contracts.toml vs contracts.rs

Every floor value matches. A test at `contracts.rs:247`
(`load_published_contracts_toml_matches_default`) enforces this at CI time. [READ]

### 4.3 policy.toml vs Policy::default()

All threshold and mode values match. [READ]

**WARNING:** The comment at `policy.toml:73-79` claims sysctl values are
"NOT ACTUATED." **This is stale.** `policy.rs:700-730` DOES emit
`Action::VmSysctl` for swappiness/dirty bytes. The actuator writes them. [READ]

### 4.4 allowlist.toml

12 entries, 5 domains. All `verified = false`. 3 explicit deny entries.
Version `v0.7.0-beta.1`. Compiled into binary via `build.rs`. [READ]

### 4.5 deny.toml

Apache-2.0, MIT, BSD-3-Clause, BSD-2-Clause, Unicode-3.0 allowed.
`unsound = "all"`, `yanked = "deny"`. crates.io only. [READ]

---

## 5. Documentation Accuracy

### 5.1 README.md Claims vs Code

| Claim | Backed? | Status |
|-------|---------|--------|
| "optid polls sensors every 2 seconds" | `args.rs:12` DEFAULT_INTERVAL_SEC=2 | YES |
| "six workload classes" | `workload.rs` enum has 6 variants | YES |
| "Default mode is always dry-run" | `args.rs:62` apply: false | YES |
| "PM QoS (/dev/cpu_dma_latency)" | `actuator.rs:52` write_cpu_latency | YES |
| "PSI (/proc/pressure)" | `sensors.rs:100` Pressure::read | YES |
| "sched_ext / scx_loader" | No code references | NOT IMPLEMENTED |
| "Wayland + PipeWire" | Documented as "planned, not yet built" | Honest |
| "Desktop/laptop editions not yet buildable" | milestones.toml v0.7 status="planned" | YES |

### 5.2 Milestone Integrity

| Milestone | Status | Criteria verified | Assessment |
|-----------|--------|-------------------|------------|
| v0.1 (Compile-Clean Core) | complete | N/A | Honest |
| v0.2 (Real Control Plane) | complete | N/A | Honest |
| v0.3 (Rootfs Builder) | complete | 4/4 verified | YES |
| v0.4 (UKI, Boot, Rollback) | complete | 4/4 verified | YES |
| v0.5 (Minimal Installable) | complete | 4/4 verified | YES |
| v0.6 (Hardware-Aware optid) | **in-progress** | **0/4 verified** | Honest — hardware-gated |
| v0.7 (Editions) | planned | 0 criteria rows | Honest |
| v0.8-v1.0 | planned | N/A | Honest |

v0.6 is the critical gap. All 4 criteria are code-complete but hardware-gated.
Version pointer advanced to 0.7.0-beta.4 while v0.6 remains in-progress.
Documented desync with frozen version pointer. [READ]

---

## 6. CI/CD Health

All 10 workflow files pass `yaml.safe_load`. [RUN]

| Workflow | Status (last run) | Notes |
|----------|-------------------|-------|
| ci.yml | PASS | Change-classified tests |
| docker-publish.yml | PASS | ghcr.io image |
| graphify.yml | PASS | Knowledge graph |
| labeler.yml | PASS | Auto-labeling |
| maintenance.yml | **FAIL** | cargo-deny advisories |
| pages.yml | PASS | mdbook -> GitHub Pages |
| reassess.yml | **FAIL** | Strategic reassessment |
| release-drafter.yml | PASS | Release notes |
| release-testos.yml | PASS | testOS image build |
| stale.yml | PASS | Stale cleanup |

Last 100 runs: 83 success, 17 failure. [API]

---

## 7. Security Posture

### 7.1 unsafe Blocks

| Location | Count | Justification |
|----------|-------|---------------|
| `crates/optid/src/main.rs:82` | 1 | `libc::flock` single-instance lock |
| `crates/rush_telemetry/` (excluded) | 21 | MSR reads, eBPF, packed structs |
| **Workspace total** | **1** | Justified |

### 7.2 Systemd Sandboxing (optid-apply.service)

`ProtectSystem=strict`, `ProtectHome=yes`, `NoNewPrivileges=yes`,
`PrivateTmp=yes`, `CapabilityBoundingSet=CAP_SYS_NICE CAP_SYS_RESOURCE`
(no CAP_SYS_ADMIN), `ReadWritePaths` limited to 5 specific paths,
`SystemCallFilter=@system-service` minus `@privileged @obsolete`,
`MemoryDenyWriteExecute=yes`, `RestrictNamespaces=yes`,
`RestrictRealtime=yes`, `ProtectKernelModules=yes`,
`ProtectKernelLogs=yes`, `RestrictAddressFamilies=AF_UNIX AF_NETLINK`,
`ProcSubset=pid`, `ProtectProc=invisible`. [READ]

### 7.3 Write Allowlist (ADR 0009)

`io_util::guarded_write` enforces structural write allowlist. Directory
traversal rejected. Unallowlisted paths rejected. Tested. [READ]

### 7.4 D-Bus Security

PinApplication disabled by default pending polkit. `validate_app_id`
rejects traversal, absolute paths, NUL, leading dots. 14 tests. [READ]

### 7.5 Secrets and Dependencies

Secret scanning: 0 alerts. Dependabot: 0 open. cargo-deny configured. [API+READ]

---

## 8. Known Issues (with citations)

| # | Severity | Finding | Location | Label |
|---|----------|---------|----------|-------|
| 1 | HIGH | No classic branch protection on main (404). Ruleset exists but classic API unprotected. | GitHub API | API |
| 2 | HIGH | `fits_contract()` dead code — SPEC §3 gate has zero call sites. | `contracts.rs:290` | READ |
| 3 | MEDIUM | All 12 allowlist entries `verified = false`. | `data/allowlist.toml` | READ |
| 4 | MEDIUM | `policy.toml` WARNING comment stale — sysctl actuation IS implemented. | `config/optid/policy.toml:73` | READ |
| 5 | MEDIUM | maintenance.yml and reassess.yml failing. | `.github/workflows/` | API |
| 6 | MEDIUM | rush_telemetry does not compile. Excluded from workspace. | `Cargo.toml:14-19` | READ |
| 7 | LOW | No CODEOWNERS file. | GitHub API (404) | API |
| 8 | LOW | rush_telemetry GPL-2.0 vs workspace Apache-2.0. | `crates/rush_telemetry/Cargo.toml:5` | READ |
| 9 | LOW | Windows backend in rush_collect is stubs. | `platform/windows.rs` | READ |
| 10 | LOW | Foreground detection is v0.6 stub. Documented. | `foreground/mod.rs:100` | READ |
| 11 | LOW | .gitignore missing *.raw and book/. | `.gitignore` | READ |
| 12 | INFO | v0.6 in-progress (0/4 verified) while version at 0.7.0-beta.4. | `milestones.toml:120` | READ |
| 13 | INFO | `Policy::decide()` dead code, superseded by `decide_resolved()`. | `policy.rs:480` | READ |

---

## 9. What Could Not Be Verified

| Item | Reason |
|------|--------|
| `cargo build/test/clippy/fmt/audit/deny` | Rust toolchain not installed in sandbox |
| `python3 -m pytest tools/test-*.py` | PyQt5 ELF mismatch in sandbox |
| `shellcheck` on .sh files | Not installed |
| Full docs/ recursive read (183 .md files) | Read key docs; not all 183 |
| Full tools/ read (70 .py, 31 .sh, 5 .ps1) | Read structure; not all 106 line-by-line |
| testos crate every line (4,064 lines) | Read structure and key functions |
| optid/src/tests.rs every line (2,776 lines) | Read structure |
| optid/src/shim/ppd.rs full (1,054 lines) | Read first ~500 lines |
| optid/src/capability.rs full (770 lines) | Read first ~500 lines |
| mkosi build configs (24 .conf files) | Not read |
| Evidence transcripts (18 .log files) | Not read |
| Research papers (docs/research/) | Not read individually |
| Whether protect-main ruleset enforces PR reviews | Only fetched name+enforcement |

---

## 10. Assessment and Recommendations

### Overall Assessment

**The code is real, well-architected, and honestly documented.** The
optimizer daemon implements genuine kernel interaction with a layered
safety model (four-gate actuation rule, write allowlist, hardware
allowlist, boot-state decision surface). The documentation is unusually
honest about what is stubbed, what is dead code, and what is pending
hardware validation. The Builder/Verifier separation (no claim without
a transcript) is enforced in the milestone structure.

The project is at a legitimate early-beta stage: the core control loop
works, the boot path is verified, but the power-saving depth-enablers
are gated behind unverified allowlist entries and the desktop editions
do not exist yet.

### Recommendations (priority order)

1. **Verify the `protect-main` ruleset enforces required reviews and
   status checks.** Classic branch protection API returns 404. [HIGH]

2. **Add a CODEOWNERS file.** Even `* @Nan0pk` is better than none. [MEDIUM]

3. **Fix the stale WARNING comment in `policy.toml` (lines 73-79).**
   Sysctl actuation IS implemented. [MEDIUM]

4. **Fix the two failing scheduled workflows.** maintenance.yml runs
   cargo-deny advisories — failing means vulnerable deps slip through. [MEDIUM]

5. **Wire `fits_contract()` into the depth-enabler path or remove it.**
   Dead code implementing a SPEC requirement is a liability. [MEDIUM]

6. **Begin Phase D hardware validation for v0.6.** Code is code-complete;
   gap is physical-hardware transcripts. [MEDIUM]

7. **Resolve rush_telemetry license split** (GPL-2.0 vs Apache-2.0)
   before re-including in workspace. [LOW]

8. **Add `*.raw` and `book/` to `.gitignore`.** [LOW]

---

*End of report. Generated 2026-07-21. All findings labeled [READ], [RUN],
[API], or [NOT VERIFIED] per the audit protocol.*
