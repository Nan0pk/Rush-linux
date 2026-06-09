# Rush Linux — Agent Work Plan v1

| Field | Value |
|---|---|
| Plan version | v1.0 |
| Date | 2026-06-10 |
| Repo state audited | commit `6c4926c`, `VERSION` = 0.4.0-alpha.1 |
| Intended location | `docs/plans/agent-work-plan-v1.md` (add to `docs/docmap.toml`) |
| Supersedes | none |

---

## TL;DR

The repo has excellent governance and a safe `optid` MVP, but three problems block real progress: (1) process noise and version drift, (2) a monolithic, under-tested daemon with an unauthorized D-Bus surface, and (3) **zero benchmark evidence** for the project's entire reason to exist. This plan is organized into **4 tracks and 13 work packages (WPs)**, each sized for a single agent session, each with explicit acceptance commands. Tracks 1–2 (hygiene + refactor) and Track 3 (close the v0.4 gate) can run in parallel. Everything converges on **Track 4: the first reproducible benchmark of `optid` vs power-profiles-daemon** — the single deliverable that justifies the project. Target: `v0.5.0-alpha.1 = "First Evidence"`.

---

## 1. Ground Truth (audit findings driving this plan)

These are verified facts from the repo as of commit `6c4926c`, not opinions:

1. **Version drift.** `VERSION` and `release/milestones.toml` say `0.4.0-alpha.1`; `ROADMAP.md` says `0.3.0-alpha.1`; both crates say `0.1.0`. Nothing validates consistency.
2. **Commit noise.** ~50% of `main` history is `chore: refresh graphify knowledge graph [skip ci]`. Generated artifacts (`graphify-out/`) are committed to the primary branch.
3. **Monolith.** `crates/optid/src/main.rs` is 1,102 lines containing args, modes, snapshot readers, policy, decision, actions, actuator, and the D-Bus server. `optid` has 4 tests; `optctl` has 2.
4. **Untestable readers.** `Snapshot::collect()`, `read_on_ac()`, `read_battery_pct()`, `read_max_thermal_millic()`, and `discover_cpu_epp_paths()` hardcode absolute paths (`/proc/...`, `/sys/...`). Policy decisions cannot be tested against fixture filesystems.
5. **Fake API surface.** `pin_application` on `io.rushlinux.Optid1` is a `println!` stub exposed on the **system bus**. `optctl benchmark` is a placeholder command.
6. **No authorization.** `set_mode` over the system bus has no polkit gating; the bus policy in `packaging/dbus/io.rushlinux.Optid.xml` must be audited and the authorization model documented.
7. **Config re-read every tick.** `Policy::load()` runs every 2 s inside the main loop — wasteful, and a partially written `policy.toml` can be picked up mid-write.
8. **No flap protection.** `decide()` has no hysteresis/dwell time; PSI oscillating around a threshold will cause mode thrashing and repeated sysfs writes.
9. **No revert path.** The actuator never records prior EPP/platform_profile values; stopping `optid-apply.service` leaves the system in whatever state was last written.
10. **v0.4 gate open.** Per `ROADMAP.md`, the remaining v0.4 gate is end-to-end validation via `tools/test-rollback.sh` and `tools/test-sign-updates.sh`.
11. **Zero benchmark data.** `benchmarks/manifest.toml` defines scenarios and competitors; no harness, no results, no report exist. This is the project's stated success criterion.
12. **Stale dependency line.** `zbus 3.14` (current major is 5.x; `dbus_interface` → `interface`, builder APIs renamed). Not urgent, but plan the migration.

---

## 2. Operating Rules for Every Agent Session

These apply to **all** WPs below, on top of the existing `AGENTS.md` mandates:

1. **One WP per session.** Start with `bash tools/start-work.sh "WP-XX: <title>"`, end with `bash tools/finish-work.sh "<conventional commit msg>"`. If interrupted, fill `DIRTY_STATE.md` completely.
2. **Branch + PR per WP**, even working solo: `git checkout -b wp/<id>-<slug>`. CI must be green before merge to `main`. No direct pushes to `main` except docs typo fixes.
3. **Acceptance commands are the contract.** A WP is done only when every command in its *Acceptance* block exits 0. Do not redefine acceptance mid-session; if a criterion is wrong, stop and record it in `DIRTY_STATE.md` for human review.
4. **Docs in the same commit.** Consult `docs/docmap.toml` (`covers_code`), update affected docs, bump `last_verified`, run `python3 tools/validate-doc-sync.py`.
5. **Respect `AI_CONTINUATION.md` Forbidden Shortcuts** verbatim — especially: no competing tuning daemons, no unallowlisted sysfs writes, no opaque ML before deterministic benchmarks.
6. **Diff budget:** target ≤400 changed lines per WP (WP-B1 mechanical split exempt; verify it with tests, not line-by-line review).
7. **Honesty rule for Track 4:** benchmark results are committed **whether optid wins or loses**. A loss is policy-tuning input, not a publishing decision.

---

## 3. Track & Dependency Overview

```text
Track 1 (hygiene)      A1 ──► A2
Track 2 (optid)        B1 ──► B2 ──► B3 ──► B4 ──► B5 ──► [B6 optional]
Track 3 (v0.4 gate)    C1 ──► C2                (independent of T1/T2)
Track 4 (benchmarks)   D1 ──► D2 ──► D3         (needs B2+B3 merged)

Parallelism: T1, T2, T3 may run concurrently (different files).
Convergence: D1 starts only after B3 (hysteresis) is on main,
             otherwise the benchmark measures a flapping daemon.
```

| WP | Title | Track | Size | Blocked by |
|---|---|---|---|---|
| A1 | Version consistency + CI gate | 1 | S | — |
| A2 | Move graphify output off `main` | 1 | S | — |
| B1 | Split `optid` into modules | 2 | M | — |
| B2 | Fixture-testable readers + policy test matrix | 2 | M | B1 |
| B3 | Actuator safety: allowlist, revert, hysteresis | 2 | M | B2 |
| B4 | D-Bus hardening: polkit + drop stub API | 2 | M | B1 |
| B5 | `optctl` truthfulness + JSON contract | 2 | S | B4 |
| B6 | zbus 3 → 5 migration (optional) | 2 | M | B4 |
| C1 | Close rollback gate (`test-rollback.sh`) | 3 | M | — |
| C2 | Close signing gate + declare v0.4 complete | 3 | S | C1, A1 |
| D1 | `rush-bench` harness, one scenario | 4 | L | B3 |
| D2 | A/B matrix vs power-profiles-daemon | 4 | M | D1 |
| D3 | First benchmark report (graphs, honest) | 4 | M | D2 |

Size legend: S ≈ ≤1 h agent session, M ≈ 1–2 sessions, L ≈ 2–3 sessions.

---

## 4. Track 1 — Stabilize the Repo

### WP-A1 — Version consistency + CI gate

**Goal:** one authoritative version, machine-enforced.

**Why:** drift already exists across four files (finding 1) and your own doc-sync philosophy demands this be automated.

**Files:** `VERSION`, `ROADMAP.md`, `Cargo.toml` (workspace), `crates/*/Cargo.toml`, `tools/validate-versions.py` (new), `.github/workflows/ci.yml`, `docs/versioning.md`, `docs/docmap.toml`.

**Steps:**
1. Decide and document the policy in `docs/versioning.md`: *crate versions track the repo version* (recommended — one product, one version). Use `version.workspace = true` in both crates; set `[workspace.package] version = "0.4.0-alpha.1"`.
2. Fix `ROADMAP.md` header to `0.4.0-alpha.1`.
3. Write `tools/validate-versions.py`: asserts `VERSION` == workspace package version == `release/milestones.toml:current_version`, and that `ROADMAP.md`'s "Current project version" line matches. Exit non-zero with a diff-style message on mismatch.
4. Add it as a step in the existing `policy` CI job.

**Acceptance:**
```sh
python3 tools/validate-versions.py
cargo metadata --no-deps --format-version 1 | python3 -c "import json,sys;v={p['version'] for p in json.load(sys.stdin)['packages']};assert v=={open('VERSION').read().strip()},v"
pwsh ./tools/validate-repo.ps1 && python3 tools/validate-doc-sync.py
```

---

### WP-A2 — Move graphify output off `main`

**Goal:** `main` history shows engineering changes only.

**Why:** finding 2; commit noise halves the signal of `git log` for both humans and agents.

**Files:** `.github/workflows/graphify.yml`, `AGENTS.md`, `docs/graphify-knowledge-graph.md`, `.gitignore` or branch config.

**Steps (pick one mechanism, recommend the first):**
1. **Dedicated branch:** the graphify workflow pushes refreshed output to a `graphify-data` branch; `main` keeps only a small pointer file (`graphify-out/REF` containing the branch + commit). Agents that lack the graphify CLI already fall back to `docs/docmap.toml`, so nothing breaks.
2. Alternative: upload as a CI artifact with 90-day retention and document retrieval.
3. Update `AGENTS.md` instructions for where agents fetch the graph; keep the local `./tools/graphify-refresh.sh` flow unchanged for working copies.
4. Do **not** rewrite history; just stop adding noise going forward.

**Acceptance:**
```sh
# After one CI run on a test push:
git log --oneline -10 main | grep -c "graphify" | grep -qx 0
git ls-remote --heads origin graphify-data | grep -q graphify-data
python3 tools/validate-doc-sync.py
```

---

## 5. Track 2 — `optid` Hardening & Refactor

### WP-B1 — Split `optid` into modules (mechanical, zero behavior change)

**Goal:** `main.rs` ≤ 150 lines; everything else in cohesive modules.

**Target layout:**
```text
crates/optid/src/
  main.rs        # arg parse + run() wiring only
  args.rs        # Args
  mode.rs        # Mode (+ Display/parse)
  snapshot.rs    # Pressure, Snapshot, all read_* fns
  policy.rs      # Policy, Thresholds, Modes, ModeConfig, load(), decide(), auto_mode()
  decision.rs    # Decision, render()
  action.rs      # Action enum + constructors + describe()
  actuator.rs    # Actuator, guarded_write, discover_cpu_epp_paths
  dbus.rs        # OptidServer + interface
```

**Rules:** move code verbatim; only add `mod`/`use`/`pub(crate)` visibility. No renames, no logic edits, no formatting "improvements" beyond `cargo fmt`. Existing 4 tests move into their owning modules.

**Acceptance:**
```sh
cargo build --workspace && cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
test "$(wc -l < crates/optid/src/main.rs)" -le 150
# Behavior identical: dry-run once produces a status file
cargo run -p optid -- --once --state-dir /tmp/optid-test && test -s /tmp/optid-test/status
```

**Docs:** `docs/architecture.md` module map; docmap `covers_code` entries split per module.

---

### WP-B2 — Fixture-testable readers + policy test matrix

**Goal:** every policy decision testable against synthetic `/proc` + `/sys` trees; this is also the foundation Track 4 needs for replayable scenarios.

**Steps:**
1. Introduce `struct SysPaths { proc_root: PathBuf, sys_root: PathBuf }` with `Default` = `/proc`, `/sys`. Thread it through `snapshot.rs` readers (constructor injection — no globals, no `#[cfg(test)]` forks of logic).
2. Add `tests/fixtures/` trees: `ac-idle/`, `battery-low/`, `cpu-pressure-high/`, `thermal-hot/`, `psi-missing/` (PSI files absent — must degrade gracefully, not panic).
3. Golden tests for `Policy::decide()` across the matrix: {AC, battery≤20%, battery>20%} × {PSI cpu avg10 low/high} × {thermal below/above trip} × each override `Mode`. Assert mode **and** the rendered reasons (explainability is a product feature — test it).
4. Malformed-input tests: truncated `loadavg`, garbage PSI line, non-UTF8 `policy.toml`, partially written TOML → must fall back to defaults with a logged reason (covers finding 7's failure mode at the parse layer).
5. Add `cargo llvm-cov` (or `tarpaulin`) as a non-blocking CI step that prints coverage; record the baseline number in `docs/testing-and-benchmarks.md`.

**Acceptance:**
```sh
cargo test --workspace            # ≥25 tests total, all green
cargo test -p optid policy::      # matrix tests visible by name
cargo clippy --workspace --all-targets -- -D warnings
```

---

### WP-B3 — Actuator safety: allowlist, revert, hysteresis

**Goal:** the only component allowed to mutate the system becomes boring and predictable.

**Steps:**
1. **Allowlist:** a static table in `actuator.rs` of permitted sysfs path patterns + permitted value sets (EPP values; platform_profile values read from the device's `platform_profile_choices`). `guarded_write` refuses anything off-list and logs `denied:` lines. This satisfies the `AI_CONTINUATION.md` allowlist mandate literally.
2. **Idempotency:** read current value first; skip + log `noop:` if already equal. (Cuts write churn and makes logs honest.)
3. **Revert journal:** before first mutation of a path, record original value to `<state_dir>/revert.json`. Implement restore on (a) SIGTERM/SIGINT shutdown and (b) `optid --revert` subcommand. Document that crash-without-revert is recovered at next start via the journal.
4. **Hysteresis/dwell:** `decide()` gains `min_dwell_sec` (default 10 s, configurable in `policy.toml`) — a computed auto-mode change is only enacted if the candidate mode has been stable for the dwell window. Explicit user `set_mode` bypasses dwell. Add flap test: oscillate PSI around the threshold in fixtures; assert ≤1 mode change per dwell window.
5. Config reload moves to the right design while you're here (finding 7): load once at startup; reload on SIGHUP; validate-then-swap so a broken file never replaces a good in-memory policy.

**Acceptance:**
```sh
cargo test -p optid actuator:: hysteresis::
cargo run -p optid -- --once --state-dir /tmp/o && grep -E "noop:|denied:|write " /tmp/o/actions.log || true
# SIGHUP reload smoke (Linux):
# start daemon w/ temp config, edit config, kill -HUP, assert decisions.log notes "policy reloaded"
```

**Docs:** `docs/adaptive-engine.md` (dwell + revert semantics), new ADR `docs/decisions/adr-XXXX-actuator-safety.md`.

---

### WP-B4 — D-Bus hardening: authorization + no fake surface

**Goal:** the system-bus API is minimal, truthful, and authorized.

**Steps:**
1. **Remove `pin_application`** from `io.rushlinux.Optid1` and from `packaging/dbus/io.rushlinux.Optid.xml` until it has a real implementation. Roadmap it for v0.6; do not ship promise-ware on a privileged bus.
2. **Bus policy:** audit/author `packaging/dbus/` system policy so only root owns `io.rushlinux.Optid`; method calls allowed for `root` and group `optid-admin`; properties readable by everyone (status is not secret).
3. **Polkit:** add `packaging/polkit/io.rushlinux.optid.policy` with action `io.rushlinux.optid.set-mode` (`auth_admin_keep` for inactive sessions, `yes` for active local admin — match power-profiles-daemon's posture so desktop integration is familiar). Check authorization in `set_mode` via `zbus_polkit` or a direct call to `org.freedesktop.PolicyKit1.Authority`.
4. Interface version discipline: document in `docs/architecture.md` that `Optid1` is frozen except additive changes; breaking changes mean `Optid2`.

**Acceptance:**
```sh
cargo build --workspace && cargo test --workspace
grep -q pin_application crates/optid/src/dbus.rs && exit 1 || true
test -f packaging/polkit/io.rushlinux.optid.policy
# On a Linux host with a session: busctl call as non-root w/o polkit auth must fail with AccessDenied (manual gate, record in PR)
```

---

### WP-B5 — `optctl` truthfulness + JSON contract

**Goal:** every advertised command does something real; machine output is stable.

**Steps:**
1. Remove `optctl benchmark` from the CLI (Track 4's harness is the benchmark entry point), or re-point it to exec `rush-bench` once D1 lands. No placeholders.
2. Define the `--json` schema in `docs/architecture.md` (fields, types, stability promise) and add a snapshot test that parses real output against it.
3. Clear UX for D-Bus failure: distinguish "daemon not running" vs "permission denied" vs "fell back to state dir" — one line each, actionable.

**Acceptance:**
```sh
cargo test -p optctl
cargo run -p optctl -- benchmark 2>&1 | grep -qi "removed\|rush-bench" || ! cargo run -p optctl -- benchmark
```

---

### WP-B6 (optional) — zbus 3 → 5 migration

Do this only after B4; it's an isolated dependency bump. `dbus_interface` → `interface`, `ConnectionBuilder` → `connection::Builder`, blocking module paths change. Acceptance = full test suite + manual `busctl introspect` parity diff. Skip if Track 4 is resource-constrained — 3.14 works.

---

## 6. Track 3 — Close the v0.4 Gate

### WP-C1 — Rollback validation end-to-end

**Goal:** `tools/test-rollback.sh` passes and its evidence is recorded.

**Steps:**
1. Run `tools/test-rollback.sh` in the canonical Linux environment (per README: QEMU/OVMF path). Fix script or product issues it surfaces — script fixes are in-scope; product fixes >400 lines get their own WP.
2. Capture the transcript/log artifact under `release/evidence/v0.4/rollback/` (new convention: every milestone gate stores its proof).
3. If the test cannot run in GitHub CI (KVM limits), document it as a **T2 manual gate** in `docs/release-checklist.md` with the exact host requirements, and add a CI job that at least shellchecks + dry-runs the script's argument parsing.

**Acceptance:**
```sh
bash tools/test-rollback.sh            # exit 0 on the canonical host
test -d release/evidence/v0.4/rollback
shellcheck tools/test-rollback.sh tools/manage-boot-entries.sh
```

### WP-C2 — Signing validation + declare v0.4 complete

**Steps:** run `tools/test-sign-updates.sh`; store evidence under `release/evidence/v0.4/signing/`; document test-key vs production-key handling boundaries in `SECURITY.md` (test keys never sign release artifacts); then update `ROADMAP.md`, `RELEASES.md`, `release/milestones.toml` (`status = "complete"` with date), and `IMPLEMENTATION_STATUS.md` — in the same commit, per doc policy. Requires A1 merged so version updates don't reintroduce drift.

**Acceptance:**
```sh
bash tools/test-sign-updates.sh
python3 tools/validate-versions.py && python3 tools/validate-doc-sync.py
grep -A2 '0.4.0-alpha.1' release/milestones.toml | grep -q 'complete'
```

---

## 7. Track 4 — The Proof (flagship: v0.5.0-alpha.1 "First Evidence")

> This track is the project's existential milestone. Per `PROJECT_BRIEF.md`, Rush Linux "succeeds only if it can prove better foreground latency… competitive battery… benchmarked." Today there is zero evidence. One honest, reproducible graph is worth more than every remaining doc in the repo.

### WP-D1 — `rush-bench` harness, one scenario, no GUI

**Goal:** a runner that executes **one** scenario from `benchmarks/manifest.toml` reproducibly on stock hardware.

**Design constraints:**
- Python 3.11+, stdlib + `matplotlib` only (matches `rush-builder.py` philosophy). Lives at `tools/rush-bench/` with a `__main__.py`.
- **Scope the first scenario down.** `mixed-load-responsiveness` as written ("input-latency-p95-ms", browser workload) requires GUI instrumentation — too hard first. Implement measurable proxies:
  - **Foreground latency proxy:** `schbench` p95/p99 wakeup latency (or `cyclictest` if RT tooling is present) for a foreground-slice task,
  - while **background load** runs: `stress-ng --cpu N --io 2` + a `make -j$(nproc)` kernel-dir build or `fio` job, placed in a background slice,
  - **PSI tracking:** sample `/proc/pressure/*` avg10 each second to CSV (reuses the fixture-tested reader semantics from B2).
- Update `benchmarks/manifest.toml` to add these proxy metrics explicitly — keep the original aspirational metrics as a later tier, documented as such. No silent redefinition.
- Output: `benchmarks/results/<date>-<host-id>/<scenario>/runN.csv` + `summary.json` (median, p95, stddev across runs) + `machine.json` (CPU model, kernel, governor, distro, AC state — auto-collected).
- Determinism hygiene: pin run count, warmup run discarded, require AC power, refuse to run if another tuning daemon (ppd/TLP/TuneD) is active *unless* it's the declared competitor under test.

**Acceptance:**
```sh
python3 -m tools.rush-bench run mixed-load-proxy --runs 5 --out benchmarks/results/
test -f benchmarks/results/*/mixed-load-proxy/summary.json
python3 -m tools.rush-bench --help
python3 tools/validate-doc-sync.py   # docs/testing-and-benchmarks.md updated
```

### WP-D2 — A/B matrix vs power-profiles-daemon

**Goal:** same machine, same workload, four arms, N≥5 runs each:

| Arm | Configuration |
|---|---|
| baseline | no tuning daemon, kernel defaults |
| ppd-balanced | power-profiles-daemon, balanced |
| ppd-performance | power-profiles-daemon, performance |
| optid-apply | `optid` with `--apply`, auto mode |

**Steps:** `rush-bench compare` subcommand orchestrates arms (start/stop services between arms, settle delay, verify the right daemon owns the knobs by reading EPP back), aggregates into one `comparison.json`, and renders `comparison.png` (grouped bars: p95/p99 latency per arm, PSI overlay). Run on at least one real laptop and one desktop; commit both result sets.

**Acceptance:**
```sh
python3 -m tools.rush-bench compare mixed-load-proxy --arms baseline,ppd-balanced,ppd-performance,optid-apply --runs 5
test -f benchmarks/results/*/comparison.png
```

### WP-D3 — First benchmark report

**Goal:** `docs/benchmarks/2026-MM-first-evidence.md` — methodology, machine manifests, graphs, results table, and an explicit verdict section: where `optid` wins, where it loses, and the top three policy changes the data motivates. Cross-link from `README.md` ("Current Implementation Status"). If `optid` loses an arm, the report says so plainly and files the follow-up issues — that is the v0.5 backlog, derived from data instead of intuition.

**Acceptance:** report merged; README links it; `python3 tools/validate-doc-sync.py` passes; three GitHub issues created and labeled `policy-v2`, each citing a specific measurement.

**v0.5.0-alpha.1 exit criteria (add to `release/milestones.toml`):**
```toml
[[milestone]]
version = "0.5.0-alpha.1"
channel = "alpha"
name = "First Evidence"
required_tiers = ["T0", "T1", "T3-bench"]
exit_criteria = [
  "rush-bench reproduces mixed-load-proxy on two machines",
  "four-arm comparison vs power-profiles-daemon committed with graphs",
  "benchmark report published and linked from README",
  "three data-motivated policy-v2 issues filed",
  "actuator has allowlist, revert journal, and dwell (WP-B3)",
  "D-Bus surface is authorized and stub-free (WP-B4)",
]
```

---

## 8. Ready-to-Paste Agent Prompts

Use these as the opening message of a Claude Code session (adjust paths if you relocate the plan). Each assumes the session-lifecycle scripts run as mandated.

**Prompt — WP-B1 (module split):**
> Read `AGENTS.md`, `AI_CONTINUATION.md`, and `docs/plans/agent-work-plan-v1.md` section WP-B1. Execute WP-B1 exactly: mechanically split `crates/optid/src/main.rs` into the listed modules with zero behavior change — move code verbatim, add only `mod`/`use`/visibility. Do not rename, refactor, or "improve" anything. Run the WP-B1 acceptance block; all commands must pass. Update `docs/architecture.md` and `docs/docmap.toml` per the doc policy. Use `tools/start-work.sh` and `tools/finish-work.sh`. If any acceptance command cannot pass, stop and write `DIRTY_STATE.md` instead of redefining the goal.

**Prompt — WP-B3 (actuator safety):**
> Read `docs/plans/agent-work-plan-v1.md` section WP-B3 plus `docs/adaptive-engine.md` and `crates/optid/src/{actuator,policy}.rs`. Implement, in order: sysfs allowlist with `denied:` logging, idempotent writes with `noop:` logging, revert journal with restore-on-shutdown and `--revert`, `min_dwell_sec` hysteresis in `decide()` with a flap test, and SIGHUP validate-then-swap config reload. Write tests first for the hysteresis and revert behavior using the WP-B2 fixture infrastructure. All WP-B3 acceptance commands must pass. Add the ADR. Same-commit doc updates per docmap.

**Prompt — WP-D1 (benchmark harness):**
> Read `docs/plans/agent-work-plan-v1.md` Track 4 and `benchmarks/manifest.toml`. Build `tools/rush-bench/` per WP-D1: Python stdlib + matplotlib, `run` subcommand, mixed-load-proxy scenario (schbench foreground + stress-ng/fio background + PSI sampling), CSV + summary.json + machine.json outputs, warmup-discard, AC-power check, competing-daemon refusal. Add the proxy metrics to `manifest.toml` without deleting the aspirational ones. Unit-test the aggregation math with canned CSVs (no benchmarks in CI). Update `docs/testing-and-benchmarks.md`. All WP-D1 acceptance commands must pass.

---

## 9. Plan Maintenance

1. Commit this file at `docs/plans/agent-work-plan-v1.md`; register it in `docs/docmap.toml` with `covers_code = []` (it covers process, not code) and today's `last_verified`.
2. Track WP status by appending a one-line ledger to the bottom of this file per merge: `2026-06-XX WP-A1 done (#PR)`. When scope changes materially, write `agent-work-plan-v2.md` and mark this one superseded — do not edit history in place.
3. After WP-D3, the v0.6 plan should be written **from the benchmark data**, not from this document.

## Status Ledger

*(append entries below as WPs merge)*
