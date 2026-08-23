# 2026-08-23 gap-analysis follow-ups

A dated workspace/session plan (see `docs/plans/current-work.md` §"Do not
select current work from these files" — this is history/handoff, not the
package selector). It records a to-do list an agent produced from a wholistic
review of `optid`'s logic, its 2026-08-22/23 run history, and the gap between
the two, and tracks progress against it across sessions.

This work is adjacent to, not a replacement for, the package ledger's
`active_general`/`active_safety` selection in `optid-package-status.toml`. It
exists because a human asked for it directly (AGENTS.md §3's top-priority
source), not because the ledger nominated it.

## Status legend

`todo` · `in-progress` · `blocked-on-human` · `blocked-on-hardware` · `done`

## Items

1. **`LatencyCritical` unreachable on battery** (`policy.rs:857`) —
   `done` (code), `blocked-on-hardware` (verification). Fixed in PR #423
   (`gamemoderun` pin, decision A of
   `docs/inbox/2026-08-22-phase-d-latency-critical-blocked.md`). Needs a real
   on-battery capture with `gamemoderun` installed to confirm
   `class_pinned_via_gamemode` actually lands — not attempted here because it
   requires unplugging the charger and running a fullscreen GL app for
   ~25 minutes on the machine this session is running on, which needs the
   human present, not an autonomous call.

2. **Silent global S5D circuit latch — no operator-facing surfacing** — `todo`.
   Add a circuit-state read path to `optctl`/`dbus.rs`; today the only tell is
   grepping `control-cycles.jsonl` by hand, which is how two silent no-op runs
   went unnoticed on 2026-08-22.

3. **Duplicated `[not set]` parsing** across `recovery.rs` and
   `reconciler/mod.rs` — `todo`. The bug is fixed (`5c3dfb2`); the two copies
   are still cross-referenced by comment only, not unified. Extract the shared
   logic so a future placeholder-handling fix can't land in only one copy
   again.

4. **`shim::detect_conflicts` bypasses the F2 injectable boundary** — `todo`.
   `crate::run()` calls `systemctl is-active` directly instead of going
   through `KernelIo`/`EventSource`, which is why
   `s2d_production_daemon_run_uses_persistent_transaction_protocol` is red on
   any host running `tuned` (host-environment-dependent, not a regression, but
   still the one behavioral fork in the daemon not covered by fault-injection
   testing).

5. **`input-latency-p95/p99-ms` has no probe** — `blocked-on-human`. Needs an
   owner decision: build the probe (`evemu` + frame observation) or formally
   accept `frametime-p95/p99-ms` + `foreground-launch-ms` as Criterion 2's
   metrics. Not something to decide unilaterally.

6. **Desktop slot unfilled for Phase D Criterion 2** — `blocked-on-human`, not
   a code problem. Needs a second physical machine.

7. **Evidence-to-verdict gap** — captures get mined for bugs,
   `release/evidence/host-bench/2026-08-23-victus/VERDICT.md` stays a
   template — `blocked-on-hardware`. Re-run the capture now that item 1 is
   fixed, and actually fill in a verdict this time instead of treating the run
   as a bug hunt.

8. **Missing benchmark config silently degrades instead of failing loudly**
   (`policy-enforce-measurement.toml` not found → fell back to curated
   baseline) — `todo`. Make the capture path hard-fail on a missing declared
   policy file.

9. **Meta-gap: `completed` in the package ledger means "cold-verified under
   injected faults," not "observed correct on real hardware"** — `todo`
   (as a proposal, not a unilateral change). Worth a decision doc under
   `docs/decisions/` proposing physical-hardware exposure as its own
   verification gate, given the first real `--apply` run alone surfaced 4
   defects across subsystems already marked `completed`. This changes
   methodology, so per AGENTS.md §3 it needs to go up through the decision
   process, not land as code.

## Sequencing this session

Working set, in order, each as its own branch/PR (repo convention: one
package/fix-sized change per PR, `docs/plans/current-work.md` §"Agent rules"):

1. Item 4 (F2 boundary) — well-scoped, has a concrete failing test as its
   acceptance check.
2. Item 3 (unify duplicated placeholder logic).
3. Item 2 (circuit-state surfacing).
4. Item 8 (hard-fail on missing capture policy file).

Items 1 and 7 need the human at the physical machine (battery unplug,
fullscreen app); items 5, 6, and 9 need a decision, not code. All are recorded
here rather than attempted, so the queue survives a session boundary.

Update the status markers above as each item lands; do not delete this file
until every item is `done` or explicitly deferred with a reason.
