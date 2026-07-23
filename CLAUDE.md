# Claude Code

Read and follow [`AGENTS.md`](AGENTS.md). It is the single instruction source
for repository work; provider-specific commands and verifier roles are not
separate project policy.

Start changes with:

```sh
bash tools/start-work.sh "short task description"
```

Validate them with:

```sh
bash tools/finish-work.sh --dry-run
```

Only the human maintainer merges pull requests.
