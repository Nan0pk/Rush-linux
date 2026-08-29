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

Run all locally relevant checks with:

```sh
bash tools/checks.sh
```

To run every cloud lane regardless of the latest changed paths, use the
**Run workflow** action for **PR Gate**. A manual run exercises the complete
Linux, Rust, Python, shell, PowerShell, native Windows, testOS, desktop image,
UEFI boot, and update/rollback matrix. It remains simulation proof: it does not
promote hardware allowlist entries or performance claims.

Run the exact CI section shown in a failed step with:

```sh
bash tools/checks.sh --section <section>
```

## Reading a failed run

The first source of truth is GitHub job and step metadata, not the combined log.
The **Linux and repository checks** job exposes each logical check as a separate
named step. Its name includes the local reproduction section:

| Failed step | Local reproduction |
|---|---|
| Repository integrity | `bash tools/checks.sh --section integrity` |
| Documentation truth | `bash tools/checks.sh --section docs` |
| optid package contract | `bash tools/checks.sh --section optid` |
| Evidence integrity | `bash tools/checks.sh --section evidence` |
| Repository policy | `bash tools/checks.sh --section policy` |
| Workflow syntax | `bash tools/checks.sh --section workflow` |
| Shell entry points | `bash tools/checks.sh --section shell` |
| PowerShell parsing | `bash tools/checks.sh --section powershell` |
| Python and tooling | `bash tools/checks.sh --section python` |
| Rust workspace | `bash tools/checks.sh --section rust` |

An agent should inspect the failed named step through the Checks page or workflow
jobs API before opening logs. Open only that step's log for the detailed tool
output. The runner also emits a GitHub error annotation and an indexed blocker
summary with the exact command and exit status.

**PR Gate** is only the lane aggregator. The legacy **Rust**,
**Documentation sync**, **Repository policy**, and **Evidence integrity
(Dragnet)** statuses are branch-protection compatibility aliases. They may all
turn red from one underlying failure and are never independent root causes.

Rust dependency policy is a separate conditional job. Keeping it outside the
Linux job prevents a skipped container action from flooding the log before the
real failed check.

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
