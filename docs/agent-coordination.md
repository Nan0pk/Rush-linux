# Multi-agent coordination

Rush may be worked on concurrently by humans, ChatGPT, Codex, Claude, local CLI sessions, other AI systems, bots, CI-driven automation, and external orchestration. This protocol lets those workers avoid collisions and hand work to one another through GitHub.

This is a coordination layer only. It does **not** replace `docs/plans/current-work.md`, `docs/plans/optid-package-status.toml`, accepted decisions, package dependencies, cold verification, or the independent review protocol in `docs/agent-protocol.md`.

## Shared rule

Assume another worker may be active unless fresh repository evidence says otherwise. Before starting or resuming write work, inspect:

- `AGENTS.md`, the live current-work selector, and the relevant package/plan;
- open issues whose title starts with `[agent-claim]`;
- open pull requests and their current head/base;
- recent branches and commits relevant to the same package, files, or subsystem;
- review comments, verification state, and CI on overlapping work.

Absence of a ChatGPT task or ChatGPT-authored pull request is not evidence that work is free.

## Work claims

A worker that is about to make a substantial repository change opens one coordination issue using the **Agent work claim** template. The claim must identify:

- the canonical package, plan item, or other owner-authorized work;
- the worker/session identity in a form another agent can distinguish;
- the role (`builder`, `reviewer`, `coordinator`, or `verifier`);
- the bounded scope and likely files/subsystem;
- branch or pull request when known;
- current state;
- a UTC lease expiry when the worker expects to remain active.

A claim is temporary ownership of a work surface, not authority to change project direction, package status, release truth, or hardware trust.

### Lease behavior

Refresh the claim while actively working. When stopping, change it to a truthful terminal or handoff state and record the exact next action.

An expired lease is a warning, **not permission to overwrite work**. Before taking over an expired claim, inspect its branch, pull request, commits, comments, CI, ledger interaction, and recent activity. Take over only when the work is clearly abandoned or explicitly handed off, and record the takeover in the issue.

## Collision rule

If another worker appears active on the same package, branch, pull request, files, decision, or tightly coupled subsystem:

1. do not start a competing implementation;
2. do not push to that worker's branch unless it is explicitly shared and coordination is clear;
3. choose the highest-priority non-overlapping work permitted by the live selector, including `ready_parallel` work when safe;
4. otherwise hold the conflicting work until it is integrated, abandoned, or handed off.

When ownership is ambiguous, behave conservatively rather than assuming the work is yours.

## Builder handoff to review

When a coherent pull-request head is ready for independent review, the builder posts a top-level PR comment in this form:

```text
Agent handoff v1
role: builder
worker: <worker/session>
work: <canonical package/plan item>
claim: <issue number or none>
head: <full SHA>
base: <full SHA>
state: review-ready
unverified: <facts the builder could not verify, or none>
```

The handoff is factual. It does not ask the reviewer to confirm the builder's preferred conclusion.

## Review claim and verdict

Before reviewing, a reviewer checks whether another reviewer is already actively reviewing the same unchanged head/base. If not, it records:

```text
Agent review claim v1
role: reviewer
worker: <worker/session>
head: <full SHA>
base: <full SHA>
state: reviewing
```

The review then follows `docs/agent-protocol.md`, including the project-to-change sequence. The final review record must be tied to the exact reviewed head/base and end with `ready`, `changes requested`, or `inconclusive`.

A reviewer must not silently fix the implementation it is reviewing. If it changes implementation or test behavior, it becomes a builder for that new head and another independent reviewer is required.

## SHA invalidation

A review authorizes only the exact head/base it reviewed. If either changes, the old verdict is stale until the reviewer checks the changed diff/integration and updates its verdict to the new pair.

CI never substitutes for independent review. Independent review never substitutes for required CI, package evidence, or cold verification.

## Package completion stays separate

A merged pull request proves only that code landed. Coordination states such as `merge-ready` or `finished` must never be interpreted as package `completed`. Package completion still requires the ledger contract, production-path integration, satisfied dependencies, all acceptance items, and separate cold verification where required.

## Handoffs and stopping

Before a worker stops substantial work, repository-visible state must say:

- what was completed;
- branch and pull request, if any;
- exact current head/base when relevant;
- actual review/CI/verification state;
- blocker, if any;
- exact next action;
- whether the claim is still active, handed off, blocked, or finished.

Do not leave essential continuation context only in a private chat session.

## Machine-readable vocabulary

Use these coordination states consistently where practical:

- `working`
- `review-ready`
- `reviewing`
- `fix-required`
- `merge-ready`
- `blocked`
- `handed-off`
- `finished`

These words describe coordination only. They do not alter the current-work selector, package ledger, release evidence, or decisions.

## Protocol version

Structured handoff blocks in this document use `v1`. Future changes must remain readable by older agents or clearly state how to interpret the new version.