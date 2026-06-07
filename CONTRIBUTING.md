# Contributing

Rush Linux is early-stage OS engineering. Contributions must preserve the
project direction: modern Linux defaults, one adaptive policy owner, and
explainable performance behavior.

## Before You Change Code

Read:

- `PROJECT_BRIEF.md`
- `AI_CONTINUATION.md`
- `IMPLEMENTATION_STATUS.md`
- `docs/versioning.md`
- `docs/release-policy.md`
- `docs/release-checklist.md`
- `docs/architecture.md`
- relevant ADRs under `docs/decisions/`
- `docs/graphify-knowledge-graph.md`

For orientation, query the committed graph before broad source scans:

```sh
graphify query "what files are related to the change I am about to make?" --graph graphify-out/graph.json
```

## Required Checks

Linux (native or container) is the canonical development environment. Run the
full set on Linux:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pwsh ./tools/validate-repo.ps1   # cross-platform repository-policy check
```

On Windows, `tools/validate-repo.ps1` can run via
`powershell -ExecutionPolicy Bypass -File .\tools\validate-repo.ps1`, but code
must still be built and tested on Linux (CI is Linux-only).

After code or supported config changes, refresh the local Graphify graph without
LLM/API token use:

```sh
./tools/graphify-refresh.sh code
```

Use `./tools/graphify-refresh.sh full --backend <backend>` only when semantic
document extraction is needed and the backend credentials are available.

## Documentation Is Required

Docs are part of acceptance criteria. Update docs in the same change when you
modify:

- `optid` or `optctl` behavior;
- optimizer policy;
- systemd units, cgroups, or slices;
- kernel fragments;
- boot, UKI, update, or rollback flow;
- package recipes or edition profiles;
- hardware support policy;
- benchmark scenarios or release criteria.
- version, channel, milestone, or release-gate policy.

Follow `docs/documentation-policy.md` for the required documentation surface.
Pull requests or commits that change behavior without matching docs should be
treated as incomplete, even if tests pass.

## Defaults Policy

Do not add obsolete or near-obsolete components as defaults. Compatibility
packages can exist later, but defaults must stay aligned with the accepted ADRs.

## Commit Quality

- Keep changes scoped.
- Prefer deterministic policy before ML or heuristic sprawl.
- Include validation output in pull requests.
- Do not weaken guardrails to make a benchmark look better.
