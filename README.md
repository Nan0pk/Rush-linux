# Rush Linux

Rush Linux is a source-built Linux distribution project centered on one native
optimization service: `optid`. The distribution target is a modern,
future-facing Linux baseline with automatic, explainable runtime policy changes
for responsiveness, battery life, thermal behavior, and resource utilization.

Repository: https://github.com/Nan0pk/Rush-linux

This repository is the first implementation slice:

- Rust workspace for `optid` and `optctl`.
- Kernel config fragments for adaptive and realtime kernels.
- Future-facing system defaults for cgroup v2, PSI, UKI boot, nftables,
  Wayland, PipeWire, and signed rollbackable updates.
- Source recipe skeletons for the core OS packages.
- Validation checks that reject legacy defaults.

The project deliberately does not make hard realtime the universal default.
The default kernel is low-latency and adaptive; PREEMPT_RT is packaged as a
specialist kernel for workloads that need bounded latency.

## Repository Layout

```text
crates/optid/             Adaptive optimization daemon
crates/optctl/            CLI for status, mode, explain, trace, benchmark
config/optid/             Default optimizer policy
distro/boot/              UKI-first boot defaults and kernel command line
distro/kernel/            Kernel config fragments
distro/network/           nftables baseline
packaging/systemd/        systemd units and tmpfiles
recipes/                  Source package recipe skeletons
release/                  Version milestones and test-tier gates
tools/                    Local validation scripts
docs/                     Architecture and implementation notes
```

## First-Class Documentation

Start with:

- [Project brief](PROJECT_BRIEF.md)
- [AI continuation guide](AI_CONTINUATION.md)
- [Implementation status](IMPLEMENTATION_STATUS.md)
- [Roadmap](ROADMAP.md)
- [Release ledger](RELEASES.md)
- [Versioning](docs/versioning.md)
- [Release policy](docs/release-policy.md)
- [Release checklist](docs/release-checklist.md)
- [v1 release plan](docs/release-plan-v1.md)
- [Architecture](docs/architecture.md)
- [Adaptive engine](docs/adaptive-engine.md)
- [Kernel policy](docs/kernel-policy.md)
- [Packaging and builds](docs/packaging-and-builds.md)
- [Boot and updates](docs/boot-and-updates.md)
- [Hardware support](docs/hardware-support.md)
- [Testing and benchmarks](docs/testing-and-benchmarks.md)
- [Non-goals](docs/non-goals.md)
- [ADRs](docs/decisions/)

Documentation is part of acceptance criteria. Changes to behavior, defaults,
policy, boot/update flow, kernel fragments, recipes, or tests must update the
relevant docs in the same change.

## Current Implementation Status

`optid` is implemented as a safe MVP:

- Reads PSI from `/proc/pressure/{cpu,memory,io}`.
- Reads battery/AC state from `/sys/class/power_supply`.
- Reads thermal state from `/sys/class/thermal`.
- Reads load average from `/proc/loadavg`.
- Computes an adaptive mode and an explainable action plan.
- Applies only guarded actions when `--apply` is passed.
- Writes status and decision logs under `/run/optid` by default.

The packaged default service is dry-run. `optid-apply.service` exists for
explicit mutating tests only.

`optctl` currently talks through the state directory:

- `optctl status`
- `optctl explain`
- `optctl mode [auto|battery|balanced|performance|realtime]`
- `optctl trace`
- `optctl benchmark`

The next implementation step is replacing file-based control with a D-Bus API
while keeping the file state as a recovery/debug path.

## Build

Install a current Rust toolchain, then run:

```sh
cargo build --workspace
cargo test --workspace
```

This Windows workspace does not currently have Rust installed, so validation is
provided through PowerShell:

```powershell
.\tools\validate-repo.ps1
```

## GitHub CI

The repository includes GitHub Actions checks for:

- `cargo fmt`
- `cargo test`
- `cargo clippy -D warnings`
- future-facing repository policy validation

## Publishing

This local workspace is already configured for:

```sh
https://github.com/Nan0pk/Rush-linux
```

If the GitHub repository does not exist yet, create it with a token that has
repository creation permission:

```powershell
$env:GH_TOKEN = '<token>'
.\tools\publish-github.ps1
```

## Design Rules

- Use `systemd` with unified cgroup v2 only.
- Use Wayland-first desktop sessions and PipeWire/WirePlumber audio.
- Use nftables, not iptables, as the firewall baseline.
- Use UKI-first boot and rollbackable kernel entries.
- Use eBPF/PSI/cgroup data for observability, with strict overhead limits.
- Do not run TLP, power-profiles-daemon, or TuneD as active default policy
  daemons. Compatibility can exist, but `optid` owns the knobs.
- Avoid legacy defaults unless no modern alternative works.
