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

2. **Silent global S5D circuit latch — no operator-facing surfacing** —
   `done` (code), `blocked-on-human` (merge). PR #427: `optctl circuits` +
   the `Circuits` D-Bus method + `circuit_breaker::render_persisted_circuits`.
   F3 and S5D receipts recertified in the same PR after their declared proof
   paths (`dbus.rs`, `optctl/main.rs`, the D-Bus XML, `circuit_breaker.rs`)
   went stale.

3. **Duplicated `[not set]` parsing** across `recovery.rs` and
   `reconciler/mod.rs` — `done` (code), `blocked-on-human` (merge). PR #426:
   extracted into a shared `systemd_placeholder.rs`, `#[path]`-included by
   both. F4, S2D, and S3D receipts recertified in the same PR.

4. **`shim::detect_conflicts` bypasses the F2 injectable boundary** —
   `done` (code), `blocked-on-human` (merge). PR #425: added
   `with_conflict_checker_override`, mirroring
   `kernel_io::with_real_kernel_override`; the previously-red
   `s2d_production_daemon_run_uses_persistent_transaction_protocol` now
   passes on this host regardless of `tuned`. S2D's receipt recertified in
   the same PR.

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
   baseline) — `done` (code), `blocked-on-human` (merge). PR #428:
   `rushbench` now reads `boot.policy_load_state` from `optctl status
   --json` and refuses to start a capture if a live daemon answers with
   anything other than `"ok"`, naming the boot state in the error. `optid`'s
   own fallback-to-curated-baseline behavior is untouched and correct by
   design; only the harness's blindness to it is fixed.

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
   acceptance check. → PR #425.
2. Item 3 (unify duplicated placeholder logic). → PR #426.
3. Item 2 (circuit-state surfacing). → PR #427.
4. Item 8 (hard-fail on missing capture policy file). → PR #428.

All four landed a real, previously-undetected side effect: each touched a
declared proof path of an already-`completed` package (F2/F3/F4/S2D/S3D/S5D
in various combinations), which staled that package's cold-verification
receipt under `tools/validate-optid-packages.py`. Each PR fixes this in the
same commit or a same-PR follow-up, following the established
builder-recertification convention (precedent in T1's `blocking_reason`
clauses 13-15): re-run every mapped acceptance test for the affected
package(s) individually with `--exact`, then bump the receipt's
`verified_commit` and record a new numbered clause. No package status
changed; none of this is independent cold verification. Worth internalizing
for any future optid change: run `python3 tools/validate-optid-packages.py
--base origin/main` on a committed diff before assuming a change is clean,
not just `cargo test`/`clippy`.

All four PRs are open as drafts, each `bash tools/finish-work.sh --dry-run`-
clean except for the one known pre-existing `tuned`-active test failure
(itself fixed by #425, which the other three branches don't include yet
since they branched before it merged). None are merged — that's the human
maintainer's call per `docs/plans/current-work.md` §"Agent rules": "Open a
draft pull request; never merge or enable auto-merge."

Items 1 and 7 need the human at the physical machine (battery unplug,
fullscreen app); items 5, 6, and 9 need a decision, not code. All are recorded
here rather than attempted, so the queue survives a session boundary.

Update the status markers above as each item lands; do not delete this file
until every item is `done` or explicitly deferred with a reason.
