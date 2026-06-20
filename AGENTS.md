# Agent Instructions

Before changing Rush Linux, read `docs/AI_CONTINUATION.md` and the relevant docs it
links. Preserve the project guardrails: modern Linux defaults, one adaptive
policy owner (`optid`), explainable behavior, and documentation updates in the
same change.

## Session Lifecycle (MANDATORY)

Every work session — whether by an AI agent or a human — must follow this
lifecycle:

### 1. Start: `bash tools/start-work.sh "what you're about to do"`

This script:
- Pulls the latest changes
- Validates the repo is in a good starting state (compiles, tests pass, docs synced)
- Checks for `DIRTY_STATE.md` (if present, previous session left work incomplete)
- Creates `DIRTY_STATE.md` to mark the repo as mid-work
- **Fails fast** if the repo is broken, so you don't build on a broken foundation

### 2. Work: Make your changes

Follow the doc management system:
- Read `docs/docmap.toml` to find which docs cover the code you're changing
- Update every affected doc
- Run `python3 tools/validate-doc-sync.py` periodically to catch drift

### 3. Finish: `bash tools/finish-work.sh "commit message"`

This script:
- Updates `last_verified` dates in `docs/docmap.toml` for changed docs
- Runs ALL validators (fmt, test, clippy, policy, doc-sync)
- Removes `DIRTY_STATE.md`
- Commits and pushes
- **Fails if anything is broken** — you must fix before it completes

### If you must leave mid-work

Edit `DIRTY_STATE.md` to fill in all fields:
- **What's done so far** — describe what you completed
- **What's left** — describe what remains
- **Known issues** — any broken tests, uncommitted changes, etc.

The next agent will read this file when they run `start-work.sh`.

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
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost). The CI will push updates to the graphify-data branch.

**If you do NOT have the `graphify` CLI** (e.g., you are Claude, Gemini, or
another non-GitHub model), skip graphify entirely. Instead, read
`docs/docmap.toml` for doc relationships and fetch GRAPH_REPORT.md from the
graphify-data branch if needed. The docmap is the more reliable source for
cross-reference information.

---

**Why the separate branch?** Main branch history was 37% graphify refresh commits.
Moving artifacts to `graphify-data` keeps `git log` focused on engineering changes.

---

## Minimal-Code Discipline (ponytail ladder)

Before writing any code, stop at the first rung that holds:

1. Does this need to exist at all? (YAGNI — skip it)
2. Does the Rust stdlib already do this? Use it.
3. Does a native platform feature cover it? Use it.
4. Does an already-imported crate in Cargo.toml solve it? Use it.
5. Can this be one line or a trivial combinator? Make it so.
6. Only then: write the minimum code that works.

Never cut input validation at trust boundaries, error handling that prevents data loss, safety invariants in `optid`'s contract layer, or anything explicitly requested. No unrequested abstractions or new dependencies. Deletion over addition.

Use `/ponytail` to invoke this check explicitly before starting a non-trivial implementation.

## Evidence Rule & Verifier Protocol (v2)

This section has been moved to the single canonical source:
👉 **[docs/agent-protocol.md](docs/agent-protocol.md)**

Builders never certify their own work. Verification is always performed by a separate verifier session.

## The Builder/Verifier Protocol & Evidence Rule

Per the `v2 Work Plan`, autonomous agents must abide by the Builder/Verifier split to prevent false-certification loops:
- **Builder Agents** execute a single Work Package, produce a branch, and open a PR. They **never certify their own work**.
- **Verifier Agents** check out a PR cold, execute the explicit acceptance commands, and write the output into `VERIFICATION.md`. A failed command is a verdict, not a task to fix.
- **The Evidence Rule**: A Work Package gate is only "Passed" if an embedded command transcript (literal input, literal output, host info) is provided. Saying "The script implements X" is insufficient. `bash -n` is a syntax check, not proof.
