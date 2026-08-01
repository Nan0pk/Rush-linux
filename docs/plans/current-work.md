# Current Work — Agent Entry Point

This file answers one question: **what should an agent work on next?**

For `optid`, the machine-readable source of truth is
[`optid-package-status.toml`](optid-package-status.toml). This page is a
validated, human-readable projection of that ledger. When the values below
disagree with the ledger, the ledger wins and CI must fail until this page is
updated.

Read these before implementation:

1. [`AGENTS.md`](../../AGENTS.md)
2. [`OPTID-COMPLETION-PLAN.md`](../../OPTID-COMPLETION-PLAN.md)
3. [`optid D2 fail-passive amendment`](../architecture/optid-d2-amendment.md) for safety work
4. [`optid-package-status.toml`](optid-package-status.toml)

<!-- RUSH_CURRENT_WORK:START -->
```toml
active_general = "F1"
active_safety = "D0"
ready_parallel = ["R1", "R2", "R3"]
other_merged_incomplete = ["F3", "F4", "T1"]
unlocks_after_active_general = ["F3"]
unlocks_after_active_safety = ["S1D"]
```
<!-- RUSH_CURRENT_WORK:END -->

## How to use the selector

- `active_general` is the package to repair in the general construction lane.
- `active_safety` is the package to repair in the safety lane. Read the D2
  amendment before touching it.
- `ready_parallel` packages may proceed without replacing either active lane.
- `other_merged_incomplete` records unfinished landed work; it is not a second
  task queue.
- `unlocks_after_active_general` and `unlocks_after_active_safety` show the
  packages whose dependency sets become satisfied if the corresponding active
  package reaches `completed` while the rest of the ledger is unchanged.

For a selected package, read its ledger entry and the matching packet in the
completion plan. The ledger provides the title, status, dependencies, blocking
reason, runtime entry points, acceptance tests, and evidence requirements.

A builder may move a package only as far as `candidate`. `completed` requires a
separate cold verifier, a committed receipt, satisfied dependencies, and all
package acceptance items.

Dependencies never unlock from `candidate` or `merged_incomplete`.

## Do not select current work from these files

The following are useful history or release views, but they are not the current
package selector:

- `docs/plans/agent-work-plan-v1.md`
- `docs/plans/work-plan-v2.md`
- dated workspace/session plans and handoff fragments
- `docs/plans/livedev-progress.json`
- `ROADMAP.md` and `release/milestones.toml` when choosing an `optid` package

Release milestones answer whether a release claim is proven. The package
ledger answers which `optid` construction work is active.

## Agent rules

- Read `active_general` and `active_safety` at execution time; do not copy their
  current values into permanent instructions.
- Work on one coherent package-sized change.
- Use `bash tools/start-work.sh "short task description"` before editing.
- Use `bash tools/finish-work.sh --dry-run` before committing.
- Open a draft pull request; never merge or enable auto-merge.
- Never self-certify a package as `completed`.

Validate this page with:

```sh
python3 tools/validate-current-work.py
```
