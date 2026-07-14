# Agent Work and Evidence Protocol

`AGENTS.md` is the constitution. `docs/project-workflow.md` describes the full
idea-to-release flow. This document defines who may make which claim.

## Three different statements

Do not mix these up:

1. **Builder result:** "I ran these checks on this change and they passed."
2. **Independent result:** "I checked the builder's work cold and reproduced
   the result."
3. **Project approval:** "Rush accepts this direction, hardware entry,
   milestone, or release."

Builders are required to make statement 1 honestly. A separate verifier is
required for statement 2 only when the claim is high-risk. Only the human
maintainer makes statement 3.

## Evidence rule

A release, milestone, hardware-safety, or performance checkmark must point to
evidence appropriate to that claim. Examples include:

- literal command and output;
- an attached automated-test log;
- a benchmark result with its method and host details;
- a physical-hardware run log;
- a source citation for an upstream fact;
- an independent review result.

"The script implements X" is a description, not proof that X worked. `bash -n`
proves syntax only. A unit test proves the behavior covered by that unit test,
not physical-hardware safety or release readiness.

## Builder

The builder:

- reads the relevant research and accepted decisions;
- implements the change;
- runs the relevant checks;
- reports failures instead of hiding them;
- updates affected documents;
- commits to a branch and opens a draft pull request;
- never marks its own hardware, security, performance, milestone, or release
  claim independently certified;
- never merges or enables auto-merge.

The builder does not wait for a second agent to run ordinary unit tests.

## Independent verifier

A cold verifier is required for:

- physical-hardware safety or performance;
- release and milestone claims;
- security-boundary changes;
- boot, firmware, signing, storage-power, display-power, or other
  difficult-to-recover actuation;
- a result that cannot be reproduced in ordinary pull-request CI.

The verifier checks out the proposed commit without relying on the builder's
workspace, runs the stated acceptance commands, records the environment and
result, and does not quietly fix a failure. A failed verification returns to
the builder with the exact failure.

## Human maintainer

Only the human maintainer may:

- merge to `main`;
- accept a project-direction decision;
- promote a hardware candidate to trusted;
- declare a milestone or release complete;
- use production signing keys;
- approve a destructive or difficult-to-recover physical action.

## Missing proof is not a dead end

Missing proof blocks the matching claim or automatic rollout. It does not block
read-only diagnosis, dry runs, simulation, research, a draft pull request, or a
clearly disabled experiment.

Report a block using the root-tracing format in `AGENTS.md` section 10 and give
the safe ways forward.
