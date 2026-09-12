# Unresolved: how a core actuator change lands without un-completing the foundation

Status: **open, waiting on the owner.** Raised 2026-09-12 from pull request #471.

## What is blocked

Pull request #471 adds a fail-closed gate to the runtime power-management
actuator: a device whose class cannot be read is skipped instead of being
autosuspended on the assumption that it is harmless. The code is reviewed and
its tests pass. It cannot merge.

The `optid package contract` check is red because the change edits files that
are declared proof paths for seven packages currently marked `completed`:
capability-state freezing (`F1`), injectable kernel I/O (`F2`), versioned
outcomes (`F3`), desired-state reconciliation (`F4`), write-ahead transactions
(`S2D`), the sealed capability table (`S4D`), and measured latency contracts
(`C1`). Touching `actuator.rs`, `envelope.rs`, `tests_impl.rs` or the reconciler
test files invalidates the cold-verification receipts that justify calling those
packages complete.

The rule is doing its job. It exists so a receipt cannot keep asserting "pass"
against an older commit while later changes modify the files it verified.

## Why no agent should resolve this alone

The validator names two remedies and a builder can take neither:

1. **Write fresh receipts.** `AGENTS.md` forbids a builder certifying its own
   package, and the existing receipts record an independent external verifier.
   An agent arranging its own verification would be inventing an approval path.
2. **Demote the seven packages to `merged_incomplete`.** That un-completes the
   foundation. Dependencies never unlock from `merged_incomplete`, so it
   cascades — the runtime power-management package depends on four of the seven
   and would block itself. It also breaks the rule that one pull request updates
   exactly one ledger entry.

## Why this is not only about one pull request

Nothing on `main` has touched `actuator.rs`, `envelope.rs` or `tests_impl.rs`
since the receipts were refreshed on 2026-08-31. The preceding slice (#469)
passed only because it touched `runtime_pm.rs` and the ledger. Pull request #471
is the first change to hit this, and **every** future change to the core actuator
will hit it the same way. The question is how core work proceeds, not what this
diff should say.

## Options, as they look from here

1. **Re-verify on demand.** A cold verifier refreshes the affected receipts as
   its own pull request whenever a core change lands. Keeps the guarantee exactly
   as strong; costs a verification pass per core change, and needs a named
   verifier who is not the builder.
2. **Narrow the declared proof paths.** Several packages declare whole shared
   files when only part concerns them. Fewer changes would collide. This weakens
   the guarantee and needs care to avoid declaring so little that the check stops
   protecting anything.
3. **Let a core change carry a demotion without cascading.** Would need a
   deliberate change to the dependency rule, and risks the foundation sitting
   incomplete indefinitely.

**Recommended: option 1, with option 2 as follow-up work if the cost proves
real.** Re-verifying on demand is the only option that keeps the guarantee at
full strength, and the guarantee is the reason anyone can trust a `completed`
mark. The cost is one verification pass per core change, which is bearable
because core changes are rare — nothing touched these files for the two weeks
before this one. If that cost turns out to bite, narrowing the declared proof
paths reduces how often the collision happens without weakening what a receipt
asserts, and can be done later on evidence rather than guessed at now.

What the recommendation does not settle, and what makes this the owner's call
rather than an agent's: who counts as the cold verifier. The existing receipts
name an independent external verifier. Whether an agent arranged by the project
can fill that role goes to how much the completion mark is worth, and an agent
answering that question for itself would be deciding its own authority.

## What remains safe to continue meanwhile

Work that does not touch the declared proof paths above. The storage depth
package (`D2`) and the three research packages (`R1`, `R2`, `R3`) are marked
ready to run in parallel and are unaffected.

## A separate, smaller finding

`bash tools/finish-work.sh --dry-run` cannot catch this before a commit exists.
The freshness check compares `verified_commit..HEAD`, so an uncommitted working
tree is invisible to it: the dry run passes and the failure only appears once the
commit is made, in continuous integration. The pre-commit gate structurally
cannot protect against this particular failure. Worth fixing whichever way the
decision above goes.
