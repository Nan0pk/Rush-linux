## Outcome

What changed, why it matters, and what a user or developer can now do.

## Risk

Name the affected boundary and the concrete failure this change could cause.
Use `low` for docs/refactors, `medium` for runtime behavior, and `high` for
privileged writes, boot/storage, security boundaries, or release claims.

## Verification

- [ ] `bash tools/finish-work.sh --dry-run`
- [ ] The PR enters through the real behavior surface, not only a new module
      (when runtime behavior changes)
- [ ] User-facing behavior and commands are reflected in README/docs

List any check that could not run and why.

## Optid package work

Delete this section when the PR is not optid package work.

- Package ID:
- [ ] Exactly one ledger entry is updated.
- [ ] Builder state is honest (`candidate` or `merged_incomplete`, never
      self-certified `completed`).
- [ ] Production entry point, integration test, and committed evidence paths
      are recorded when claiming `candidate`.
- [ ] Cold verification is separate before `completed`.

Automation never merges this PR.
