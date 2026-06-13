# Rush Linux

Rush Linux is a source-built Linux distribution project centered on one native
optimization service: `optid`. The distribution target is a modern,
future-facing Linux baseline with automatic, explainable runtime policy changes
for responsiveness, battery life, thermal behavior, and resource utilization.

Repository: https://github.com/Nan0pk/Rush-linux

**[Contributing](CONTRIBUTING.md)** · **[Code of Conduct](CODE_OF_CONDUCT.md)** · **[Security](SECURITY.md)** · **[Discussions](https://github.com/Nan0pk/Rush-linux/discussions)**

This repository is the first implementation slice:

- Rust workspace for `optid` and `optctl`.
- Kernel config fragments for adaptive and realtime kernels.
- Future-facing system defaults for cgroup v2, PSI, UKI boot, nftables,
  Wayland, PipeWire, and signed rollbackable updates.
- Declarative `mkosi` image composition for the core OS.
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
mkosi/                    Declarative image definitions and overlays
release/                  Version milestones and test-tier gates
tools/                    Local validation scripts
docs/                     Architecture and implementation notes
```

## First-Class Documentation

Start with:

- **[How Rush is Built](docs/how-rush-is-built.md)** — The honest provenance manifesto
- **[Agent Protocol](docs/agent-protocol.md)** — Rules of engagement and the Evidence Rule
- [Project brief](PROJECT_BRIEF.md)
- [AI continuation guide](AI_CONTINUATION.md)
- [Implementation status](IMPLEMENTATION_STATUS.md)
- [Roadmap](ROADMAP.md)
- [Release ledger](RELEASES.md)
- [Versioning](docs/versioning.md)
- [Release policy](docs/release-policy.md)
- [Release checklist](docs/release-checklist.md)
- [v1 release plan](docs/release-plan-v1.md)
- [Documentation policy](docs/documentation-policy.md)
- [Graphify knowledge graph](docs/graphify-knowledge-graph.md)
- [Architecture](docs/architecture.md)
- [Adaptive engine](docs/adaptive-engine.md)
- [Kernel policy](docs/kernel-policy.md)
- [Packaging and builds](docs/packaging-and-builds.md)
- [Boot and updates](docs/boot-and-updates.md)
- [Hardware support](docs/hardware-support.md)
- [Testing and benchmarks](docs/testing-and-benchmarks.md)
- [Non-goals](docs/non-goals.md)
- [Doc registry](docs/docmap.toml) — maps every doc to its purpose, code coverage, and dependencies
- [Keeping docs in sync](docs/contributing/keeping-docs-synced.md) — how to update docs without drift
- [ADRs](docs/decisions/)

Documentation is part of acceptance criteria. Changes to behavior, defaults,
policy, boot/update flow, kernel fragments, recipes, or tests must update the
relevant docs in the same change.

## Knowledge Graph For Continuation

This repository commits a Graphify knowledge graph under `graphify-out/` so
future agents can query architecture and code relationships before spending
context on broad file reads. Start with:

```sh
graphify query "what should I inspect before changing optid policy?" --graph graphify-out/graph.json
```

After code or supported config changes, refresh the graph without LLM/API token
use:

```sh
./tools/graphify-refresh.sh code
```

For Markdown/design-document semantic refreshes, run the explicit full mode with
a configured backend. See [Graphify knowledge graph](docs/graphify-knowledge-graph.md).

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

The v0.3 VM path boots to `multi-user.target`, and the v0.4 UKI path is now
validated under QEMU/OVMF with systemd-boot loading `/EFI/Linux/rush-linux.efi`
and `optid.service` starting.

`optctl` communicates with `optid` via D-Bus (system bus) with automatic
fallback to the state directory when D-Bus is offline:

- `optctl status` — show current optimizer state (`--json` for machine-readable output)
- `optctl explain` — show decision history with reasons
- `optctl mode [auto|battery|balanced|performance|realtime]` — get or set optimizer mode
- `optctl pin <app_id> <mode>` — pin an application to a specific mode
- `optctl trace` — show applied action log
- `optctl benchmark` — benchmark suite placeholder

## Build

Install a current Rust toolchain, then run:

```sh
cargo build --workspace
cargo test --workspace
```

Linux (native or a container) is the canonical development and build
environment: the rootfs builder, UKI generation, `systemd-repart`, and QEMU
boot all require Linux, and CI builds and tests on Linux. Develop and verify on
Linux.

The repository-policy check is cross-platform and runs under PowerShell Core
(`pwsh`), including in CI on Linux:

```sh
pwsh ./tools/validate-repo.ps1
```

It is a convenience for contributors on Windows, not a substitute for building
and testing on Linux.

## GitHub CI

The repository includes GitHub Actions checks for:

- `cargo fmt`
- `cargo test`
- `cargo clippy -D warnings`
- future-facing repository policy validation
- Graphify knowledge-graph refresh on `main` pushes

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
- Use `sched_ext` (via `scx_loader` and `optid`) as the default scheduler, with EEVDF fallback.
- Use `mkosi` for declarative, reproducible image composition on an Arch Linux base.
- Use eBPF/PSI/cgroup data for observability, with strict overhead limits.
- Do not run TLP, power-profiles-daemon, or TuneD as active default policy
  daemons. Compatibility can exist, but `optid` owns the knobs.
- Avoid legacy defaults unless no modern alternative works.

## Community

- **Issues:** [Report bugs or request features](https://github.com/Nan0pk/Rush-linux/issues)
- **Discussions:** [Ask questions, share ideas](https://github.com/Nan0pk/Rush-linux/discussions)
- **Good first issues:** [Starter tasks for new contributors](https://github.com/Nan0pk/Rush-linux/labels/good%20first%20issue)
- **Security:** [Report vulnerabilities privately](https://github.com/Nan0pk/Rush-linux/security/advisories/new)

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to get started.
