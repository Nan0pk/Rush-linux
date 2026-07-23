# Branch protection: one stable required check

Require exactly `PR Gate / PR Gate` on `main`. The aggregator fails unless the
always-on repository/Linux lane and every path-selected Windows or image lane
pass.

The implementation lives in `.github/workflows/ci.yml`. Individual lane names
may evolve without repeatedly editing branch protection; the aggregator is the
stable contract.

Labeling, release drafting, publishing, scheduled maintenance, pages, and
reassessment workflows are not quality gates. They create artifacts or report
state and must not be configured as required PR checks.

Only the human maintainer merges. Automatic merge remains disabled for agents
and repository automation.
