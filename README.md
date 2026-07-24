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

This is an early beta. Dry-run is the safe default. The boot/update path and
benchmark tooling are usable; automatic hardware actuation and crash-safe
handback are still under active construction.

**Rush LiveDev is the quickest way to try the project:** one smart command
chooses the useful VM, USB, or results-resume path for the machine it runs on.

<!-- RUSH_FRONTPAGE:START -->
### Current repository truth

- **Version:** `0.7.0-beta.4`
- **Stage:** Early beta: dry-run is the safe default; automatic hardware actuation is not release-ready.
- **Build profiles:** `desktop`, `laptop`, `livedev`, `realtime-audio`, `server`, `testos`
- **Active general repair:** `F1` — Freeze capability states and domain configuration (`candidate`)
- **Active safety repair:** `D0` — Prototype capability sealing and supervisor-managed cold restart (`merged_incomplete`)
- **Other merged but incomplete packages:** `F2`, `F3`, `F4`
- **Safety architecture:** [D2 fail-passive](docs/architecture/optid-d2-amendment.md)
- **Canonical work state:** [optid package ledger](docs/plans/optid-package-status.toml)

### Practical command guide

Use the first command that matches what you want to do. Detailed options stay in the linked runbooks.

#### Run the smart LiveDev flow — Linux / macOS

```sh
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.sh -o livedev-bootstrap.sh && bash livedev-bootstrap.sh
```

Detects an existing results USB, QEMU, or a USB-preparation path and chooses the useful next action.

#### Run the smart LiveDev flow — Windows PowerShell (Administrator)

```powershell
curl.exe -fL -o livedev-bootstrap.ps1 https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.ps1; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; powershell -ExecutionPolicy Bypass -File .\livedev-bootstrap.ps1
```

Open PowerShell as Administrator. Elevation is needed only for raw USB writing and temporary ESP mounting.

#### Compare optid on an existing Linux install — Linux

```sh
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/rush-host-bench.sh | sudo bash
```

No USB, VM, reboot, or Rush installation. Apply mode is experimental; a reboot remains the final recovery fallback.

#### Start a clean development task — Cloned repository

```sh
bash tools/start-work.sh "short task description"
```

Creates a work branch when needed and runs the fast starting-state checks.

#### Run every check relevant to a change — Cloned repository

```sh
bash tools/finish-work.sh --dry-run
```

Uses the same change-aware check runner as CI and does not commit or push.

#### Build the LiveDev image — Supported Linux build host

```sh
sudo bash tools/build-mkosi-image.sh --edition livedev
```

Requires Rust, mkosi, and the host dependencies listed in the build documentation.

<!-- RUSH_FRONTPAGE:END -->

## How the smart LiveDev command behaves

The same bootstrap command finds the useful next step:

- results USB present → copy, validate, and offer an evidence submission;
- QEMU available → build or reuse the LiveDev image and run the VM path;
- neither available → prepare the real-hardware USB path and print the boot
  instructions.

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

The practical commands above are the supported entry points. Useful details:

- [build system](docs/build-system.md) — host dependencies, mkosi profiles, and
  image outputs;
- [how Rush is built](docs/how-rush-is-built.md) — source and packaging flow;
- [testing](docs/testing-and-benchmarks.md) — claims, evidence, and release
  tiers;
- [project workflow](docs/project-workflow.md) — the risk-based contributor
  process.

Only the human maintainer merges to `main`. A merged optid PR proves that code
landed; package completion additionally requires production-path integration
and independent committed evidence.

## Project map

- [Northstar specification](docs/SPEC-northstar.md)
- [Architecture](docs/architecture.md)
- [Active optid completion plan](OPTID-COMPLETION-PLAN.md)
- [Hardware support](docs/hardware-support.md)
- [Roadmap](ROADMAP.md)
- [All documentation](docs/SUMMARY.md)

> Latest release: [v0.7.0-beta.4](https://github.com/Nan0pk/Rush-linux/releases/tag/v0.7.0-beta.4)

## Contributing

Start with [CONTRIBUTING.md](CONTRIBUTING.md). The repository expects small
coherent pull requests, change-aware tests, and evidence that matches the claim.

## License

[Apache-2.0](LICENSE). Copyright the Rush Linux authors.
