# Implementation Status

Last updated: 2026-07-10

> **Evidence state:** Implementation status (what code exists) is distinct from
> evidence state (what is certified by a committed transcript). For the
> milestone-criterion evidence state, the canonical source is
> `release/milestones.toml` and the Dragnet ledger
> (`release/evidence/dragnet/LEDGER.md`). Run `python3 tools/dragnet.py --observe`
> before relying on any "verified" claim.

## Overall State

The repository is an early implementation scaffold, not a bootable distro yet.
It defines the architecture, accepted defaults, Rust optimizer MVP, config
fragments, packaging skeletons, CI, and documentation layer needed for the next
engineering milestones.

## Implemented

- Current project version recorded in `VERSION` as `0.7.0-beta.1`. The
  preceding `0.6.0-beta.1` (Hardware-Aware optid) is code-complete (PPD +
  GameMode shims, `vm.guest` class, foreground stub — PRs #183–#186) but its two
  quantitative exit criteria remain **pending Phase D** physical-hardware
  benchmarks (`release/evidence/host-bench/`, see
  `docs/strategy/reference-hardware.md` and `docs/strategy/mixed-load-workload.md`).
- Release plan, versioning rules, release policy, release checklist, and
  machine-readable milestone/test-tier manifests.
- Documentation policy requiring future changes to document purpose, impact,
  validation, safety implications, and follow-up work.
- Git repository initialized locally on `main`.
- GitHub remote configured for `https://github.com/Nan0pk/Rush-linux.git`.
- Apache-2.0 license, CI workflow, security policy, and publishing helper.
- `optid` Rust MVP:
  - reads PSI from `/proc/pressure/{cpu,memory,io}`;
  - reads AC/battery state from `/sys/class/power_supply`;
  - reads thermal state from `/sys/class/thermal`;
  - reads load average from `/proc/loadavg`;
  - decides battery, balanced, performance, or realtime mode;
  - workload classifier pure function mapping PSI/load/AC/pin to the five classes (`idle`, `light`, `interactive`, `latency-critical`, `throughput`) with hysteresis, D-Bus override pinning (`optctl pin`), and state publication.
  - writes explainable status and decision logs;
  - applies guarded EPP, platform profile, and systemd cgroup actions only with
    `--apply`.
- PM QoS enforcement implemented: workload class latency-budget contract table (`config/optid/contracts.toml`) is now **enforced** (resolving class to CPU wakeup latency floor via `/dev/cpu_dma_latency` and per-device resume latency floor via `/sys/bus/pci/devices/*/power/pm_qos_resume_latency_us`).
- `fits_contract(exit_latency_us, floor_us) -> bool` helper is **defined**, not yet wired to devices.
- PM QoS budget values are **provisional pending WP-B1** validation.
- `optctl` Rust MVP:
  - reads status and decision logs;
  - sets mode through the state directory;
  - exposes pin and allowlist controls;
  - includes a WP-N3 focused-core `doctor` command that reads wakeup-source and
    per-device runtime-PM state once, reports visible blockers and safe review
    opportunities, supports `--json`, and never changes settings. Long-running
    observability and hardware validation remain pending.
- systemd service and tmpfiles config for `optid`.
- Default `optid.service` runs dry-run; `optid-apply.service` is the explicit
  mutating service.
- D-Bus interface contract for the future first-class control API.
- Kernel config fragments:
  - default adaptive kernel;
  - optional PREEMPT_RT kernel;
  - experimental sched_ext fragment.
- UKI-first boot config and systemd-sysupdate descriptors (renamed from
  `adaptive` to `rush-linux` branding; `InstancesMax=3` for rollback retention).
- nftables baseline firewall.
- cgroup and slice accounting defaults.
- Source recipe skeletons for kernel, RT kernel, optid, systemd, desktop, and
  server profiles.
- Edition profiles for desktop, laptop, realtime audio, and server.
- Benchmark manifest covering responsiveness, battery, gaming, realtime audio,
  and server throughput.
- Validation script enforcing required files and future-facing defaults.

- D-Bus server implementation inside `optid` and client integration inside `optctl` (supporting both system bus calls and file-based fallback).
- Rust workspace compilation and test verification on Windows using local Rust toolchain.
- Dynamic system-wide policy loading and parsing (`/config/optid/policy.toml`) with clean fallback to defaults.
- JSON status output option (`--json`) for `optctl status` to support machine-readable telemetry output.
- Custom package and rootfs builder (`tools/rush-builder.py`) using Python standard libraries (including TOML parsing via `tomllib`).
- Package metadata database generation (`repodata.json`) and signature validation stubs (`repodata.json.sig`).
- Extensible rootfs generator populating output rootfs from recipe-resolved dependency trees.
- GPT raw VM disk image compiler using native `systemd-repart` to format ext4 partition and clone rootfs trees without loop mounts or root privileges.
- Measurement rig (`rushbench`) implemented: pure Rust workspace member that captures battery drain (BAT/energy_now or intel-rapl) and responsiveness metrics per SPEC §1 class, pinning class via `optctl` and validating resolved PM QoS floors. `contracts.toml` values remain provisional; this tool enables collecting the validation dataset, but no results are committed yet.

### v0.6 Phase A — Hardware-Aware optid (allowlist default-flip + Criterion 4 harness)

- **WP-N4 hardware allowlist foundation** — verified (A1). 12 `optid` allowlist tests + 4 `optctl allow` tests pass; seeded baseline + override-directory precedence + HWID modalias detection + default-deny + audit trail all in place since WP-N4 landed.
- **Criterion 4 enumeration harness** — `crates/optid/tests/write_site_gating.rs` (A2). 29 write sites across `actuator.rs` and `io_util.rs` inventoried; every site classified as `allowlist` / `adr0009-baseline` / `state-file` / `revert-path` / `non-sysfs`; drift-detection assertions count `guarded_write(`, `pmqos_sink.write_*(`, `atomic_write_state_file(`, `Command::new("systemctl")` occurrences via `include_str!` and fail mechanically if a new site is added without classification.
- **`--allowlist` default flipped to `enabled`** — `crates/optid/src/args.rs` (A3). `Args::parse` now initializes `allowlist: true`; new `--no-allowlist` flag is the emergency escape hatch for bring-up on hardware the seeded baseline does not yet cover. Five `args::tests` pin the new default (regression fails loudly if a future refactor flips it back).
- **Research-0006 §7 medium-term plan** — updated (A5). The "Land behind `--allowlist=enabled` flag (default `disabled` in v0.x)" item is marked Done in v0.6 Phase A3; the "Promote from WIP to Validated" item notes Phase D's 2-machine relaxation; docmap.toml `covers_code` extended to include `crates/optid/src/args.rs`; `last_verified` bumped to 2026-06-28.

### v0.6 Phase B — PPD/GameMode shims + conflict detection (in progress)

- **B3 conflict detection** — `crates/optid/src/shim/{mod,conflict}.rs` + `crates/optid/src/policy.rs::PolicySection` + `crates/optid/src/main.rs` startup wiring. `optid` now reads `competing_policy_daemons` from `config/optid/policy.toml`'s `[policy]` section, checks `systemctl is-active --quiet <svc>` for each, and downgrades `--apply` to dry-run with a logged reason when any conflict is active. Fails OPEN if `systemctl` is unavailable (containers, non-systemd). 7 `shim::conflict::tests` cover no-conflicts, single-conflict, multi-conflict-in-input-order, empty-list, unknown-daemon, and the render-advice format. PPD shim (B1) and GameMode shim (B2) are the next chunks.
- **B1 PPD shim** — `crates/optid/src/shim/ppd.rs` + `crates/optid/src/policy.rs::ShimConfig`/`PpdShimConfig` + `crates/optid/src/main.rs` ConnectionBuilder wiring. `optid` now implements `net.hadess.PowerProfiles` on its D-Bus connection at `/net/hadess/PowerProfiles`, claiming the `net.hadess.PowerProfiles` bus name so GNOME Settings and KDE `powerdevil` drive `optid` mode changes without code changes. Properties: `ActiveProfile` (Get/Set with `emits_changed_signal = "true"`), `Profiles` (`aa{sv}` of the 3 standard profiles), `Actions` (`[]`), `PerformanceDegraded` (`""`). Methods: `HoldProfile(profile, reason, app_id) → cookie`, `ReleaseProfile(cookie)`. Cookie registry is an in-memory `Mutex<HashMap<u32, Hold>>` with monotonic counter starting at 1; most-recently-registered hold wins; registry empties on release and persisted mode reverts to `auto`. Custom signals `ActiveProfileChanged` and `ProfileReleased` are declared (introspection-visible) but not emitted in v0.6 — clients see changes via the standard `PropertiesChanged` signal. Profile→mode mapping is configurable via `policy.toml`'s `[shim.ppd.profiles]` table (defaults: `power-saver→battery`, `balanced→auto`, `performance→performance`). When B3 detects a PPD conflict, the shim is skipped (the bus name would conflict) and `--apply` is downgraded to dry-run. 25 inline `shim::ppd::tests` cover mapping, registry, getter, setter, hold/release, configurable map, and concurrency; 16 source-level smoke tests in `crates/optid/tests/shim_ppd.rs` pin the interface signature and prevent silent refactors.
- **B2 GameMode shim** — `crates/optid/src/shim/gamemode.rs` + `crates/optid/src/policy.rs::GameModeShimConfig` + `crates/optid/src/main.rs` ConnectionBuilder wiring. `optid` now implements `com.feralinteractive.GameMode` on its D-Bus connection at `/com/feralinteractive/GameMode`, claiming the `com.feralinteractive.GameMode` bus name so Steam, Lutris, and Heroic drive `optid` pin changes without code changes. Methods: `RegisterGame(pid) → i32` (1=newly registered, 2=already registered, 0=error), `UnregisterGame(pid) → i32` (1=unregistered, 2=was not registered, 0=error), `QueryStatus() → i32` (count of active games), `QueryStatusClient(pid) → i32` (0 or 1). No properties, no signals (matches GameMode's official spec). On `RegisterGame(pid)`, optid writes `state_dir/pins/<pid>` with the configured workload class (default: `latency-critical`); the run loop reads the pin on the next tick and applies the latency contract. On `UnregisterGame(pid)`, the pin file is removed. Each registration carries a TTL (default: 30 minutes, configurable via `[shim.gamemode].ttl_sec`); stale entries are expired lazily on the next `RegisterGame`/`QueryStatus`/`QueryStatusClient` call (no background thread — YAGNI). Re-registering an existing PID refreshes the TTL. Negative PIDs are rejected with `0`. When B3 detects a conflict, the shim is skipped alongside PPD. 28 inline `shim::gamemode::tests` cover registry semantics, register/unregister, query methods, configurable pin class, lazy expiration, and state_dir sharing with `OptidServer`; 16 source-level smoke tests in `crates/optid/tests/shim_gamemode.rs` pin the interface signature and prevent silent refactors.

## Not Yet Implemented

> **Categorization note (Dragnet-001):** several entries below are in fact
> *implemented* — boot assessment + rollback retention, Ed25519 update signing,
> and the WP-N4 hardware allowlist (`crates/optid/src/allowlist.rs`, `build.rs`,
> `data/allowlist.toml`, `optctl allow/deny`) all exist in the tree. What remains
> outstanding for the boot/rollback/install items is *committed acceptance
> evidence*, not the code — see `release/evidence/BUILD-HOST-RUNBOOK.md`. They are
> left under this heading only until the next full status rewrite; treat
> `release/milestones.toml` as authoritative.

- Boot assessment and rollback retention behavior for the validated UKI boot
 path, including simulated bad-kernel rollback.
  - Boot entry manager: `tools/manage-boot-entries.sh` rotates UKIs into
    versioned rollback entries and prunes entries beyond `INSTANCES_MAX` (default 3).
  - Boot assessment service: `packaging/systemd/optid-boot-assess.service`
    runs after `multi-user.target` and writes a boot-good marker via
    `tools/optid-boot-assess`.
  - Boot assessment tool: `tools/optid-boot-assess` supports `mark-good`,
    `check`, `count-failed`, and `reset` commands for boot-good/bad tracking.
  - Rollback integration test: `tools/test-rollback.sh` validates all three
    v0.4 exit criteria (UKI boot, rollback entry retention, bad-kernel rollback).
- Update signing system replaces the mock signature stub with real Ed25519
  signatures:
  - `tools/sign_updates.py` — Python module for generating Ed25519 key pairs
    and signing/verifying `repodata.json` using the `cryptography` library.
  - `tools/sign-updates.sh` — Shell wrapper using OpenSSL as a fallback.
  - `tools/test-sign-updates.sh` — Validates key generation, signing,
    verification, and tamper detection.
  - Test keys are stored in `config/keys/` (private key git-ignored).
  - `rush-builder.py repo-init` now uses real signatures when keys are present.
- Real UKI signing keys, Secure Boot enrollment path, and measured boot policy.
- eBPF probes and overhead budget enforcement.
- GPU, foreground app, video call, fullscreen, and build-system detection.
- Hardware allowlist database for unsafe device power knobs.
- Installer and role selection UI.
- Benchmark harness execution and published results.
- The `Cargo.lock` file is checked in, but it still needs confirmation from
  real Cargo on Linux.
- `optid` sysctl actuation: the per-mode `vm.*` keys (`vm_swappiness`,
  `vm_dirty_*`) defined in `config/optid/policy.toml` are **implemented and applied** by
  the daemon when running with `--apply`. High `vm.swappiness` is conditional on
  ZRAM-backed swap (`high_swappiness_requires_zram`), and every write goes
  through the explainable allowlist per ADR 0009. The prior values are journaled
  and reverted on service stop.
- Bootable VM disk image (`disk.raw`) produced by `tools/build-vm-final.sh`.
 Verified 2026-06-08: QEMU direct-kernel boot reaches `multi-user.target`
 with `optid.service` active.
- UEFI UKI VM boot validation through OVMF/systemd-boot is now implemented via
 `tools/validate-uefi-boot.sh` and verified 2026-06-08: OVMF loads
 `EFI/BOOT/BOOTX64.EFI`, systemd-boot selects the Rush Linux entry, the UKI
 loads its embedded initrd, the root filesystem mounts from `/dev/vda2`,
 systemd reaches `multi-user.target`, and `optid.service` starts.
- Rollback entry management (`tools/manage-boot-entries.sh`) rotates UKI
  entries and retains at least 3 rollback entries.
- Boot assessment marker service (`optid-boot-assess.service`) and tool
  (`optid-boot-assess`) for marking boots as good/bad.
- Rollback integration test (`tools/test-rollback.sh`) validates UKI boot,
  rollback entry retention, and bad-kernel recovery.
- Update metadata signing with Ed25519 test keys (`tools/sign_updates.py`,
  `tools/sign-updates.sh`) and signing validation test
  (`tools/test-sign-updates.sh`).

## Known Local Constraints

- `rustc` and `cargo` are installed locally under both Windows host and WSL2 Ubuntu environment.
- `gh` is not installed locally.
- The GitHub repository exists at `https://github.com/Nan0pk/Rush-linux`.

## Acceptance Rule

Any change that modifies behavior, defaults, policy, boot/update flow, kernel
fragments, recipes, service files, or command behavior must update the relevant
documentation in the same change.
