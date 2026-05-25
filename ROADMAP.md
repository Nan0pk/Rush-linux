# Roadmap

## Phase 0: Repository Foundation

Status: mostly complete.

- Keep architecture docs and ADRs in the repository.
- Keep future-facing default checks in CI.
- Publish the repository to GitHub.
- Run Rust CI once a toolchain is available.

Exit criteria:

- GitHub repo exists.
- CI runs on pull requests.
- `tools/validate-repo.ps1` passes.
- `cargo fmt`, `cargo test`, and `cargo clippy` pass on Linux.

## Phase 1: Compile-Clean Optimizer MVP

Status: next.

- Make `optid` and `optctl` compile cleanly.
- Replace file-based control with the D-Bus API while keeping file state for
  diagnostics and recovery.
- Add structured status output.
- Add config parsing for `config/optid/policy.toml`.
- Add integration tests with fixture `/proc` and `/sys` trees.

Exit criteria:

- `optid --once` works on Linux in dry-run mode.
- `optctl status`, `optctl explain`, and `optctl mode` work through D-Bus.
- No privileged action runs without `--apply`.
- All decisions include a reason.

## Phase 2: Minimal Rootfs And Package Builder

Status: planned.

- Build a minimal rootfs from recipes.
- Produce signed binary packages from source.
- Install systemd, kernel, nftables, optid, and boot files into a rootfs.
- Generate UKI artifacts from the kernel package.
- Add rollback entries.

Exit criteria:

- Minimal VM boots to multi-user target.
- `optid.service` runs in dry-run or guarded apply mode.
- `nftables.conf` loads.
- cgroup v2 and PSI are active.

## Phase 3: Hardware-Aware Adaptive Policy

Status: planned.

- Add CPU topology, EPP, platform profile, storage class, and thermal sensor
  discovery.
- Add hardware allowlists for risky sysfs knobs.
- Add foreground app/session detection.
- Add GPU runtime power and display policy.
- Add zswap/zram and memory pressure policy.

Exit criteria:

- Mixed-load responsiveness improves against baseline on at least two machines.
- Battery policy improves or matches mainstream defaults on at least one laptop.
- Thermal and unsupported-hardware guardrails prevent unsafe writes.

## Phase 4: Editions And Installer

Status: planned.

- Add installer role selection.
- Build desktop, laptop, server, and realtime audio images.
- Add KDE Plasma Wayland desktop image.
- Add server/minimal image with systemd-networkd option.
- Add PREEMPT_RT image path for realtime audio.

Exit criteria:

- Each edition installs in a VM or test machine.
- Edition defaults match documentation and validation checks.
- Bad kernel/update rollback is tested.

## Phase 5: Benchmark Lab

Status: planned.

- Automate benchmark scenarios from `benchmarks/manifest.toml`.
- Compare against Fedora, Ubuntu, Arch, and a minimal tuned baseline.
- Publish results and regressions.
- Add policy rollback on measured regressions.

Exit criteria:

- Public benchmark report exists.
- Regressions block release.
- Optimizer decisions can be correlated with benchmark traces.

