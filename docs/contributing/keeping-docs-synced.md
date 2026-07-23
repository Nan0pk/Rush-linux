# Keeping Documentation in Sync

Rush keeps user commands and project state current without registering every
historical file as active truth.

## Three small indexes

- `docs/docmap.toml` maps maintained user, architecture, workflow, and release
  documents to the code or state they describe.
- `docs/decisions/README.md` indexes architecture decisions. Each ADR carries
  its own status.
- `docs/research/README.md` indexes numbered research papers. Research is
  context, not accepted direction.

Archives, old plans, and `docs/inbox/` do not belong in the active docmap.

## What automation checks

`python3 tools/validate-doc-sync.py` verifies:

- every active doc and dependency exists;
- every numbered research paper appears in the research index;
- current versions agree;
- ADR statuses are valid;
- important internal links resolve;
- the optid plan, package ledger, safety amendment, README, and agent contract
  agree;
- maintained docs have a recent `last_verified` date.

`python3 tools/render-frontpage.py --check` independently verifies the
generated README status and every advertised command target.

## Updating documentation

1. Change the smallest document that owns the affected behavior.
2. Update `README.md` only for a public command, project-stage, or generated
   status change.
3. If an active mapped document changed, update its `last_verified` date.
4. Add a research paper to `docs/research/README.md`; add an ADR to
   `docs/decisions/README.md`.
5. Run:

   ```sh
   bash tools/finish-work.sh --dry-run
   ```

For common code areas:

- `optid` behavior → `docs/adaptive-engine.md` and implementation status when
  feature status changed;
- kernel policy → `docs/kernel-policy.md`;
- image/package flow → `docs/build-system.md` or
  `docs/packaging-and-builds.md`;
- LiveDev behavior → the user/operator guide that exposes it;
- release truth → `VERSION`, `RELEASES.md`, `ROADMAP.md`, and the milestone or
  evidence records that support the claim.

Do not touch unrelated docs merely to satisfy a gate. If the documented truth
did not change, no documentation rewrite is required.
