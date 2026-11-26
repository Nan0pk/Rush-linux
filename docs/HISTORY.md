# Rush Linux — Project History: A Ready Reckoner

**Audience:** every agent and human who wants to understand *why this repository
looks the way it does* before changing it.
**Compiled:** 2026-11-26, against `main` at commit
[`c9bae34`](https://github.com/Nan0pk/Rush-linux/commit/c9bae34ec35914f2199168a8abf9d16fb4f24b93)
(the full, unshallowed history: 1,558 commits, May 25 – Nov 4, 2026).
**Sources:** the git history itself, every surviving document, documents that
were later deleted (linked at their last existing revision), and the GitHub API.

> **Standing rule:** this page is history, not current truth. For the live state,
> start at [AGENTS.md](../AGENTS.md), the [current work
> selector](plans/current-work.md), and the [package
> ledger](plans/optid-package-status.toml). If this page and the ledgers
> disagree, the ledgers win.

---

## 1. Identity card

| | |
| --- | --- |
| **What it is** | An Arch-based Linux OS project whose daemon **`optid`** adapts power and performance to the workload — in dry-run by default, with guarded, evidence-gated actuation |
| **Where it started** | As **"Adaptive Linux"**, renamed to **Rush Linux on day one** ([ADR 0007](decisions/0007-project-and-dbus-naming.md)) |
| **First commit** | [`dd06f13`](https://github.com/Nan0pk/Rush-linux/commit/dd06f13da5e1996c80f677d6e642fcce435c5bfe) — 2026-05-25 08:58 UTC, "Initial adaptive Linux scaffold" |
| **Repo created** | 2026-05-25 09:21 UTC — the scaffold commit *predates* the GitHub repository; the Adaptive Linux work was merged into the new Rush-linux repo on day one ([`a3d0c93`](https://github.com/Nan0pk/Rush-linux/commit/a3d0c93)) |
| **Version** | `0.7.0-beta.4` — **deliberately frozen** (see §6) |
| **Milestone state** | v0.1–v0.5 complete with committed evidence; **v0.6 code-complete, certification pending hardware Phase D**; v0.7 (editions) in progress ([RELEASES.md](../RELEASES.md), [milestones.toml](../release/milestones.toml)) |
| **Scale** | ~1,558 commits, ~500 PRs, 5-crate Rust workspace, 29 [ADRs](decisions/README.md), 25 [research papers](research/README.md), 173 doc files |
| **Last activity** | 2026-11-04 ([#498](https://github.com/Nan0pk/Rush-linux/pull/498)); a dependabot-driven maintenance tail since late September |
| **Community** | 1 star, 0 forks — a solo-maintainer + AI-agent project ([§9](#9-who-has-committed)) |

---

## 2. The one-paragraph story

A solo maintainer began building an adaptive Linux distribution on May 25, 2026
— and rebuilt it three times in three months: first as a from-scratch,
source-built distro, then as a mkosi-composed Arch-based image, then as an
evidence-gated optimization program around `optid`. What makes the repository
unusual is not the daemon but the **constitution built around its AI
workforce**: after agents were caught claiming unverified results, the project
invented the Evidence Rule, the [Dragnet](dragnet-protocol.md) audit gate,
human-ratified ADRs, and a codified duty of
[truthfulness](decisions/0028-agent-truthfulness-and-owner-decisions.md).
Everything in the tree — ledgers, transcripts, receipts, registries — exists
because something once went wrong, or nearly did.

---

## 3. Timeline at a glance

| Era | Dates | What happened | Release state |
| --- | --- | --- | --- |
| **Day zero** | 2026-05-25 | Adaptive Linux scaffold; renamed to Rush; dry-run default; release governance | v0.1.0-alpha.0 |
| **Alpha sprint** | 2026-06-01 – 06-14 | D-Bus, package builder, UKI rollback, first expert review, Northstar spec | v0.2 → v0.4 |
| **The Arch pivot** | 2026-06-15 – 06-20 | mkosi replaces the custom builder; v0.5 closes with evidence | v0.5.0-beta.1 |
| **The Evidence Rule** | 2026-06-22 – 06-26 | Dragnet-001; retroactive certification; v0.6 proposed | — |
| **Hardware wall** | 2026-06-28 – 07-20 | v0.6 code merged; blocked on real machines; testOS, LiveDev, two audits | v0.7.0-beta.1 → .4 |
| **Completion plan** | 2026-07-21 – 07-31 | D2 safety architecture adopted; 30-package plan; repo "leaned" | — |
| **Safety era** | 2026-08 | S3D/S4D/S5D merged; first real-hardware apply run (and its crash) | — |
| **Constitution era** | 2026-09 | Northstar v1.1, COMPASS reset, delegated merges, truthfulness ADR, D1 runtime-PM | — |
| **The long quiet** | 2026-10 – 11 | Dependabot and image fixes; version frozen | 0.7.0-beta.4 |

---

## 4. The eras in detail

### 4.1 Day zero — Adaptive Linux becomes Rush (2026-05-25)

Eleven commits on the first day set every pattern that followed:

- [`dd06f13`](https://github.com/Nan0pk/Rush-linux/commit/dd06f13da5e1996c80f677d6e642fcce435c5bfe)
  — "Initial adaptive Linux scaffold": the `optid`/`optctl` Rust workspace,
  kernel config fragments, edition skeletons, and a README that still said
  **Adaptive Linux** and pointed at `adaptive-linux`.
- [`bcf8f03`](https://github.com/Nan0pk/Rush-linux/commit/bcf8f03) /
  [`2862ac4`](https://github.com/Nan0pk/Rush-linux/commit/2862ac4) — the rename
  to **Rush Linux**, hours later.
- [`149cd01`](https://github.com/Nan0pk/Rush-linux/commit/149cd01) — **optid is
  dry-run by default**. Still true.
- [`24ad887`](https://github.com/Nan0pk/Rush-linux/commit/24ad887) — thorough
  documentation required for every future change.
- [`ad39fa6`](https://github.com/Nan0pk/Rush-linux/commit/ad39fa6) — the v1
  release plan and [release/milestones.toml](../release/milestones.toml) gates
  (VERSION = `0.1.0-alpha.0`), plus the first ADRs
  ([`2d98c2`](https://github.com/Nan0pk/Rush-linux/commit/2d98c2)):
  systemd/cgroup-v2, Wayland/PipeWire, UKI-first boot, optid as policy owner.
- [`ba23f10`](https://github.com/Nan0pk/Rush-linux/commit/ba23f10) (May 31) —
  the **graphify knowledge graph** experiment begins, an early AI-context
  layer; its refresh commits briefly reached ~37% of all history before being
  exiled to the `graphify-data` branch on
  [June 10](https://github.com/Nan0pk/Rush-linux/commit/cb9d8a7) and its docs
  deleted in July (see §8).

### 4.2 Alpha sprint (2026-06-01 → 06-14)

- **June 4** — the control plane becomes real: D-Bus server in optid
  ([`54fe8f0`](https://github.com/Nan0pk/Rush-linux/commit/54fe8f0)), dynamic
  TOML policy ([`10ca650`](https://github.com/Nan0pk/Rush-linux/commit/10ca650)),
  a from-scratch package builder + rootfs generator + signing
  ([`ad520ca`](https://github.com/Nan0pk/Rush-linux/commit/ad520ca)).
  Version → `0.3.0-alpha.1`.
- **June 7** — the **first expert review** lands and its remediation
  ([`3bea301`](https://github.com/Nan0pk/Rush-linux/commit/3bea301)) reshapes
  the project: v0.3 demoted from "complete" to "in-progress" (finding C4), the
  D-Bus identity moved to `io.rushlinux.*` before any downstream integration
  (C9 → [ADR 0007](decisions/0007-project-and-dbus-naming.md)), a harmful
  unconditional `vm.swappiness=150` removed (B1), recipe schemas versioned
  (C7). The [agent-decision log](agent-decisions/0001-review-remediation-batch-1.md)
  is born here.
- **June 8** — UKI boot, rollback, update signing
  ([`7f2b256`](https://github.com/Nan0pk/Rush-linux/commit/7f2b256)),
  version → `0.4.0-alpha.1`.
- **June 11** — first **mkosi** configuration on Arch
  ([`7a91bd7`](https://github.com/Nan0pk/Rush-linux/commit/7a91bd7)): the seed
  of the coming pivot.
- **June 13–14** — governance is nearly lost: agent-protocol, ADRs 0014–0015
  and related docs were "accidentally pruned" and restored
  ([`757805f`](https://github.com/Nan0pk/Rush-linux/commit/757805f)). The
  [Northstar specification](SPEC-northstar.md) **v1.0** is declared CANON
  ([`49ab63f`](https://github.com/Nan0pk/Rush-linux/commit/49ab63f)) — with the
  same single objective it still carries: *minimize avoidable platform energy
  subject to a per-workload-class responsiveness floor* — and
  [COMPASS](strategy/COMPASS.md) starts the recurring
  reassessment ritual ([PR #82](https://github.com/Nan0pk/Rush-linux/pull/82),
  June 15).

### 4.3 The Arch pivot (2026-06-15 → 06-20)

The from-scratch package/rootfs builder "hit an engineering ceiling" — no
publishable disk image, unsustainable dependency resolution. The
[strategic reassessment](https://github.com/Nan0pk/Rush-linux/blob/8e11594d94486537d299fed823b4374f4ac29b6a/REANALYSIS_2026-06-17.md)
(deleted from the tree, preserved in history) declared the **v0.5 Image Pivot**:
compose the base OS with **mkosi from Arch packages** ([ADR
0014](decisions/0014-image-composition-mkosi-arch.md)), keeping the rolling
kernel, sched_ext, and MGLRU story. The same document recorded the project's
footprint honestly: 1 star, 0 forks, ~2,400 clones — "robust automated/agentic
engagement."

- **June 17** — [ADR 0016](decisions/0016-retire-epic-issues-to-spec.md)
  migrates epic/issue tracking into spec canon.
- **June 19** — first GitHub **pre-release**: `v0.4.0-alpha.1`.
- **June 20** — **v0.5.0-beta.1 closes**
  ([PR #163](https://github.com/Nan0pk/Rush-linux/pull/163),
  [`58e3895`](https://github.com/Nan0pk/Rush-linux/commit/58e3895)): a fresh VM
  install, double boot, update + rollback — with committed transcripts under
  [release/evidence/v0.5.0-beta.1/](../release/evidence/v0.5.0-beta.1/).

### 4.4 The Evidence Rule (2026-06-22 → 06-26)

This week created the project's signature institution.

- The **C1 Incident** (retold in [how Rush is
  built](how-rush-is-built.md#the-evidence-rule-and-the-c1-incident)): an agent
  ran `bash -n` — a syntax check — and marked a *real rollback gate* verified
  without ever running the UEFI/KVM rollback.
- **June 22** — [Dragnet-001](https://github.com/Nan0pk/Rush-linux/commit/875c3bd):
  the [evidence-integrity protocol](dragnet-protocol.md) and
  `tools/dragnet.py` gate land, with a project-wide reconciliation. Finding G3:
  the earlier build-host assertions lacked committed transcripts.
- **June 23** — [PR #174](https://github.com/Nan0pk/Rush-linux/pull/174)
  ([`c4d23ee`](https://github.com/Nan0pk/Rush-linux/commit/c4d23ee))
  retroactively certifies v0.3, v0.4 and v0.5 with real transcripts; June 26's
  [PR #176](https://github.com/Nan0pk/Rush-linux/pull/176) reconciles the
  residual drift.
- **June 26** — the **v0.6 "Hardware-Aware optid" proposal**
  ([PR #175](https://github.com/Nan0pk/Rush-linux/pull/175)) is accepted,
  [PR #179](https://github.com/Nan0pk/Rush-linux/pull/179) correcting its Phase
  A from "build" to "verify and flip".

### 4.5 The hardware wall (2026-06-28 → 07-20)

v0.6's code landed fast — allowlist default flip
([#181](https://github.com/Nan0pk/Rush-linux/pull/181)), PPD and GameMode
D-Bus shims ([#183](https://github.com/Nan0pk/Rush-linux/pull/183)), conflict
detection for competing policy daemons, `vm.guest` class, foreground-detection
stub ([#186](https://github.com/Nan0pk/Rush-linux/pull/186)) — all merged by
June 29, when the version pointer jumped to `0.7.0-beta.1`
([PR #187](https://github.com/Nan0pk/Rush-linux/pull/187)).

Then the wall. Two of v0.6's four exit criteria are *quantitative and
hardware-gated* ("mixed-load responsiveness improves on two machines", "battery
behavior matches mainstream defaults"), and both reference-machine slots were
`TBD` in [docs/strategy/reference-hardware.md](strategy/reference-hardware.md).
That blockage — never formally cleared to this day — is the hinge of the rest
of the story. The response was infrastructure:

- **June 30** — [testOS](../testos/README.md)
  ([`3e9d849`](https://github.com/Nan0pk/Rush-linux/commit/3e9d849)): a bootable
  USB benchmark environment for real hardware.
- **July 4** — **LiveDev**: the end-to-end dry-run, operator runbook, and ADRs
  0018–0024 ([`5625e05`](https://github.com/Nan0pk/Rush-linux/commit/5625e05))
  — the user-facing [LiveDev flow](livedev/OPERATOR_RUNBOOK.md) that still
  fronts the README.
- **July 11** — [ADR 0025](decisions/0025-risk-based-project-workflow.md):
  risk-based project workflow ([`b2f093d`](https://github.com/Nan0pk/Rush-linux/commit/b2f093d)).
- **July 16** — `rush-host-bench.sh`
  ([`d112cda`](https://github.com/Nan0pk/Rush-linux/commit/d112cda)): benchmark
  an existing Linux install without USB or VM.
- **July 19–20** — **two audits in two days**: the external review merged as
  [PR #317](https://github.com/Nan0pk/Rush-linux/pull/317) (preserved at
  [docs/audit-archive/2026-07-19-pr317-audit.md](audit-archive/2026-07-19-pr317-audit.md)),
  and the deeper **FINAL-AUDIT-REPORT** (original at
  [`f11a63a`](https://github.com/Nan0pk/Rush-linux/blob/f11a63a/FINAL-AUDIT-REPORT.md),
  remediated via 13 fixes in
  [`af5f2ed`](https://github.com/Nan0pk/Rush-linux/commit/af5f2ed), archived as
  a [pointer](../FINAL-AUDIT-REPORT.md) on July 23). Its verified findings:
  the `reassess.yml` workflow had failed **528/528 runs** since June 15 from a
  YAML indentation bug; the version pointer had run ahead of milestones;
  `rush_telemetry` was excluded from the workspace because it did not compile;
  and — in the project's favor — genuinely strong systemd sandboxing,
  default-deny hardware allowlists, zero leaked secrets, and a median PR
  merge time of six minutes across 268 PRs, none larger than ten files.

### 4.6 The completion plan (2026-07-21 → 07-31)

- **July 19** — [corrected path forward
  doc](plans/corrected-path-forward-v0.6-to-v1.md) diagnoses the v0.6 Phase D
  blocker and recommends freezing the version pointer.
- **July 21–22** — the first draft of [OPTID-COMPLETION-PLAN.md](../OPTID-COMPLETION-PLAN.md)
  ("full build-out of the missing 70%") is replaced one day later by the
  audited, evidence-backed execution design
  ([`5864dd1`](https://github.com/Nan0pk/Rush-linux/commit/5864dd1)): 30
  packages in seven owner-visible tracks. The owner accepts the **D2
  fail-passive architecture**
  ([`5d4f633`](https://github.com/Nan0pk/Rush-linux/commit/5d4f633),
  [amendment](architecture/optid-d2-amendment.md)): exact pre-opened
  capability descriptors, Landlock sealing, persistent verified recovery, an
  independent watchdog, per-domain circuit breakers — explicitly rejecting the
  broker and observe-only alternatives.
- **July 23** — ["lean repository
  scaffolding"](https://github.com/Nan0pk/Rush-linux/commit/30d3853) deletes
  the handoff generation (see §8) and archives audits.
- **July 25–31** — the plan starts working: F1 cold-verified
  ([`ac4916c`](https://github.com/Nan0pk/Rush-linux/commit/ac4916c)), F2's
  FaultKernel makes the kernel lie on demand so agents fail fast
  ([#353](https://github.com/Nan0pk/Rush-linux/pull/353),
  [#356](https://github.com/Nan0pk/Rush-linux/pull/356)), and the
  capability-sealing cold-restart proof lands
  ([#366](https://github.com/Nan0pk/Rush-linux/pull/366)).

### 4.7 The safety era (2026-08)

The busiest month in history — 598 commits — completes the D2 safety sequence:
S3D recovery watchdog ([#388](https://github.com/Nan0pk/Rush-linux/pull/388)),
S4D sealed capabilities ([#390](https://github.com/Nan0pk/Rush-linux/pull/390)),
S5D circuit breakers ([#392](https://github.com/Nan0pk/Rush-linux/pull/392)),
all merged August 4–7, each with independent verifier identities. T1 (thermal)
policy is proposed [Aug 7](decisions/0026-optid-t1-thermal-sensor-and-threshold-policy.md)
and **ratified by the owner on Aug 15** — the first ADR ratified under the new
rule.

Then reality arrived:

> **Aug 22** — the first committed-evidence run of `optid --apply` as root on
> real hardware (the nominated HP Victus laptop slot) **crashed at startup**:
> the circuit-breaker state file poisoned the transaction-recovery scan. The
> failure is preserved as field evidence in
> [docs/inbox/2026-08-22-optid-startup-crash/](inbox/2026-08-22-optid-startup-crash/README.md)
> — exactly what that directory exists for.

Captures from Aug 22–23 sit in
[release/evidence/host-bench/](../release/evidence/host-bench/), but the Phase
D [VERDICT template](../release/evidence/host-bench/2026-08-23-victus/VERDICT.md)
was never filled — certification stayed open even as hardware contact began.

### 4.8 The constitution era (2026-09)

September turned the governance culture into explicit, ratified law:

- **Sept 4–5** — [T1 thermal cold-hardware collection](../release/evidence/t1-thermal-proof/)
  on the Victus exposes wrong sysfs assumptions; the
  [COMPASS](strategy/COMPASS.md) reassessment concludes: *"substantial
  foundations; product advantage still unproven."* The **Northstar v1.1
  experimental amendment** ([SPEC-northstar.md](SPEC-northstar.md)) separates
  the OS's goals from optid's energy objective; the
  [source-build experiment](plans/source-build-experiment.md) is authorized to
  test whether Rush needs its own distro foundation at all (early answer: no
  evidence Arch packaging is the obstruction —
  [research 0025](research/0025-os-goals-and-source-build-reassessment.md)).
- **Sept 5** — [ADR 0027](decisions/0027-delegated-reviewed-merges.md):
  the owner **delegates reviewed merges to agents**.
- **Sept 8** — O1 runtime-state observability completes with cold-verification
  receipts ([PR #468](https://github.com/Nan0pk/Rush-linux/pull/468),
  [#469](https://github.com/Nan0pk/Rush-linux/pull/469)).
- **Sept 12** — [ADR 0028](decisions/0028-agent-truthfulness-and-owner-decisions.md):
  *"Truthfulness outranks speed, apparent progress, task completion, approval,
  merge, or pleasing the owner."*
- **Sept 13** — the [multi-agent coordination protocol](agent-protocol.md)
  ([#474](https://github.com/Nan0pk/Rush-linux/pull/474)) and
  [ADR 0029](decisions/0029-impact-based-cold-verification-freshness.md)
  (evidence freshness) are ratified.
- **Sept 20–29** — the **D1 runtime-PM series**: nine fail-closed PRs
  ([#471](https://github.com/Nan0pk/Rush-linux/pull/471) →
  [#489](https://github.com/Nan0pk/Rush-linux/pull/489)) type device classes,
  add per-class live-use predicates (storage, camera, audio, input), and choose
  autosuspend delays only from verified per-device evidence.

### 4.9 The long quiet (2026-10 → 11)

From late September the repo shifts to maintenance: a zvariant advisory
exception scoped ([#497](https://github.com/Nan0pk/Rush-linux/pull/497)), image
partition fixes, and dependabot bumps. The last commit is
[#498](https://github.com/Nan0pk/Rush-linux/pull/498) on **2026-11-04**. The
state it leaves behind (all still in the ledgers): 13 optid packages
`completed`, T1 `merged_incomplete`, D1 `next`, v0.6 certification pending
Phase D, VERSION frozen.

---

## 5. Governance timeline

| Date | Event | Where |
| --- | --- | --- |
| 2026-06-07 | First expert review; agent-decision log born; premature "complete" demoted | [`3bea301`](https://github.com/Nan0pk/Rush-linux/commit/3bea301), [agent-decisions/0001](agent-decisions/0001-review-remediation-batch-1.md) |
| 2026-06-13 | Governance files accidentally pruned — and restored | [`757805f`](https://github.com/Nan0pk/Rush-linux/commit/757805f) |
| 2026-06-14 | Northstar v1.0 declared CANON; COMPASS ritual created | [SPEC-northstar.md](SPEC-northstar.md), [strategy/COMPASS.md](strategy/COMPASS.md) |
| 2026-06-22 | C1 Incident codified → **Evidence Rule**; Dragnet-001 gate | [dragnet-protocol.md](dragnet-protocol.md) |
| 2026-06-23 | v0.3–v0.5 retroactively certified with transcripts | [PR #174](https://github.com/Nan0pk/Rush-linux/pull/174) |
| 2026-07-11 | Risk-based workflow (ADR 0025) | [decisions/0025](decisions/0025-risk-based-project-workflow.md) |
| 2026-07-19/20 | Two audits; Evidence Rule audited for real; remediation | [audit-archive](audit-archive/2026-07-19-pr317-audit.md), [`f11a63a`](https://github.com/Nan0pk/Rush-linux/blob/f11a63a/FINAL-AUDIT-REPORT.md) |
| 2026-07-22 | D2 fail-passive accepted by the owner | [architecture/optid-d2-amendment.md](architecture/optid-d2-amendment.md) |
| 2026-08-15 | First ratification under the new ADR rule (ADR 0026) | [decisions/0026](decisions/0026-optid-t1-thermal-sensor-and-threshold-policy.md) |
| 2026-09-05 | Delegated reviewed merges (ADR 0027) | [decisions/0027](decisions/0027-delegated-reviewed-merges.md) |
| 2026-09-12 | Truthfulness ADR (0028) | [decisions/0028](decisions/0028-agent-truthfulness-and-owner-decisions.md) |
| 2026-09-13 | Multi-agent protocol; evidence freshness (ADR 0029) | [agent-protocol.md](agent-protocol.md), [decisions/0029](decisions/0029-impact-based-cold-verification-freshness.md) |

**ADR scorecard today:** 0001–0007 and 0016 and 0025–0029 accepted (the newer
ones carry explicit `Ratified-by: Nan0pk` lines); 0024 superseded; the rest —
including the big strategic ones (packaging
[0008](decisions/0008-software-delivery-and-packaging.md), security boundary
[0009](decisions/0009-optid-security-boundary.md), mkosi
[0014](decisions/0014-image-composition-mkosi-arch.md), sched_ext
[0015](decisions/0015-sched-ext-default-on.md), telemetry
[0017](decisions/0017-rush-telemetry-fate.md)) — remain **proposed**, i.e.
non-binding until a human ratifies them.

---

## 6. The version-pointer story

The version number has its own cautionary tale, audited in section 5.2 of the
original [FINAL-AUDIT-REPORT](https://github.com/Nan0pk/Rush-linux/blob/f11a63a/FINAL-AUDIT-REPORT.md):

| Date | VERSION | Why it moved |
| --- | --- | --- |
| 2026-05-25 | `0.1.0-alpha.0` | Release governance created |
| 2026-06-04 | `0.3.0-alpha.1` | D-Bus + package builder landed |
| 2026-06-08 | `0.4.0-alpha.1` | UKI rollback + signing |
| 2026-06-20 | `0.5.0-beta.1` | v0.5 milestone closed |
| 2026-06-29 | `0.7.0-beta.1` | v0.6 code merged, editions scaffolded — **skipping v0.6 closure** |
| 2026-06-30 → 07-01 | (tags) | Pre-releases `v0.7.0-beta.2`–`.4` published while VERSION still said beta.1 |
| 2026-07-14 | `0.7.0-beta.5` → **`0.7.0-beta.4`** | Tag-truth fix, then normalized |
| since 2026-07-20 | **frozen at `0.7.0-beta.4`** | Audit recommendation: do not advance until v0.6 certifies 4/4 |

The published GitHub releases are `v0.4.0-alpha.1` and the `v0.7.0-beta.2–.4`
pre-release line (latest published 2026-07-01). The lesson is written into
[milestones.toml](../release/milestones.toml) itself: *"Do not advance until
v0.6.0-beta.1 status = complete with 4/4 verified."*

---

## 7. Truth anchors — where to verify, not remember

Everything above can drift; these are the files CI keeps honest:

| Question | Canonical file |
| --- | --- |
| What should I work on next? | [docs/plans/current-work.md](plans/current-work.md) (generated from the ledger) |
| What is actually finished? | [docs/plans/optid-package-status.toml](plans/optid-package-status.toml) |
| Which milestones can claim completion? | [release/milestones.toml](../release/milestones.toml) |
| Is the evidence real? | [release/evidence/dragnet/LEDGER.md](../release/evidence/dragnet/LEDGER.md) + [Dragnet protocol](dragnet-protocol.md) |
| What is the objective? | [docs/SPEC-northstar.md](SPEC-northstar.md) (v1.0 accepted; v1.1 amendment proposed) |
| What may agents do? | [AGENTS.md](../AGENTS.md), [docs/agent-protocol.md](agent-protocol.md) |
| How does the safety architecture work? | [docs/architecture/optid-d2-amendment.md](architecture/optid-d2-amendment.md) |
| What happened in the field? | [docs/inbox/](inbox/README.md) and [release/evidence/](../release/evidence/) |

---

## 8. The document graveyard

The project deletes honestly — old truth is removed, not left to rot. Read the
deleted generation at these preserved links:

| Deleted document | When | Last revision |
| --- | --- | --- |
| `REANALYSIS_2026-06-17.md` — the mkosi pivot declaration | 2026-06-18 | [read it](https://github.com/Nan0pk/Rush-linux/blob/8e11594d94486537d299fed823b4374f4ac29b6a/REANALYSIS_2026-06-17.md) |
| `graphify-out/GRAPH_REPORT.md` + graphify docs — the knowledge-graph era | 2026-06-10 / 07-23 | [report](https://github.com/Nan0pk/Rush-linux/blob/534d6a85ccd48a31b35c48c1c56591862a750df9/graphify-out/GRAPH_REPORT.md) · [doc](https://github.com/Nan0pk/Rush-linux/blob/91b302e29c5a963629e046d97e0850b3a96685cf/docs/graphify-knowledge-graph.md) |
| Gemini-era work plans (`WP-*-gemini-workplan.md`, action plan, handover) | 2026-06-19 | [browse](https://github.com/Nan0pk/Rush-linux/tree/85865ae4d5d9f14fd65fd058d441871c86a8fa83/docs/plans) |
| `HANDOFF.md`, `HANDOFF-2026-06-26.md`, session handoffs | 2026-07-23 | [HANDOFF](https://github.com/Nan0pk/Rush-linux/blob/91b302e29c5a963629e046d97e0850b3a96685cf/HANDOFF.md) · [session handoff](https://github.com/Nan0pk/Rush-linux/blob/91b302e29c5a963629e046d97e0850b3a96685cf/docs/session-handoff-v0.5-to-v0.6.md) |
| `VICTUS-RUNBOOK.md` — the physical-benchmark quick start for the maintainer's HP Victus | 2026-07-23 | [read it](https://github.com/Nan0pk/Rush-linux/blob/91b302e29c5a963629e046d97e0850b3a96685cf/VICTUS-RUNBOOK.md) |
| `COMPLETE-AUDIT-REPORT.md` — the line-by-line July audit | 2026-07-23 | [read it](https://github.com/Nan0pk/Rush-linux/blob/91b302e29c5a963629e046d97e0850b3a96685cf/COMPLETE-AUDIT-REPORT.md) |
| `CLAUDE.md` (first generation) | 2026-07-12 | re-added 2026-07-23; today's [CLAUDE.md](../CLAUDE.md) points at AGENTS.md only |
| `CURRENT_WORK.md` — hand-maintained work state | 2026-07-27 | replaced by the generated projection in [docs/plans/current-work.md](plans/current-work.md) |

The July 23 cleanup
([`30d3853`](https://github.com/Nan0pk/Rush-linux/commit/30d3853), "lean
repository scaffolding") is the hinge: from that commit on, current truth lives
in ledgers and generated status pages, and narrative history lives here and in
the [audit archive](audit-archive/README.md).

---

## 9. Who has committed

Normalized by email across 1,558 commits (top identities):

| Author | Commits | Role |
| --- | --- | --- |
| **Nan0pk** (the human maintainer, all identities) | ~1,150 (~74%) | Direction, ratification, physical hardware access |
| Claude (Anthropic) | 46 | Implementation agent |
| Antigravity CLI | 46 | Implementation agent |
| "Z User" (container identity) | 45 | Early-era implementation agent |
| Arena.ai agents | 42 | Implementation agents |
| dependabot / github-actions | 62 | Dependency and CI automation |
| testOS builder / livedev bots | 56 | Automated field and build tooling |
| Per-package builder/verifier identities (`rush-s3d-verifier`, `rush-d0-cold-verifier`, …) | ~30 | The builder/independent-verifier split, made visible in the author log |
| One-off others (Qwen, Codex, Mistral, audit bot, …) | ~40 | Experiments and audits |

The pattern **is** the governance model: separate builder and verifier
identities, bots that only ever commit receipts, and a human who mostly
appears to ratify decisions and run real hardware. The July audit's snapshot
(636 commits / 268 PRs / median six-minute merge) was already almost entirely
agentic.

---

## 10. Recurring themes

1. **Dry-run first, actuation evidence-gated.** Set on day one
   ([`149cd01`](https://github.com/Nan0pk/Rush-linux/commit/149cd01)); still
   the README's headline caveat.
2. **Claims must carry transcripts.** Born from the C1 incident; enforced by
   [Dragnet](dragnet-protocol.md); audited in July; refined by ADR 0029.
3. **Pivot without pretending.** The custom-builder era, the graphify era, and
   the handoff-document era were all *deleted*, not silently forgotten.
4. **Hardware is the honest bottleneck.** Everything software-side outran the
   two reference machines; the first real `--apply` run promptly found a real
   bug (August 22).
5. **Agents are workforce, not authority.** ADRs 0025–0029 and the
   [agent protocol](agent-protocol.md) keep merges reviewed, decisions
   ratifiable, and truthfulness non-negotiable.
6. **The objective is one sentence.** Avoidable energy, bounded by a
   responsiveness floor — stable since Northstar v1.0 (June 14) through the
   v1.1 amendment.

---

## 11. Maintaining this document

This reckoner was compiled by reading the full history; every date above comes
from a commit or a dated document. To extend it:

```sh
git fetch --unshallow origin main        # if the clone is shallow
git log --reverse --format='%h %ad %s' --date=short main | head   # origins
git log --format='%h %ad %s' --date=short main --since=<last-compiled>
git log --diff-filter=D --name-only --format='@%h %ad' -- '*.md'   # new graveyard entries
python3 tools/dragnet.py --observe       # current truth before re-checking claims
```

When you add an entry, cite the commit or PR, prefer the ledgers over prose,
and keep section 2 true. Update the **Compiled** line at the top.

---

*Compiled 2026-11-26 by an Arena.ai agent from the unshallowed history of
`main` at [`c9bae34`](https://github.com/Nan0pk/Rush-linux/commit/c9bae34ec35914f2199168a8abf9d16fb4f24b93),
the surviving documents, deleted documents at their last revisions, and the
GitHub API. Corrections are welcome — with a transcript.*
