# Current Work — Agent Entry Point

This file answers one question: **what should an agent work on next?**

For `optid`, the machine-readable source of truth is
[`docs/plans/optid-package-status.toml`](docs/plans/optid-package-status.toml).
This page is a validated, human-readable projection of that ledger. When the
values below disagree with the ledger, the ledger wins and CI must fail until
this page is regenerated.

Read these before implementation:

1. [`AGENTS.md`](AGENTS.md)
2. [`OPTID-COMPLETION-PLAN.md`](OPTID-COMPLETION-PLAN.md)
3. [`docs/architecture/optid-d2-amendment.md`](docs/architecture/optid-d2-amendment.md) for safety work
4. [`docs/plans/optid-package-status.toml`](docs/plans/optid-package-status.toml)

<!-- RUSH_CURRENT_WORK:START -->
```toml
active_general = "F2"
active_safety = "D0"
ready_parallel = ["R1", "R2", "R3"]
other_merged_incomplete = ["F1", "F3", "F4", "T1"]
unlocks_after_active_general = ["E1"]
unlocks_after_active_safety = ["S1D"]
```
<!-- RUSH_CURRENT_WORK:END -->

## Work now

### General lane — F2

**Introduce injectable kernel I/O, clock, and event boundaries.**

Finish the package requirements recorded under `F2` in the ledger. A builder
may move it only as far as `candidate`; `completed` requires a separate cold
verifier and a committed verification receipt.

### Safety lane — D0

**Prototype capability sealing and supervisor-managed cold restart.**

Read the D2 amendment first. Complete only the D0 acceptance items and keep the
prototype fail-safe and disabled where the accepted architecture requires it.
Do not revive the superseded permanent broker design.

### Parallel research — R1, R2, R3

These packages may proceed without replacing the active general or safety
repair. They must remain research or specification work until their own
accepted construction gates are satisfied.

## What unlocks next

- Completing **F2** makes **E1** dependency-ready.
- Completing **D0** makes **S1D** dependency-ready.
- **F3** remains locked until **F1** is genuinely `completed` again.
- Downstream dependencies never unlock from `candidate` or
  `merged_incomplete`.

The entries under `other_merged_incomplete` are real unfinished work, but they
are not an invitation to ignore the active lane selectors. Repair one only
when the ledger, package dependency graph, or an explicit maintainer direction
selects it.

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
