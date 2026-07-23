# Validation

The pull-request mechanism is defined in `docs/project-workflow.md` and run by
`tools/checks.sh`. There is one stable required GitHub status: **PR Gate**.

The runner selects checks from the files that changed:

- repository structure, accepted-decision ratification, versions, docs,
  no-auto-merge safety, evidence integrity, and generated front-page
  consistency on every change;
- Rust format, tests, Clippy, and all-target/all-feature compilation for Rust
  changes;
- Python compilation, Ruff, tests, and hardware-evidence fixtures for tooling
  changes;
- parser and ShellCheck coverage for changed shell entry points;
- native PowerShell parsing and Windows parity tests for Windows/shared
  LiveDev changes;
- image and boot contract tests plus a real product image build for image/build
  changes;
- dependency policy when Cargo dependencies change.

Run locally with:

```sh
bash tools/checks.sh
```

If an optional local tool is missing, the runner names the skipped area. It
does not hide the cause or block unrelated work. Pull-request CI installs the
required tools and runs that area authoritatively. `PR Gate` fails unless every
selected lane passes.

## What a green check means

A green pull-request check means the changed repository behavior passed the
automated tests relevant to it. It does not prove a hardware claim, benchmark
win, complete milestone, or release.

Hardware and release proof is kept in `release/evidence/` and follows the
independent-verification rules in `docs/agent-protocol.md`.

## Hardware validation

Hardware validation covers foreground latency, battery use, suspend/resume,
thermals, storage and device power state, gaming frame time, realtime audio,
and server workloads. A missing hardware result blocks promotion of that exact
claim or allowlist entry. It does not block dry-run work, read-only diagnosis,
simulation, or an off-by-default prototype.
