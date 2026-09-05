# Agent review and integration protocol

`AGENTS.md` is the constitution. `docs/project-workflow.md` describes the full
idea-to-release flow. The owner delegated reviewed merges on 2026-09-05:
agents should check accuracy and completeness and continue without waiting for
the owner to merge each PR. [ADR 0027](decisions/0027-delegated-reviewed-merges.md)
records that authority. It covers existing eligible PRs as well as new work.

## Authority

The owner sets direction and priorities. A coordinating agent organizes work,
obtains independent review, integrates eligible changes and continues the
authorized plan. A builder implements and tests. A separate reviewer challenges
the implementation and its claims. One coordinator may also build, but must
obtain another agent's review before merging its own implementation.

Repository integration is distinct from accepting new strategic direction,
declaring a release or milestone complete, promoting trusted hardware, using
production signing keys, or authorizing a destructive physical operation. Those
retain their existing authority and evidence requirements. Honor authorization
already supplied; do not turn its implementation into repeated approval prompts.
Routine implementation choices within accepted direction belong to the agents.

## Builder

Read source, decisions and acceptance requirements. Implement one coherent
change, run relevant checks, update affected docs, and open a draft PR with
accurate results and limitations. Useful partial work may merge if explicitly
scoped as partial; it cannot be called complete or unlock an unmet dependency.

The builder does not wait for another agent to do ordinary self-testing and
cannot declare its own work independently verified. Compilation, mocked tests,
physical measurements and release acceptance support different claims.

## Independent review

The coordinator spawns or assigns a separate agent after the patch is available.
Give it owner intent, PR, full head and base SHAs, requirements and evidence,
and require the project-to-change sequence below. The reviewer independently
reconstructs context from current sources, then reads the actual diff and
production entrypoints from a clean checkout rather than accepting the builder's
summary or its asserted impact boundary. It must not silently fix the work
it is reviewing. Return defects to the builder; if the reviewer becomes a
builder, another reviewer must assess its changes.

### Always start with the whole project, then zoom in

Every review starts at the project's "60,000-foot" view and descends in order.
Apply this to issue diagnosis and package verification as well as PR review;
when there is no patch yet, identify the affected path and evidence needed.

1. **Project purpose and present reality.** Re-ground in the latest owner
   direction, Northstar, project brief, current strategy and live work state.
   What user outcome is Rush pursuing? What works, what is merely proposed,
   and what presently limits progress? Identify relevant drift or conflicting
   guidance using the source-of-truth hierarchy. Do not treat an old roadmap
   or a package-completion count as proof of product readiness.
2. **Architecture and boundaries.** Place the issue in the whole OS, including
   relevant upstream kernel/firmware responsibilities, Optid, desktop/session
   integration, image/build, updates/recovery and evidence tooling. Identify
   owners, interfaces, invariants and existing mechanisms before judging a new
   mechanism. Which architectural constraint makes this approach appropriate?
3. **Interactions and consequences.** Trace actual callers, consumers, shared
   state, dependencies and downstream effects beyond changed files. Examine
   relevant control-loop interactions, resource costs, security/ownership,
   failure propagation, recovery, compatibility and maintenance burden. Explain
   where an apparent local improvement could regress another user outcome or
   duplicate existing behavior. Support impact claims with source paths or
   evidence; mark unverified dependencies and assumptions explicitly.
4. **The issue and proposed change.** Reconstruct the causal path from the
   reported problem to its real entrypoint and intended result. Does the change
   address a cause or conceal a symptom? Does its scope fit the current plan,
   and would an existing upstream facility or simpler change meet the need?
   Keep necessary integration distinct from unrelated desirable work.
5. **Implementation and proof.** Inspect behavior, edge cases, units, missing
   input, concurrency and failure paths as applicable. Test the accuracy and
   completeness criteria below against actual acceptance requirements. Verify
   relevant interactions through production-path or integration evidence;
   isolated unit tests do not prove that the whole affected path works.
6. **Return to the whole-system verdict.** State whether the change advances
   the intended user outcome, fits its neighbors and preserves the relevant
   constraints. Distinguish measured benefit from a plausible contribution.
   Name remaining integration gaps, uncertainty and necessary follow-up. Local
   correctness alone is insufficient when a demonstrated system conflict remains.

Always perform the broad orientation, including for a typo; scale depth to
impact. A small wording fix may need only a brief context check and sentence
in the review. This is a reasoning order, not six mandatory reports or a demand
to reread the entire repository or redo all research on every PR. Reuse verified
context when its source revisions and assumptions still apply, checking what
changed. Expand investigation only along relevant dependencies or uncertainties.

A system-level concern blocks this change only when tied to a concrete violated
requirement, credible affected path or missing proof necessary for its claims.
Record unrelated debt, speculative improvements and broader ambitions as
follow-up; do not turn the wider view into another indefinite approval gate.

### Accuracy, completeness and risk

For every delegated merge, review:

- **Accuracy:** behavior, source claims, units, failure handling, test results
  and documentation agree with the evidence.
- **Completeness:** promised scope reaches the real consumer/entrypoint;
  acceptance items, integration, regression coverage and necessary docs exist.
  Identify stubs, omitted error paths, fake proof and unmet dependencies.
- **Risk:** tests match affected behavior; security, rollback and hardware
  restrictions remain valid. Do not infer physical safety from CI.

A typo needs a short focused review, not a certification packet. Existing cold
package/security verification can satisfy merge review if it covers the same
changes; do not create another reviewer chain by default. Separate blocking
correctness findings from optional preferences. Repeated disagreement gets a
fresh reviewer with the disputed facts, not an automatic request for the owner
to debug it. Ask only for an unresolved product choice or action outside the
delegated authority.

Record the real review on the PR, tied to full head and base SHAs: reviewer
task/session, project fit, affected component interactions, acceptance scope,
checks inspected or run, findings, limitations,
and verdict (`ready`, `changes requested`, or `inconclusive`). Reuse the review
result; no additional receipt file is required for routine merges. Existing
package-completion receipts remain required for their separate claims.

Separate agents may share a GitHub credential. Record that provenance honestly;
use a review comment if GitHub disallows author approval. Do not impersonate
another account or call that comment independently authenticated approval.
Independence comes from the actual separate review session: a process control,
not cryptographic proof. An arbitrary comment, label, bot verdict or CI badge
does not authorize merging. The coordinator must actually obtain the review.

## Coordinating a merge

Use existing agent orchestration and GitHub tools. Do not install another queue,
merge framework or review service merely to implement this policy.

1. Obtain the independent result. Resolve blocking findings and active change
   requests; `inconclusive` is not permission to merge.
2. Re-fetch the PR. Confirm intended repository, base `main`, current head/base
   SHAs, open state, no conflicts and no unresolved review threads. Take the PR
   out of draft only when its scope is ready for review/integration.
3. Check required CI for the current proposed commit/merge candidate. Missing,
   pending, cancelled, failing or ambiguous required results block merge. Do not
   infer success from an earlier head or another PR. Consult the live ruleset;
   retain compatibility checks described in
   [branch protection](frontpage/BRANCH_PROTECTION.md).
4. If head or base changed since review, have the reviewer inspect the new diff
   and affected integration and update its verdict to both new SHAs. Reuse
   unaffected evidence; do not restart all verification mechanically. Even a
   source-only rebase requires this integration check and current CI.
5. Immediately before merging, re-fetch and compare both SHAs and eligibility.
   Use GitHub's normal merge operation with the expected **head SHA** supplied.
   Never use administrator override, bypass protection, force-push main, fake a
   check or weaken rules. GitHub's strict required-check policy handles base
   movement; the API's head SHA prevents a new unreviewed push from merging.
   A rejection means refresh and review what changed, not remove the guard.
6. Confirm GitHub reports the PR merged and record the resulting commit. Refresh
   main and the live work selector, inspect post-merge checks, and continue
   eligible work. A failed check is repair work, not a completed package. Do not
   silently remove someone else's branch or files.

This covers merges within approved direction, including evidence-backed
higher-risk implementation. Collecting a hardware report can merge without
promoting its machine; merging disabled code does not approve activation.
Preserve those distinctions in the PR.

## Keep work moving

Start review when a coherent patch is ready. Other agents can work on independent
tasks while review and CI run. After merging, use actual dependency status to
select work. Do not serialize everything behind one PR or pretend an incomplete
package satisfies a dependency. When session/runtime limits interrupt execution,
leave the PR, exact commit, outstanding finding/check and next action so the next
coordinator resumes without asking the owner to repeat the instruction.

An active coordinator can perform this loop now. Docs and GitHub auto-merge
settings do not spawn agents or provide an always-on worker. The repository has
auto-merge disabled; direct protected merges after checks need no setting change.
A future background service needs an available runtime, credentials and bounded
execution budget before claiming unattended operation.

Collectors, submission helpers and unattended repository jobs still cannot merge
their own PRs. `check-workflow-safety.py` preserves that containment. Its lexical
scan does not authenticate reviewers or enforce this entire protocol. GitHub
protection enforces required CI and thread resolution; the coordinating agent
is responsible for genuine independent review and its recorded result.

If access or review is unavailable, name the blocked action, concrete risk,
missing permission/evidence and safe next work. Do not replace a resolvable
engineering failure with a routine request for the owner to merge.
