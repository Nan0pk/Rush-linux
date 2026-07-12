# LiveDev Transition Plan

> **Status:** proposed (binding on LiveDev work once ADR 0018 is ratified).
> **Source ADR:** `docs/decisions/0018-rush-livedev-architecture-contract.md`
> **Companion files:**
> - `docs/automation-human-interface.md`
> - `docs/ai-interface-policy.md`
> - `docs/plans/livedev-progress.json`
> - `docs/plans/livedev-workspace-enablement.md`
> **Captured at (UTC):** 2026-07-04T09:30:00Z

This document defines the **exact future phase sequence** to bring Rush
LiveDev from a docs-only track to a working E2E dry run. Each phase is its
own PR; each phase must run `tools/start-work.sh` /
`tools/finish-work.sh`; each phase must respect
`docs/automation-human-interface.md` and `docs/ai-interface-policy.md`.

The sequence is **fixed**: no phase may be skipped, no phase may be
reordered, and no phase may begin until the previous phase is merged to
`main` and the `livedev-progress.json` `current_phase` is updated. This is
load-bearing — the phases have prerequisites that are not obvious from
their names, and reordering them produces broken or unsafe surfaces.

---

## Phase sequence (canonical)

```
1. optid-safety
2. rush-exec / rush-capture
3. evidence schema / validator
4. autopilot planner
5. plan runner / testOS transition
6. AI harness
7. LiveDev image / profile
8. PR submission / CI
9. E2E dry run
```

Each phase is described below with: **goal**, **entry condition**, **exit
condition**, **deliverables**, **non-deliverables**, and **safety notes**.

---

## Phase 1 — optid-safety

**Goal:** Harden the `optid` write-site boundary and the evidence gate so
that the eventual LiveDev automation can collect evidence without
amplifying existing risks. This phase is **`optid`-scoped, not
LiveDev-scoped**: it closes audit findings that pre-date LiveDev and that
LiveDev will eventually depend on being closed.

**Entry condition:** ADR 0018 ratified by the maintainer;
`docs/plans/livedev-progress.json` `current_phase = "architecture-contract"`.

**Exit condition:**

- `crates/optid/src/policy.rs::Policy::load` fails **closed** on parse
  error (audit #1, Critical). Invalid TOML returns an error rather than a
  default policy. The `--apply` mode refuses to start if the policy is
  malformed.
- The `optid` revert path (`revert_sysctls`, `revert_pm_qos`) is tested
  end-to-end (audit #3). A test injects a sysctl/PM QoS change, signals
  `optid` to revert, and asserts the original values are restored. The
  test runs in CI.
- `tools/validate-evidence.py` validates **content**, not just existence
  (audit #4). Required markers (literal command, literal output, date,
  host) are checked; a transcript that is empty or that contains only a
  command (no output) fails validation. Test fixtures cover the pass and
  fail cases.
- `tools/finish-work.sh` is reconciled with `.github/workflows/ci.yml`
  (audit #15). The two run the same commands (`cargo nextest run
  --workspace`, not `cargo test --workspace`; same clippy flags; same
  validator set).

**Deliverables:**

- Patch to `crates/optid/src/policy.rs` (fail-closed `Policy::load`).
- Patch to `crates/optid/src/main.rs` (refuse `--apply` on malformed
  policy).
- New test `crates/optid/tests/revert_path.rs` (end-to-end revert).
- Patch to `tools/validate-evidence.py` (content-aware validation).
- New test fixtures under `tools/test-fixtures/evidence/` (pass and fail).
- Patch to `tools/finish-work.sh` (CI parity).
- Update to `docs/IMPLEMENTATION_STATUS.md` (note the closures).
- Update to `docs/docmap.toml` (`last_verified` bumps).

**Non-deliverables:**

- No LiveDev crate. No `crates/livedev*`.
- No new ADR (this phase closes audit findings, not architectural
  decisions).
- No change to `release/milestones.toml` (the v0.6 Phase D criteria remain
  `verified = false`; this phase hardens the gate, it does not close the
  gate).

**Safety notes:**

- The fail-closed `Policy::load` change is **breaking** for any deployment
  that relied on the silent default. The change is gated behind a
  deprecation cycle: a warning is logged for one release, then the error
  becomes fatal. The deprecation cycle is documented in the PR.
- The revert-path test runs `optid` with `--apply` against fixture sysfs
  paths, not real sysfs. The test does not mutate the host.

---

## Phase 2 — rush-exec / rush-capture

**Goal:** Build the two primitives that LiveDev's automation will use to
**execute** plans and **capture** their outputs: `rush-exec` (a guarded
command runner that logs every command, its exit code, and its output) and
`rush-capture` (a transcript writer that produces evidence-shaped files
from `rush-exec` logs).

**Entry condition:** Phase 1 merged to `main`;
`docs/plans/livedev-progress.json` `current_phase = "optid-safety"`.

**Exit condition:**

- `crates/rush_exec` exists and is in the workspace. It exposes a single
  function: `run(command: CommandSpec) -> Result<ExecutionRecord>` where
  `CommandSpec` is a typed command (no shell string) and `ExecutionRecord`
  is `{ command, exit_code, stdout, stderr, duration_ms, started_at,
  ended_at, host }`.
- `crates/rush_capture` exists and is in the workspace. It exposes a
  single function: `write_transcript(record: ExecutionRecord, path: Path)
  -> Result<()>` that writes a transcript file in the format
  `tools/validate-evidence.py` (Phase 1) accepts.
- Both crates have unit tests with ≥90% line coverage.
- Both crates have integration tests that round-trip a real command (e.g.,
  `echo hello`) through `rush_exec::run` and `rush_capture::write_transcript`
  and pass the Phase 1 validator.
- Neither crate depends on `crates/rush_telemetry` (ADR 0017 not yet
  ratified).

**Deliverables:**

- `crates/rush_exec/{Cargo.toml,src/lib.rs,src/main.rs,tests/}`.
- `crates/rush_capture/{Cargo.toml,src/lib.rs,src/main.rs,tests/}`.
- Root `Cargo.toml` `members` extended.
- `docs/docmap.toml` entries for both crates.
- `docs/decisions/0019-rush-exec-rush-capture.md` (new ADR, `proposed`):
  scopes the two crates, their interface, and their non-goals (they are
  not a shell; they are not a workflow engine; they do not call AI).

**Non-deliverables:**

- No LiveDev crate yet. `rush-exec` and `rush-capture` are general
  primitives; LiveDev is their first consumer, not their only consumer.
- No autopilot planner (Phase 4). `rush-exec` is invoked by hand or by a
  test in this phase.
- No AI calls. `rush-exec` runs commands; it does not consult a model.

**Safety notes:**

- `rush_exec::run` accepts a `CommandSpec` (a typed command with argv
  array), **not** a shell string. This is the same discipline as
  `crates/optid/src/io_util.rs::guarded_write`: no string interpolation
  into a shell. A `CommandSpec` that tries to invoke `sh -c "..."` is
  rejected at parse time.
- `rush_exec::run` logs every execution to an append-only log at
  `/run/rush/exec.log` (or `./exec.log` for a non-LiveDev session). The
  log is the audit trail for §4 of `docs/automation-human-interface.md`.
- `rush_capture::write_transcript` writes only to the path the caller
  specified; it does not infer a path. It does not write to
  `release/evidence/` unless the caller explicitly points it there.

---

## Phase 3 — evidence schema / validator

**Goal:** Replace the ad-hoc transcript format with a typed schema and
extend the Phase 1 validator to enforce it. This phase makes evidence
**machine-readable**, which the autopilot planner (Phase 4) and the AI
harness (Phase 6) need.

**Entry condition:** Phase 2 merged to `main`;
`docs/plans/livedev-progress.json` `current_phase = "rush-exec-rush-capture"`.

**Exit condition:**

- `docs/evidence-schema.md` exists and defines the transcript schema:
  required fields (`command`, `exit_code`, `stdout`, `stderr`,
  `started_at`, `ended_at`, `host`, `host_arch`, `kernel_version`), field
  types, and the failure modes the validator rejects.
- `crates/rush_capture` writes transcripts in the schema (Phase 2's
  format is updated to match; the migration is a no-op for callers).
- `tools/validate-evidence.py` validates against the schema. A transcript
  that is well-formed JSON but missing a required field fails. A
  transcript with a type mismatch (`exit_code: "0"` instead of `0`) fails.
- The validator runs in CI (`.github/workflows/ci.yml` `evidence` job) on
  every PR that touches `release/evidence/`.
- Existing transcripts under `release/evidence/v0.{3,4,5}*/` pass the new
  validator (or, where they don't, the failure is documented and the
  transcript is patched to match the schema — the schema is the source of
  truth, not the historical transcripts).

**Deliverables:**

- `docs/evidence-schema.md` (new).
- Patch to `crates/rush_capture` (schema-conformant output).
- Patch to `tools/validate-evidence.py` (schema validation).
- New test fixtures under `tools/test-fixtures/evidence-schema/` (pass,
  missing-field, type-mismatch, empty-output).
- Patch to `.github/workflows/ci.yml` (run validator on evidence-touching
  PRs).
- Patch to `docs/docmap.toml` (entry for `docs/evidence-schema.md`).

**Non-deliverables:**

- No change to `release/milestones.toml`. Existing `verified = true` flags
  remain; the validator strengthens the gate, it does not re-open
  previously closed criteria.
- No AI calls. The validator is deterministic code.

**Safety notes:**

- Patching historical transcripts to match the schema is a **destructive
  action** (`docs/automation-human-interface.md` §2.4) because it modifies
  evidence. The patches are made transparently: the PR description lists
  every transcript modified, the diff is human-readable, and the
  Verifier role must approve. The Dragnet ledger
  (`release/evidence/dragnet/LEDGER.md`) is updated to record the
  migration.
- The schema is **additive**: future fields can be added without breaking
  old validators. Required fields cannot be removed without a major
  version bump to the schema (documented in `docs/evidence-schema.md`).

---

## Phase 4 — autopilot planner

**Goal:** Build the component that decides **what to do next**. The
planner reads `docs/plans/livedev-progress.json`, the repo state, the
evidence tree, and CI status, and produces a typed plan: a sequence of
`rush_exec::CommandSpec` invocations with a goal, a default, and a
rollback. The planner is **deterministic** in this phase; AI consultation
arrives in Phase 6.

**Entry condition:** Phase 3 merged to `main`;
`docs/plans/livedev-progress.json` `current_phase = "evidence-schema-validator"`.

**Exit condition:**

- `crates/rush_planner` exists and is in the workspace. It exposes:
  `plan(goal: Goal, context: Context) -> Result<Plan>` where `Goal` is a
  typed goal (e.g., `RunBenchmark { scenario: "mixed-load-001" }`) and
  `Context` is the repo + evidence + CI state.
- `Plan` is a typed sequence of `PlanStep`s, each with a `CommandSpec`, a
  `default` (`proceed` / `skip` / `ask` / `abort`), a `reason`, and a
  `rollback`.
- The planner is deterministic: the same `Goal` + `Context` produces the
  same `Plan`. No AI calls.
- The planner is testable: a `Context` can be constructed from fixtures,
  so the planner's output can be asserted in CI.
- The planner **never** produces a `PlanStep` whose `default` is
  `proceed` for a destructive or final-approval action
  (`docs/automation-human-interface.md` §2.4, §2.5). This is enforced by a
  test.

**Deliverables:**

- `crates/rush_planner/{Cargo.toml,src/lib.rs,src/main.rs,tests/}`.
- Root `Cargo.toml` `members` extended.
- `docs/docmap.toml` entry for `crates/rush_planner`.
- `docs/decisions/0020-rush-planner.md` (new ADR, `proposed`): scopes the
  planner, its determinism rule, and its non-goals (it does not execute
  plans; it does not call AI; it does not modify release truth).

**Non-deliverables:**

- The planner does not execute plans (Phase 5 does).
- The planner does not call AI (Phase 6 adds optional AI consultation).
- The planner does not modify `release/milestones.toml` or any release
  truth.

**Safety notes:**

- The planner's determinism rule is load-bearing: it is the difference
  between a planner that can be tested and a planner that cannot. A
  future PR that introduces non-determinism (e.g., "pick a random
  benchmark to run") must do so behind an explicit `--randomize` flag,
  not by default.
- The planner's `default` field is the **safety floor**. A `PlanStep`
  with `default: proceed` for a destructive action is a release blocker.
  The test that enforces this is in `crates/rush_planner/tests/safety.rs`.

---

## Phase 5 — plan runner / testOS transition

**Goal:** Build the component that **executes** a `Plan` produced by the
planner, using `rush_exec::run` and `rush_capture::write_transcript`. This
phase also defines the **testOS transition**: how a LiveDev plan runner
relates to the existing testOS workflow, and what it takes for the plan
runner to eventually subsume testOS's single-shot role.

**Entry condition:** Phase 4 merged to `main`;
`docs/plans/livedev-progress.json` `current_phase = "autopilot-planner"`.

**Exit condition:**

- `crates/rush_runner` exists and is in the workspace. It exposes:
  `run_plan(plan: Plan) -> Result<RunRecord>` where `RunRecord` is a
  sequence of `ExecutionRecord`s plus a per-step outcome (`proceed`,
  `skipped`, `asked`, `aborted`, `failed`).
- The runner **respects** the `default` field on each `PlanStep`. For
  `default: proceed`, the runner executes the step. For `default: ask`,
  the runner emits a prompt (`docs/automation-human-interface.md` §4) and
  waits. For `default: abort`, the runner stops the plan and rolls back.
- The runner **logs** every prompt's reason/default/outcome to
  `/run/rush/prompts.log`.
- The runner **does not** modify `release/milestones.toml` or any release
  truth.
- `docs/plans/testos-transition.md` exists and defines: (a) the
  conditions under which the runner can drive a testOS-style benchmark
  session, (b) the conditions under which the runner can drive a LiveDev
  session (Phase 7+), (c) the explicit boundary — the runner does not
  delete or deprecate testOS; testOS remains the default until a
  follow-up ADR declares otherwise.
- `testos/README.md` is **not modified** in this phase. testOS continues
  to ship on every `v*` tag.

**Deliverables:**

- `crates/rush_runner/{Cargo.toml,src/lib.rs,src/main.rs,tests/}`.
- Root `Cargo.toml` `members` extended.
- `docs/docmap.toml` entry for `crates/rush_runner`.
- `docs/plans/testos-transition.md` (new).
- `docs/decisions/0021-rush-runner.md` (new ADR, `proposed`): scopes the
  runner, its prompt contract, and its relationship to testOS.

**Non-deliverables:**

- The runner does not call AI (Phase 6).
- The runner does not boot a LiveDev image (Phase 7).
- The runner does not open PRs (Phase 8).
- testOS is not modified, deprecated, or renamed.

**Safety notes:**

- The runner's prompt contract is the load-bearing safety surface. A
  runner that executes a `default: ask` step without prompting is a
  release blocker. The test that enforces this is in
  `crates/rush_runner/tests/prompt_contract.rs`.
- The runner's rollback path must be tested. If a step fails, the runner
  must execute the `rollback` of every previously-succeeded step in
  reverse order. This is the same discipline as `optid`'s revert path
  (Phase 1, audit #3).

---

## Phase 6 — AI harness

**Goal:** Build the harness that calls online AI providers for plan
synthesis, code review, and evidence summarization — under the contract in
`docs/ai-interface-policy.md`. This phase makes AI a **first-class but
bounded** surface in the LiveDev track.

**Entry condition:** Phase 5 merged to `main`;
`docs/plans/livedev-progress.json` `current_phase = "plan-runner-testos-transition"`.

**Exit condition:**

- `crates/rush_ai` exists and is in the workspace. It exposes the harness
  CLI specified in `docs/ai-interface-policy.md` §1:
  `rush-ai-harness --provider <name> --model <name> --prompt-file <path>
  --response-file <path> --budget-tokens <n> --budget-usd <n> --mock <bool>
  --log-file <path>`.
- The harness supports `--mock true` (§3 of the policy) and ships with
  fixture responses under `crates/rush_ai/tests/fixtures/`.
- The harness supports at least one real provider (the ratified provider
  list in `docs/ai-interface-policy.md` §2.1 is updated to include it).
- The harness **never** calls `system()` / `exec()` / `subprocess.run()`
  (policy §6). The model's text output is written only to
  `--response-file`.
- The harness's budget tracking is tested: a call that would exceed the
  monthly cap is refused; the cap state is persisted at
  `~/.config/rush/ai-budget.json`.
- `docs/ai-interface-policy.md` §2.1 is updated with the ratified
  provider, model, budget cap, and the ADR that ratified it.

**Deliverables:**

- `crates/rush_ai/{Cargo.toml,src/lib.rs,src/main.rs,tests/,tests/fixtures/}`.
- Root `Cargo.toml` `members` extended.
- `docs/docmap.toml` entry for `crates/rush_ai`.
- `docs/decisions/0022-rush-ai-harness.md` (new ADR, `proposed`): ratifies
  the first provider, the budget cap, the mock contract, and the
  non-execution rule.

**Non-deliverables:**

- The harness does not modify the planner (Phase 4). The planner remains
  deterministic. AI consultation is an **optional caller** of the harness,
  not a planner integration.
- The harness does not merge PRs, mark evidence verified, or modify
  release truth (policy §§7, 8).
- The harness does not run during benchmark sessions (policy §4).

**Safety notes:**

- Provider credentials are read from `~/.config/rush/secrets.env`
  (`docs/automation-human-interface.md` §2.2). The harness does not log,
  echo, or commit them.
- The harness's `--mock` flag is the test floor. A test that calls a real
  provider is a release blocker (policy §3).
- The harness's budget tracking is the money-action floor. A call that
  exceeds the cap is refused; the human is prompted (policy §2.3).

---

## Phase 7 — LiveDev image / profile

**Goal:** Build the bootable LiveDev image. This is the first phase that
produces a LiveDev-named artifact. Until this phase, LiveDev has been a
collection of crates (`rush_exec`, `rush_capture`, `rush_planner`,
`rush_runner`, `rush_ai`) that run on any Linux host; this phase makes
LiveDev bootable.

**Entry condition:** Phase 6 merged to `main`;
`docs/plans/livedev-progress.json` `current_phase = "ai-harness"`.

**Exit condition:**

- `mkosi/mkosi.profiles/livedev/mkosi.conf` exists. It extends the base
  `mkosi/mkosi.conf` with: the `rush_exec`, `rush_capture`, `rush_planner`,
  `rush_runner`, and `rush_ai` binaries; `git`; the GitHub CLI `gh`; the
  network tools needed to sync with the repo; and nothing else (no
  desktop, no audio, no games).
- `distro/editions/livedev.toml` exists. It is the edition descriptor for
  LiveDev, mirroring the existing `server.toml` / `desktop.toml` /
  `laptop.toml` / `realtime-audio.toml` shape.
- `tools/build-mkosi-image.sh --edition livedev` produces a bootable
  `.raw` image. The image boots to `multi-user.target` with `optid.service`
  active and the LiveDev binaries on `PATH`.
- The image is **read-only on the host disk by default**
  (`docs/decisions/0018-rush-livedev-architecture-contract.md` §6.3). An
  explicit `--mutate-host-disk` flag is required for any host-disk write.
- `testos/` is **not modified**. testOS continues to ship on every `v*`
  tag. LiveDev is a parallel image, not a replacement.

**Deliverables:**

- `mkosi/mkosi.profiles/livedev/mkosi.conf` (new).
- `distro/editions/livedev.toml` (new).
- Patch to `tools/build-mkosi-image.sh` (add `--edition livedev`).
- `docs/editions/livedev.md` (new): describes the LiveDev edition, its
  package set, its default boot behavior, and its `--mutate-host-disk`
  flag.
- `docs/docmap.toml` entries for the new files.
- `docs/decisions/0023-livedev-image.md` (new ADR, `proposed`): ratifies
  the LiveDev image composition, its read-only default, and its
  relationship to testOS.

**Non-deliverables:**

- LiveDev does not replace testOS in the README. The README's "Try it on
  real hardware" section continues to point at testOS until a follow-up
  ADR declares LiveDev the default.
- LiveDev does not call AI on boot. The `rush_ai` harness is present on
  the image but is not invoked automatically.
- LiveDev does not open PRs automatically. The `gh` CLI is present on the
  image but is not invoked automatically (Phase 8).

**Safety notes:**

- The `--mutate-host-disk` flag is a **destructive action** under
  `docs/automation-human-interface.md` §2.4. Setting it requires a
  human-confirmed prompt; the default is `wait`.
- The LiveDev image's network access is **outbound only** by default
  (https to github.com and to the ratified AI provider). Inbound
  connections are refused by the existing `distro/network/nftables.conf`
  baseline. SSH inbound is opt-in via a boot flag.
- The LiveDev image does **not** carry production signing keys. Test keys
  only; production keys are injected at runtime by the human operator
  (`docs/automation-human-interface.md` §2.2).

---

## Phase 8 — PR submission / CI

**Goal:** Build the LiveDev surface that opens PRs with evidence attached.
This is the phase where LiveDev becomes a Builder under
`docs/agent-protocol.md`: it produces branches, pushes them, and opens
PRs. It does **not** merge them.

**Entry condition:** Phase 7 merged to `main`;
`docs/plans/livedev-progress.json` `current_phase = "livedev-image-profile"`.

**Exit condition:**

- `crates/rush_pr` exists and is in the workspace. It exposes:
  `open_pr(branch: BranchName, title: String, body: PrBody, evidence: Vec<EvidencePath>)
  -> Result<PrUrl>`. It uses the `gh` CLI or the GitHub API.
- `crates/rush_pr` **never** merges PRs (policy §7). It opens, comments,
  and closes-without-merge only with a human-confirmed prompt.
- `crates/rush_pr` attaches evidence files to the PR by committing them
  under `release/evidence/<pr-branch>/` and referencing them in the PR
  body. It does **not** modify `release/milestones.toml` (the `verified`
  flag is the Verifier's, not the Builder's).
- The PR body is composed from a template (`docs/templates/livedev-pr.md`)
  that includes: the goal, the plan, the execution record, the evidence
  paths, the inferred verdict, and an explicit "Awaiting Verifier review"
  line.
- LiveDev CI (a new job in `.github/workflows/ci.yml`) runs the
  `--mock` harness against every LiveDev PR, asserting that the harness's
  contract holds.
- `docs/agent-protocol.md` is **not modified**. The Builder/Verifier/Human
  split is unchanged; LiveDev is a Builder.

**Deliverables:**

- `crates/rush_pr/{Cargo.toml,src/lib.rs,src/main.rs,tests/}`.
- Root `Cargo.toml` `members` extended.
- `docs/docmap.toml` entry for `crates/rush_pr`.
- `docs/templates/livedev-pr.md` (new).
- Patch to `.github/workflows/ci.yml` (LiveDev mock-harness job).
- `docs/decisions/0024-livedev-pr-submission.md` (new ADR, `proposed`):
  ratifies the PR submission surface, the non-merge rule, and the
  evidence-attachment convention.

**Non-deliverables:**

- LiveDev does not merge PRs (policy §7; ADR 0018 §6.2).
- LiveDev does not mark evidence verified (policy §8; ADR 0018 §6.1).
- LiveDev does not bump `VERSION` or modify `RELEASES.md` (policy §8).
- `docs/agent-protocol.md` is not modified.

**Safety notes:**

- The `open_pr` function requires a GitHub token. The token is read from
  `~/.config/rush/secrets.env` (`docs/automation-human-interface.md`
  §2.2); it is never logged, echoed, or committed.
- The token's scope is **minimal**: `repo:status` and `pull-requests:write`
  for the project's repo only. The LiveDev image does not carry a token
  with broader scope.
- A PR that touches `release/milestones.toml`, `VERSION`, `RELEASES.md`,
  or any ADR's `Status:` line is **rejected at open time** by
  `crates/rush_pr`. The rejection is logged; the human is prompted. This
  is the enforcement of `docs/ai-interface-policy.md` §8.

---

## Phase 9 — E2E dry run

**Goal:** End-to-end dry run of the entire LiveDev track: the maintainer
boots the LiveDev image on a reference machine, the planner proposes a
benchmark, the runner executes it, the AI harness summarizes the
transcript (mock provider, not real), `rush_pr` opens a PR with the
evidence attached, the Verifier reviews it, and the Human merges. The
dry run uses the **mock** AI provider throughout; no real AI calls are
made.

**Entry condition:** Phase 8 merged to `main`;
`docs/plans/livedev-progress.json` `current_phase = "pr-submission-ci"`.

**Exit condition:**

- A dry-run PR exists on a feature branch, opened by `crates/rush_pr`,
  with evidence attached under `release/evidence/livedev-dry-run/`.
- The dry-run PR's body follows the `docs/templates/livedev-pr.md`
  template.
- The dry-run PR's CI is green (LiveDev mock-harness job passes).
- A Verifier agent (separate session, cold checkout) has written a
  `VERIFICATION.md` report on the dry-run PR with per-criterion verdicts.
- The Human has merged the dry-run PR (final-approval action,
  `docs/automation-human-interface.md` §2.5).
- `docs/plans/livedev-progress.json` `current_phase = "e2e-dry-run"` and
  `next_phase = "graduation"` (or `"complete"`, at the maintainer's
  discretion).
- `docs/decisions/0026-livedev-graduated.md` (new ADR, `proposed`):
  declares LiveDev graduated from the transition plan, lists the
  remaining steps (if any) to deprecate testOS, and records the
  maintainer's signoff. (Number 0026 because 0025 is taken by
  `0025-risk-based-project-workflow.md`.)

**Deliverables:**

- The dry-run PR (merged).
- `release/evidence/livedev-dry-run/` (transcripts + meta).
- `docs/decisions/0026-livedev-graduated.md` (new ADR, `proposed`).
- Final update to `docs/plans/livedev-progress.json`.
- Update to `docs/IMPLEMENTATION_STATUS.md` (note LiveDev graduation).
- Update to `README.md` (optional, at maintainer discretion: add a
  "LiveDev" section pointing at the new docs).

**Non-deliverables:**

- testOS is **not** deprecated by this phase. testOS deprecation, if it
  happens, is a separate follow-up ADR after LiveDev has been in
  production use for a maintainership-defined soak period.
- LiveDev does **not** merge its own graduation PR. The Human merges.
- LiveDev does **not** mark its own evidence verified. The Verifier does.

**Safety notes:**

- The dry run is the **first** time the full LiveDev track runs
  end-to-end. Every safety surface in the previous phases is exercised:
  `optid`'s fail-closed policy (Phase 1), `rush_exec`'s no-shell rule
  (Phase 2), the evidence schema (Phase 3), the planner's `default` floor
  (Phase 4), the runner's prompt contract (Phase 5), the AI harness's
  mock-only mode (Phase 6), the image's read-only default (Phase 7), and
  `rush_pr`'s non-merge + release-truth rejection (Phase 8).
- Any safety surface that fails during the dry run is a release blocker.
  The dry run does not graduate until every surface passes.
- The dry run uses the **mock** AI provider. Real AI calls are gated on a
  separate, post-graduation ADR that the maintainer ratifies after
  reviewing the dry-run transcripts.

---

## Phase ordering is fixed

The phases have the following prerequisite chain, which is why they cannot
be reordered:

```
1. optid-safety ─────► 2. rush-exec/rush-capture ─────► 3. evidence schema/validator
                                                                      │
                                                                      ▼
4. autopilot planner ◄────────────────────────────────────────────────┘
        │
        ▼
5. plan runner / testOS transition ─────► 6. AI harness ─────► 7. LiveDev image/profile
                                                                       │
                                                                       ▼
8. PR submission / CI ─────► 9. E2E dry run
```

- Phase 2 (rush-exec) needs Phase 1 (optid-safety) because `rush_exec`
  will eventually execute `optid` and must trust the fail-closed policy.
- Phase 3 (evidence schema) needs Phase 2 because the schema is the
  shape `rush_capture` writes.
- Phase 4 (planner) needs Phase 3 because the planner reads evidence
  state, which must be machine-readable.
- Phase 5 (runner) needs Phase 4 because the runner executes the
  planner's plan.
- Phase 6 (AI harness) needs Phase 5 because the harness is invoked by
  the runner for plan synthesis (optional) and evidence summarization.
- Phase 7 (LiveDev image) needs Phase 6 because the image carries the
  harness.
- Phase 8 (PR submission) needs Phase 7 because the PR surface runs on
  the LiveDev image.
- Phase 9 (E2E dry run) needs Phase 8 because the dry run exercises the
  PR surface.

Reordering any phase produces a surface that depends on a prerequisite
that does not exist yet. Such a surface is broken by construction and
will be rejected at review.

---

## Updating `livedev-progress.json` after each phase

After each phase merges to `main`, the next agent updates
`docs/plans/livedev-progress.json`:

- `current_phase` → the just-completed phase.
- `next_phase` → the next phase in this sequence.
- `source_commit` → the new HEAD of `main`.
- A new entry under `phases` recording the just-completed phase's
  deliverables, exit-condition evidence (PR URL, transcript paths), and
  any deviations from this plan.

The `phases` field is the historical record. Agents must not delete or
rewrite completed phase entries; they may only append.

---

## Relationship to ADR 0018

This plan is the **operationalization** of ADR 0018. ADR 0018 defines the
**contract** (what LiveDev is, what it may not do); this plan defines the
**sequence** (in what order LiveDev is built). The two are complementary:

- ADR 0018 is the **what** and the **why not**.
- This plan is the **when** and the **in what order**.

If ADR 0018 is rejected, this plan is void. If this plan is rejected, ADR
0018 stands but LiveDev implementation halts until a new plan is drafted.
