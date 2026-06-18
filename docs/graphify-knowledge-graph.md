# Graphify Knowledge Graph

Rush Linux keeps a Graphify knowledge graph on the `graphify-data` branch so
future agents and maintainers can orient themselves with scoped graph queries
before reading large parts of the repository.

## Purpose

The graph is an acceleration layer, not a source of truth. Source code, config,
recipes, docs, and release files remain authoritative. Use the graph to find the
smallest useful context set, then verify behavior in the referenced files.

Primary benefits:

- persistent cross-session map for AI agents and humans;
- lower token use through `graphify query`, `graphify path`, and
  `graphify explain` before broad file reads;
- AST-only refresh path for code changes that does not spend LLM/API tokens.

## Storage Location

**As of 2026-06-10, graph artifacts live on the `graphify-data` branch**, not
on `main`. This keeps main branch history clean — previously 37% of commits were
`chore: refresh graphify knowledge graph [skip ci]`.

Nothing under `graphify-out/` is tracked on `main` (the directory is fully
git-ignored there). The `graphify-data` branch is a **disposable snapshot**: CI
force-replaces it on each refresh from an orphan commit, so it carries no
history of its own. Never base work on it; this document is the canonical
reference for where the data lives and how to fetch it. No workflow ever
pushes to `main`.

### Fetching the Graph

```bash
# Fetch the dedicated branch
git fetch origin graphify-data:graphify-data

# Checkout artifacts to your working tree
git checkout graphify-data -- graphify-out/
```

Or browse online:
https://github.com/Nan0pk/Rush-linux/tree/graphify-data/graphify-out

## Committed Artifacts (on graphify-data branch)

These files are updated by CI:

```text
graphify-out/graph.json          Machine-readable graph for query/MCP use
graphify-out/GRAPH_REPORT.md     Human-readable highlights and communities
graphify-out/graph.html          Offline interactive visualization
graphify-out/.graphify_labels.json
```

Do not commit local-only cache/cost/mtime files. On `main`, `.gitignore`
excludes the entire `graphify-out/` directory; CI publishes only the snapshot
artifacts above to the `graphify-data` branch.

```sh
uv tool install graphifyy
```

Alternatives:

```sh
pipx install graphifyy
python3 -m pip install --user graphifyy
```

The package name is `graphifyy`; the command is `graphify`.

## Agent Workflow

Before broad search or multi-file reads, prefer graph queries:

```sh
graphify query "how do optid decisions reach optctl output?" --graph graphify-out/graph.json
graphify explain "Decision" --graph graphify-out/graph.json
graphify path "optid" "systemd" --graph graphify-out/graph.json
```

Read `graphify-out/GRAPH_REPORT.md` for broad architecture orientation. Read raw
files only after the graph identifies the likely files/symbols.

## Refresh Modes

Use the repository wrapper so every environment runs the same commands.

### Code-only refresh: no API tokens

```sh
./tools/graphify-refresh.sh code
```

This runs Graphify's AST/local update path and is safe for routine use after
Rust, shell, PowerShell, JSON, or other supported code/config changes. It also
regenerates communities, `GRAPH_REPORT.md`, and `graph.html`.

### Full semantic refresh: may use backend tokens

```sh
GEMINI_API_KEY=... ./tools/graphify-refresh.sh full --backend gemini
# or a local backend:
./tools/graphify-refresh.sh full --backend ollama
```

Use full mode when Markdown/YAML/design-document changes should be semantically
re-extracted. This is intentionally explicit because documentation is large and
LLM-backed extraction can spend tokens.

### Local automatic refresh hooks

Git hooks cannot be cloned through normal Git, so each developer or agent should
install them once per clone:

```sh
./tools/graphify-refresh.sh install-hooks
./tools/graphify-refresh.sh hook-status
```

The hooks refresh the code graph after commits and branch switches. They skip
commits that only change `graphify-out/` to avoid loops.

## GitHub Automation

`.github/workflows/graphify.yml` refreshes the code graph on pushes to `main`
and can be run manually. Manual runs can request `full` mode when the repository
has the required backend secrets configured.

## Acceptance Rules

When a change affects code or supported config, refresh the graph in the same
change:

```sh
./tools/graphify-refresh.sh code
git add graphify-out/ docs/graphify-knowledge-graph.md AGENTS.md .agents/ .codex/
```

When a change affects important documentation or release-policy text, either:

1. run full semantic refresh with an available backend, or
2. state in the PR/commit notes that only the code graph was refreshed and full
   document extraction still needs a backend-enabled run.

Never treat the graph as permission to skip the existing Rush Linux validation
rules. Continue to run the Rust and policy checks documented in
`docs/AI_CONTINUATION.md` and `CONTRIBUTING.md`.
