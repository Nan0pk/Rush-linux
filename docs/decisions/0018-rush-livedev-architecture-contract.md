# ADR 0018: Rush LiveDev Architecture Contract

Status: proposed

> Marked **proposed**; needs human ratification. LiveDev is a fresh track and
> this ADR scopes it before any code lands. Do not assume LiveDev exists in
> code until this ADR is `accepted` with a `Ratified-by:` line and the
> transition plan in `docs/plans/livedev-transition-plan.md` has executed its
> first phase.

Date: 2026-07-04
Authors: Z.ai (architecture-contract phase)
Tags: architecture, livedev, automation, evidence, release-governance

## Context

Rush Linux has a working **testOS** today: a bootable USB image that boots a
minimal Rush Linux skeleton on real hardware, runs the `rushbench` measurement
rig, writes results to the USB, and reboots back to the host OS so an operator
can pull and commit the results. testOS is the path that closed the
Dragnet-001 evidence debt for v0.3 / v0.4 / v0.5 (PR #174) and it is the
documented path for the v0.6 Phase D hardware-gated criteria
(`docs/strategy/reference-hardware.md`).

testOS is, by design, a **single-shot appliance**: boot, run benchmarks,
reboot, ingest results off-USB on the host. It cannot, by design:

- run continuously and observe long-running workloads,
- sync with the online repo while running,
- open pull requests with evidence attached,
- consult an online AI provider for plan synthesis, or
- drive itself through a multi-step plan without a human at every transition.

The long-term objective of the LiveDev track is to remove those limits
**without** weakening any of the project's load-bearing invariants: the
Evidence Rule (`docs/agent-protocol.md`), the ADR-0009 write-site boundary,
the Builder/Verifier/Human role split, the `start-work.sh` / `finish-work.sh`
session lifecycle, and the `release/milestones.toml` canonical state machine.

This ADR exists to scope LiveDev **before** code is written, so that future
agents do not drift the track toward "testOS but it can also merge PRs and
mark milestones complete." Those drifts are explicitly forbidden below.

### Related documents

- `docs/plans/livedev-workspace-enablement.md` — workspace map and reusable
  assets (Prompt 1 output).
- `docs/plans/livedev-progress.json` — machine-readable phase handoff.
- `docs/plans/livedev-transition-plan.md` — exact future build sequence
  (this ADR's acceptance criteria are its entry conditions).
- `docs/automation-human-interface.md` — what the system may infer vs. what it
  must prompt a human for.
- `docs/ai-interface-policy.md` — AI interface shape, provider policy, and the
  hard ceiling on AI authority.
- `docs/decisions/0009-optid-security-boundary.md` — `optid` write-site
  boundary; LiveDev inherits it unchanged.
- `docs/decisions/0016-retire-epic-issues-to-spec.md` — the precedent for
  scoping a long-running track via a Ratified-by ADR rather than open issues.
- `docs/decisions/0017-rush-telemetry-fate.md` — the `rush_telemetry`
  licensing decision; LiveDev must not silently depend on that crate until
  ADR 0017 is ratified.

## Decision

### 1. testOS is the current hardware-test appliance

testOS (`testos/` + `crates/testos/` + the
`.github/workflows/release-testos.yml` release pipeline) is the **current,
canonical, supported** path for real-hardware benchmark evidence. Nothing in
this ADR deprecates, renames, shadows, or removes testOS. testOS continues to
ship on every `v*` tag and continues to be the documented "Try it on real
hardware" path in the README.

### 2. Rush LiveDev is the long-term successor path

Rush LiveDev ("LiveDev") is a **future** track that, when implemented, will
provide a minimal bootable Rush Linux skeleton capable of: running
continuously on real hardware, capturing evidence, syncing with the online
repo, opening pull requests with evidence attached, and optionally consulting
online AI APIs for plan synthesis. LiveDev is the long-term successor path to
testOS for the workflows that testOS's single-shot appliance model cannot
serve.

LiveDev does **not** exist in code today. The next phase after this ADR is
ratified is `optid-safety` (per `docs/plans/livedev-progress.json`), which
hardens the `optid` write-site boundary that LiveDev will eventually depend
on. No LiveDev crate, binary, image, or profile will be added until the
transition plan in `docs/plans/livedev-transition-plan.md` reaches the
"LiveDev image/profile" phase.

### 3. testOS remains available during the transition

The transition from testOS to LiveDev is **additive and reversible**. testOS
remains the default hardware-test appliance until ALL of the following are
true:

1. This ADR is `accepted` with a `Ratified-by:` line.
2. The transition plan in `docs/plans/livedev-transition-plan.md` has reached
   its final phase ("E2E dry run").
3. The maintainer has signed off, in writing, on a separate follow-up ADR that
   declares testOS deprecated (or, if LiveDev does not subsume it, that
   declares testOS permanent).

Until then, testOS is the default. LiveDev is a parallel track that does not
displace it. If LiveDev is abandoned mid-transition, testOS continues to work
unchanged.

### 4. Rush LiveDev is a minimal bootable Rush Linux skeleton

When LiveDev is implemented, it will be a **minimal** bootable Rush Linux
skeleton, not a full distribution. "Minimal" means:

- Built from the existing `mkosi/mkosi.conf` base + a future
  `mkosi/mkosi.profiles/livedev/mkosi.conf` profile, not a custom image script.
- Boots to `multi-user.target` with `optid.service` active, exactly like the
  current server edition.
- Carries only the packages needed to: run `optid` + `rushbench`, sync with
  git over the network, open PRs via `gh` or the GitHub API, and (optionally)
  call an online AI provider.
- Does **not** carry a desktop, a display server, audio, games, or anything
  that is not load-bearing for its function.

LiveDev's image composition plane is the existing mkosi profile system
(ADR 0014). LiveDev does not introduce a parallel image builder.

### 5. LiveDev capabilities (when implemented)

When the transition plan is complete, LiveDev will be able to:

- **Test real hardware** — boot on a physical machine, run `optid` against
  the live kernel, run `rushbench` against the live workload, and capture
  the same evidence format testOS captures.
- **Capture evidence** — write transcripts and meta files into the
  `release/evidence/` tree in the format defined by the existing
  `release/evidence/BUILD-HOST-RUNBOOK.md` and
  `release/evidence/host-bench/_TEMPLATE/`.
- **Sync with the online repo** — `git fetch`, `git push` to a feature branch
  on the project's GitHub remote. LiveDev never pushes to `main` (see §6).
- **Submit PRs** — open pull requests with evidence attached, following the
  Builder/Verifier/Human role split in `docs/agent-protocol.md`. LiveDev may
  author a PR but may not merge it (see §6).
- **Optionally use online AI APIs** — call an online model provider for plan
  synthesis, code review, or evidence summarization. Online AI use is
  governed by `docs/ai-interface-policy.md` and must use a mock provider
  during tests.

### 6. LiveDev hard limits (non-negotiable)

These limits apply to LiveDev for the entire transition and remain in force
after the transition completes. Relaxing any of them requires a follow-up ADR
ratified by the maintainer.

#### 6.1 LiveDev does not self-verify milestones

LiveDev may **collect** evidence (run benchmarks, write transcripts), but it
may not **verify** milestone exit criteria. The `verified = true` flag in
`release/milestones.toml` is set exclusively by:

- a human maintainer, or
- a Verifier agent (a separate session from the Builder, per
  `docs/agent-protocol.md`), running the acceptance block verbatim on a
  cold checkout.

LiveDev is, by construction, a Builder. It cannot be its own Verifier. The
Evidence Rule (`docs/agent-protocol.md`) is unchanged: a checkmark may only
appear next to an embedded command transcript, and LiveDev-generated
transcripts are subject to the same Dragnet sweep
(`tools/dragnet.py --observe`) as any other evidence.

#### 6.2 LiveDev does not self-merge

LiveDev may open PRs but may not merge them. The authority matrix in
`docs/agent-protocol.md` is unchanged: **only the Human role may merge to
`main`**. LiveDev is a Builder; it creates branches and opens PRs. A Verifier
agent writes a `VERIFICATION.md` report. The Human merges.

This limit exists because self-merging collapses the Builder/Verifier/Human
split, which is the project's primary defense against evidence fabrication
and silent regressions. LiveDev's ability to open PRs is a convenience, not
a delegation of merge authority.

#### 6.3 LiveDev does not mutate host disks by default

By default, LiveDev runs **read-only on the host disk**. It boots from its
own medium (USB, netboot, or a dedicated partition), runs in RAM, and writes
only to:

- its own boot medium (evidence files, git working tree),
- `/run` and `/tmp` (volatile state),
- the `optid` allowlisted sysfs/cgroup paths (via `guarded_write`, per ADR
  0009).

Any operation that writes to the host's permanent disk (e.g., installing
Rush Linux onto the host, repartitioning, formatting) requires an explicit
`--mutate-host-disk` flag set by a human operator, a confirmation prompt
per `docs/automation-human-interface.md`, and a logged reason. The default
is non-mutating; this is the same default testOS enforces today.

### 7. LiveDev inherits the existing security boundary

LiveDev does not introduce a new write-site discipline. It inherits ADR 0009
in full:

- All sysfs/cgroup writes flow through `crates/optid/src/io_util.rs::guarded_write`.
- The hardware allowlist (`crates/optid/data/allowlist.toml`,
  `crates/optid/src/allowlist.rs`, `crates/optid/build.rs`) is default-enabled
  since v0.6 Phase A3.
- The 29 enumerated write sites in `crates/optid/tests/write_site_gating.rs`
  are the complete write surface; any new LiveDev-driven write site must be
  classified there and routed through `guarded_write`.
- The `--no-allowlist` flag is the emergency escape hatch, not the default.

LiveDev features that need to write outside the `optid` boundary (e.g., to
the git working tree, to evidence files, to network) must do so through
dedicated, named, logged primitives — not by reusing `guarded_write` for
non-sysfs paths. The boundary between "sysfs/cgroup writes" (allowlisted,
`guarded_write`) and "evidence/repo writes" (named, logged, not allowlisted)
must remain explicit.

### 8. LiveDev and `rush_telemetry`

LiveDev must **not** silently depend on `crates/rush_telemetry` until ADR 0017
is ratified. If the maintainer ratifies ADR 0017 Option A (fix and re-include),
LiveDev may consume `rush_telemetry` under its GPL-2.0-only license boundary.
If ADR 0017 Option B (move out) or D (delete) is ratified, LiveDev must not
reference `rush_telemetry` at all. Until ADR 0017 is ratified, LiveDev
designs must assume `rush_telemetry` is unavailable.

## Options considered

### Option A — Scope LiveDev now, implement later (recommended)

Write this ADR, the automation-human-interface policy, the AI-interface
policy, and the transition plan **before** any LiveDev code lands. The next
phase is `optid-safety` (hardening the boundary LiveDev will eventually
depend on), not LiveDev implementation.

- **Pros:** every future agent lands on a documented contract; the
  Builder/Verifier/Human split and the Evidence Rule are extended to the
  LiveDev surface explicitly rather than implicitly; testOS is protected by
  name; the transition sequence is fixed before any code can drift it.
- **Cons:** 4 docs written before any behavior ships. If the maintainer
  rejects the contract, the docs are rework.
- **Cost of deferral:** grows linearly with LiveDev surface area. Every
  LiveDev PR written without this contract drifts the track toward
  "testOS but it can merge PRs," which is exactly what §6 forbids.

### Option B — Implement LiveDev incrementally, document later

Start with a minimal LiveDev image, add capabilities PR by PR, and document
the contract once the shape is clear.

- **Pros:** less upfront writing; the contract reflects shipped behavior.
- **Cons:** violates the project's own rule that architecture documentation
  is part of acceptance criteria (`docs/architecture.md`, "Documentation
  Rule"). The first LiveDev PR that opens a PR or marks evidence would
  cross the §6 lines before any ADR exists to call it a violation.
  Rejected.

### Option C — LiveDev as a testOS feature flag

Instead of a separate track, add LiveDev capabilities (continuous running,
network sync, PR submission) to testOS behind feature flags.

- **Pros:** no new image, no new profile, no new ADR.
- **Cons:** testOS's single-shot appliance model is load-bearing for its
  safety story (boot, run, reboot — nothing persists, nothing phones home).
  Adding persistent network and PR-submission to testOS would break that
  safety story for every existing testOS user. The two tracks serve
  different purposes and should remain separate images. Rejected.

## Consequences

### If this ADR is accepted

- The transition plan in `docs/plans/livedev-transition-plan.md` becomes the
  canonical build sequence. Each phase is its own PR; each phase must run
  `tools/start-work.sh` / `tools/finish-work.sh`.
- The next phase is `optid-safety`: harden `Policy::load` fail-closed
  behavior (audit #1, Critical), test the revert path (audit #3), and add
  content-aware evidence validation (audit #4). These are prerequisites
  because LiveDev will eventually collect evidence automatically and the
  evidence gate must be trustworthy before automation multiplies the
  evidence volume.
- `docs/automation-human-interface.md` and `docs/ai-interface-policy.md`
  become binding on every LiveDev PR and on every agent session that touches
  the LiveDev track.
- testOS continues to ship on every `v*` tag, unchanged.
- No `crates/livedev*` crate is added until the transition plan reaches the
  "LiveDev image/profile" phase.

### If this ADR is rejected

- LiveDev work halts. The `docs/plans/livedev-progress.json` file's
  `next_phase` is reset to `architecture-contract` and a new ADR is drafted.
- testOS continues unchanged.
- No code under `crates/livedev*` is merged.

### Reversibility

Accepting this ADR is reversible: the maintainer can supersede it with a
later ADR (e.g., "ADR 0018 is superseded by ADR 00NN"). The transition plan
is reversible at every phase: each phase produces a standalone artifact
(optid safety hardening, evidence schema, planner, harness, image, CI) that
can be reverted independently. testOS is not touched by any phase, so even a
full LiveDev rollback leaves the hardware-test appliance intact.

## Acceptance criteria

- [ ] This ADR exists at `docs/decisions/0018-rush-livedev-architecture-contract.md`
      and is `proposed`.
- [ ] `docs/automation-human-interface.md` exists and defines the
      human-only actions and the prompt-logging contract.
- [ ] `docs/ai-interface-policy.md` exists and defines the AI interface
      shape, provider policy, and authority ceiling.
- [ ] `docs/plans/livedev-transition-plan.md` exists and lists the exact
      future phase sequence (optid-safety → rush-exec/rush-capture →
      evidence schema/validator → autopilot planner → plan runner/testOS
      transition → AI harness → LiveDev image/profile → PR submission/CI →
      E2E dry run).
- [ ] `docs/docmap.toml` has entries for all four new docs.
- [ ] `docs/SUMMARY.md` references the new ADR and the two policy docs.
- [ ] `docs/plans/livedev-progress.json` is updated:
      `current_phase = "architecture-contract"`,
      `next_phase = "optid-safety"`.
- [ ] No file under `crates/`, `tools/`, `release/`, `config/`, `mkosi/`,
      `distro/`, `packaging/`, or `testos/` is modified.
- [ ] `release/milestones.toml`, `RELEASES.md`, `VERSION`, and `Cargo.toml`
      are untouched.
- [ ] `git diff --check` is clean (no whitespace errors).

## Ratification

This ADR is `proposed`. To ratify, the maintainer edits this file to:

1. change `Status: proposed` to `Status: accepted`,
2. add a line `Ratified-by: <name or GitHub handle>, <YYYY-MM-DD>`,
3. remove the "needs human ratification" callout at the top,
4. update `docs/docmap.toml` `last_verified` for this ADR to the ratification
   date.

Until ratified, LiveDev remains a docs-only track and no LiveDev code may
land.

## References

- `docs/SPEC-northstar.md` — the single objective; LiveDev is mechanism in
  service of it, not a new objective.
- `docs/agent-protocol.md` — Builder/Verifier/Human split, Evidence Rule.
- `docs/decisions/0009-optid-security-boundary.md` — write-site boundary.
- `docs/decisions/0014-image-composition-mkosi-arch.md` — mkosi as the
  image composition plane (LiveDev profile inherits this).
- `docs/decisions/0016-retire-epic-issues-to-spec.md` — precedent for
  scoping a long-running track via a Ratified-by ADR.
- `docs/decisions/0017-rush-telemetry-fate.md` — `rush_telemetry` licensing
  decision; LiveDev must not depend on that crate until ADR 0017 is
  ratified.
- `docs/plans/livedev-workspace-enablement.md` — workspace map and reusable
  assets (Prompt 1 output).
- `docs/plans/livedev-progress.json` — machine-readable phase handoff.
- `docs/plans/livedev-transition-plan.md` — exact future build sequence.
- `docs/automation-human-interface.md` — human-only actions and prompt
  logging.
- `docs/ai-interface-policy.md` — AI interface shape and authority ceiling.
- `testos/README.md` — testOS's self-description; LiveDev does not modify
  testOS's role.
- `release/evidence/BUILD-HOST-RUNBOOK.md` — evidence capture protocol
  LiveDev will eventually automate.
- `docs/research/0020-third-pass-tech-debt-audit.md` — audit findings #1
  (fail-open `Policy::load`), #3 (untested revert path), #4 (evidence gate
  validates existence not content), #15 (`finish-work.sh` diverges from CI)
  are the load-bearing prerequisites for the `optid-safety` phase that
  follows this ADR.
