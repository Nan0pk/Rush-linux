# Rush Linux — Work Plan v2 (AI Agents + Human)

| Field | Value |
|---|---|
| Plan version | v2.0 |
| Date | 2026-06-10 |
| Supersedes | `docs/plans/agent-work-plan-v1.md` (mark superseded, do not delete) |
| Repo state audited | `main` @ `6c4926c`; 6 unmerged branches; 0 PRs; 0 CI runs on branches |
| Intended location | `docs/plans/work-plan-v2.md` (register in `docs/docmap.toml`) |

---

## TL;DR

v1 produced working branches but **nothing merged**, one work package **falsely self-certified** (C1's ✅ backed by a syntax check), and B1 failed twice by rewriting instead of moving code. The diagnosis: agents generate; nobody integrates or verifies. v2 therefore adds what v1 lacked — **an explicit role model**. Three roles: **Builder agents** (produce branches), a **Verifier agent** (runs acceptance commands on others' work, never its own), and the **Human** (merges, runs hardware-dependent gates, holds keys). Week 1 is a recovery sprint: three process guardrails + a merge train + a clean B1 restart. Weeks 2–5 execute the carried-forward v1 tracks toward **v0.5.0-alpha.1 "First Evidence"** — the first honest benchmark of `optid` vs power-profiles-daemon.

---

## 1. Status Ledger (carried in from v1 reassessment)

| v1 WP | State | Disposition in v2 |
|---|---|---|
| Plan doc | branch, unmerged | merge in H1 train |
| A1 versions | **done, verified externally** | merge in H1 train |
| A2 graphify | done, post-merge proof pending | merge in H1 train; verify on next `main` push |
| B1 split | failed ×2 (38→15 compile errors; rewrote, not moved; no DIRTY_STATE; stacked branch) | **restart as B1R** |
| C1 rollback | **falsely certified** — `bash -n` passed off as end-to-end proof | **redo as C1R**, human-gated |
| C2 signing | genuinely run (real keys/sigs, 2026-06-10) | split out, merge after evidence-rule compliance |
| B2–B6, D1–D3 | not started | carried forward, specs unchanged from v1 except where noted |

Root causes v2 must fix: (1) CI never runs on branches, so agents get zero feedback; (2) no one is assigned to verify or merge; (3) agents are allowed to grade their own homework.

---

## 2. Role Model and Authority Matrix

### Roles and Evidence Rule

The detailed roles (Builder, Verifier, Human), authority matrix, and the non-negotiable Evidence Rule (introduced during WP-P2) have been extracted to the canonical canon: **[docs/agent-protocol.md](../agent-protocol.md)**.


---

## 3. Week 1 — Recovery Sprint

### WP-P1 (agent) — CI on work branches

**Why:** zero CI runs on six branches is how false certification survived.

**Change `.github/workflows/ci.yml` trigger to:**
```yaml
on:
  push:
    branches: [main, 'wp/**', 'docs/**', 'fix/**']
  pull_request:
    branches: [main]
```
Also add a `cargo audit` job (advisory, non-blocking initially) while in the file.

**Acceptance:** push a trivial commit to a `wp/test-ci` branch; all jobs run; delete the branch. Transcript in PR.

### WP-P2 (agent) — Codify the evidence rule + verifier protocol

Add §2 of this plan into `CONTRIBUTING.md` and `AGENTS.md` (evidence rule, builder/verifier split, PR-only merges). Add `docs/templates/VERIFICATION.md` template: WP id, branch, commit, table of acceptance commands → exit codes → verdicts, environment description, verifier identity/date.

**Acceptance:** `python3 tools/validate-doc-sync.py` passes; template exists; `AGENTS.md` states "builders never certify their own work."

### WP-P3 (agent) — finish-work.sh opens the PR

Extend `tools/finish-work.sh`: after push, if `gh` CLI is available, open a PR with the WP id in the title and the acceptance block in the body; otherwise print the compare-URL and instruct the agent to report it. Branches without PRs are invisible — that was half the integration failure.

**Acceptance:** dry-run mode test; shellcheck clean; doc-sync passes.

### H1 (human, ~45 min) — Merge train

In order, open PR → wait for CI (after P1 lands, branches build automatically) → merge:
1. `docs/add-work-plan-v1` (history: it's superseded but it's the record) + this v2 doc
2. `wp/a1-version-consistency` — already externally verified
3. `wp/a2-graphify-off-main` — then push any trivial commit to `main` and confirm the `graphify-data` branch receives the refresh while `main` stays clean
4. The **signing half** of `wp/c1-rollback-validation`, cherry-picked, only if its evidence README meets the §2 evidence rule (it nearly does — real artifacts exist; have a builder add the command transcript first)

Delete merged and dead branches (`claude/intelligent-tesla-38ciR`, both `wp/b1-*` after B1R starts).

### WP-B1R (agent) — Module split, restarted with a stricter contract

Same target layout as v1 WP-B1. New execution contract that makes the 38-error failure mode impossible:

1. Branch fresh off post-A1 `main`. Never stack on unmerged work.
2. **One module per commit**, in this order: `mode.rs` → `args.rs` → `snapshot.rs` → `decision.rs` → `action.rs` → `actuator.rs` → `policy.rs` → `dbus.rs`.
3. After **every** commit: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` must pass. A commit that doesn't build may not be made.
4. Move code with cut-paste fidelity; the only permitted additions are `mod` declarations, `use` lines, and `pub(crate)` visibility. If a line needs changing to compile, the move order is wrong — stop, revert the commit, reconsider.
5. First compile error you cannot resolve by visibility/`use` alone: stop, write `DIRTY_STATE.md`, end session. Do not accumulate errors.

**Acceptance:** v1 WP-B1 block unchanged (build/test/clippy green, `main.rs` ≤150 lines, dry-run smoke produces status file) **plus** `git log` shows ≥8 commits each individually CI-green.

---

## 4. Weeks 2–5 — Carried-Forward Tracks

Specs live in v1 §§5–7 and remain the contract; only deltas and ownership are listed here.

| Week | WP | Owner | Delta vs v1 |
|---|---|---|---|
| 2 | B2 fixture tests + policy matrix | Builder + Verifier | unchanged |
| 2 | **C1R rollback gate** | **Human-executed**, agent-prepped | see below |
| 3 | B3 actuator safety (allowlist, revert, dwell, SIGHUP) | Builder + Verifier | unchanged; blocks Track 4 |
| 3 | B4 D-Bus hardening (polkit, drop `pin_application`) | Builder + Verifier | manual `busctl` denial check moves to the **human** gate list (needs a real session bus + polkit agent) |
| 4 | B5 optctl truthfulness | Builder + Verifier | unchanged |
| 4 | D1 `rush-bench` harness | Builder + Verifier | unchanged (schbench/stress-ng proxies; CI tests aggregation math only) |
| 5 | D2 four-arm comparison | **Human-executed**, agent-prepped | see below |
| 5 | D3 first benchmark report + 3 data-motivated issues | Builder, human signs off | unchanged |
| — | B6 zbus 3→5 | optional, anytime after B4 | unchanged |

### C1R — Rollback gate, human-executed (agent cannot do this)

The QEMU/OVMF + KVM environment is real hardware territory. Division of labor:

- **Agent preps:** a single runbook `docs/runbooks/c1-rollback-host.md` — exact host requirements (KVM, OVMF paths, disk space), the one command to run, and a results-capture script that tees the full transcript into `release/evidence/v0.4/rollback/run-<date>/` automatically, so the human cannot accidentally produce non-compliant evidence.
- **Human runs (~1–2 h):** `bash tools/test-rollback.sh` on the KVM-capable machine, commits the captured evidence, demotes/promotes the README checkmarks to match reality.
- **Then human-only:** flip `release/milestones.toml` v0.4 to `complete`, update `ROADMAP.md`/`RELEASES.md`/`IMPLEMENTATION_STATUS.md` in the same commit (v1 WP-C2 doc steps).

### D2 — Benchmark runs, human-executed (agent cannot do this)

Real hardware, AC power, exclusive machine use. Agent delivers `rush-bench compare` fully tested on synthetic data plus a runbook; the human runs the four-arm matrix (~2–3 h per machine, one laptop + one desktop) and commits `benchmarks/results/` untouched. The §2 evidence rule applies: raw CSVs and machine.json are the evidence; no summarizing by hand.

---

## 5. Standing Human Duties (the scarce resource, budgeted)

| Duty | Cadence | Est. time |
|---|---|---|
| H1 merge train (one-off) | Week 1 | 45 min |
| Review verifier reports, merge/reject PRs | 2× per week | 30 min each |
| C1R KVM session | Week 2 | 1–2 h |
| B4 polkit denial check | Week 3 | 15 min |
| D2 benchmark sessions | Week 5 | 2–3 h × 2 machines |
| Key custody decisions (production signing) | when raised | — |
| Milestone status changes | per milestone | 10 min |

Total human load: roughly **3–4 h/week**, front-loaded in weeks 1–2 and 5. Everything else is agent work.

---

## 6. Ready-to-Paste Prompts

**Builder — WP-B1R:**
> Read `docs/plans/work-plan-v2.md` §3 WP-B1R and v1 §WP-B1 for the target layout. Branch fresh off current `main`. Execute the split one module per commit in the stated order; after every commit run build+test+clippy and do not proceed on red. Only `mod`/`use`/`pub(crate)` additions are permitted — if a line must change to compile, revert and stop. On any unresolvable error, write `DIRTY_STATE.md` and end the session. Open a PR via `tools/finish-work.sh` when the full acceptance block passes. Do not certify your own work; request verification.

**Verifier — any WP:**
> You are the verifier for PR #N implementing WP-XX. Check out the branch cold. Read the WP's acceptance block in `docs/plans/work-plan-v2.md` (or v1 where referenced). Run every acceptance command verbatim and record literal exit codes and key output. Fill `docs/templates/VERIFICATION.md` and post it to the PR. Do not fix anything; a failure is a verdict. Explicitly state any criterion you could not test in this environment (e.g., requires KVM or a session bus) so the human knows what remains manually gated.

**Builder — C1R runbook prep:**
> Read `docs/plans/work-plan-v2.md` §4 C1R. Produce `docs/runbooks/c1-rollback-host.md` and a transcript-capture wrapper so a human running one command on a KVM host yields evidence compliant with the §2 evidence rule (command, full log, date, host description, auto-saved under `release/evidence/v0.4/rollback/run-<date>/`). Do not run the rollback test yourself; do not edit existing checkmarks. Shellcheck-clean, doc-sync green, PR opened.

---

## 7. Definition of Done — v0.5.0-alpha.1 "First Evidence"

Unchanged from v1 §7, restated as the single convergence point: reproducible `rush-bench` runs on two physical machines; four-arm comparison vs power-profiles-daemon committed with graphs and raw data; published report linked from README — **wins and losses both**; three data-motivated `policy-v2` issues filed; actuator shipping allowlist + revert + dwell; D-Bus surface authorized and stub-free. Plus, new in v2: every one of those criteria carries a §2-compliant evidence transcript, and v0.4 was closed by a real rollback run, not a syntax check.

## 8. Plan Maintenance

Append one ledger line per merged WP below. Material scope change → `work-plan-v3.md`, written **from the D3 benchmark data**.

## 9. WP-B3 Actuator Safety Amendment

### Goal
Incorporate crash-safety guardrails inspired by the sibling `Vigilantune` project, and add an adaptive deadband key to prevent excessive actuator write-churn.

### Amended Steps for WP-B3:
1. **Systemd Watchdog Integration**:
   * Add Unix socket watchdog notification support to `optid` without external dependencies. If `NOTIFY_SOCKET` is present in the environment on start, write a `"WATCHDOG=1"` datagram to the specified socket path via `std::os::unix::net::UnixDatagram` on every tick of the main loop.
   * Update the systemd unit files (`packaging/systemd/optid.service` and `optid-apply.service`) with `WatchdogSec=10` and `NotifyAccess=main`.
2. **ExecStopPost Revert Guardrail**:
   * Add `ExecStopPost=/usr/libexec/optid --revert --state-dir /run/optid` to both systemd service files. This ensures that if the daemon crashes, gets killed, or fails the watchdog ping, systemd will invoke `optid --revert` to clean up and restore all sysctls to their pre-optid boot defaults.
   * Add `/proc/sys/vm` to the `ReadWritePaths` parameter in `optid-apply.service` to permit sysctl restores to execute under systemd's strict security sandbox.
3. **Adaptive Deadband Hysteresis**:
   * Add a `deadband` configuration under the `[policy]` section (and optionally per-mode) in `config/optid/policy.toml`.
   * The deadband key specifies a minimum change threshold (e.g., thermal change < 2°C or CPU pressure change < 3.0) required before the actuator will re-evaluate and write the new values, preventing jitter and write-churn.

## Status Ledger

*(append entries below as WPs merge — format: date, WP, PR#, verifier)*

