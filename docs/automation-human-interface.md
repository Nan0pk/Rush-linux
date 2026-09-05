# Automation–Human Interface Policy

> **Status:** proposed (binding on LiveDev work once ADR 0018 is ratified;
> descriptive for the rest of the project until then).
> **Owners:** maintainer of record.
> **See also:** `docs/agent-protocol.md` (Builder/Verifier/Human split),
> `docs/ai-interface-policy.md` (AI interface shape), ADR 0018 (LiveDev
> architecture contract), `docs/plans/livedev-transition-plan.md` (build
> sequence).

This document defines the boundary between **what the system may infer** and
**what requires a human**. It exists because the LiveDev track
(`docs/decisions/0018-rush-livedev-architecture-contract.md`) will eventually
let automation drive multi-step plans, open PRs, and call online AI providers
— and without an explicit interface policy, the system will either (a) block
on a human for every trivial decision, or (b) silently take actions that
should have required a human. Both failure modes are unacceptable.

The policy is mechanical. Every automation surface in the LiveDev track (and,
as far as practical, every agent session in the rest of the project) must
implement these rules. The policy is not a recommendation; it is a contract.

---

## 1. Principle: infer maximally, prompt minimally

The system's default mode is **inference from observable state**, not
prompting. The system should read the repo, the hardware, the evidence tree,
and CI status, and decide what to do next without asking a human — except in
the five categories listed in §2.

This principle exists because prompting is expensive in two ways. First, it
costs human attention, which is the project's scarcest resource. Second, it
shifts the decision burden onto a human who has less context than the system
does: the system has read the failing test, the evidence transcript, and the
git log; the human has not. A prompt that asks "should I retry the test?" is
a prompt that the system should have answered itself by re-reading the test
output and retrying with a fix.

The inverse is also true: a system that **fails to prompt** when a human-only
action is required (e.g., booting a new kernel on physical hardware) is a
system that will silently hang or, worse, take destructive action to avoid
hanging. The categories in §2 are the floor below which the system must not
fall.

---

## 2. Human-only actions

The following action categories require a human. The system may not perform
them, may not delegate them to an AI provider, and may not pre-approve them
with a stale confirmation. Each category has a strict definition; if an
action does not clearly fall into one of these categories, the system may
infer it.

### 2.1 Physical actions

A **physical action** is any action that requires a human body in front of
the hardware. Examples:

- Plugging in a USB stick to write a testOS image.
- Booting a reference machine from USB.
- Pressing a key to enter firmware setup.
- Connecting a wattmeter or thermal probe.
- Inserting or removing a battery.
- Replacing a DIMM, NVMe drive, or network card.
- Powering a machine off physically (pulling the plug), when software
  poweroff has failed.

The system may **detect** that a physical action is needed (e.g., "no USB
device found at `/dev/sdX`") and may **prompt** the human to perform it, but
the system may not perform the action itself. The system may not, for
example, attempt to write to a USB device that is not present, or to boot a
machine that is powered off.

Physical-action prompts must follow the wait-and-detect protocol in §3.

### 2.2 Secrets

A **secret** is any credential, key, or token whose disclosure would let an
adversary take an action they should not be able to take. Examples:

- Production Ed25519 signing keys (`config/keys/testing.private.pem` is the
  test key; production keys never live in the repo).
- GitHub personal access tokens or GitHub App installation tokens.
- Online AI provider API keys (OpenAI, Anthropic, etc.).
- SSH private keys for the project's release infrastructure.
- Webhook secrets.

The system may **read** a secret from a well-known location (e.g.,
`~/.config/rush/secrets.env`) but may not **log** it, **echo** it, **commit**
it, or **transmit** it to a destination other than the one for which it was
issued. Secrets are passed to subprocesses via environment variables or
files with restrictive permissions (`0600`); never via command-line
arguments (which appear in `ps` and shell history).

The system may not prompt a human to "paste a secret into the chat" — the
chat is a logging surface, and secrets must not appear in logs. If a secret
is missing, the system prompts the human to **place the secret file at the
well-known path** and re-run, not to type the secret into the prompt.

### 2.3 Money

A **money action** is any action that incurs non-trivial, irreversible cost.
Examples:

- Calling a paid online AI provider API beyond a pre-configured monthly
  budget.
- Provisioning cloud compute (e.g., a GitHub Actions larger runner, an AWS
  EC2 instance) beyond the free tier.
- Purchasing a domain, certificate, or SaaS subscription.
- Shipping physical hardware.

The system may **propose** a money action with a cost estimate and a default
("proceed" / "skip" / "ask"), but may not **execute** it without a human
confirmation. The default for any money action above a threshold (default
threshold: $0; override via `RUSH_MONEY_AUTO_APPROVE_USD` if the maintainer
has explicitly set it) is **ask**.

### 2.4 Destructive confirmation

A **destructive action** is any action that, if wrong, cannot be undone by
the system itself within the same session. Examples:

- Force-pushing a git branch (rewrites history; the previous commits may be
  unrecoverable if the reflog has expired).
- Deleting a tag that has been pushed to the remote.
- Repartitioning a disk (existing partitions are gone).
- Issuing `git clean -fdx` (untracked files are gone).
- Closing a GitHub PR without merge (the PR's review state is lost).
- Deleting an evidence directory.
- Modifying `release/milestones.toml` to flip a `verified = true` flag
  (this is also covered by the Evidence Rule; see §2.5).

The system may propose a destructive action with a preview of what will be
lost, but may not execute it without a human confirmation. The default for
destructive actions is **ask**, always, regardless of any other
configuration.

### 2.5 Final approval

Reviewed repository integration is delegated to coordinating agents under
[ADR 0027](decisions/0027-delegated-reviewed-merges.md) and
[the agent protocol](agent-protocol.md). Collectors cannot merge their own PRs.
A source-tree change alone is not a human-only action. The remaining final
approvals include:
- Marking a milestone criterion `verified = true` in
  `release/milestones.toml` (changes the canonical release state).
- Bumping `VERSION`, `Cargo.toml`, `RELEASES.md`, or the os-release file
  (changes the canonical version pointer).
- Ratifying an ADR (changes the canonical architectural contract).
- Closing a Dragnet finding (changes the canonical evidence-integrity
  state).
- Pushing a `v*` tag (triggers the release pipeline; publishes artifacts).

These actions are the maintainer's alone, per `docs/agent-protocol.md`. The
system may **prepare** them (draft the PR, draft the milestone update, draft
the ADR, draft the tag) but may not **execute** them. The system may not
prompt the maintainer with a default of "proceed" for these actions; the
default is always "await explicit human instruction."

---

## 3. Physical prompts wait for detectable state

When the system prompts a human to perform a physical action (§2.1), the
prompt must follow the **wait-and-detect** protocol:

1. The system emits the prompt with: the action requested, the reason, the
   default ("wait"), and the detection signal that will cancel the wait.
2. The system polls the detection signal at a reasonable cadence (default:
   once per second for USB/disk events, once per 5 seconds for network
   events, once per 30 seconds for boot events).
3. When the detection signal fires, the system proceeds automatically
   without re-prompting.
4. If the detection signal does not fire within a timeout (default: 5
   minutes for disk/USB, 10 minutes for boot), the system re-prompts with
   the elapsed time and offers "keep waiting" / "abort" defaults.

The system does **not** loop on "press Y to continue" prompts. Physical
actions are async; the prompt is a status line, not a question. The human
performs the action when they are able; the system detects and proceeds.

**Example: writing a testOS image to USB.**

```
[wait] Plug in a USB device (≥1 GB). Default: wait.
       Detecting: lsblk --json RM,SIZE,NAME every 1s.
       Reason: testos-launcher needs a removable device to write to.
       Timeout: 5m.
       Detected /dev/sdb (RM=1, 14.9 GB) after 8s. Proceeding.
```

**Example: booting a reference machine.**

```
[wait] Boot the laptop from USB. Default: wait.
       Detecting: ssh rush@192.0.2.10 'echo READY' every 30s.
       Reason: Phase D benchmark requires the reference laptop.
       Timeout: 10m.
       Detected READY after 4m12s. Proceeding.
```

The detection signal must be **named in the prompt** so the human knows what
the system is waiting for. A prompt that says "please boot the machine" with
no detection signal is non-compliant; the human cannot tell whether the
system is waiting for SSH, for a webhook, or for them to press a key.

---

## 4. Every prompt logs reason / default / outcome

Every prompt the system emits — physical (§3), money (§2.3), destructive
(§2.4), or final-approval (§2.5) — must be logged with three fields:

- **reason**: why the system is prompting. Must reference a specific
  observable state ("CI job `tests` failed on commit `abc123`",
  "no USB device detected for 5m", "AI provider budget at 95% of $50/mo").
  Vague reasons ("need human input") are non-compliant.
- **default**: what the system will do if the human does not respond.
  Must be one of: `wait`, `proceed`, `skip`, `ask`, `abort`. The default
  must be safe: for destructive and money actions, the default is `wait`
  or `ask`, never `proceed`.
- **outcome**: what the human actually did, or what the system did under
  the default. Must be one of: `human-confirmed`, `human-declined`,
  `human-edited`, `default-wait`, `default-proceed`, `default-skip`,
  `default-abort`, `detected` (for §3 wait-and-detect). With a timestamp.

The log is append-only and lives at a well-known path
(`/run/rush/prompts.log` on a LiveDev system; `./prompts.log` for a
non-LiveDev agent session). The log is not committed to the repo (it is
session-local) but it may be attached to a PR as evidence that the human-only
boundary was respected.

**Compliant prompt record:**

```json
{
  "ts": "2026-07-04T09:14:22Z",
  "kind": "destructive",
  "action": "git push --force-with-lease origin docs/livedev-contract",
  "reason": "Rebase on main dropped 2 commits; --force-with-lease required to update remote. CI on the dropped commits was red.",
  "default": "wait",
  "outcome": "human-confirmed",
  "outcome_ts": "2026-07-04T09:15:01Z"
}
```

**Non-compliant prompt records (each violates one field):**

- `"reason": "need human input"` — vague; does not reference observable
  state.
- `"default": "proceed"` on a destructive action — unsafe default.
- `"outcome": "ok"` — not in the controlled vocabulary.
- Missing `outcome_ts` — outcome must be timestamped.

---

## 5. What the system infers (the default mode)

Everything not in §2 is inferred. The system reads observable state and
decides. Concretely, the system may infer:

- **Which branch to work on** — from the conversation context or the most
  recent `docs/plans/livedev-progress.json` `next_phase`.
- **Which tests to run** — from the files changed in the working tree
  (`cargo nextest run --workspace` for Rust changes;
  `python3 tools/validate-doc-sync.py` for docs changes).
- **Whether to retry a failing test** — from the failure mode (flaky → retry
  once with backoff; deterministic → do not retry, fix the code).
- **Which AI provider to call** — from the task type and the policy in
  `docs/ai-interface-policy.md`.
- **Whether to open a PR** — from whether the working tree is clean, the
  branch is pushed, and CI is green on the latest commit.
- **Whether to re-run `tools/start-work.sh`** — from whether `DIRTY_STATE.md`
  exists.
- **Whether evidence is well-formed** — from `tools/validate-evidence.py`
  (existence check) and, once the `evidence schema/validator` phase of the
  transition plan lands, from the content-aware validator.

Inference is **logged** (the system records what it inferred and why) but
not **prompted**. The log of inferences lives at `/run/rush/inferences.log`
(or `./inferences.log` for a non-LiveDev session). Inference logs may be
attached to a PR as evidence that the system's decision was auditable.

---

## 6. Non-goals

This policy does **not**:

- Replace the Builder/Verifier/Human split in `docs/agent-protocol.md`. It
  extends it to the LiveDev surface.
- Authorize a collector to merge its own PR. Coordinating agents may merge
  independently reviewed changes under `docs/agent-protocol.md`.
- Authorize the system to mark evidence verified. Final approval (§2.5)
  remains the maintainer's (or a separate Verifier agent's) alone.
- Authorize the system to call online AI providers without limit. Money
  (§2.3) and the AI interface policy (`docs/ai-interface-policy.md`) apply.
- Eliminate all prompts. The five categories in §2 are the floor; the system
  may prompt above the floor if the inference is genuinely ambiguous, but
  must log the reason/default/outcome per §4.
- Apply retroactively to existing testOS workflows. testOS's existing
  install.sh / install.ps1 prompts are compliant-by-construction (they
  already wait for USB detection and ask before writing); this policy
  codifies the pattern.

---

## 7. Acceptance criteria

A LiveDev automation surface is compliant with this policy if and only if:

- [ ] Every human-only action (§2) it performs is gated by a prompt that
      records reason/default/outcome (§4).
- [ ] Every physical-action prompt (§2.1, §3) names the detection signal
      and the timeout, and proceeds automatically when the signal fires.
- [ ] No secret (§2.2) is logged, echoed, committed, or transmitted to a
      destination other than the one for which it was issued.
- [ ] No money action (§2.3) above the configured threshold executes
      without a human-confirmed outcome.
- [ ] No destructive action (§2.4) executes without a human-confirmed
      outcome, regardless of threshold.
- [ ] No final-approval action (§2.5) executes automatically. The default
      is always `wait` for these actions.
- [ ] Every inference (§5) is logged with the observable state that
      produced it.
- [ ] The prompt log and inference log are append-only and timestamped.

Non-compliance is a release blocker for the LiveDev track. A LiveDev PR
that violates this policy is rejected at review, regardless of whether its
code is otherwise correct.

---

## 8. Relationship to `docs/agent-protocol.md`

`docs/agent-protocol.md` defines the **roles** (Builder, Verifier, Human)
and the **Evidence Rule**. This document defines the **interface shape** —
when a role (typically the Builder, increasingly automated under LiveDev)
must stop and hand off to the Human. The two documents are complementary:

- `agent-protocol.md` says **who** may do what.
- This document says **when** the system must stop doing and ask.

A future ADR may merge this document into `agent-protocol.md` if the
maintainer prefers a single canonical interface document. Until then, the
two are separate to keep `agent-protocol.md` focused on roles and evidence,
and this document focused on the interface contract.
