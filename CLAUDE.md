# Claude Code Index — Rush Linux

Tight index. Full agent protocol lives in `AGENTS.md`; project docs in `docs/`.
Read `AI_CONTINUATION.md` before non-trivial changes.

## Build / Test / Lint

```sh
cargo build --workspace
cargo test  --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
pwsh ./tools/validate-repo.ps1          # repo policy
python3 tools/validate-doc-sync.py       # doc/docmap drift
```

## Session lifecycle (required)

```sh
bash tools/start-work.sh  "what you're about to do"
bash tools/finish-work.sh "commit message"
```

If you must leave mid-work, fill `DIRTY_STATE.md`. See `AGENTS.md` §Session Lifecycle.

## Folder map

| Path                  | Purpose                                          |
| --------------------- | ------------------------------------------------ |
| `crates/optid/`       | Adaptive optimization daemon                     |
| `crates/optctl/`      | CLI (status, mode, explain, trace, benchmark)    |
| `config/optid/`       | Default policy                                   |
| `distro/{boot,kernel,network}/` | UKI, kernel fragments, nftables baseline |
| `packaging/systemd/`  | Units + tmpfiles                                 |
| `recipes/`            | Source package recipe skeletons                  |
| `release/`            | Version milestones, test-tier gates              |
| `tools/`              | Local validation scripts                         |
| `docs/`               | Architecture + impl notes (see `docs/docmap.toml`) |

## Discovery rules (token-economy)

- Prefer the **codegraph** MCP tools over `grep`/`glob`/`Read` for code discovery; fall back to grep only when codegraph misses.
- Filter command output with `grep`/`head` **before** reading it; never `cat` a large file you only need lines from.
- When dispatching subtasks, name **exact files and line ranges** instead of "look in optid".
- For broad architecture, prefer `docs/docmap.toml` and `docs/architecture.md` over file-by-file reads.

## Doc-sync rule (non-negotiable)

Code change → update docs covered by `docs/docmap.toml` → bump `last_verified` → run `python3 tools/validate-doc-sync.py` → commit code+docs+docmap together. Detail in `docs/contributing/keeping-docs-synced.md`.

## Design rules (do / don't)

- cgroup v2 only · Wayland-first · PipeWire · nftables · UKI-first boot · eBPF/PSI observability.
- Do **not** run TLP, power-profiles-daemon, or TuneD as active policy. `optid` owns the knobs.
- Avoid legacy defaults unless no modern alternative works.

## Evidence rule

Exit-criterion ✓ requires an embedded command transcript (literal command, literal output, date, host). `bash -n` is not a test. Builders never certify their own work — see `AGENTS.md` §Verifier Protocol.

## Compact instructions

When compacting this conversation, **preserve**: code changes, test results, open task state, current branch, unresolved decisions. **Drop**: exploration transcripts, redundant file reads, tool-output dumps already acted on.

## Token-optimization stack

Local-machine setup for CodeGraph + Headroom is in `docs/token-optimization-setup.md`. Repo-side settings live in `.claude/settings.json`.
