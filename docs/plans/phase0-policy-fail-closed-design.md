# Phase 0 Design Doc: Fail-Closed Policy Loading in `--apply` Mode

**Date:** 2026-07-02
**Status:** Draft for review
**Effort:** 2–3 dev-days
**Blocks:** v1 release
**Severity:** Critical (pending verification gate in §2.1)
**Auditor:** Z.ai audit (third pass)

## 1. Context and scope

The optid daemon reads its operational policy from a TOML file at startup. When the file is missing or fails to parse, the current implementation falls back to `Policy::default()` and continues operating. The same fail-open pattern applies to the hardware allowlist override loader: malformed override files are skipped with a warning, and the daemon proceeds with whatever overrides did parse, layered on top of the seeded allowlist.

This behavior is documented as safe in the source comments because a corrupt policy "can never break the daemon — it only loses overrides." That framing is correct for *dry-run mode*, where the daemon's only output is explanations and proposed actions. It is much less clearly correct for **`--apply` mode**, where the main loop actually writes to sysfs, cgroups, and systemd unit properties. The scope of this design doc is the loading path: how policy and allowlist overrides enter the daemon, and what the daemon does when those inputs are missing or malformed.

> **What this doc is NOT about:** This doc does not change the policy file format, the action enum, or the actuation surface. It changes only the **load-failure semantics**: what happens when the inputs to the policy/allowlist subsystem are not what the operator intended. Revert-path correctness and panic-in-write-path hardening are covered in separate Phase 0 deliverables.

## 2. Risk analysis

The fail-open risk only bites if the fallback default is actuation-capable. If `Policy::default()` produces only observe-only actions (no `Action::Set*` variants), failing open to defaults is safe even in `--apply` mode, because there is nothing to apply. The audit therefore requires one verification before this finding ranks Critical: open optid's `Policy::default()` and confirm whether it produces any actuation actions.

### 2.1 The causal chain

Three conditions must hold simultaneously for the fail-open behavior to produce an unsafe outcome. Each link in the chain is independently verifiable; if any one is missing, the risk is bounded.

- **Link 1:** `Policy::load` falls back to `Policy::default()` on parse failure (inherited from audit — confirmed by source comments).
- **Link 2:** `Policy::default()` produces at least one `Action::Set*` variant (e.g. `SetCpuEpp`, `SetSystemdUnit`, `SetSataAlpm`). *This link is unverified.* If it does not hold, the entire finding downgrades to Medium and the fix becomes a regression test.
- **Link 3:** The daemon is invoked with `--apply`, so the actions produced by the defaulted policy are actually written to sysfs/cgroups/systemd. Confirmed by CLI design.

> **Verification gate (do this first):** Before implementing any code change, write a one-line test that asserts `Policy::default().actions` contains no `Action::Set*` variants. If the test passes, the current behavior is safe-by-accident; this doc then becomes a hardening exercise to make the safety invariant explicit and regression-proof. If the test fails, the full Critical-ranking fix below applies.

### 2.2 The allowlist-override variant

The allowlist override loader has the same fail-open shape but with an additional causal link. A skipped malformed override only re-exposes a device if both (a) the seeded allowlist permits that device and (b) the malformed override was a deny. The first condition is checkable from the seeded allowlist data; the second is not recoverable from a malformed file, because we cannot know what the operator intended.

This asymmetry is the core problem. With a missing policy file, we know we have no policy and can choose to refuse actuation. With a malformed allowlist override, we do not know whether the operator intended to allow or deny the affected devices, so the safe choice is to deny actuation for any device that might have been covered by the malformed file.

## 3. Proposed design

Split loading behavior into two modes keyed off `args.apply`. In dry-run mode, the current fail-open behavior is preserved (it is safe and operator-friendly). In apply mode, the daemon refuses hardware actuation on any load failure and enters an explain-only state.

### 3.1 Mode matrix

| Scenario | Behavior |
|---|---|
| Dry-run + missing/malformed policy | Warn and use `Policy::default()`. Continue normally. Set `policy_load_state=defaulted`. |
| Dry-run + missing/malformed allowlist override | Warn and skip the malformed file. Set `allowlist_load_state=partial`. Continue normally. |
| **Apply + missing/malformed policy** | **Refuse hardware actuation. Enter explain-only mode.** Set `policy_load_state=invalid` and `apply_armed=false`. Daemon stays up, serves status/D-Bus queries, performs no writes. |
| **Apply + missing/malformed allowlist override** | **Refuse hardware actuation for the affected device domains.** Set `allowlist_load_state=invalid`. Operator can override with `--allowlist-ignore-bad-overrides` if they accept the risk. |

### 3.2 Boot-time status surface

Surface three new fields in the daemon's structured log at startup, in any status/D-Bus command, and in the JSONL event stream:

```
policy_load_state    = ok | defaulted | invalid
allowlist_load_state = ok | partial | invalid
apply_armed          = true | false
```

The `apply_armed` field is derived: it is `true` only when `args.apply` is true AND both `policy_load_state` and `allowlist_load_state` are not `invalid`. This makes the daemon's actual actuation capability visible to a sysadmin without requiring them to read logs or reconstruct state.

### 3.3 Reference implementation sketch

The implementation is a small refactor of the existing load path. The `Policy::load` function returns a `Result<Policy, LoadError>` instead of falling back silently. The main loop branches on `args.apply` to decide whether a load error is fatal-to-actuation or warn-and-continue.

```rust
pub enum LoadError {
    FileMissing(PathBuf),
    ParseError { path: PathBuf, source: toml::de::Error },
    OverrideSkipped { path: PathBuf, devices: Vec<SysfsDeviceId> },
}

pub struct BootState {
    pub policy_load_state: LoadState,
    pub allowlist_load_state: LoadState,
    pub apply_armed: bool,
}

pub enum LoadState { Ok, Defaulted, Partial, Invalid }

// In main():
let boot = match (Policy::load(&path), args.apply) {
    (Ok(p), _) => BootState { policy_load_state: LoadState::Ok, ... },
    (Err(_), false) => {  // dry-run: warn + default
        warn!("policy load failed; using defaults");
        BootState { policy_load_state: LoadState::Defaulted, ... }
    }
    (Err(e), true) => {   // apply: refuse actuation
        error!("policy load failed in --apply mode: {e}");
        BootState { policy_load_state: LoadState::Invalid,
                    apply_armed: false, ... }
    }
};

// Main loop:
if boot.apply_armed {
    actuator.apply(&actions)?;
} else {
    emit_explanations(&actions);
}
```

## 4. Migration plan

The new behavior is a breaking change for any deployment that relies on `--apply` continuing through a corrupt policy file. That set is probably empty in practice (operators do not usually intend to ship a corrupt policy to production), but the breaking-change semantics still need a migration path so existing CI and packaging tests do not break unexpectedly.

- **v0.7.0-beta.1** (current): ship the `BootState` surface and the new status fields, but keep the existing fail-open behavior. This is purely observational — operators can see the load state, but actuation is unchanged.
- **v0.8.0-beta.1**: ship the fail-closed behavior behind a feature flag (`--strict-load` or env var). Operators who opt in get the new behavior; the default remains fail-open for one release.
- **v0.9.0-rc.1**: flip the default to fail-closed. Document the change in the release notes as a behavior change, with the migration command (`--allowlist-ignore-bad-overrides`) for operators who need the old behavior in a pinch.
- **v1.0.0**: remove the feature flag. Fail-closed is now the only behavior.

## 5. Open product decisions (need maintainer input)

Two design choices are product decisions, not engineering decisions. They should be made by the project maintainer before implementation begins, because they affect operator experience and the daemon's failure mode.

> **Decision 1: hard-fail vs. degrade-to-observe.** When `--apply` is set and policy load fails, should the daemon (a) **refuse to start** with a non-zero exit code, forcing the operator to fix the policy file, or (b) **start in observe-only mode**, staying up and serving status queries but performing no writes? Option (a) is safer (no actuation capability exists) but worse for systemd service availability. Option (b) is what this doc assumes, but it requires the systemd unit to tolerate a daemon that is technically up but functionally inert. **Recommendation: option (b)**, with `apply_armed=false` visible in status so monitoring can alert on it.

> **Decision 2: override-skip granularity.** When a single allowlist override file is malformed, should the daemon (a) deny actuation only for the *devices that file was supposed to cover*, or (b) deny actuation for *all devices* until the file is fixed? Option (a) is more surgical but requires the loader to know which devices each override file targets, which may not be discoverable from a malformed file. Option (b) is safer but more disruptive. **Recommendation: option (b)**, with the `--allowlist-ignore-bad-overrides` escape hatch for operators who explicitly accept the risk.

## 6. Acceptance criteria

The implementation is complete when all of the following are true:

- [ ] `Policy::load` returns `Result<Policy, LoadError>`; no silent fallback inside the function.
- [ ] `BootState` struct exists and is populated at startup; `apply_armed` is correctly derived from `args.apply` and both load states.
- [ ] Status command (and any D-Bus interface) exposes all three fields.
- [ ] Integration test: `--apply` + missing policy file → daemon starts, `apply_armed=false`, no writes emitted to sysfs (verified via fake-sysfs tree).
- [ ] Integration test: `--apply` + malformed allowlist override → daemon denies actuation for affected devices, `allowlist_load_state=invalid` in status.
- [ ] Integration test: `--allowlist-ignore-bad-overrides` flag restores the old behavior with a clear warning in logs.
- [ ] Unit test asserting `Policy::default()` is observe-only (the verification gate from §2.1). If this test fails, the Critical ranking of the original finding is upheld and the full fix applies. If it passes, the fix is hardening only.
- [ ] Release notes for v0.7.0-beta.3 explicitly call out the behavior change and the `--allowlist-ignore-bad-overrides` escape hatch.

## 7. Out of scope

- Panic-in-write-path hardening (separate Phase 0 deliverable, see audit #8).
- Revert-path integration testing (separate Phase 0 deliverable, see audit #3).
- Type-system proof for allowlist-checked writes (audit #16, Phase 3).
- Typed action values (audit #19, Phase 1 — not blocking).
- Systemd unit sandbox cross-check (audit #12, parallel Phase 0 item).

## References

- Audit third pass, finding #1 (fail-open `Policy::load` + allowlist overrides in `--apply` mode).
- Audit third pass, finding #8 (panic mid-write breaks reversibility).
- Audit third pass, finding #3 (revert path is the core promise and is untested).
- `docs/decisions/0009-optid-security-boundary.md` (existing ADR on optid security/allowlist boundary).
