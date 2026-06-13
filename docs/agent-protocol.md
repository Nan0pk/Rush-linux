# Agent Work Protocol (v2)

This document is the single canonical source for the evidence rule, builder/verifier/human roles, and the authority matrix.

## Evidence Rule (non-negotiable)

An exit-criterion checkmark may **only** appear next to an **embedded command transcript**: the literal command, literal output (or attached log file), date, and host description.  
"The script implements X" is a description, not evidence.  
`bash -n` is a syntax check, not a test run.  
Any evidence README violating this rule is rejected at review without further reading.

## Roles

**Builder agent**  
- Executes exactly one WP per session under `tools/start-work.sh` / `finish-work.sh`.  
- Produces a branch and opens a PR.  
- May *claim* completion but **must never** *certify* it.

**Verifier agent**  
- A separate session (ideally a different model/tool than the builder).  
- Checks out the branch cold.  
- Runs the WP's acceptance block verbatim.  
- Writes a `VERIFICATION.md` report (see `docs/templates/VERIFICATION.md`) into the PR or as a comment.  
- Records each command, its exit code, and a one-line verdict per criterion.  
- Never fixes code — a failed check is a verdict, not a task.  
- Builder ≠ verifier for the same WP.

**Human (maintainer)**  
- The only role that can merge to `main`.  
- Runs hardware-dependent gates (KVM rollback test, physical benchmarks).  
- Holds production signing keys.  
- Changes milestone status.  
- Resolves disagreements between builder and verifier.

## Authority Matrix

| Action | Builder | Verifier | Human |
|--------|---------|----------|-------|
| Create branch / push commits | ✅ | ❌ | ✅ |
| Open PR | ✅ | ❌ | ✅ |
| Run acceptance commands | ✅ (self-check) | ✅ (authoritative) | ✅ |
| Mark WP criteria ✅ in evidence/docs | ❌ | ✅ (in VERIFICATION.md only) | ✅ |
| Merge to `main` | ❌ | ❌ | ✅ only |
| Edit `release/milestones.toml` status | ❌ | ❌ | ✅ only |
| Touch signing keys beyond test keys | ❌ | ❌ | ✅ only |
| Declare a gate "passed" without command transcript | ❌ | ❌ | ❌ — nobody |

## PR-only Merges

All merges to `main` happen via reviewed PRs. Direct pushes to `main` are not permitted except for emergency hotfixes by the human maintainer.
