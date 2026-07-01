# Research 0020: Rush Linux Technical Debt Audit (Third Pass)

**Date:** 2026-07-02
**Auditor:** Z.ai (third-pass audit, audit-of-audits)
**Status:** Reference document — not a decision record

## Scope and epistemics

This is a third-pass audit. It builds on two prior audits:
- **A1** (debt inventory): 11 findings, numeric priority formula with one arithmetic error, phased plan.
- **A2** (threat-model framing): 12 findings, qualitative bands, "truth maintenance under acceleration" thesis.

The auditor has **not** opened the repository, run `cargo build`/`clippy`/`pytest`/`shellcheck`, or read a single source file. Every code-level fact below is inherited from A1 or A2's citations. **This is an audit of audits, plus architectural inference.** That is a weaker evidentiary basis than either prior audit claimed, and it is named loudly because the project's own thesis (per A2) is that verified claims outrun evidence — applying that standard to itself is the first test of whether it can clear it.

What this audit offers in exchange:
1. Three findings A2 missed or under-developed (restored panic-in-write-path, revert-path-untested, Dragnet-as-recursive-SPOF).
2. Two findings neither audit raised (update-signing unaudited, v0.7-as-attack-surface-expansion rather than test-coverage gap).
3. A corrected priority methodology — fixes A1's arithmetic without losing A2's qualitative texture, with concrete day-estimates and an explicit tiebreak column.
4. A phased plan that distinguishes "blocks v0.6 closure" from "blocks v1" from "blocks binary distribution" — different release surfaces, different stakeholders.

If a finding below contradicts what the actual source shows, trust the source. The auditor would rather be wrong and named than wrong and hidden.

## Priority methodology

- **Severity** (Critical / High / Medium / Low) — impact × blast radius if it fires
- **Effort** — developer-days including tests and review (concrete, not "medium")
- **Priority** = `severity_score × (6 − effort_score)` where severity is {C:5, H:4, M:3, L:2} and effort is {1d:1, 2-3d:2, 4-5d:3, 1-2w:4, 2w+:5}
- **Blocks** — what milestone or release surface this gates. Ties in Priority broken by Blocks: v0.6-closure > v1-release > binary-distribution > hygiene.

| # | Finding | Sev | Effort | Pri | Blocks |
|---|---|---|---|---|---|
| 1 | Fail-open `Policy::load` + allowlist overrides in `--apply` mode | Critical | 2-3d | 25 | v1 release |
| 2 | `rush_telemetry` excluded + concrete PSI bug + GPL distribution trap | Critical | 3-5d | 20 | v0.7, binary dist |
| 3 | Revert path is the core promise and is untested | High | 2d | 16 | v1 release |
| 4 | Evidence gate validates transcript existence, not content | High | 2d | 16 | v0.6 closure |
| 5 | "Three clocks" drift (version/evidence/impl), plus governance clock | High | 1d | 20 | v0.6 closure |
| 6 | GPL-2.0-only inside Apache-2.0 workspace — distribution implications | High | 1d | 20 | Binary dist |
| 7 | Unprivileged VM builder still broken at boot (kmod/libkmod2) | High | 1d | 20 | CI evidence loop |
| 8 | Panic mid-write in `actuator.rs`/`io_util.rs` breaks reversibility | High | 3d | 12 | v1 release |
| 9 | Governance Python scripts are CI-critical and unlinted/untested | High | 3d | 12 | All |
| 10 | Dragnet itself is a recursive single-point-of-trust, unaudited | Medium | 2d | 12 | Recursive trust |
| 11 | Update signing infrastructure unaudited | Medium | 2d | 12 | Auto-update feature |
| 12 | Systemd unit sandbox not reviewed against actual write paths | Medium | 1d | 15 | v1 release |
| 13 | ADR lifecycle validates format more than adoption truth | Medium | 1d | 15 | Hygiene |
| 14 | Shell scripts (24, several sudo) lack shellcheck | Medium | 1d | 15 | Hygiene |
| 15 | `finish-work.sh` diverges from CI; passes while CI would fail | Medium | 1d | 15 | Hygiene |
| 16 | Write-site gate is lexical; fix is type-system proof, not tests | Medium | 1-2w | 3 | v1 release |
| 17 | PSI/proc parsing 4× duplicated — symptom of missing `rush_core` pattern | Medium | 3d | 9 | Architectural |
| 18 | v0.7 desktop expands attack surface (D-Bus, portals, Wayland) | Medium | ongoing | 9 | v0.7 design |
| 19 | `Action` value types still stringly-typed | Low | 3d | 6 | v1 |
| 20 | Foreground detection is a documented stub; v0.7 gate undefined | Low | ongoing | 6 | v0.7 |

---

## Theme 1: Truth maintenance (and its recursion)

A2's thesis was "truth maintenance under acceleration." This audit extends it: **recursive truth maintenance under acceleration.** The systems that check truth are themselves unverified, so a green check doesn't prove truth — it proves the checker didn't catch a lie. Each layer inherits the epistemic gap of the layer above it.

### #1 — Fail-open `Policy::load` and allowlist overrides in `--apply` mode

**Inherited from A2.** `Policy::load` falls back to `Policy::default()` on missing/malformed input. Allowlist override loader skips malformed files with a warning. A2 calls this fail-open and flags it critical.

**The causal chain A2 left unproven — the critique, applied to itself:** the fail-open risk only bites if `Policy::default()` is actuation-capable. If the default is observe-only, failing open to defaults is safe even in `--apply` mode. A2 asserts the risk without establishing the default's behavior — the same shape-not-truth gap A2 flags for evidence gates, applied to its own claim. **This finding therefore requires one verification before it ranks #1:** open `optid`'s `Policy::default()` and confirm whether it produces any `Action::Set*` variants. If yes, Critical stands. If no, downgrade to Medium and reframe as "default policy should be explicitly observe-only, with a test asserting it."

The allowlist-override skip has the same gap. "Skip malformed override" only re-exposes a device if (a) the seeded allowlist permits that device and (b) the override was a deny. Both conditions need verification.

### #4 — Evidence gate validates transcript existence, not content

**Inherited from A2.** `validate-evidence.py` checks that `verified = true` criteria have a transcript path, and that the path is non-empty. It does not check what the transcript contains. Given the project's history of eight false-verified instances (L-001 through L-008), this is the next recurrence vector.

### #5 — Three clocks drift (plus a fourth A2 missed)

**Inherited from A2, extended.** Version clock says 0.7.0-beta.1; evidence clock says v0.6 unclosed; implementation clock says v0.6 code-complete.

**Extension:** there's a fourth clock A2 missed — the **governance clock.** When did Dragnet last run? When were ADRs last reconciled to adoption state? When was `LESSONS.md` last reviewed? Governance artifacts have their own staleness curve, and stale governance is invisible because it still *looks* governed.

### #10 — Dragnet itself is a recursive single-point-of-trust

**New finding.** Dragnet is the evidence-integrity auditor. If Dragnet has a bug that causes it to falsely report GREEN — a regex that matches a path that doesn't exist, a check that skips a category silently — every downstream claim inherits the false positive. A2 notes the governance scripts are untested; A2 doesn't name the recursive trust problem explicitly.

The fix is not just "add tests for Dragnet" (which is #9). It's a **self-test fixture:** deliberately inject a false-verified claim into a test fixture and assert that Dragnet catches it. This is the difference between "Dragnet has test coverage" and "Dragnet is verified to catch the failure mode it exists to catch."

---

## Theme 2: Trust boundary expansion

This theme is **new** — neither A1 nor A2 framed it this way. The project is about to multiply its trust surface, and the prior audits haven't caught that the *nature* of the surface changes, not just its size.

### #2 — `rush_telemetry` excluded, with concrete PSI bug + GPL trap

**Inherited from A2, extended.** 2,361 LOC outside the workspace, doesn't compile (missing `libc`, BPF codegen incomplete), declares GPL-2.0-only inside an Apache-2.0 workspace, and A2 found a concrete PSI parser bug: a 21-byte `pread` that can include a newline and break parsing because `.trim()` won't strip text after an internal newline.

**The deeper issue A2 named but didn't quite close:** the crate is in a half-state — present enough to drift, absent enough to escape every gate. A2's recommendation (decide in or out) is right. The extension: **if it stays in the repo, it gets a feature-flag-gated CI job that runs at least `cargo check --features=stub` on every push**, so dependency drift is caught even if the crate doesn't fully build.

### #6 — GPL distribution trap

**New framing of an A1/A2 finding.** A1 and A2 both note the GPL/Apache split and recommend an ADR. Neither names the actual consequence: **if Rush Linux ever ships as a binary** (which a systemd daemon with auto-update might), GPL-2.0-only code in `rush_telemetry` may require the *whole binary* to be distributed under GPL-2.0-compatible terms. This isn't a documentation issue; it's a distribution-strategy issue.

### #18 — v0.7 desktop expands attack surface, not just test-coverage requirements

**New framing.** A2 frames v0.7 desktop work as a test-coverage gap. That's true but understates the risk. v0.7 means `optid` will start interacting with D-Bus (GameMode shim, portal APIs), Wayland protocols, portal APIs (screen, input), and session-scoped systemd. Each is a place where `optid`'s "explainable, reversible, safe actuation" promise meets a new class of adversarial input. Sysfs is a relatively trusted surface; D-Bus and Wayland are not. The right framing is **security review of the new trust boundaries before v0.7 work begins**, not after.

### #11 — Update signing infrastructure unaudited

**New finding.** A2's Phase 3 mentions "security review of... update signing" in passing. Neither audit digs in. If `optid` accepts signed updates (the presence of `sign_updates.py` in tools suggests it does), the signing infrastructure is its own audit surface: key storage, rotation, failure mode (fail open vs. closed), revocation.

### #12 — Systemd unit sandbox not reviewed against actual write paths

**New finding, extends A2's #8.** A2 notes the lexical write-site gate can't prove "the systemd unit sandbox permits exactly the intended writes." Neither audit actually reviews the systemd unit file. The relevant directives are concrete and checkable: `ProtectSystem=`, `ProtectHome=`, `RestrictAddressFamilies=`, `SystemCallFilter=`, `CapabilityBoundingSet=`, `ReadWritePaths=`.

If `ReadWritePaths=` is broader than the write-site inventory, the sandbox is theatre. If narrower, the daemon fails at runtime in ways CI doesn't catch. **The write-site inventory and the systemd unit file should be cross-checked.**

---

## Theme 3: Reversibility and safety

The project's core promise is "explainable, reversible, safe actuation." Two of those three (explainable, safe) have prior findings. **Reversibility is the one neither audit checked.**

### #3 — Revert path is the core promise and is untested

**New finding.** The daemon writes to sysfs, cgroups, and systemd unit properties. It claims to be reversible. Where is the test that proves a revert actually restores prior state? Not a unit test on the revert function — an integration test that: (1) snapshots prior state, (2) applies a policy, (3) triggers revert, (4) asserts the state matches the snapshot exactly.

If this test doesn't exist, the reversibility claim is unverified — the exact "shape not truth" pattern A2 flags for evidence gates. The daemon's central promise is itself an unverified claim.

### #8 — Panic mid-write breaks reversibility

**Restored from A1 (which had it as #9, priority 18).** A2 dropped this finding — a regression. A1 found 33 `.unwrap()`/`.expect()`/`panic!()` sites in production paths, concentrated in `actuator.rs`, `io_util.rs`, `sensors.rs`.

**Why this matters separately from #1:** #1 is about *continuing to apply the wrong policy*. A panic mid-write is about *aborting mid-apply and leaving partial state*. For a daemon whose pitch is reversibility, a panic between "write EPP" and "write revert checkpoint" breaks the guarantee itself — the checkpoint may not have been written, revert can't fire, and the system is left partially-actuated with no automatic recovery.

### #16 — Write-site gate is lexical; fix is type-system proof, not more tests

**Extension of A2's #8.** A2 correctly identifies that the lexical write-site gate can't prove semantic properties like "the allowlist check dominates the write on all control-flow paths." A2's recommendation is "add one integration-level safety test layer using a fake sysfs tree."

**The critique, applied to A2:** that recommendation doesn't solve the problem it names. An integration test proves the deny path works for the cases you wrote tests for. It doesn't prove domination on *all* paths. The bug class is "a new code path bypasses the allowlist check"; an integration test catches that only if you wrote a test for that specific path, which by definition you didn't.

The real fix is **structural**: make sysfs writes only reachable through a typed handle that carries a proof the allowlist check has returned `Allow` for that specific `SysfsDeviceId`. This makes the bypass unrepresentable in the type system. New code that tries to write without the check won't compile.

---

## Theme 4: Architectural debt

### #17 — PSI/proc parsing duplication is a symptom, not the disease

**Reframes A1/A2.** Four crates each parse `/proc/pressure/*` independently. Prior audits recommend extracting a shared crate. Correct, but the deeper question is **why did four engineers (or one engineer, four times) each write their own?** Not because they're sloppy — because there was no obvious place to put shared code, and the cost of extracting a crate felt higher than copy-pasting 30 lines.

That's an architectural signal: the workspace lacks a `rush_core` or `rush_sys` shared-utility layer. The fix isn't "extract PSI" — it's "establish the shared-utility crate pattern so the fifth parser doesn't get written either."

---

## Theme 5: Process and hygiene

Brief treatment — mostly inherited.

- **#13 — ADR status drift (8/16 stuck "proposed"):** reconcile to actual adoption in one pass.
- **#14 — Shell scripts lack shellcheck:** add `ludeeus/action-shellcheck@master` CI job.
- **#15 — `finish-work.sh` diverges from CI:** add `--ci-parity` mode.
- **#20 — Foreground detection is a documented stub:** v0.7 readiness gate must be defined before foreground detection counts toward release evidence.

---

## Phased plan

### Phase 0 — before more feature work

1. Make `--apply` fail closed on malformed policy or malformed allowlist overrides (#1). **Design doc:** `docs/plans/phase0-policy-fail-closed-design.md`.
2. Add content-aware evidence validation for host-bench transcripts (#4). **Schema:** `docs/plans/phase0-host-bench-evidence-schema.md`.
3. Decide whether `rush_telemetry` is in or out. No half-state (#2). **ADR:** `docs/decisions/0017-rush-telemetry-fate.md`.
4. Test the revert path end-to-end (#3).
5. Audit update signing infrastructure if auto-update is on the v0.7 roadmap (#11).
6. Cross-check systemd unit sandbox against write-site inventory (#12).

### Phase 1 — next 1-2 PRs

1. Add Python `pyproject.toml`, `ruff`, and pytest fixtures for governance scripts (#9, #10).
2. Add shellcheck CI (#14).
3. Fix the stale docs that still claim `Cargo.lock` needs Linux confirmation.
4. Document the GPL/Apache boundary before telemetry re-entry (#6).

### Phase 2 — before v0.7 grows

1. Finish unprivileged VM boot to `multi-user.target` (#7).
2. Extract shared proc/sysfs parsing (#17).
3. Add typed action values (#19).
4. Add fake-sysfs integration tests for allowlist denial and revert behavior.
5. Write the v0.7 threat model (#18) before any of the integration tests.

### Phase 3 — before v1

1. Hardware benchmark evidence on nominated machines.
2. Release schema freeze.
3. Security review of systemd sandbox, sysfs write paths, D-Bus shims, and update signing.
4. Type-system proof for allowlist-checked writes (#16).
5. GPL distribution decision (#6).

---

## The concise verdict

A2's thesis was "truth maintenance under acceleration." This audit extends it: **recursive truth maintenance under acceleration.** The systems that verify truth are themselves unverified, so a green check doesn't prove truth — it proves the checker didn't catch a lie. Each layer inherits the gap of the layer above it.

The highest-leverage improvements, in order:
1. Make every safety-sensitive subsystem fail closed in mutating mode (#1).
2. Make every release claim machine-check not just that evidence exists, but that the evidence says what the milestone claims (#4, #10).
3. Test the core promise — reversibility — before any v1 release (#3, #8).
4. Decide the trust boundaries before v0.7 expands them (#2, #6, #11, #18).
5. Establish the shared-utility pattern before the next duplication (#17).

The project's biggest asset is its governance discipline. The project's biggest risk is that governance discipline has become load-bearing without itself being governed.

---

## References

- A1 (first-pass audit): debt inventory, 11 findings, numeric priority formula.
- A2 (second-pass audit): threat-model framing, 12 findings, "truth maintenance under acceleration" thesis.
- `LESSONS.md` L-001 through L-008: evidence fabrication recurrence pattern.
- `docs/decisions/0001-0016`: existing ADR canon.
- `docs/research/0001-0019`: existing research notes.
