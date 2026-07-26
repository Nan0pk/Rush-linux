<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/banner-dark.svg">
    <img src="docs/assets/banner-light.svg" alt="Rush Linux — adaptive power and performance for Arch" width="720">
  </picture>
</p>

<p align="center">
  <em>adaptive power &amp; performance for Arch</em>
</p>

<p align="center">
  <a href="https://github.com/Nan0pk/Rush-linux/actions"><img src="https://img.shields.io/github/actions/workflow/status/Nan0pk/Rush-linux/ci.yml?branch=main&style=flat-square" alt="CI"></a>
  <a href="https://github.com/Nan0pk/Rush-linux/releases"><img src="https://img.shields.io/github/v/release/Nan0pk/Rush-linux?include_prereleases&style=flat-square" alt="release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square" alt="license"></a>
  <a href="VERSION"><img src="https://img.shields.io/badge/version-0.7.0--beta.4-blue?style=flat-square" alt="version"></a>
</p>

Rush Linux is an Arch-based operating-system project that adapts power and
performance to the work being done. `optid` observes the machine, classifies the
workload, explains its decisions, and can apply guarded hardware settings when
explicitly enabled.

## Start here

| I want to… | Best starting point |
| --- | --- |
| Try Rush in a VM or from USB | [LiveDev for Linux/macOS](#command-livedev-posix) or [Windows](#command-livedev-windows) |
| Compare `optid` on my current Linux install | [Run the host benchmark](#command-host-benchmark) |
| Understand the adaptive control loop | [What `optid` does](#what-optid-does) |
| Continue project development | [Current work selector](CURRENT_WORK.md) |
| Build Rush or contribute | [Build and development](#build-and-development) |

<!-- RUSH_FRONTPAGE:START -->
<a id="command-livedev-posix"></a>
## Rush LiveDev quick start

**Environment:** Linux / macOS

```sh
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.sh -o livedev-bootstrap.sh && bash livedev-bootstrap.sh
```

Detects an existing results USB, QEMU, or a USB-preparation path and chooses the useful next action.

## Repository status

This table is generated from the repository's canonical version, build, and work-state files.

| Item | Current state |
| --- | --- |
| Project stage | Early beta: dry-run is the safe default; automatic hardware actuation is not release-ready. |
| Version | `0.7.0-beta.4` |
| Active general repair | `F2` — Introduce injectable kernel I/O, clock, and event boundaries (`merged_incomplete`) |
| Active safety repair | `D0` — Prototype capability sealing and supervisor-managed cold restart (`merged_incomplete`) |
| Other merged, incomplete packages | `F1`, `F3`, `F4`, `T1` |
| Build profiles | `desktop`, `laptop`, `livedev`, `realtime-audio`, `server`, `testos` |
| Safety architecture | [D2 fail-passive](docs/architecture/optid-d2-amendment.md) |
| Canonical work state | [optid package ledger](docs/plans/optid-package-status.toml) |

## Choose a command

Pick the goal that matches what you want to do. Detailed options stay in the linked runbooks.

| Goal | Environment |
| --- | --- |
| [Run the smart LiveDev flow](#command-livedev-posix) | Linux / macOS |
| [Run the smart LiveDev flow](#command-livedev-windows) | Windows PowerShell (Administrator) |
| [Compare optid on an existing Linux install](#command-host-benchmark) | Linux |
| [Start a clean development task](#command-start-development) | Cloned repository |
| [Run every check relevant to a change](#command-verify-change) | Cloned repository |
| [Build the LiveDev image](#command-build-livedev) | Supported Linux build host |

## Other command details

<a id="command-livedev-windows"></a>
### Run the smart LiveDev flow

**Environment:** Windows PowerShell (Administrator)

```powershell
curl.exe -fL -o livedev-bootstrap.ps1 https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.ps1; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; powershell -ExecutionPolicy Bypass -File .\livedev-bootstrap.ps1
```

Open PowerShell as Administrator. Elevation is needed only for raw USB writing and temporary ESP mounting.

<a id="command-host-benchmark"></a>
### Compare optid on an existing Linux install

**Environment:** Linux

```sh
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/rush-host-bench.sh | sudo bash
```

No USB, VM, reboot, or Rush installation. Apply mode is experimental; a reboot remains the final recovery fallback.

<a id="command-start-development"></a>
### Start a clean development task

**Environment:** Cloned repository

```sh
bash tools/start-work.sh "short task description"
```

Creates a work branch when needed and runs the fast starting-state checks.

<a id="command-verify-change"></a>
### Run every check relevant to a change

**Environment:** Cloned repository

```sh
bash tools/finish-work.sh --dry-run
```

Uses the same change-aware check runner as CI and does not commit or push.

<a id="command-build-livedev"></a>
### Build the LiveDev image

**Environment:** Supported Linux build host

```sh
sudo bash tools/build-mkosi-image.sh --edition livedev
```

Requires Rust, mkosi, and the host dependencies listed in the build documentation.

<!-- RUSH_FRONTPAGE:END -->

## How the smart LiveDev command behaves

The bootstrap command chooses the next step from what it detects:

| Detected environment | Next action |
| --- | --- |
| Results USB present | Copy and validate results, then offer an evidence submission |
| QEMU available | Build or reuse the LiveDev image and start the VM path |
| Neither available | Prepare the real-hardware USB path and print boot instructions |

It preserves an existing checkout and does not merge pull requests or mark
milestones verified. USB writing needs `sudo` on Linux/macOS or an Administrator
PowerShell on Windows. GitHub authentication is needed only when opening an
evidence PR.

The full operator procedure is in the
[LiveDev runbook](docs/livedev/OPERATOR_RUNBOOK.md).

testOS remains the current boot backend and manual fallback for real-hardware
USB testing.

## What optid does

Every control-loop iteration follows the same basic path:

1. read pressure, power, thermal, application, and device state;
2. classify the workload as `idle`, `light`, `interactive`,
   `latency-critical`, `throughput`, or `vm.guest`;
3. resolve the desired contract and domain gates;
4. log intended actions in dry-run mode, or attempt guarded writes with
   `--apply`;
5. report support, skip, failure, and recovery outcomes.

Implemented paths include core CPU controls and initial runtime-PM, PCIe ASPM,
SATA ALPM, backlight, VM-sysctl, PM QoS, and cgroup controls. The packaged apply
service cannot yet reach all dynamic device paths, and the persistent verified
D2 recovery protocol is not complete. Treat apply mode as experimental.

See [adaptive engine](docs/adaptive-engine.md) for the control model and
[implementation status](docs/IMPLEMENTATION_STATUS.md) for the detailed
inventory.

## Build and development

The commands above are the supported entry points. Use these guides for detail:

| Topic | Guide |
| --- | --- |
| Host dependencies, mkosi profiles, and image outputs | [Build system](docs/build-system.md) |
| Source and packaging flow | [How Rush is built](docs/how-rush-is-built.md) |
| Claims, evidence, and release tiers | [Testing and benchmarks](docs/testing-and-benchmarks.md) |
| Risk-based contributor process | [Project workflow](docs/project-workflow.md) |

Only the human maintainer merges to `main`. A merged optid PR proves that code
landed; package completion additionally requires production-path integration
and independent committed evidence.

## Project map

| Area | Document |
| --- | --- |
| Product direction | [Northstar specification](docs/SPEC-northstar.md) |
| System design | [Architecture](docs/architecture.md) |
| Current agent work | [Current work selector](CURRENT_WORK.md) |
| Current implementation work | [Active `optid` completion plan](OPTID-COMPLETION-PLAN.md) |
| Compatibility | [Hardware support](docs/hardware-support.md) |
| Release direction | [Roadmap](ROADMAP.md) |
| Documentation index | [All documentation](docs/SUMMARY.md) |

> Latest release: [v0.7.0-beta.4](https://github.com/Nan0pk/Rush-linux/releases/tag/v0.7.0-beta.4)

## Contributing

Start with [CONTRIBUTING.md](CONTRIBUTING.md). The repository expects small
coherent pull requests, change-aware tests, and evidence that matches the claim.

## License

[Apache-2.0](LICENSE). Copyright the Rush Linux authors.
