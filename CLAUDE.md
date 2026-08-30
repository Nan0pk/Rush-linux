# Claude Code

Read and follow [`AGENTS.md`](AGENTS.md). It is the single instruction source
for repository work; provider-specific commands and verifier roles are not
separate project policy.

## Talk in names, not codes

Restated here because this file is loaded automatically and `AGENTS.md` is not.
The full rule is AGENTS.md section 12.

Never lead with a package code when addressing the human. Write "the thermal
sensing and budget-model package (`T1`)", not "T1". Never answer with a bare
list such as "F1-F4 and S2D-S5D" — name them, or describe the set in words and
then name whichever ones the human has to decide about. Titles are the `title`
field in [`docs/plans/optid-package-status.toml`](docs/plans/optid-package-status.toml);
read them rather than guessing.

Codes stay unchanged inside the ledger, the receipts, filenames, and tool code.
This governs how you *talk*, not how the repository is *named*.

Start changes with:

```sh
bash tools/start-work.sh "short task description"
```

Validate them with:

```sh
bash tools/finish-work.sh --dry-run
```

Only the human maintainer merges pull requests.
