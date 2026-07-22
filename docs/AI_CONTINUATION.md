# AI Continuation — Current Rush Linux Handoff

Last verified: 2026-07-22

This is the short orientation for a fresh worker. `AGENTS.md` and
`docs/SPEC-northstar.md` remain higher authority.

## Mission

Continue building Rush Linux: an Arch-based adaptive operating-system project
focused on power intelligence, responsiveness, evidence, and explainable
behavior. `optid` is the policy brain, but LiveDev, testOS, image/boot/update
work, measurement, and release evidence are also first-class project systems.

Do not reduce the project to a power-daemon rewrite or a queue of GitHub tasks.

## Read in this order

1. [`AGENTS.md`](../AGENTS.md) — project constitution and authority order.
2. [`docs/SPEC-northstar.md`](SPEC-northstar.md) — product invariants and
   blocked work.
3. [`OPTID-COMPLETION-PLAN.md`](../OPTID-COMPLETION-PLAN.md) — active optid
   execution plan.
4. [`docs/architecture/optid-d2-amendment.md`](architecture/optid-d2-amendment.md)
   — owner-approved safety architecture.
5. [`docs/plans/optid-package-status.toml`](plans/optid-package-status.toml) —
   machine-readable package state and dependencies.
6. [`docs/IMPLEMENTATION_STATUS.md`](IMPLEMENTATION_STATUS.md) — repository
   implementation truth.
7. `release/milestones.toml` and `release/evidence/` — release/evidence truth.

Older audits and plans are useful history, not the current work selector.

## Current state

- Version pointer: `0.7.0-beta.4`.
- v0.5 Minimal Installable System has committed milestone evidence.
- v0.6 remains in progress. PPD/GameMode surfaces, `vm.guest`, the hardware
  allowlist, core CPU controls, and initial dynamic-device actions have landed.
- Runtime PM, PCIe ASPM, SATA ALPM, backlight, per-device PM QoS, and VM-sysctl
  paths exist; they are not stubs.
- The packaged apply service intentionally cannot write dynamic device paths.
- Policy does not yet produce complete inverse desired state on every
  transition.
- Current recovery runs on startup and clean shutdown but is not the accepted
  persistent verified crash-recovery protocol.
- Every seeded hardware allowlist row is unverified, so automatic depth writes
  remain denied by default.
- Foreground/compositor integration is still a stub, so GameMode's per-PID
  effect path is incomplete.
- `sched_ext` is SPEC-blocked until WP-B1 evidence exists.

## Next Task

There are two independent lanes:

### F1 — next general construction package

Implement validated per-domain configuration:

- `DomainMode { Off, Observe, Actuate }`;
- typed configuration for every domain;
- strict mixed/legacy-key validation;
- a visible `EffectiveConfig` through `optctl`; and
- compatibility for old boolean keys for one release with warnings, while
  rejecting conflicting old/new keys.

F1 changes no hardware-write behavior.

### D0 — next safety-lane package

Build an experimental proof for:

- pre-opened sysfs descriptors remaining usable after Landlock;
- prohibited new write opens failing;
- restrictions applying before worker threads and remaining inherited;
- removed sysfs objects failing safely;
- a dedicated topology-rebuild exit status; and
- systemd recovery completing before cold restart rebuilds the descriptor
  table.

D0 does not connect to production actuation. It gates S4D only. If the proof
fails on a supported kernel, record the exact kernel/ABI/object/syscall result
and stop S4D; do not invent a silent unsealed fallback.

## Accepted D2 safety lane

The owner rejected the former broker/one-root-daemon/observe-only menu and
accepted D2:

`D0 → S1D → S2D → S3D → S4D → S5D`

“Architecture D2” is the safety decision. Package `D2` is still the separate
storage-depth package; do not confuse the two in worker packets or status.

- exact pre-opened typed capability descriptors;
- Landlock installed before worker threads;
- no permanent actuation broker and no steady-state write-path IPC;
- rollback distinct from emergency stabilization for every lever;
- persistent write-ahead recovery under `/var/lib/optid/recovery/`;
- apply/readback/compensation transactions;
- a minimal independent `optid-recover` executable;
- a real control-loop systemd watchdog heartbeat;
- supervisor-managed cold restart for hotplug topology rebuild; and
- persistent per-domain/HWID circuit breakers with canary re-entry.

Do not revive S1–S3 from the pre-amendment plan. D-Bus/session authentication
still requires consistent polkit and is handled by X1; it is not an actuation
broker.

## Hardware evidence is not a build gate

Physical reference machines are required to:

- promote a HWID/firmware combination from observe to automatic actuation;
- verify v0.6 responsiveness and battery criteria; and
- make release or performance claims.

They are not required for observation, simulation, dry-run, disabled code,
F1–F4, D0, deterministic fixtures, diagnostics, or pure controller work.

## Work-session lifecycle

Start non-read-only work with:

```bash
bash tools/start-work.sh "short task description"
```

Implement one coherent package, update its ledger entry, add behavior tests,
and run:

```bash
bash tools/finish-work.sh --dry-run
```

Agents may commit, push a branch, and open a draft pull request. They never
merge or enable auto-merge. The human maintainer merges.

## Forbidden Shortcuts

- Do not bypass explicit apply mode, typed capability/path validation,
  responsiveness-contract fit, or verified hardware authorization.
- Do not call a kernel-valid value hardware-safe without matching evidence.
- Do not call emergency stabilization “rollback” or “kernel default.”
- Do not mark hardware verified from mocks.
- Do not start `sched_ext`, fan actuation, MUX writes, or production render/ALS
  actuation without the missing higher-authority decision/evidence.
- Do not integrate the quarantined `rush_telemetry` crate as-is.
- Do not make hardware nomination a reason to stop unrelated safe work.

## Handoff facts

When finishing or stopping, report:

- immutable base SHA and package ID;
- exact files changed;
- exact commands and results;
- assumptions and source citations;
- unresolved dependency or stop condition;
- ledger status/PR update; and
- whether any claim still needs hardware or independent verification.

Never invent a branch, commit, pull request, check result, approval, or
hardware receipt.
