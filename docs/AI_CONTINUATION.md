# Agent Start Pointer

This compatibility file exists for older links. The canonical instructions are
[`AGENTS.md`](../AGENTS.md); do not maintain a second project-state narrative
here.

## Next Task

Read the [package ledger](plans/optid-package-status.toml) for active work and
the [README](../README.md) for current user-facing status. Never infer package
completion from a merged pull request.

For safety work, read the amendment first:
[D2 fail-passive](architecture/optid-d2-amendment.md).

## Forbidden Shortcuts

- Do not invent branches, pull requests, tests, or evidence.
- Do not bypass the package ledger or claim completion from isolated tests.
- Do not merge; only the human maintainer merges.
- Do not duplicate project policy in provider-specific hooks or command files.

Validate changes with:

```sh
bash tools/finish-work.sh --dry-run
```
