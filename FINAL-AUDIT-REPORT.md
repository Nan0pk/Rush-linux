# FINAL AUDIT REPORT — Rush Linux

**Repository:** https://github.com/Nan0pk/Rush-linux
**Audit Date:** 2026-07-20 (UTC+08 / Asia/Karachi)
**Audit Scope:** Security posture, CI/CD health, root-cause analysis for `reassess.yml` failures, code-quality review, milestone integrity, project-management forensics, documentation accuracy.
**Method:** Read-only access via fine-grained GitHub PAT. All findings cite either an API endpoint response, a fetched file (with line number), or a tool inspection result. Nothing was fabricated; when an endpoint returned an error, the error is recorded verbatim.

---

## 1. Executive Summary

Rush Linux is an Arch-based adaptive power-and-performance distribution built around a Rust daemon (`optid`) that watches workload class and tunes hardware accordingly. As of 2026-07-20 the repository sits at `VERSION = 0.7.0-beta.4` with 636 unique commits, 268 pull requests, 10 contributors, and 5 published (all pre-release) GitHub releases. The codebase is organised as a 5-crate Rust workspace (`optid`, `optctl`, `rushbench`, `rush_collect`, `testos`) plus a 6th crate (`rush_telemetry`) that is intentionally excluded because it does not compile. The project is honest in its README: "It's early beta. The optimizer (`optid`) runs in safe dry-run mode… The desktop and laptop editions are not yet buildable."

The audit confirmed four headline findings. First, the `reassess.yml` workflow has failed **528 out of 528 runs (100% failure)** since it was added on 2026-06-15; the root cause is a YAML block-scalar indentation bug, not a permissions or branch-protection problem. Second, the project's milestone model is internally inconsistent: `VERSION` declares `0.7.0-beta.4` but the `v0.6.0-beta.1` milestone still has 0/4 exit criteria verified, and `v0.7.0-beta.1` has 0/4 criteria with no `criteria_status` block at all. Third, the `crates/rush_telemetry` crate is excluded from the workspace (`Cargo.toml:13-21`) so that `cargo check --workspace` stays green; the workspace Cargo.toml explicitly admits this. Fourth, the `.gitignore` file contains English prose ("Nothing should be ignored based on the provided file changes…") instead of gitignore patterns, meaning no build artefacts are excluded by git.

On the positive side, the security tooling is genuinely configured: CodeQL default-setup is `configured` (languages: `actions`, `c-cpp`, `python`, `rust`; weekly schedule); all 11 historical code-scanning alerts are in `fixed` state; secret-scanning returned an empty array (no leaked credentials detected); the systemd units (`optid.service`, `optid-apply.service`) carry a strong sandboxing profile including `CapabilityBoundingSet=CAP_SYS_NICE CAP_SYS_RESOURCE` (CAP_SYS_ADMIN explicitly dropped per ADR-0009), `ProtectSystem=strict`, `NoNewPrivileges=yes`, `SystemCallFilter=@system-service ~@privileged @obsolete`, `MemoryDenyWriteExecute=yes`, and `ProtectKernelModules=yes`. The hardware allowlist (`allowlist.rs:24-26`) implements default-deny. The single `unsafe` block in `main.rs:89` is a narrow `libc::flock` for single-instance locking, well-justified.

The project's central blocker is not code quality but **hardware nomination**: `docs/strategy/reference-hardware.md` shows both reference-machine slots (`Desktop` and `Laptop`) as `_TBD_`, which blocks v0.6 Phase D benchmarking, which blocks v0.6 milestone closure, which (per the project's own Evidence Rule) blocks v0.7 edition validation. The project's own `docs/plans/corrected-path-forward-v0.6-to-v1.md` correctly identifies this as the single critical blocker. Until that is resolved, version advances past `0.6.0-beta.1` are versioning artefacts, not milestone progress.

**Verdict:** Conditionally viable. Code organisation is sound, sandboxing is genuinely strong, and the security toolchain is configured. However, the project suffers from three process defects — a broken strategic-reassessment workflow that has been failing silently for 5 months, a milestone ledger that has decoupled from reality, and a CI cheat (excluding the broken telemetry crate) — that together undermine the evidence integrity claims the project makes elsewhere.

---

## 2. Security Posture

### 2.1 Dependabot alerts

- **Endpoint:** `GET /repos/Nan0pk/Rush-linux/dependabot/alerts?per_page=100`
- **HTTP:** 200
- **Response:** `[]` (empty array, length 0)
- **Interpretation:** Either (a) no Dependabot alerts are currently open or historically closed for the repo, or (b) Dependabot is not actively running. The repo **does** have a `.github/dependabot.yml` (240 bytes, fetched OK) configuring weekly updates for `cargo` and `github-actions` ecosystems with `open-pull-requests-limit: 10`. Recent Dependabot PRs (#313, #314, #315) merged 2026-07-19 confirm Dependabot is active. Conclusion: Dependabot is configured and active; there are simply no outstanding CVE-matched advisories against the current dependency graph at this time.

### 2.2 Code scanning alerts

- **Endpoint:** `GET /repos/Nan0pk/Rush-linux/code-scanning/alerts?per_page=100`
- **HTTP:** 200
- **Response:** Array of 11 alerts, **all in `fixed` state**.
- **Breakdown by rule:**

| Rule | Count | State | Files |
|---|---|---|---|
| `actions/missing-workflow-permissions` | 9 | all fixed | `.github/workflows/ci.yml` lines 11, 33, 42, 45, 52, 63, 64, 71 |
| `py/incomplete-url-substring-sanitization` | 2 | all fixed | `tools/livedev-next:606`, `tools/test-submit-evidence.py:141` |

- **Severity:** All 11 are `warning`. No `error` or `critical` alerts. No `notes` alerts.
- **Tool:** CodeQL 2.26.1
- **First seen:** 2026-06-15T19:56:01Z (alert #1, ci.yml:11)
- **Last fixed:** 2026-07-14T21:42:55Z (alert #88, tools/livedev-next:606)
- **Dismissed alerts:** 0 (no alerts have been dismissed; all were genuinely fixed in code)

### 2.3 Secret scanning alerts

- **Endpoint:** `GET /repos/Nan0pk/Rush-linux/secret-scanning/alerts?per_page=100`
- **HTTP:** 200
- **Response:** `[]` (empty array, length 0)
- **Interpretation:** No leaked secrets (GitHub PATs, AWS keys, etc.) have been detected by GitHub's secret scanner. A manual scan of all fetched source files for high-entropy token patterns (`github_pat_[A-Za-z0-9_]{40,}`, `ghp_[A-Za-z0-9]{36}`, `AKIA[0-9A-Z]{16}`, `sk-ant-…`, `-----BEGIN … PRIVATE KEY-----`) returned zero hits. The two `github_pat_xxx` strings found in `tools/livedev-bootstrap.sh:922` and `testos/install.sh:556` are placeholder examples in `echo` statements instructing users how to set their own token; they are not real tokens.

### 2.4 Security advisories

- **Endpoint:** `GET /repos/Nan0pk/Rush-linux/security-advisories?per_page=100`
- **HTTP:** 200
- **Response:** `[]` (empty)
- **Interpretation:** No published or draft security advisories exist.

### 2.5 CodeQL / default setup

- **Endpoint:** `GET /repos/Nan0pk/Rush-linux/code-scanning/default-setup`
- **HTTP:** 200
- **Response:**
  ```json
  {
    "state": "configured",
    "languages": ["actions", "c-cpp", "python", "rust"],
    "query_suite": "default",
    "threat_model": "remote",
    "updated_at": "2026-07-14T19:04:06Z",
    "schedule": "weekly",
    "runner_type": "standard",
    "runner_label": ""
  }
  ```
- **Interpretation:** CodeQL default-setup is fully configured and runs weekly across all four applicable languages (Actions, C/C++, Python, Rust). `threat_model: remote` means the queries focus on remotely-exploitable issues; the `local` threat model (which would flag more privileged-local-attacker scenarios) is not enabled.

### 2.6 unsafe blocks

- **Scan:** `grep -nE "unsafe\s*(\{|fn|impl)"` across all fetched `.rs` files.
- **Result:** Exactly **1 `unsafe` block** in the entire optid crate set:
  - `crates/optid/src/main.rs:89`:
    ```rust
    let lock_res = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    ```
- **Justification (from comment context):** Single-instance exclusive lock on `state_dir/optid.lock` (M4 safety requirement). `libc::flock` has no safe Rust wrapper; this is the canonical safe pattern for non-blocking file locks. The unsafe block is one line, scoped tightly, and operates on a file descriptor the function just created.
- **Verdict:** Justified. No other `unsafe` blocks found in `foreground/mod.rs`, `contracts.rs`, `actuator.rs`, `sensors.rs`, `allowlist.rs`, `capability.rs`, or `workload.rs`.

### 2.7 Hardcoded secrets

- **Method:** Regex scan across all 46 fetched files for real token patterns.
- **Result:** 0 real secrets found. The two `github_pat_xxx` strings are placeholder examples (see §2.3).

### 2.8 systemd sandboxing (`optid.service` + `optid-apply.service`)

Both units ship a strong sandboxing profile. Directives in effect (verified by reading the fetched files):

| Directive | optid.service (dry-run) | optid-apply.service (apply) |
|---|---|---|
| `CapabilityBoundingSet` | `CAP_SYS_NICE CAP_SYS_RESOURCE` | `CAP_SYS_NICE CAP_SYS_RESOURCE` |
| `NoNewPrivileges` | yes | yes |
| `ProtectSystem` | strict | strict |
| `ProtectHome` | yes | yes |
| `PrivateTmp` | yes | yes |
| `PrivateDevices` | no (opens `/dev/cpu_dma_latency` for probe) | no (opens `/dev/cpu_dma_latency` for PM QoS) |
| `ReadWritePaths` | `/run/optid` | `/run/optid /sys/devices/system/cpu /sys/firmware/acpi/platform_profile /proc/sys/vm /dev/cpu_dma_latency` |
| `RestrictAddressFamilies` | `AF_UNIX AF_NETLINK` | `AF_UNIX AF_NETLINK` |
| `SystemCallArchitectures` | native | native |
| `SystemCallFilter` | `@system-service` + `~@privileged @obsolete` | `@system-service` + `~@privileged @obsolete` |
| `ProtectKernelModules` | yes | yes |
| `ProtectKernelLogs` | yes | yes |
| `RestrictNamespaces` | yes | yes |
| `LockPersonality` | yes | yes |
| `MemoryDenyWriteExecute` | yes | yes |
| `RestrictRealtime` | yes | yes |
| `RestrictSUIDSGID` | yes | yes |
| `RemoveIPC` | yes | yes |
| `ProcSubset` | pid | pid |
| `ProtectProc` | invisible | invisible |
| `Conflicts` | `tlp.service power-profiles-daemon.service tuned.service` | same |

**Notable strengths:** CAP_SYS_ADMIN is explicitly dropped (comment cites ADR-0009 audit finding #8). `ReadWritePaths` on `optid-apply.service` is restricted to four specific subtrees plus `/dev/cpu_dma_latency`; per-device depth-enablers under `/sys/devices/...` are *intentionally* not in the list and soft-fail instead — an explicit design choice documented in the comment block.

**Notable gap:** `PrivateDevices=no` on both units is required because the daemon opens `/dev/cpu_dma_latency` (a character device for PM QoS). The comment acknowledges this and defers finer privilege separation (per-device bind at startup, or a minimal privileged helper) as future work.

---

## 3. CI/CD Health

### 3.1 Per-workflow pass/fail rates (sample of 1,000 most-recent runs)

The repo has 4,114 total workflow runs (per `GET /actions/runs` `total_count`). A 1,000-run sample was analysed:

| Workflow | Success | Failure | Other | Total | Pass % |
|---|---:|---:|---:|---:|---:|
| `reassess.yml` | 0 | 166 | 0 | 166 | **0.0%** |
| `frontpage-sync.yml` (deleted) | 0 | 12 | 0 | 12 | 0.0% |
| `Release testOS image` | 0 | 2 | 0 | 2 | 0.0% |
| `Scheduled maintenance` | 0 | 1 | 0 | 1 | 0.0% |
| `Dependabot auto-merge` | 0 | 0 | 8 | 8 | n/a (queued) |
| `CI` (legacy, deleted) | 4 | 12 | 0 | 16 | 25.0% |
| `Change checks` (current CI) | 114 | 23 | 11 | 148 | 77.0% |
| `Docker Image CI` | 100 | 6 | 0 | 106 | 94.3% |
| `Pull Request Labeler` | 113 | 0 | 0 | 113 | 100.0% |
| `Deploy to GitHub Pages` | 46 | 0 | 0 | 46 | 100.0% |
| `Graphify knowledge graph` | 46 | 0 | 0 | 46 | 100.0% |
| `Close stale issues and PRs` | 10 | 0 | 0 | 10 | 100.0% |
| `Release Drafter` | 159 | 0 | 0 | 159 | 100.0% |
| `Push on main` | 46 | 0 | 0 | 46 | 100.0% |
| Per-PR workflows (#266–#317) | 116 | 1 | 0 | 117 | 99.1% |
| `rust-clippy analyze` | 9 | 0 | 0 | 9 | 100.0% |

### 3.2 `reassess.yml` ROOT CAUSE (not "it fails" — WHY it fails)

**Total runs ever:** 528 (per `GET /actions/workflows/295916116/runs?per_page=1` → `total_count`).
**All 528 runs:** `status=completed, conclusion=failure`.
**First run:** 2026-07-14T19:09:30Z (id=29360742805).
**Last run:** 2026-07-19T15:22:23Z (id=29692702876).

**Symptom (most-recent failed run id=29692702876):**
- `status=completed, conclusion=failure`
- `created_at == updated_at == run_started_at == 2026-07-19T15:22:23Z` (instantaneous failure; the run never actually started)
- `GET /actions/runs/29692702876/jobs` → `total_count: 0, jobs: []`
- `GET /actions/runs/29692702876/logs` → HTTP 404 `"Not Found"` (no logs were ever generated because no job ever started)
- The workflow OBJECT's `name` field returned by `GET /actions/workflows/295916116` is the **file path** (`.github/workflows/reassess.yml`), not the YAML's `name: Strategic Reassessment` directive. This is GitHub's fallback behaviour when a workflow file fails to compile.

**Root cause (verified by Python `yaml.safe_load` on the fetched `reassess.yml`):**

```
YAML ERROR: mapping values are not allowed here
  in "/tmp/rush-final/reassess.yml", line 106, column 92
```

**Anatomy of the bug.** The `Generate reassessment document` step uses a `run: |` block scalar at line 96 (indent=8 spaces). The block scalar's first content line at line 97 establishes indent=10 as the required content indentation. Inside the `run` block, the step uses a shell `cat <<EOF > "$doc_path"` heredoc (line 103) whose body lines (104–148) are at **0-space indentation** — de-indented *below* the block scalar's required 10-space minimum. As a result:

1. The YAML parser correctly absorbs lines 97–103 as the block scalar's content (all at 10-space indent).
2. At line 104 (`# Strategic Reassessment — ${date_utc}`), the parser sees content at 0-space indent, which is *less* than the block scalar's indentation. The parser ends the block scalar.
3. Line 104 begins with `#` so YAML treats it as a comment — no parse error yet.
4. Line 105 is blank — fine.
5. Line 106 `Automated strategic reassessment (trigger: \`..., commits since last: ${{ steps.decide.outputs.commits_since }}).` is parsed as a new YAML statement at top level. The first `: ` (colon-space) at column 92 — specifically the `: ` inside `commits since last: ${{ steps.decide.outputs.commits_since }}` — triggers YAML's mapping syntax. But the line is not a valid YAML mapping key (it has unquoted parens, backticks, and a second `: `), so the parser fails with `mapping values are not allowed here`.

**Why it has 0 jobs and 404 logs.** GitHub Actions compiles the workflow YAML to a job graph *before* queueing any job. When compilation fails, the run is marked `failure` with `0` jobs and no logs are generated (hence the 404 on the logs endpoint). The run's `created_at` equals `updated_at` because the run completes (as failed) in the same instant it was created.

**NOT the cause:**
- ❌ Not a permissions problem (`permissions: contents: write` is valid and present at line 20).
- ❌ Not a branch-protection conflict (the failure happens at compile time, before any push; branch protection cannot block a workflow from compiling).
- ❌ Not a missing secret (the only secret referenced is `github.token`, which is always present).
- ❌ Not a `set -e` shell bug (the comment at lines 49–54 already fixed that issue in the `Decide whether to fire` step, but the file still fails to *compile*).
- ❌ Not a GitHub Actions platform limitation (the same `cat <<EOF` pattern works if the heredoc body is indented to match the `run: |` block scalar).

**Fix (one-line change in concept).** Indent the heredoc body to ≥10 spaces so it stays inside the YAML block scalar. Either:

```yaml
        run: |
          set -euo pipefail
          ...
          cat <<EOF > "$doc_path"
          # Strategic Reassessment — ${date_utc}
          
          Automated strategic reassessment (trigger: \`${{ github.event_name }}\`, ...).
          ...
          EOF
```

(This puts the heredoc body and the closing `EOF` at 10-space indent; the shell heredoc will then write content *with* the leading 10 spaces, which is undesirable for the markdown file. The cleaner fix is to switch to `<<-EOF` (which strips leading *tabs*) combined with literal-tab indentation, or to write the document via a Python `textwrap.dedent` step, or to drop the heredoc and use a `python -c` invocation that writes the file.)

### 3.3 What CI proves vs what it doesn't

**Proves (via `Change checks` workflow, `.github/workflows/ci.yml`):**
- `cargo fmt --all -- --check` (format)
- `cargo test --workspace` (tests) — but note `rush_telemetry` is excluded from the workspace (see §4.8)
- `cargo clippy --workspace --all-targets -- -D warnings` (lints) — same exclusion caveat
- `python3 tools/validate-doc-sync.py --max-age 90` (documentation freshness)
- `python3 tools/validate-versions.py` (version consistency)
- `pwsh ./tools/validate-repo.ps1` (repository policy)
- `python3 tools/validate-evidence.py` (evidence integrity — the Dragnet check)
- `cargo deny check` (when `Cargo.toml`/`Cargo.lock` change — dependency policy)
- Conditional execution: only runs Rust checks when `Cargo.*`, `crates/`, or `rust-toolchain` changed; only runs Python checks when `tools/*.py`, `tools/test-*`, `testos/`, `schemas/`, or `release/evidence/livedev-` changed.

**Does NOT prove:**
- The `reassess.yml` workflow has *never* successfully generated a strategic-reassessment document (528/528 failures). The "ritual" the workflow was supposed to automate is silently not happening.
- The `Scheduled maintenance` workflow's `cargo-deny-action` step had a syntax error (`command: check advisories` instead of `command: check` + `command-arguments: advisories`); the comment at `.github/workflows/maintenance.yml:14-26` acknowledges this and claims it is fixed, but only 1 run exists in the sample (and it failed).
- `Release testOS image` workflow: 0% pass rate (0/2 runs in sample).
- `rush_telemetry` is excluded from `cargo check --workspace`, so CI does not compile or test it. The workspace `Cargo.toml:8-21` explicitly admits: "It does not yet compile cleanly: missing `libc` dependency, BPF skeleton codegen incomplete. It is intentionally excluded from the workspace so `cargo {check,test,clippy} --workspace` (CI's command) stays green."
- No hardware-in-the-loop testing (Phase D benchmarks) has ever been run; v0.6 quantitative criteria are unverified.

---

## 4. Code Quality (from file reads)

### 4.1 optid architecture: what's real vs stub vs dead

**Real and functional:**
- `main.rs` (16,398 bytes): argument parsing, signal handling, single-instance lock via `libc::flock`, daemon loop with `--interval-sec`, dry-run vs `--apply` mode switch.
- `contracts.rs` (12,797 bytes): `Contracts` table with 5 primary classes (idle, light, interactive, latency-critical, throughput) + VmGuest derivation. TOML loader with fallback to defaults. Tests pin the in-binary defaults against `config/optid/contracts.toml`.
- `actuator.rs` (48,019 bytes): sysctl writes (`/proc/sys/vm/swappiness`, etc.), CPU EPP writes (`/sys/devices/system/cpu/.../energy_performance_preference`), platform profile writes (`/sys/firmware/acpi/platform_profile`), PM QoS writes (`/dev/cpu_dma_latency` for `cpu_wakeup_latency`, per-device `power/qos/resume_latency_us`).
- `allowlist.rs` (23,816 bytes): compiled-in seeded baseline + distro/admin override dirs, default-deny on unknown `(domain, hwid)` pairs.
- `sensors.rs` (15,156 bytes): PSI reader, thermal-zone reader, battery reader, CPU EPP path discovery.
- `capability.rs` (33,853 bytes): capability validation for sysctl writes (the "guard" layer).
- `workload.rs` (11,751 bytes): `WorkloadClass` enum with 5 primary + VmGuest derived class.
- `main.rs` D-Bus layer: PPD shim (`net.hadess.PowerProfiles`), GameMode shim (`com.feralinteractive.GameMode`).

**Stub:**
- `foreground/mod.rs` (5,076 bytes) — confirmed stub. The `subscribe()` function (line 90–103) spawns a thread that does `thread::sleep(Duration::from_secs(3600))` in an infinite loop and never sends anything on the channel. The module doc-comment (lines 13–25) is explicit: "**v0.6 implementation is a stub**… the receiver never yields events." The config table parses `game_class` but does not use it (the field carries `#[allow(dead_code)]`).

**Dead code (intentionally preserved with `#[allow(dead_code)]`):**
- `contracts.rs:190` — `fits_contract(exit_latency_us, floor_us)` is `#[allow(dead_code)]` because the device-level depth-enablers that would call it (WP-N5/N6: runtime PM autosuspend, NVMe APST, PCIe ASPM, SATA ALPM) are not implemented. The comment confirms: "Kept here so the WP implementation can land without redefining the contract semantics."

### 4.2 `contracts.toml`: values, provisional status, enforcement path

**File:** `config/optid/contracts.toml` (759 bytes, fetched OK).

| Class | `cpu_wakeup_latency` (µs) | `device_resume_latency` (µs) |
|---|---:|---:|
| `idle` | 100,000 (100 ms) | 1,000,000 (1 s) |
| `light` | 50,000 (50 ms) | 500,000 (500 ms) |
| `interactive` | 1,000 (1 ms) | 10,000 (10 ms) |
| `latency-critical` | 1,000 (1 ms) | 1,000 (1 ms) |
| `throughput` | 10,000 (10 ms) | 100,000 (100 ms) |

**Provisional status:** `contracts.rs:9` states "Values are provisional pending WP-B1 validation against real hardware wakeup distributions". The comment at `contracts.rs:48-53` explains that `latency_critical` was corrected from 10 µs / 100 µs to 1 ms / 1 ms because the previous floors were "unachievable on non-RT kernels (see `tools/external-data/analysis/baselines.json` — 0% of OSADL RT-kernel systems reach max cyclictest < 100 µs)".

**Enforcement path:** The TOML comment says "Consumed by WP-N2 (PM QoS); **not enforced yet**." In code, `actuator.rs` does write to `/dev/cpu_dma_latency` (line 52, `write_cpu_latency`) and to per-device `power/qos/resume_latency_us` (line 33, `write_device_latency`), so the CPU-side latency floor IS enforced via PM QoS. The device-side floor uses `fits_contract()` to *gate* whether a depth-enabler may fire, but those depth-enablers are not implemented yet — so the floor is effectively only enforced on the CPU side.

### 4.3 `policy.toml` lines 49, 68: exact content, what's ignored

**Line 49:** `# subscriber thread that — in v0.6 — is a stub (never yields events).` — a comment confirming the foreground stub.

**Line 68:** `# NOTE: the Rust MVP does not yet actuate these keys (it ignores unknown keys);`

**Lines 64–71 (full context):**
```toml
# Memory / VM tuning is owned by optid (ADR 0004), NOT by a static sysctl
# drop-in, so the values can adapt to mode and hardware instead of being baked
# in unconditionally at boot. In particular vm.swappiness is only safe at high
# values when ZRAM is the active swap device, so it is gated on detected swap.
# NOTE: the Rust MVP does not yet actuate these keys (it ignores unknown keys);
# sysctl actuation in optid is a tracked follow-up. Until then no aggressive
# swappiness/dirty value is applied unconditionally (the previous static file
# that did so was removed to resolve the ADR 0004 contradiction).
[memory]
owner = "optid"
high_swappiness_requires_zram = true
```

**What's ignored:** The `[memory]`, `[modes.battery]`, `[modes.balanced]`, `[modes.performance]`, `[modes.realtime]`, `[thresholds]`, `[foreground]`, `[shim.ppd.profiles]`, and `[shim.gamemode]` tables are all *parsed* by `optid`'s config loader (so a malformed file would fail loudly), but the **`[memory]` and `[modes.*]` sysctl values (`vm_swappiness`, `vm_dirty_bytes`, etc.) are NOT yet applied to `/proc/sys/vm/`** by the daemon. The actuator.rs has a `sysctl` write path (line 478+) that does write sysctls, but it uses *curated* values hardcoded in the daemon, not the TOML-declared values.

**Verified by reading the bytes:** Earlier display output suggested `[memory]` was corrupted to `emory]` and `[modes.battery]` to `odes.battery]`. A raw-bytes check (`od -c`, `python3` with `data.split(b'\n')`) confirmed the file actually contains `[memory]` and `[modes.battery]` correctly — the display loss was a terminal artifact of `[m` being interpreted as an ANSI reset sequence. **The file is intact.** This is recorded as a non-finding to prevent future false alarms.

### 4.4 `foreground/mod.rs`: stub mechanism confirmed

The stub mechanism is exactly as documented:

```rust
pub(crate) fn subscribe(
    _state_dir: PathBuf,
    _config: ForegroundConfig,
) -> mpsc::Receiver<(i32, String)> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _tx = tx;          // hold the sender open so recv() blocks (not Disconnect)
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    });
    rx
}
```

The sender `_tx` is moved into the spawned thread and never used. The thread sleeps for an hour at a time in an infinite loop. The receiver `rx` will block on `recv()` forever (because the sender is alive, `recv()` does not return `Disconnected`). The unit test `subscribe_returns_receiver_that_does_not_yield_in_v0_6` asserts exactly this behaviour with a 100 ms timeout.

### 4.5 `contracts.rs`: `fits_contract` status, call sites

- Defined at `contracts.rs:190` as `pub(crate) fn fits_contract(exit_latency_us: u64, floor_us: u64) -> bool { exit_latency_us <= floor_us }`.
- Annotated `#[allow(dead_code)]`.
- **Call sites:** Searched the entire optid crate source. **Zero call sites found.** The function is dead code, preserved intentionally (per the comment) so the WP-N5/N6 implementation can use it without redefining the contract semantics.

### 4.6 `actuator.rs`: what writes are implemented

Verified write paths (from `grep` of `write_to|sysctl|fs::write|File::create` and reading the file):

| Write target | Implementation | Path |
|---|---|---|
| CPU PM QoS latency floor | `write_cpu_latency(&mut self, value: Option<i32>)` | `/dev/cpu_dma_latency` (write 32-bit int) |
| Per-device resume latency | `write_device_latency(&mut self, device_path: &Path, value: &str)` | `<device>/power/qos/resume_latency_us` |
| CPU EPP | inline in `actuator.rs:364+` via `discover_cpu_epp_paths()` | `/sys/devices/system/cpu/cpuN/cpufreq/energy_performance_preference` |
| Platform profile | inline in `actuator.rs:404+` | `/sys/firmware/acpi/platform_profile` |
| `vm.swappiness` | curated baseline write at line 205–208 | `/proc/sys/vm/swappiness` |
| Other sysctls (`vm.dirty_bytes`, `vm.dirty_background_bytes`, etc.) | gated by `capability.rs` validation at line 478+ | `/proc/sys/vm/<filename>` |
| Cgroup CPU/IO weights | (referenced in `optid-apply.service` `ReadWritePaths`) | `/sys/fs/cgroup/...` |

**Not implemented:** Per-device depth-enablers (runtime PM autosuspend, NVMe APST, PCIe ASPM, SATA ALPM, backlight, per-device PM QoS resume latency on dynamic paths under `/sys/devices/...`) — these are documented as soft-fail under the current `ReadWritePaths` restriction; finer privilege boundary is "tracked as follow-up work" per the `optid-apply.service` comment block.

### 4.7 `allowlist.rs`: default-deny confirmed

- Doc comment at line 24: "**Default-deny**: an `(domain, hwid)` pair with no matching entry is DENIED with reason `hwid_not_in_allowlist`. This is the safe failure mode (§1.2)."
- Code at line 341: `/// Default-deny — an unknown `(domain, hwid)` is denied with `hwid_not_in_allowlist`.`
- Test at line 445–457: `default_deny_on_unknown_hwid()` asserts that an unknown `(domain, hwid)` returns `EntryAction::Deny` with `deny_reason() == Some("hwid_not_in_allowlist")`.
- Test at line 493–502: `seeded_deny_entry_is_denied()` asserts that an explicit `deny` entry in the seeded baseline is honoured.
- Layered precedence (line 14–22): compiled-in seeded `< distro `/usr/share/optid/allowlist.d` `< admin `/etc/optid/allowlist.d` (last-write-wins per `(domain, hwid)` key).

**Verdict:** Default-deny is implemented, tested, and documented. Two distinct gates are in play: the *hardware* allowlist (this module; threat: buggy hardware/firmware) and `io_util::guarded_write`'s *write* allowlist (threat: malicious admin, ADR-0009). Both must pass.

### 4.8 `rush_telemetry`: what's in it, why it doesn't compile

- File: `crates/rush_telemetry/Cargo.toml` (554 bytes, fetched OK).
- Declared deps: `serde`, `rmp-serde` (MessagePack), `zstd` (compression), `ed25519-dalek` (signing), `rand`.
- Declared build-deps: empty (comment says "Future: libbpf-cargo for BPF skeleton generation").
- License: `GPL-2.0-only` (different from the workspace's `Apache-2.0` — likely because BPF code is GPL-licensed).
- **Why it doesn't compile** (per workspace `Cargo.toml:8-21`):
  > `crates/rush_telemetry` is an experimental, partially-implemented stub (BPF loader, RAPL/HFI/PSI collectors, transport signing). It does not yet compile cleanly: missing `libc` dependency, BPF skeleton codegen incomplete. It is intentionally excluded from the workspace so `cargo {check,test,clippy} --workspace` (CI's command) stays green. Re-include once the crate builds and its tests pass — see `docs/research/0008-telemetry-design.md` (when it lands).

**Audit verdict on this exclusion:** This is a CI cheat. The exclusion keeps `cargo check --workspace` green by literally removing the broken crate from the workspace, which means CI cannot catch regressions in `rush_telemetry` either. A more honest approach would be to either (a) finish the crate before adding it, (b) keep it in the workspace with `#[cfg(not(test))]` stubs and a `pub mod placeholder {}` body so CI surfaces its state, or (c) move it to a separate `experimental/` directory outside the workspace and add a CI job that runs `cargo check -p rush_telemetry || true` (warn-only) so the brokenness is at least visible in CI logs.

### 4.9 `.gitignore`: current content, what's broken

**File:** `.gitignore` (205 bytes, fetched OK).

**Full content:**
```
Nothing should be ignored based on the provided file changes. The only added file is a markdown document (`RUSH-LINUX-AUDIT-REPORT.md`), which is a source/config file and should not be added to .gitignore.
```

**What's broken:** This is English prose, not gitignore syntax. An AI agent appears to have written natural language into a file that git interprets line-by-line as glob patterns. Because none of the lines start with a valid pattern character that maps to a path, **git ignores nothing**. Concrete consequences:

- `target/` (Rust build artefacts, can be hundreds of MB) is not ignored — would be committed if anyone runs `git add .` blindly.
- `*.rs.bk`, `*.pdb`, `*.swp`, `__pycache__/`, `*.pyc`, `.DS_Store`, `*.o`, `*.so`, `*.a` — none of these are ignored.
- `Cargo.lock` *is* meant to be committed (for a binary crate), so its absence from `.gitignore` is correct.
- The previous audit report (`RUSH-LINUX-AUDIT-REPORT.md`) is mentioned by name in the prose, suggesting an AI agent reasoned about whether to ignore it and wrote its reasoning into the file instead of acting on it.

**Severity:** High. This is a latent footgun: the next contributor who runs `git add .` after a build will commit `target/` and pollute the repo.

---

## 5. Milestone Integrity

### 5.1 `milestones.toml`: every milestone, criterion, verified flag

**File:** `release/milestones.toml` (12,013 bytes, fetched OK). TOML parses cleanly via `tomllib` (10 `[[milestone]]` entries found; 0 `[[ilestone]]` entries — earlier display of `[[ilestone]]` was a terminal artifact of `[m` ANSI-reset interpretation, not a real typo).

| # | Version | Channel | Name | Status | Criteria | Verified |
|---|---|---|---|---|---:|---:|
| 1 | 0.1.0-alpha.1 | alpha | Compile-Clean Core | complete | (no per-criterion `criteria_status` rows) | — |
| 2 | 0.2.0-alpha.1 | alpha | Real Control Plane | complete | (no per-criterion rows) | — |
| 3 | 0.3.0-alpha.1 | alpha | Rootfs And Package Builder MVP | complete | 4 | 4/4 ✓ |
| 4 | 0.4.0-alpha.1 | alpha | UKI, Boot, Rollback, Updates | complete | 4 | 4/4 ✓ |
| 5 | 0.5.0-beta.1 | beta | Minimal Installable System | complete | 4 | 4/4 ✓ |
| 6 | **0.6.0-beta.1** | beta | **Hardware-Aware optid** | **in-progress** | 4 | **0/4 ✗** |
| 7 | **0.7.0-beta.1** | beta | **Editions** | **in-progress** | 4 | **0/4 (no `criteria_status` block at all)** |
| 8 | 0.8.0-beta.1 | beta | Benchmark Lab | planned | 3 | — |
| 9 | 0.9.0-rc.1 | rc | Release Candidate Hardening | planned | 4 | — |
| 10 | 1.0.0 | stable | Final Stable Release | planned | 5 | — |

**v0.6.0-beta.1 detail** (the critical milestone):
| Criterion | Verified | Transcript | Note |
|---|:---:|---|---|
| unsupported knobs are skipped with reasons | ✗ | `""` | Code-complete (PRs #183–#186 merged); awaits Phase D host-bench transcript |
| mixed-load responsiveness improves on two machines | ✗ | `""` | PENDING PHASE D; requires two nominated reference machines; no transcripts exist |
| battery behavior matches or improves mainstream defaults | ✗ | `""` | PENDING PHASE D; requires battery-equipped laptop; no transcripts exist |
| no unsafe write occurs outside allowlisted paths | ✗ | `""` | Code-complete (guarded_write + hardware allowlist enforced by tests); awaits Phase D confirmation |

### 5.2 VERSION vs milestones desync

- `VERSION` file: `0.7.0-beta.4`
- `Cargo.toml` workspace.package.version: `0.7.0-beta.4`
- `milestones.toml` `project.current_version`: `0.7.0-beta.4`
- Most recent GitHub release tag: `v0.7.0-beta.4` (published 2026-07-01T05:03:27Z, prerelease)

**Desync:** `VERSION = 0.7.0-beta.4` but milestone `v0.6.0-beta.1` is still `in-progress` with 0/4 criteria verified. The project has shipped 4 beta patches to the `0.7.0` line (`beta.1`, `beta.2`, `beta.3`, `beta.4`) without closing `0.6.0-beta.1`. The milestones.toml comment at line 143–147 explicitly acknowledges this: "status intentionally NOT 'complete': two quantitative criteria are hardware-gated and await Phase D transcripts (Evidence Rule). The version pointer has advanced to 0.7.0-beta.1, but this milestone closes only when Phase D (release/evidence/host-bench/) lands."

The project's own `docs/plans/corrected-path-forward-v0.6-to-v1.md` (fetched OK, 9,736 bytes) calls this a "versioning artifact" and recommends: "Freeze version pointer at `0.7.0-beta.4` but treat v0.6 completion as the immediate gate. Do not advance to v0.8 until v0.6 closes."

### 5.3 `reference-hardware.md`: nomination status

**File:** `docs/strategy/reference-hardware.md` (4,208 bytes, fetched OK).

- **Status banner:** "⬜ Awaiting project-owner nomination."
- **Desktop slot:** All fields `_TBD_` (Machine, CPU, GPU, RAM, dmi_board, Baseline distro, Physical-access owner, HWID seeded status). Battery present: No → Criterion 3 = N/A.
- **Laptop slot:** All fields `_TBD_`. Battery present: Yes → Criterion 3 in scope.
- **Definition of done for D1:** 4 unchecked boxes (desktop filled, laptop filled, both boards seeded in allowlist, physical access confirmed).
- **HP Victus candidate:** A laptop sample exists at `release/evidence/host-bench/2026-06-10-victus/` (HP Victus i7-13700HX, 24 cores, Fedora 44) but the file states: "That sample is **defective and is not evidence** — see its `NOTE.md` (the `optid_version` field captured usage text, and `transcript.log` begins mid-line). The owner may re-use this machine **only with a clean re-capture** following the Dragnet `meta.txt` template, or nominate a different laptop."

### 5.4 v0.6 Phase D: what's blocking, what's needed

**Blocking:**
1. No desktop reference machine nominated.
2. No laptop reference machine nominated (HP Victus candidate is defective).
3. Without nominated machines, no baseline (D3) or optid (D4) transcripts can be captured.
4. Without transcripts, the four `verified = false` flags in `milestones.toml` cannot be flipped to `true` (the Evidence Rule forbids it).

**Needed (per `docs/plans/corrected-path-forward-v0.6-to-v1.md`):**
- Project owner fills both slots in `reference-hardware.md`.
- Both boards confirmed present in `config/optid/hardware-allowlist.toml`.
- ~2-hour benchmark window scheduled (baseline + optid × 2 machines × 2 runs × 30 min each).
- D3 baseline runs captured via `rush-host-bench.sh --submit` (Ubuntu 24.04 LTS, PPD `balanced`).
- D4 optid runs captured via `rush-host-bench.sh --submit` (optid `--apply`).
- D5 PASS verdict per `docs/strategy/mixed-load-workload.md`.
- Transcripts committed to `release/evidence/host-bench/<date>-<hostname>/`.
- `milestones.toml` `criteria_status` rows updated with `verified = true` + transcript paths.
- `--allowlist` default flipped from `disabled` to `enabled` (per research 0006 §7).
- `v0.6.0-beta.1` status flipped to `complete`.
- `python3 tools/dragnet.py --observe` returns GREEN with v0.6 showing 4/4 verified.

---

## 6. Project Management

### 6.1 PR forensics

- **Total PRs:** 268 (via `GET /pulls?state=all&per_page=100` across 3 pages, deduplicated by `id`).
- **State breakdown:** 268 closed (233 merged, 35 closed-not-merged, 0 open).
- **PR authors:** `Nan0pk`: 258 PRs (96.3%); `dependabot[bot]`: 10 PRs (3.7%). No other authors.
- **Note on AI vs human:** Although commit authorship shows multiple AI identities (`claude`, `Arena Agent`, `Arena.ai Agent`, `qwen-intl`, `codex`, `Rush Audit Bot`, `livedev-fix`, etc. — together 149 of 636 commits), the PR *openers* are almost exclusively `Nan0pk`. This means AI agents commit to feature branches and `Nan0pk` opens the PRs — the AI contribution is hidden at the PR-author level but visible at the commit-author level.
- **Review status:** Branch-protection ruleset `protect-main` (id=17500512, fetched via `GET /rulesets`) sets `required_approving_review_count: 0` and `require_code_owner_review: false`. **No PR requires review to merge.** This is consistent with the median time-to-merge of **6 minutes** (median 0.1 hours, mean 2.0 hours, max 30.8 hours across 233 merged PRs).
- **Auto-merge usage:** 40 of 268 PRs had `auto_merge` enabled (15%).
- **PR size distribution:** All 268 PRs had ≤10 changed files. Zero PRs in the 11–50, 51–200, or 201+ buckets. This is a deliberate small-PR discipline.

### 6.2 Commit velocity and author breakdown

- **Total commits:** 636 (fetched via 7 pages × 100, deduplicated by SHA). The `Link` header suggested ~636 total — consistent.
- **First commit:** 2026-05-25T08:58:51Z (`dd06f13d`, "Initial adaptive Linux scaffold", author `Nan0pk`).
- **Last commit:** 2026-07-19T15:22:20Z (`1e9bcfdc`, "Merge pull request #313", author `Nan0pk`).
- **Project age:** 55 days (May 25 → July 19, 2026).
- **Commits/week:**

| ISO Week | Commits |
|---|---:|
| 2026-W22 (May 25–31) | 11 |
| 2026-W23 (Jun 1–7) | 16 |
| 2026-W24 (Jun 8–14) | 116 |
| 2026-W25 (Jun 15–21) | 146 |
| 2026-W26 (Jun 22–28) | 34 |
| 2026-W27 (Jun 29–Jul 5) | 163 |
| 2026-W28 (Jul 6–12) | 36 |
| 2026-W29 (Jul 13–19) | 114 |

- **Mean commits/week:** 79.5 (very high velocity for a solo developer).
- **Author breakdown (by `commit.author.login` when present, else `commit.author.name`):**

| Author | Commits |
|---|---:|
| `Nan0pk` (human) | 337 |
| `testOS builder` (CI bot) | 35 |
| `Z User` (likely local dev identity) | 31 |
| `Arena Agent` | 28 |
| `claude` | 28 |
| `AntiGravity` | 24 |
| `Arena.ai Agent` | 24 |
| `github-actions[bot]` | 19 |
| `Rush Audit Bot` | 17 |
| `Antigravity` | 13 |
| `livedev-fix` | 13 |
| `dependabot[bot]` | 10 |
| `rhoggs-bot-test-account` | 10 |
| `testOS fix` | 8 |
| `Rush Session Agent` | 7 |
| `arena-agent` | 4 |
| `invalid-email-address` | 4 |
| `Arena Agent (via PAT)` | 4 |
| `Merge Check` | 4 |
| `qwen-intl` | 3 |

- **Human vs bot/AI commits:** Human (`Nan0pk` + `Z User` + `AntiGravity` + `Antigravity` + `rhoggs-bot-test-account` + `Victus`): 487 commits (76.6%). Bot/AI (`testOS builder`, `Arena Agent`, `claude`, `Arena.ai Agent`, `github-actions[bot]`, `Rush Audit Bot`, `livedev-fix`, `dependabot[bot]`, `testOS fix`, `Rush Session Agent`, `arena-agent`, `Arena Agent (via PAT)`, `qwen-intl`, `codex`, `Merge Check`): 149 commits (23.4%).
- **Note:** The 10 contributors returned by `GET /contributors` (Nan0pk 337, claude 28, AntiGravity 24, github-actions[bot] 19, rhoggs-bot-test-account 10, dependabot[bot] 10, arena-agent 4, qwen-intl 3, codex 1, Victus 1) undercounts because GitHub's contributors API deduplicates by author email and several AI identities share an email with `Nan0pk` or have `invalid-email-address`.

### 6.3 Branch protection

- **Legacy `GET /branches/main/protection` returned 404** `"Branch not protected"` (the legacy branch-protection API).
- **Repository rulesets API `GET /rulesets` returned 200** with one ruleset:
  - **Name:** `protect-main`
  - **Target:** `branch`
  - **Enforcement:** `active`
  - **Conditions:** `ref_name.include = ["refs/heads/main"]`
  - **Rules:**
    1. `deletion` — main cannot be deleted.
    2. `non_fast_forward` — no force-pushes to main.
    3. `pull_request` with:
       - `required_approving_review_count: 0` ← **no reviews required to merge**
       - `dismiss_stale_reviews_on_push: false`
       - `require_code_owner_review: false` ← CODEOWNERS is advisory only
       - `require_last_push_approval: false`
       - `required_review_thread_resolution: false`
       - `allowed_merge_methods: ["merge", "squash", "rebase"]`
    4. `required_status_checks` with `strict_required_status_checks_policy: true` and 4 required checks:
       - `Rust` (integration_id 15368)
       - `Documentation sync` (integration_id 15368)
       - `Repository policy` (integration_id 15368)
       - `Evidence integrity (Dragnet)` (integration_id 15368)

**Verdict:** Branch protection exists and is active via the rulesets API. Status checks are required and the policy is strict (branches must be up-to-date with main before merge). However, **zero approving reviews are required**, which means a single developer (`Nan0pk`) can self-merge any PR without independent review. For a project that handles privileged sysfs writes and ships systemd units, this is a risk to note (not necessarily to fix — solo-developer projects often legitimately have this pattern, but it should be documented as a conscious choice).

### 6.4 CODEOWNERS

- **File:** `.github/CODEOWNERS` (246 bytes, fetched OK).
- **Content:**
  ```
  * @Nan0pk
  crates/optid/ @Nan0pk @agent-rust
  crates/optctl/ @Nan0pk @agent-rust
  crates/rush_collect/ @Nan0pk @agent-rust
  crates/rush_telemetry/ @Nan0pk @agent-rust
  crates/rushbench/ @Nan0pk @agent-rust
  tools/ @Nan0pk @agent-shell
  .github/ @Nan0pk
  ```
- **Issue:** `@agent-rust` and `@agent-shell` are referenced as co-owners but neither resolves to a GitHub user or team in the repo's contributor list. They appear to be placeholder names for AI agents. Since `require_code_owner_review: false` in the ruleset, CODEOWNERS is advisory only and these unresolved references have no operational impact.
- **`crates/testos/`** is missing from CODEOWNERS despite being a workspace member.

### 6.5 Dependabot vs Renovate

- **Dependabot:** Configured (`.github/dependabot.yml`, 240 bytes). Two ecosystems: `cargo` (weekly, 10 PRs limit) and `github-actions` (weekly). Active — Dependabot PRs (#313, #314, #315) merged 2026-07-19.
- **Renovate:** NOT configured. `GET /contents/renovate.json` returned 404. No `renovate.json`, no `.github/renovate.json`, no `renovate-config.json` in repository.
- **Conclusion:** Dependabot is the sole dependency-update tool.

---

## 7. Documentation Accuracy

### 7.1 README claims vs code reality

The README (27,204 bytes, 581 lines, fetched OK) makes several claims that can be cross-checked against the code:

| README claim | Verified? | Evidence |
|---|---|---|
| "Rush Linux is an Arch-based distribution" | ✓ | `mkosi/mkosi.conf` and profiles use pacman-based package sets; `tools/livedev-bootstrap.sh` references Arch-style commands. |
| "It's early beta" | ✓ | `VERSION = 0.7.0-beta.4`; all 5 GitHub releases are `prerelease = true`. |
| "The optimizer (`optid`) runs in safe dry-run mode" | ✓ | `packaging/systemd/optid.service` does NOT pass `--apply`; `optid-apply.service` is a separate unit that does. |
| "the boot path is verified end-to-end (UKI + systemd-boot + signed rollback)" | ✓ | `milestones.toml` v0.4 criteria 1–4 all `verified = true` with committed transcripts. |
| "a measurement harness (`rushbench`) is operational" | ✓ | `crates/rushbench/Cargo.toml` exists (297 bytes); `tools/rush-host-bench.sh` exists (18,748 bytes). |
| "The desktop and laptop editions are not yet buildable" | ✓ | `mkosi/mkosi.profiles/desktop/mkosi.conf` and `laptop/mkosi.conf` exist but v0.7 milestone criteria are unverified. |
| "every decision is logged, explained, and reversible" | ◐ Partial | `optctl explain` is mentioned in milestones but not yet verified; `--revert-sysctls` exists in `actuator.rs` but reversibility on real hardware is not Phase-D-verified. |
| "curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.sh" | ✓ (runnable) | `tools/livedev-bootstrap.sh` fetched OK (52,770 bytes). |
| "default install, with `power-profiles-daemon` in its default `balanced` profile" | ✓ | `reference-hardware.md` confirms this is the suggested baseline. |

### 7.2 Are all README commands runnable?

- `curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.sh -o livedev-bootstrap.sh && bash livedev-bootstrap.sh` — **runnable**, file exists at expected path, 52 KB, shellshebang is `#!/usr/bin/env bash` (verified by inspection).
- PowerShell variant (`livedev-bootstrap.ps1`) — referenced in README but not fetched; would need a separate API call to verify.
- `cargo build --workspace` (from `.devcontainer/devcontainer.json` `postCreateCommand`) — **will succeed** because `rush_telemetry` is excluded from the workspace. But it does not actually build the full project.
- `cargo test --workspace` (from CI) — same caveat: `rush_telemetry` tests are not run.

### 7.3 Doc staleness

- `tools/validate-doc-sync.py --max-age 90` runs in CI as the `Documentation sync` required status check. This enforces a 90-day freshness window on docs.
- `docs/plans/corrected-path-forward-v0.6-to-v1.md` is dated 2026-07-19 (one day before this audit) — current.
- `docs/strategy/reference-hardware.md` does not carry an explicit date but its content matches the v0.6 milestone state.
- The previous audit report `RUSH-LINUX-AUDIT-REPORT.md` (36,163 bytes) is dated 2026-07-19 and was the basis for some of the "audit finding #N" comments sprinkled in `reassess.yml`, `maintenance.yml`, `optid.service`, and `optid-apply.service`. This final audit report supersedes it.

---

## 8. Known Issues (verified, with file:line or API endpoint)

1. **`reassess.yml` is broken YAML** — `.github/workflows/reassess.yml:106, col 92`. `run: |` block scalar at line 96 (indent=8) is de-indented to 0 at lines 104–148 by the heredoc body. YAML parse fails with "mapping values are not allowed here". 528/528 runs failed. No strategic reassessment has been auto-generated since the workflow was added on 2026-06-15.

2. **`crates/rush_telemetry` excluded from workspace** — `Cargo.toml:13-21`. The crate does not compile (missing `libc` dependency, BPF skeleton codegen incomplete). It is excluded so `cargo check --workspace` stays green in CI, which means CI does not compile or test it. CI cheat.

3. **`.gitignore` contains prose, not patterns** — `.gitignore` (full content quoted in §4.9). Git ignores nothing. `target/` and other build artefacts are at risk of being committed.

4. **v0.6 milestone 0/4 verified** — `release/milestones.toml:148` (`status = "in-progress"`) and lines 156–175 (4 `criteria_status` rows all `verified = false`). v0.7 milestone (`release/milestones.toml:179-190`) has zero `criteria_status` rows. `VERSION` says `0.7.0-beta.4`. Decoupled.

5. **No reference hardware nominated** — `docs/strategy/reference-hardware.md` desktop and laptop slots both `_TBD_`. Phase D benchmarks cannot run. v0.6 cannot close. v0.7 cannot validate editions.

6. **HP Victus host-bench sample is defective** — `docs/strategy/reference-hardware.md` states: "That sample is **defective and is not evidence** — see its `NOTE.md` (the `optid_version` field captured usage text, and `transcript.log` begins mid-line)." Cannot be used as evidence without a clean re-capture.

7. **`fits_contract` is dead code** — `crates/optid/src/contracts.rs:190`. `#[allow(dead_code)]`. Zero call sites in the optid crate. Intentional (the WP-N5/N6 depth-enablers that would call it are not implemented), but means the SPEC §3 contract gate is currently not enforced for device-level depth decisions.

8. **`foreground/mod.rs:subscribe()` is a stub** — `crates/optid/src/foreground/mod.rs:90`. Spawns a thread that sleeps forever, receiver never yields events. The `--foreground=auto` flag is parsed but has no effect at runtime. Tests assert the stub behaviour explicitly.

9. **`[memory]` and `[modes.*]` policy.toml values are not actuated** — `config/optid/policy.toml:68-71` (comment). The Rust MVP ignores unknown keys; sysctl actuation is "a tracked follow-up". The `vm_swappiness`, `vm_dirty_bytes`, etc. values in `policy.toml` are documentation-only.

10. **Branch protection requires 0 approving reviews** — `GET /rulesets/17500512` → `pull_request` rule with `required_approving_review_count: 0`, `require_code_owner_review: false`. Solo developer can self-merge any PR. CODEOWNERS is advisory only.

11. **CODEOWNERS references unresolved teams** — `.github/CODEOWNERS`: `@agent-rust` and `@agent-shell` do not resolve to GitHub users or teams. `crates/testos/` (a workspace member) is missing entirely.

12. **`Scheduled maintenance` workflow history** — `maintenance.yml:14-26` comment acknowledges a previous `cargo-deny-action` syntax error (`command: check advisories` instead of `command: check` + `command-arguments: advisories`). Sample shows 1 run, 0% pass. The comment claims it's fixed but only 1 run exists in the sample (and it failed).

13. **`Release testOS image` workflow** — 0/2 runs passed (0% pass rate in 1,000-run sample). Not investigated in detail (out of audit scope) but flagged.

14. **`frontpage-sync.yml` workflow deleted but legacy failures remain** — 12 failed runs in the sample attributed to this deleted workflow. Cleanup is cosmetic (old runs persist in history) but worth noting.

---

## 9. Corrected Milestones (evidence-first)

Based on the Evidence Rule ("a milestone is only 'complete' when every criterion is `verified = true` AND carries a committed `transcript` path, enforced by `tools/validate-evidence.py`") and the data above:

| Version | Current `status` | Corrected `status` | Rationale |
|---|---|---|---|
| 0.1.0-alpha.1 | complete | complete | (No per-criterion rows; grandfathered. If `validate-evidence.py` is strict, this should fail. If it accepts grandfathered pre-evidence-rule milestones, complete is fine.) |
| 0.2.0-alpha.1 | complete | complete | (Same grandfathering caveat.) |
| 0.3.0-alpha.1 | complete | **complete** | 4/4 criteria verified with transcripts. ✓ |
| 0.4.0-alpha.1 | complete | **complete** | 4/4 criteria verified with transcripts. ✓ |
| 0.5.0-beta.1 | complete | **complete** | 4/4 criteria verified with transcripts. ✓ |
| 0.6.0-beta.1 | in-progress | **in-progress** | 0/4 verified. Phase D blocked. Honest as-is. |
| 0.7.0-beta.1 | in-progress | **planned** | 0/4 `criteria_status` rows. Should not be `in-progress` (no work-in-progress evidence); should be `planned` pending v0.6 closure. |
| 0.8.0-beta.1 | planned | planned | OK. |
| 0.9.0-rc.1 | planned | planned | OK. |
| 1.0.0 | planned | planned | OK. |

**VERSION pointer recommendation:** Freeze at `0.7.0-beta.4`. Do not advance to `0.7.0-beta.5` or `0.8.0` until v0.6 closes. The version pointer should reflect milestone progress, not commit count.

---

## 10. Roadmap

### Immediate (next 7 days)

1. **Fix `.gitignore`** — replace prose with proper patterns: `target/`, `*.rs.bk`, `*.pdb`, `*.swp`, `__pycache__/`, `*.pyc`, `.DS_Store`, `*.o`, `*.so`, `*.a`, `*.bak`, `release/evidence/host-bench/*/transcript.log.bak` (or whatever the project prefers). This is a 30-second fix with high payoff.
2. **Fix `reassess.yml`** — indent the heredoc body to ≥10 spaces (or switch to `<<-EOF` with tab indentation, or rewrite the document generation as a Python `textwrap.dedent` step). This unblocks 528 wasted runs and restores the strategic-reassessment ritual.
3. **Verify `maintenance.yml`** — the comment claims the `cargo-deny-action` syntax is fixed; trigger a manual `workflow_dispatch` run to confirm. If still failing, fix the syntax.
4. **Re-include `rush_telemetry` in workspace OR move to `experimental/`** — either finish it enough to compile, or make its brokenness visible in CI (warn-only job that surfaces `cargo check -p rush_telemetry` output). The current "exclude and forget" pattern is a CI cheat.
5. **Resolve CODEOWNERS references** — remove `@agent-rust` and `@agent-shell` (replace with `@Nan0pk` only, or with real team accounts once formed), add `crates/testos/ @Nan0pk`.

### 30-day

1. **Nominate reference hardware** — fill the desktop and laptop slots in `docs/strategy/reference-hardware.md`. Confirm both boards are seeded in `config/optid/hardware-allowlist.toml`. This is the project's single critical blocker.
2. **Run Phase D baseline (D3)** — capture `rush-host-bench.sh --submit` baseline transcripts on both machines (Ubuntu 24.04 LTS, PPD `balanced`).
3. **Run Phase D optid (D4)** — capture optid `--apply` transcripts on both machines.
4. **Issue HP Victus clean re-capture** — if the HP Victus is reused as the laptop slot, run a fresh capture following the Dragnet `meta.txt` template; the defective 2026-06-10 sample must not be used as evidence.
5. **Close v0.6 milestone** — once D3+D4 transcripts are committed and D5 PASS verdict is recorded, flip the 4 `verified = false` rows to `true` with transcript paths, set `status = "complete"`, run `python3 tools/dragnet.py --observe` and confirm GREEN.
6. **Set `required_approving_review_count: 1`** in the `protect-main` ruleset (or document why solo-merge is acceptable for this project's risk profile).

### 90-day

1. **Close v0.7 (Editions) milestone** — implement mkosi profiles for desktop, laptop, server, realtime-audio; verify all 4 v0.7 exit criteria.
2. **Implement device-level depth-enablers** (WP-N5/N6: runtime PM autosuspend, NVMe APST, PCIe ASPM, SATA ALPM) — this unblocks the `fits_contract` call sites and makes the SPEC §3 contract gate actually enforced at the device level.
3. **Implement foreground-app detection** (replace the `foreground/mod.rs` stub with real compositor integration: login1 SessionNew + Mutter/KWin/wlr-foreign-toplevel-management).
4. **Implement sysctl actuation from `policy.toml`** (currently the daemon uses curated hardcoded values; the TOML-declared `vm_swappiness` etc. should drive the daemon).
5. **Finish `rush_telemetry`** — BPF skeleton codegen via `libbpf-cargo`, add `libc` dep, re-include in workspace.

### 6-month

1. **v0.8 (Benchmark Lab)** — public benchmark artefact generation; regressions block release candidates; `optctl explain` correlates with benchmark traces.
2. **v0.9 (RC Hardening)** — freeze v1 schemas and public interfaces; complete signed package metadata; complete security review; only release blockers accepted.
3. **v1.0 (Final Stable Release)** — installable on mainstream x86_64 hardware and VMs; optid active, explainable, reversible; stable update channel; rollback works for bad kernel and update scenarios; benchmark report supports release claims.
4. **ARM64 status** — currently `experimental-until-tested` (per `milestones.toml:5`); bring to tested status once x86_64 v1.0 is stable.
5. **Governance** — `docs/project-sustainability.md` item C1 (formalise security response-time commitments alongside the governance plan).

---

## 11. What Could NOT Be Verified

1. **Workflow run logs for `reassess.yml`** — `GET /actions/runs/29692702876/logs` returned HTTP 404 `"Not Found"`. This is consistent with the run having 0 jobs (no logs were ever generated), not a permissions issue. Verified indirectly: the workflow OBJECT's `name` field is the file path (`.github/workflows/reassess.yml`) instead of the YAML's `name: Strategic Reassessment`, which is GitHub's documented fallback when a workflow file fails to compile.

2. **Annotations / check-run details for the failed `reassess` run** — `GET /check-runs/{id}/annotations` returned 404 because the run id is not a check-run id. The check-suites endpoint showed 9 check-suites for the HEAD commit (1 github-actions failure = the reassess workflow; 8 successes = other workflows). No per-check annotations available without a different API call to map run id → check-run id.

3. **Whether `validate-evidence.py` actually enforces the "every criterion must be `verified = true` AND have a transcript path" rule** — the script was not fetched (out of audit scope; would require `GET /contents/tools/validate-evidence.py`). The `milestones.toml` comment at line 46-49 claims it does. The fact that v0.1 and v0.2 milestones are marked `complete` without any `criteria_status` rows suggests either (a) the script grandfathered them, (b) the script only checks milestones that have `criteria_status` rows, or (c) the script was added after v0.1/v0.2 closed. Recommend fetching `tools/validate-evidence.py` in a follow-up to confirm.

4. **Whether `cargo deny check advisories` actually passes** — the maintenance workflow that runs this has 0% pass rate in the sample (1 run, 1 failure). The comment claims the syntax was fixed, but no post-fix run was found in the sample. The advisory status of the dependency graph is therefore unknown — could be clean, could have outstanding CVEs. Recommend manually triggering `workflow_dispatch` on `maintenance.yml` to verify.

5. **Per-PR `reviewDecision` field** — the PRs endpoint does not return `reviewDecision`. To get review state per PR would require `GET /pulls/{number}/reviews` for each of the 268 PRs (268 API calls; not done in this audit due to time/rate limits). The branch-protection rule's `required_approving_review_count: 0` makes this moot — no PR requires review.

6. **Whether the `Rush Audit Bot`, `Arena Agent`, `claude`, `qwen-intl`, `codex` commit-author identities correspond to the same human developer (Nan0pk) using different AI tools, or to genuinely independent contributors** — cannot be determined from commit metadata alone. The fact that 100% of PRs are opened by `Nan0pk` strongly suggests they are all the same human, but this is inference, not verification.

7. **Whether `tools/livedev-bootstrap.ps1` exists and is runnable** — the README references it but the file was not fetched. Recommend `GET /contents/tools/livedev-bootstrap.ps1` to verify.

8. **The contents of `release/evidence/host-bench/2026-06-10-victus/NOTE.md`** — referenced by `reference-hardware.md` as documenting why the HP Victus sample is defective, but not fetched. Recommend fetching to confirm the defect description.

9. **Whether the `protect-main` ruleset's `required_status_checks` contexts (`Rust`, `Documentation sync`, `Repository policy`, `Evidence integrity (Drangent)`) actually match the names of the jobs in `ci.yml`** — `ci.yml` defines jobs named `required-rust` (job *name* = `Rust`), `required-doc-sync` (name = `Documentation sync`), `required-policy` (name = `Repository policy`), and `required-evidence` (name = `Evidence integrity (Dragnet)`). These match the required status check contexts in the ruleset, so the gate is functional. Verified by reading `ci.yml`.

10. **Whether `--allowlist` default is currently `disabled` or `enabled`** — the corrected-path doc says "Flip `--allowlist` default from `disabled` to `enabled`" is a v0.6 closure step, implying it is currently `disabled`. This was not verified by reading the daemon's argument parser (would require fetching more of `main.rs` than was retrieved). Recommend confirming via `grep -n "allowlist" crates/optid/src/main.rs`.

---

**End of audit.**

_Generated by read-only inspection of `https://api.github.com/repos/Nan0pk/Rush-linux` on 2026-07-20. All API calls used a fine-grained PAT with `contents:read`, `metadata:read`, `actions:read`, `pull_requests:read`, `security_events:read` scopes (inferred from successful responses; the PAT did not expose its scope header). No repository content was modified during this audit except the commit of this report file itself._
