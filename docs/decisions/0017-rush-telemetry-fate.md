# ADR 0017: rush_telemetry Fate Decision — In, Out, or Feature-Flagged

Status: proposed
Date: 2026-07-02
Authors: Z.ai audit (third pass)
Tags: architecture, telemetry, licensing, ci

## Context

The `crates/rush_telemetry` crate contains 2,361 lines of Rust implementing hardware probes, transport modules, and an eBPF loader. It is currently **excluded from the Cargo workspace** via the `exclude` field in the root `Cargo.toml`. The exclusion comment states the crate "does not yet compile cleanly: missing `libc` dependency, BPF skeleton codegen incomplete."

Because it is excluded, `rush_telemetry` escapes every CI gate: `cargo fmt`, `cargo clippy -D warnings`, `cargo nextest run --workspace`, `cargo audit`, `cargo deny`, and the doc-sync checks. It can silently rot (dependency drift, API mismatches with the rest of the workspace, license policy violations) with nothing to notice until someone tries to re-include it. The audit's third-pass finding #2 (Critical) further identified a concrete PSI parser bug in the crate: a 21-byte `pread` that can include a newline and break parsing because `.trim()` does not strip text after an internal newline.

The crate also declares `license = "GPL-2.0-only"` while the workspace package license is `Apache-2.0`. The workspace `deny.toml` allows Apache/MIT/BSD/Unicode-family licenses but not GPL. Reintroducing `rush_telemetry` into the workspace will trigger `cargo deny` failures unless the policy is explicitly updated, which forces the license-boundary decision to be deliberate.

### Why this decision cannot be deferred

The project's v0.7 roadmap includes desktop-facing telemetry work (foreground detection, compositor integration, GameMode shims). That work will need a telemetry crate to live in. If `rush_telemetry` is still in its current half-state when v0.7 work begins, the new desktop surface will be built on top of an already-drifting foundation, and the eventual re-inclusion will be much more expensive because it will require reconciling both the existing rot and the new surface.

## Options considered

### Option A — Fix and re-include (recommended)

Add the missing `libc` dependency, complete the BPF skeleton codegen, fix the PSI parser bug, update `deny.toml` to explicitly permit GPL-2.0-only for `rush_telemetry`, and re-include the crate in the workspace. All CI gates apply.

- **Pros:** eliminates the rot vector entirely; CI catches future drift; the crate is ready when v0.7 desktop work needs it; license boundary is made explicit and documented.
- **Cons:** 3–5 dev-days of work; requires resolving the BPF codegen (may need `libbpf-cargo` or equivalent); forces the license decision now rather than later.
- **Cost of deferral:** grows linearly with v0.7 surface area. Every new crate that wants to consume telemetry APIs either re-implements them (duplicating the existing rot) or waits for re-inclusion.

### Option B — Move out of the repo

Move `crates/rush_telemetry` to a separate repository (e.g. `rush-telemetry`) or to a long-lived branch. It is no longer workspace-adjacent; it does not drift against the main workspace because it is not present. Re-integration happens when the v0.7 telemetry work actually starts, at which point the team decides whether to vendor it back, publish it as a separate crate, or consume it via a git dependency.

- **Pros:** eliminates the silent-rot vector; the main repo's license policy stays clean; the crate's eventual fate becomes an explicit decision rather than a deferred one.
- **Cons:** fragments the project; the separate repo needs its own CI, license review, and release process; if v0.7 work starts soon, the move is wasted effort (you'll move it back).
- **Cost of deferral:** low. The crate continues to exist; it just doesn't drift against the main workspace.

### Option C — Feature-flagged CI inclusion

Re-include `rush_telemetry` in the workspace behind a `[features] default = ["stub"]` flag. The stub feature compiles the crate with mocked BPF loaders and no `libc` dependency, so `cargo check --workspace` and `cargo clippy --workspace` pass. The full build is gated behind `--features real-bpf` and runs in a separate CI job that is allowed to fail until the BPF codegen is complete.

- **Pros:** catches dependency drift and API mismatches immediately; the PSI parser bug and license split become visible; allows incremental fixes without blocking the main CI pipeline.
- **Cons:** requires designing the stub interface (which is real design work, not just a CI hack); the `real-bpf` job being allowed-to-fail can become a permanent yellow badge that everyone ignores.
- **Cost of deferral:** medium. Drift is caught, but the underlying decision (is this crate real?) is still deferred.

### Option D — Delete it

Delete `crates/rush_telemetry` entirely. When v0.7 telemetry work starts, write a new telemetry crate from scratch, informed by what the team learned from the v0.6 attempt but unencumbered by its design decisions.

- **Pros:** simplest; eliminates the rot vector entirely; no license headache; the new crate can be designed against v0.7's actual requirements rather than v0.6's anticipated ones.
- **Cons:** throws away 2,361 LOC of working hardware probe code that could plausibly be reused; loses the design exploration value of the existing crate.
- **Cost of deferral:** zero (this is the terminal state).

## Decision

**Recommendation: Option A (fix and re-include).**

The crate already contains full hardware/transport modules. The two blocking issues (libc dep, BPF codegen) are well-scoped. The PSI parser bug is a 10-line fix. The license split is plausibly necessary (eBPF loaders often need GPL for kernel symbol access) and is cheap to document. The cost of fixing it now (3–5 dev-days) is less than the cost of carrying the rot through v0.7 (which would require re-doing this work later, on top of reconciling new desktop surface).

If the maintainer cannot allocate 3–5 dev-days in the next sprint, **Option B (move out of repo)** is the next-best choice. It eliminates the rot vector without requiring the fix work, and it forces the eventual re-integration to be a deliberate decision rather than a default.

**Option C (feature-flagged CI)** is rejected because it creates a permanent allowed-to-fail CI job, which is a known anti-pattern. **Option D (delete)** is rejected because the existing code has reuse value and the design exploration is worth preserving.

## Consequences

### If Option A is accepted

- The crate enters CI. Every PR that touches it runs fmt/clippy/test against it.
- `deny.toml` is updated to explicitly permit GPL-2.0-only for this crate. The permit is scoped, not a blanket policy change.
- A new ADR (separate from this one) documents the GPL/Apache license boundary and its distribution implications (audit #6).
- The PSI parser bug is fixed before re-inclusion, with the test cases specified in audit #2.
- A `SAFETY:` comment convention is established for the `unsafe` blocks in the eBPF loader (audit #7).
- v0.7 desktop work can consume the crate directly.

### If Option B is accepted

- The crate is moved to `github.com/Nan0pk/rush-telemetry` (or a long-lived branch).
- The main repo's `crates/` directory no longer contains `rush_telemetry`.
- The main repo's license policy stays clean (no GPL exception needed).
- v0.7 desktop work begins with a deliberate re-integration decision: vendor, publish as a separate crate, or git-dependency.
- The license-boundary ADR (audit #6) is still needed, but is deferred to whenever re-integration happens.

### Reversibility

Both Option A and Option B are reversible. A→B is cheap (move the crate out; CI goes back to excluding it). B→A is more expensive (re-apply the fixes that may have been deferred), but still tractable. The decision is not load-bearing in the sense of being permanent; it is load-bearing in the sense of forcing a choice rather than continuing the half-state.

## Decision criteria

The maintainer should choose based on the answer to one question:

> **Can the team allocate 3–5 dev-days to fix `rush_telemetry` in the next sprint?**

- **If yes:** Option A. The crate is fixed, re-included, and ready for v0.7.
- **If no:** Option B. The crate is moved out, the rot vector is eliminated, and re-integration is a deliberate future decision.

Do **not** choose Option C or D unless there is additional context not captured in this ADR.

## Acceptance criteria

### If Option A:

- [ ] `crates/rush_telemetry` is removed from the workspace `exclude` list.
- [ ] `cargo check --workspace` passes locally and in CI.
- [ ] `cargo clippy --workspace -- -D warnings` passes.
- [ ] `cargo deny check` passes with the updated GPL exception.
- [ ] PSI parser bug is fixed; the four test cases from audit #2 pass.
- [ ] License-boundary ADR (audit #6) is committed.
- [ ] `SAFETY:` comments exist on every `unsafe` block in the eBPF loader.

### If Option B:

- [ ] `crates/rush_telemetry` directory no longer exists in the main repo.
- [ ] A new repo (or branch) exists with the crate's contents.
- [ ] Main repo's `deny.toml` is unchanged (no GPL exception).
- [ ] `HANDOFF.md` and `IMPLEMENTATION_STATUS.md` are updated to reflect the move.
- [ ] An issue is filed for the v0.7 re-integration decision.

## References

- Audit third pass, finding #2 (rush_telemetry excluded + concrete PSI bug + GPL trap).
- Audit third pass, finding #6 (GPL distribution trap, requires separate ADR).
- Audit third pass, finding #7 (PSI/proc parsing duplication across 4 crates).
- Audit third pass, finding #18 (v0.7 desktop expands attack surface).
- `LESSONS.md` L-001 through L-008 (evidence fabrication recurrence pattern).
- Root `Cargo.toml` exclude field and comment.
- `deny.toml` license allow-list.
