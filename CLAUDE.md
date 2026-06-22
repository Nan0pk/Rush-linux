# CLAUDE.md

> **START HERE — Dragnet:** before analyzing the project or deciding "what's next,"
> run `python3 tools/dragnet.py --observe` and read the newest report in
> `release/evidence/dragnet/`. It is the evidence-integrity gate and the project's
> current state of truth; the ledger shows what evidence is still owed. A milestone
> criterion is real only when `release/milestones.toml` marks it `verified = true`
> **with** a committed `transcript`. See `docs/dragnet-protocol.md`.

Guidance for Claude Code working in this repo. Kept short on purpose — this file
is loaded into every turn. See `AGENTS.md` for the full session/verifier
protocol; this file is the fast path.

## What this is

Rush Linux — a source-built, adaptive Linux architecture. The engineering work
lives in a Rust workspace. Core daemon is `optid`; CLI is `optctl`.

## Workspace layout

- `crates/optid/` — the adaptive optimization daemon (sensors, policy, decision,
  actuator, contracts). Main logic lives here.
- `crates/optctl/` — CLI to trace/override policy.
- `crates/rushbench/` — benchmark harness (probes, energy, report).
- `crates/rush_telemetry/` — telemetry collection library.
- `docs/` — documentation; `docs/docmap.toml` maps docs to the code they cover.
- `tools/` — build, benchmark, signing, and validation scripts.

## Build & test (scope to a crate to keep output small)

```bash
cargo check -p optid        # fast feedback while editing one crate
cargo test -p optid         # test a single crate
cargo build --workspace     # full build
cargo test --workspace      # full test run
cargo fmt --all             # format
cargo clippy --workspace    # lint
```

Prefer the `-p <crate>` form during iteration — full-workspace output is large
and gets re-sent on every later turn.

## Session protocol (from AGENTS.md)

- This project uses a Builder/Verifier separation and an Evidence Rule: success
  claims need a literal command transcript. Don't certify your own work.
- Docs are synced to code via `docs/docmap.toml`. When you change code, update
  the docs it `covers_code` and run `python3 tools/validate-doc-sync.py`.
- `tools/start-work.sh` / `tools/finish-work.sh` wrap the full lifecycle
  (pull, validate, commit, push). `finish-work.sh` runs all validators.

## Conventions

- Rust edition 2021, MSRV 1.78.
- Keep code + docs + `docmap.toml` changes in the same commit.
- No marketing claims in docs without supporting evidence.

## Skip graphify

If you don't have the `graphify` CLI (you, Claude, generally won't), don't try to
use it. Use `docs/docmap.toml` for cross-reference instead.
