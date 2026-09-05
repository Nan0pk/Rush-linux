# Branch protection and delegated integration

`PR Gate` is the stable aggregate quality check. The aggregator fails unless the
always-on repository/Linux lane and every path-selected Windows or image lane
pass.

The implementation lives in `.github/workflows/ci.yml`. Individual lane names
may evolve without repeatedly editing branch protection; the aggregator is the
stable contract.

Labeling, release drafting, publishing, scheduled maintenance, pages, and
reassessment workflows are not quality gates. They create artifacts or report
state and must not be configured as required PR checks.

As inspected on 2026-09-05, the active
[protect-main ruleset](https://github.com/Nan0pk/Rush-linux/rules/17500512)
requires these exact check contexts: `PR Gate`, `Rust`, `Documentation sync`,
`Repository policy`, and `Evidence integrity (Dragnet)`. The last four are
compatibility aliases that propagate the aggregate result. Preserve them until
the live ruleset is deliberately migrated; the workflow display name is not an
additional prefix in the context string.

The ruleset also requires a PR, an up-to-date base for required checks and
resolved review threads, forbids deletion/force pushes, and has no bypass actors.
Its required approving-review count is zero. That does not replace the separate
agent review required by [the agent protocol](../agent-protocol.md).

The owner has delegated reviewed merges to coordinating agents. Repository
auto-merge is disabled; a coordinator can use a normal protected merge after
review and CI without changing that setting. No protection or check is disabled
by this policy. Inspect live settings before merging; this page is a dated
description, not a substitute for GitHub's enforcement.
