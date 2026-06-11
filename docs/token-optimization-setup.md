# Token-Optimization Stack — Local Machine Setup

This document covers tools that must be installed on your **local development
machine** (not the ephemeral Claude Code web sandbox). The web sandbox is
reclaimed after inactivity, so anything installed there does not persist.

The repo-side pieces of this stack are already in place:

- `CLAUDE.md` — tight index with discovery + compact rules.
- `.claude/settings.json` — `MAX_THINKING_TOKENS=10000`.
- `.claude/agents/README.md` — subagent default-model guidance.

What follows is the local-machine work.

## 0. Pre-flight

Confirm these are present on your workstation:

```sh
git --version
curl --version | head -1
jq --version
python3 --version            # require 3.10+ for headroom-ai
pip --version
node --version               # require 18+ for codegraph
```

## 1. CodeGraph (code knowledge graph)

Project: <https://github.com/colbymchenry/codegraph>

Install via npm (avoids piped shell execution):

```sh
npm i -g @colbymchenry/codegraph
codegraph --version
```

Wire it into Claude Code only (skips Cursor / Gemini):

```sh
codegraph install            # interactive — pick "Claude Code" only
```

Initialize the index for this repo:

```sh
cd <path-to>/Rush-linux
codegraph init -i
ls -la .codegraph/           # expect a populated DB dir
```

Verify with one query against the optid daemon:

```sh
# example — list callers of a core function
codegraph query "callers of optid::policy::decide"
```

The repo already keeps a complementary Graphify graph under `graphify-out/`
(see `docs/graphify-knowledge-graph.md`). CodeGraph is the **MCP-callable**
graph; Graphify is the offline-refresh graph. Both can coexist.

**Disable / remove:**

```sh
codegraph uninstall          # removes MCP wiring from Claude Code
rm -rf .codegraph            # drop the per-repo index
npm uninstall -g @colbymchenry/codegraph
```

## 2. Headroom (API-layer compression)

Project: <https://pypi.org/project/headroom-ai/>

Install with `pipx` (preferred) or `pip --user` (no system Python pollution):

```sh
pipx install 'headroom-ai[all]'         # preferred
# or
pip install --user 'headroom-ai[all]'
headroom --version
```

Wrap Claude Code so every future session routes through it. The wrap command
prints a shell function — add it to your shell rc:

```sh
headroom wrap claude >> ~/.bashrc       # or ~/.zshrc
exec $SHELL
```

Confirm RTK shell-output compression is on (it ships enabled in `[all]`):

```sh
headroom config show | grep -i rtk
```

**Important:** do not enable any component that sends data off-machine. Keep
`headroom config show` audited; the relevant keys are `telemetry`, `cloud`,
and `share_*` — all should be `false` / unset.

Verify interception works on a trivial command:

```sh
headroom stats --reset
claude -p "print hello"                 # any short test
headroom stats                          # expect non-zero intercepted tokens
```

**Disable / remove:**

```sh
# remove the wrap line from your shell rc, then:
pipx uninstall headroom-ai   # or:  pip uninstall headroom-ai
```

## 3. Checking savings

| Check                    | Command                                  |
| ------------------------ | ---------------------------------------- |
| Per-session Claude cost  | `/cost` inside a Claude Code session     |
| Headroom interception    | `headroom stats`                         |
| CodeGraph index health   | `codegraph status` in repo root          |
| Doc-sync drift           | `python3 tools/validate-doc-sync.py`     |

## 4. Layer-by-layer disable

| Layer            | Disable command                                                |
| ---------------- | -------------------------------------------------------------- |
| CLAUDE.md        | `git rm CLAUDE.md && git commit`                               |
| settings.json    | `git rm .claude/settings.json && git commit`                   |
| Headroom wrap    | Remove the `headroom wrap` line from `~/.bashrc`               |
| Headroom pkg     | `pipx uninstall headroom-ai`                                   |
| CodeGraph MCP    | `codegraph uninstall`                                          |
| CodeGraph pkg    | `npm uninstall -g @colbymchenry/codegraph`                     |
| Thinking budget  | Remove `MAX_THINKING_TOKENS` from `.claude/settings.json`      |

## 5. Idempotency notes

- `codegraph init -i` is safe to re-run; it incrementally refreshes.
- `headroom wrap claude` appended twice to a shell rc duplicates the function
  definition (harmless but ugly). Guard with `grep -q 'headroom wrap claude' ~/.bashrc ||` before appending.
- `.claude/settings.json` re-write is a literal overwrite — keep edits in git.
