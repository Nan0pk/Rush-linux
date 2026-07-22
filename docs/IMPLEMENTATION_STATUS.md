# Implementation Status

Last updated: 2026-07-22

> **Code is not evidence.** This file describes what exists in the repository.
> Release and hardware-verification truth comes from
> `release/milestones.toml`, committed evidence under `release/evidence/`, and
> the Dragnet ledger. A merged write path is not automatically enabled,
> hardware-safe, or milestone-complete.

## Overall state

Rush Linux is an early beta with real optimizer, image, boot, install,
benchmark, testOS, and LiveDev components. It is not yet a consumer-installable
desktop or laptop distribution.

The version pointer is `0.7.0-beta.4`. v0.6 is still in progress: core
hardware-aware slices landed, but optid capability completion and Phase D
physical-hardware evidence remain unfinished. Hardware nomination blocks those
promotion/release claims; it does not block observation, simulation, dry-run,
disabled implementation, or the active construction packages.

The active optid sources are:

- [`OPTID-COMPLETION-PLAN.md`](../OPTID-COMPLETION-PLAN.md) — executable plan;
- [`docs/architecture/optid-d2-amendment.md`](architecture/optid-d2-amendment.md)
  — accepted fail-passive safety architecture; and
- [`docs/plans/optid-package-status.toml`](plans/optid-package-status.toml) —
  machine-readable current package state.

F1 is the next general construction package. D0 is the next safety-lane
package.

## Optid: implemented

### Observation and decisions

- PSI reads from `/proc/pressure/{cpu,memory,io}`.
- AC/battery, thermal, load-average, runtime-PM device, storage-link, and
  backlight discovery.
- Six workload classes: `idle`, `light`, `interactive`, `latency-critical`,
  `throughput`, and derived `vm.guest`.
- Battery, balanced, performance, realtime, and automatic modes with
  hysteresis.
- Status and decision reports, JSON-capable `optctl`, and decision logs.
- Dynamic policy/contract loading with explicit load state and fail-closed
  behavior for dynamic writes.

### Guarded action paths

With explicit `--apply`, the code contains guarded operations for:

- CPU energy-performance preference;
- ACPI platform profile;
- VM sysctls, gated by the zram-backed memory policy;
- CPU DMA PM QoS;
- per-device resume-latency PM QoS;
- runtime-PM control and autosuspend delay;
- PCIe ASPM;
- SATA ALPM;
- backlight brightness; and
- systemd/cgroup slice weights.

The per-device actions are not stubs. They include typed path checks,
allowlist checks, journaling, writes, and revert functions. They are still
incomplete as a deployed product:

- the packaged apply service grants fixed write paths and intentionally cannot
  reach dynamic `/sys/devices/...` targets;
- every seeded device allowlist row is currently unverified, so automatic
  depth writes remain denied by default;
- policy does not yet emit a complete inverse desired state on every
  transition; and
- the current runtime-directory journal is not the persistent verified D2
  recovery protocol.

The PM QoS contract table drives CPU and device constraint actions. The
`fits_contract(exit_latency_us, floor_us)` helper exists but is not wired into
device-depth selection, and PM QoS constraints must not be misreported as
measured device exit latency.

### Safety and compatibility surfaces

- Hardware allowlist loading, override precedence, default-deny behavior,
  audit reporting, and `optctl allow|deny|list-allow`.
- `--allowlist` defaults enabled; seeded rows remain unverified.
- Structural path validation and guarded writes.
- Startup and clean-shutdown revert calls for sysctls, PM QoS, runtime PM,
  storage, and display state.
- Single-instance lock and signal-driven clean exit.
- Competing power-daemon detection that downgrades apply mode.
- PPD-compatible `net.hadess.PowerProfiles` D-Bus surface.
- GameMode-compatible D-Bus registration surface and PID pin files.

GameMode registration is not yet a complete effect path: the foreground
producer is a stub and per-PID selection is not connected to a real foreground
identity. State-changing D-Bus authorization is also inconsistent and remains
part of X1/security work.

### Operator and integration tooling

- `optctl` commands for status, explain, mode, pin, trace, and allowlist
  management, including machine-readable output.
- `rushbench` energy/responsiveness measurement harness.
- Rush LiveDev planner, runner, capture, evidence validation/submission, Linux
  bootstrap, and Windows PowerShell bootstrap.
- Bootable testOS USB backend and manual installers.
- Reproducible mkosi image/profile scaffolding.
- UKI/systemd-boot path, boot assessment, retained rollback entries,
  systemd-sysupdate descriptors, and Ed25519 update metadata support.
- A blank-disk install path using `systemd-repart`, with committed VM/boot
  evidence for the milestones that are marked verified.

## Optid: active gaps

The active completion plan owns these gaps; this list is a summary, not a
second work queue.

- **F1–F4:** per-domain `off|observe|actuate` configuration, injectable test
  boundaries, versioned outcomes, and complete transition reconciliation.
- **D0, S1D–S5D:** prove Landlock/sysfs capability sealing; define rollback vs
  stabilization; add persistent verified transactions; build independent
  recovery/watchdog ordering; move writes to exact sealed descriptors; add
  domain circuit breakers.
- **C1:** measured/provenance-aware latency contracts instead of treating PM
  QoS constraints as measurements.
- **E1/O1/O2:** event-driven reevaluation, runtime-state observability, and
  cgroup resource-pull context.
- **X1/X2:** authenticated session context, consistent polkit, real compositor
  backends, and a connected GameMode effect path.
- **D1–D5:** complete runtime PM, NVMe APST/storage depth, display ownership,
  conservative dGPU runtime PM, and memory ownership.
- **T1–T3:** thermal/powercap observation and controller simulation before any
  evidence-gated PL1 actuation.
- **I1–I3:** one truthful diagnostic surface, whole-system simulation, and
  hardware promotion PRs.

Fan actuation is excluded. MUX writes and render/ALS production actuation lack
accepted interfaces. `sched_ext` has no authorized work package until WP-B1
evidence satisfies `docs/SPEC-northstar.md:207-212`.

## Distribution and evidence gaps

- Desktop, laptop, realtime-audio, and server profiles are present, but
  consumer editions are not yet built, validated, and released as complete
  products.
- v0.6 quantitative responsiveness and battery criteria require matching
  physical-hardware runs.
- Automatic hardware writes require evidence tied to exact HWID/firmware and a
  separate promotion decision.
- Real production signing/enrollment and release-channel operations remain
  later release work even where test-key and VM paths exist.

## Acceptance rule

Any change that modifies behavior, defaults, policy, boot/update flow, kernel
fragments, recipes, services, or commands must update the relevant
documentation in the same pull request. No status row or checkmark becomes a
hardware/release claim without its required committed evidence.
