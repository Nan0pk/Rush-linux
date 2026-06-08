# Agent Instructions

Before changing Rush Linux, read `AI_CONTINUATION.md` and the relevant docs it
links. Preserve the project guardrails: modern Linux defaults, one adaptive
policy owner (`optid`), explainable behavior, and documentation updates in the
same change.

## Doc Management (REQUIRED)

This project uses a documentation sync system. Before AND after making changes:

1. Read `docs/docmap.toml` to find which docs cover the code you're changing
   (check `covers_code` fields).
2. After changes, update every affected doc and bump its `last_verified` date
   in `docs/docmap.toml`.
3. Run `python3 tools/validate-doc-sync.py` — it must pass before committing.
4. Commit code + docs + docmap changes together.

See `docs/contributing/keeping-docs-synced.md` for the full guide.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community
structure, and cross-file relationships. If you have the `graphify` CLI
available (GitHub Copilot/Codex), use it:

When the user types `/graphify`, invoke the `skill` tool with `skill: "graphify"` before doing anything else.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- Dirty graphify-out/ files are expected after hooks or incremental updates; dirty graph files are not a reason to skip graphify. Only skip graphify if the task is about stale or incorrect graph output, or the user explicitly says not to use it.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).

**If you do NOT have the `graphify` CLI** (e.g., you are Claude, Gemini, or
another non-GitHub model), skip graphify entirely. Instead, read
`docs/docmap.toml` for doc relationships and `graphify-out/GRAPH_REPORT.md`
for a static architecture overview. The docmap is the more reliable source for
cross-reference information.
