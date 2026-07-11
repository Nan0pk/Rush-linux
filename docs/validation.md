# Validation

The pull-request mechanism is defined in `docs/project-workflow.md` and run by
`tools/checks.sh`. There is one normal GitHub status: **Change checks**.

The runner selects checks from the files that changed:

- repository structure, accepted-decision ratification, versions, docs, and
  no-auto-merge safety;
- Rust format, tests, and Clippy for Rust changes;
- Python/tooling tests and hardware-evidence fixtures for tooling changes;
- shell and PowerShell parsing for changed scripts;
- evidence integrity when evidence or release truth changes;
- generated front-page consistency when its inputs change;
- dependency policy when Cargo dependencies change.

Run locally with:

```sh
bash tools/checks.sh
```

If an optional local tool is missing, the runner names the skipped area. It
does not hide the cause or block unrelated work. Pull-request CI installs the
required tools and runs that area authoritatively.

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
