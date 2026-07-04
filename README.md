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
  <a href="VERSION"><img src="https://img.shields.io/badge/version-0.7.0--beta.1-blue?style=flat-square" alt="version"></a>
</p>

<hr>

**Rush Linux is an Arch-based distribution that watches what you're doing and tunes the hardware to match.** Fast CPU when you're compiling. Tight latency when you're on a video call. Idle states when you're reading. No manual power profiles to flip between — every decision is logged, explained, and reversible.

It's early beta. The optimizer (`optid`) runs in safe dry-run mode, the boot path is verified end-to-end (UKI + systemd-boot + signed rollback), and a measurement harness (`rushbench`) is operational. The desktop and laptop editions are not yet buildable; a consumer-installable distribution is the long-term goal. What works today is real, verified, and committed.

---

## Rush LiveDev quick start

Run benchmarks, capture evidence, and prepare PRs — all from one command:

```sh
python3 tools/livedev-next          # check repo state + next commands
python3 tools/livedev-next --mock   # run mock tests (no hardware, no network)
python3 tools/livedev-next --plan   # generate a benchmark plan
```

Full runbook: [`docs/livedev/OPERATOR_RUNBOOK.md`](docs/livedev/OPERATOR_RUNBOOK.md)

LiveDev is the automation foundation: it plans benchmark campaigns, runs them through `rush-exec`, captures tamper-evident evidence with `rush-capture`, validates with `validate-hwtest-evidence`, optionally repairs failures with the mock AI harness (`rush-agent`), and prepares evidence PRs for maintainer review. It never merges, never marks milestones verified, and never edits release truth.

---

## Try it on real hardware (testOS — legacy/manual path)

Prefer a USB stick and a real machine? testOS boots a minimal Rush Linux image, runs the benchmark suite, and writes results back to the USB. No toolchain install required.

> **Latest release: [v0.7.0-beta.2](https://github.com/Nan0pk/Rush-linux/releases/tag/v0.7.0-beta.2)**

<details>
<summary><strong>Linux</strong> — write the USB</summary>

```bash
wget https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh
less install.sh    # inspect it if you like
sudo bash install.sh /dev/sdX
```

Safety checks: refuses the host's root disk, refuses non-removable disks without `--force`, refuses mounted devices, warns if the target is much larger than the image, asks `yes` before writing.
</details>

<details>
<summary><strong>Windows</strong> — native PowerShell, no WSL</summary>

```powershell
curl.exe -L -o install.ps1 https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.ps1
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

If blocked: `Unblock-File .\install.ps1` first. The installer scans for USB disks automatically. Safety checks: refuses the system disk, refuses non-USB bus types without `-Force`, asks `yes` before writing.
</details>

<details>
<summary><strong>macOS</strong> — download-then-run</summary>

```bash
diskutil list
curl -fsSL -o install.sh https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh
sudo bash install.sh /dev/diskN
```

</details>

**After writing the USB:** plug it into the test machine, reboot, pick USB from the boot menu (disable Secure Boot if it refuses). testOS boots to a console menu — type `0` for all benchmarks, or pick specific numbers. Press Esc to abort early. Results are saved to the USB; testOS reboots back to the host OS when done.

**Pull results into the repo:**

```bash
sudo testos-ingest pull /dev/sdX
testos-ingest format
testos-ingest commit
git push
```

Or on Windows: `.\collect-results.ps1` does everything (mount, copy, branch, commit, push, PR) in one command. Full testOS docs: [`testos/README.md`](testos/README.md).

---

## How it works

```mermaid
flowchart LR
    subgraph Sensors["Sensors (every 2s)"]
        PSI["/proc/pressure"]
        Power["/sys/power_supply"]
        Thermal["/sys/thermal"]
        Load["/proc/loadavg"]
    end

    subgraph Classify["Classify into 5 classes"]
        Idle["idle"]
        Light["light"]
        Interactive["interactive"]
        LatCrit["latency-critical"]
        Throughput["throughput"]
    end

    Contracts["contracts.toml<br/>PM QoS floors per class"]

    subgraph Actuate["Actuate (--apply)"]
        EPP["energy_perf_preference"]
        Profile["platform_profile"]
        PMQoS["PM QoS latency floors"]
        Cgroup["cgroup slice weights"]
    end

    Sensors --> Classify
    Classify --> Contracts
    Contracts --> Actuate
```

`optid` polls sensors every 2 seconds, maps the current system state to one of five workload classes, looks up the PM QoS contract for that class, and either logs its intended actions (dry-run, the default) or applies them to the kernel.

`optctl pin <app> <class>` lets applications claim a class directly — a game can hold `latency-critical`, a build job can hold `throughput`.

Default mode is always **dry-run**. Kernel writes require explicit `--apply` on a supported host.

| Workload class | CPU governor | EPP | Platform profile | PM QoS CPU latency | Use case |
|:---------------|:-------------|:----|:------------------|:-------------------|:---------|
| `idle` | powersave | power | low-power | 100 ms | Screen off, no foreground app |
| `light` | powersave | balance_performance | balanced | 50 ms | Reading, browsing, light editor |
| `interactive` | performance | performance | balanced | 1 ms | Typing, scrolling, UI interaction |
| `latency-critical` | performance | performance | performance | 10 µs | Video call, game, audio session |
| `throughput` | performance | performance | performance | 10 ms | Compile, render, batch job |

PM QoS CPU latency is the hard floor — the kernel will not let any CPU enter a C-state deeper than that floor allows. EPP is the hint the CPU scheduler uses to trade frequency vs. efficiency. Platform profile is the ACPI-level hint that drives fan curves, dGPU power, USB autosuspend, etc.

---

## What's built

- **`optid` daemon** — PSI + thermal + power-supply sensor polling, workload classification, PM QoS contract enforcement. Applies EPP, platform profile, and cgroup slices when run with `--apply`. Every decision is logged and explainable.
- **`optctl` CLI** — D-Bus client (`io.rushlinux.Optid1`): `status`, `explain`, `mode`, `pin`. Machine-readable output via `--json`.
- **`rushbench` harness** — measures battery drain (`energy_now` or RAPL) and latency (PSI avg10, cyclictest, foreground launch) per workload class.
- **Rush LiveDev** — automation foundation: planner, runner, capture, evidence validator, AI harness, PR submission. See [`docs/livedev/OPERATOR_RUNBOOK.md`](docs/livedev/OPERATOR_RUNBOOK.md).
- **testOS** — bootable USB image for real-hardware benchmarking. See [testOS README](testos/README.md).
- **Bootable VM** — Arch-based rootfs boots to `multi-user.target` via UKI through OVMF/systemd-boot with `optid.service` active. Verified.
- **Installable system** — `tools/rush-install.sh` stamps the mkosi-built image onto a blank disk via `systemd-repart`; installed system boots twice cleanly with `optid.service` active.
- **Rollback + signing** — systemd-sysupdate descriptors, ≥3 retained boot entries, boot assessment service, Ed25519 update metadata signing. Bad-kernel rollback verified.

---

## Technology choices

| Layer | Choice | Why |
|:------|:-------|:----|
| Optimizer | `optid` (Rust) | No GC pauses; direct sysfs access; auditable actuator path |
| Pressure sensing | PSI (`/proc/pressure`) | Kernel-native; quantifies actual CPU/IO/memory stall time |
| Latency enforcement | PM QoS (`/dev/cpu_dma_latency`) | Hard per-class latency floors, not soft hints |
| Image composition | mkosi + Arch Linux | Declarative, reproducible; no bespoke build scripts |
| Scheduling | sched_ext / scx_loader | BPF user-space scheduler; EEVDF as the verified fallback |
| Boot | UKI + systemd-boot | Atomic, signed, single-file boot entries |
| Updates | systemd-sysupdate | Structured, rollback-aware OTA |
| Firewall | nftables | Current kernel default; replaces iptables |
| Desktop | Wayland + PipeWire | Native compositor and audio stack *(planned, not yet built)* |

---

## Build from source

```bash
git clone https://github.com/Nan0pk/Rush-linux.git
cd Rush-linux
cargo build --workspace --release

# One sensor read + classify cycle (dry-run, exits after one pass)
./target/release/optid --once

# Check current status from the state directory
./target/release/optctl status

# Build the testOS USB image from source (requires mkosi + archlinux-keyring;
# ~10 minutes the first time)
sudo bash testos/build-testos.sh

# Run the full test suite
cargo test --workspace
```

Or open in VS Code Dev Containers or GitHub Codespaces — the checked-in [dev container](.devcontainer/devcontainer.json) provisions Rust stable, Python 3.11+, and PowerShell Core, then builds the workspace automatically.

---

## Documentation

- [Architecture](docs/architecture.md) — how `optid`, `optctl`, and the systemd units fit together
- [Boot and updates](docs/boot-and-updates.md) — UKI, systemd-boot, signed rollback
- [Adaptive engine](docs/adaptive-engine.md) — workload classification, PM QoS contracts
- [LiveDev operator runbook](docs/livedev/OPERATOR_RUNBOOK.md) — how to run benchmarks, capture evidence, submit PRs
- [LiveDev developer guide](docs/livedev-developer-guide.md) — architecture boundaries, tool roles, data flow
- [Benchmark methodology](docs/decisions/0011-benchmark-methodology.md) — how claims are measured
- [Testing strategy](docs/testing-and-benchmarks.md) — release gates and tiers
- [testOS README](testos/README.md) — full design rationale for the USB benchmark environment
- [Roadmap](ROADMAP.md) — where the project is going
- [All docs](docs/SUMMARY.md)

---

## Status

| Milestone | State |
|:----------|:------|
| Phase 0 — repo, ADRs, CI | ✅ complete |
| v0.1 — compile-clean core, `optid --once` | ✅ complete |
| v0.2 — D-Bus control plane, `optctl` | ✅ complete |
| v0.3 — rootfs builder, VM boots | ✅ complete |
| v0.4 — UKI boot, rollback, update signing | ✅ complete |
| v0.5 — minimal installable system (mkosi/Arch) | ✅ complete |
| **v0.6 — hardware-aware optid, PPD/GameMode shims** | ⚙ in progress |
| v0.7 — desktop / laptop / realtime / server editions | planned |
| v0.8 — benchmark lab, published results | planned |
| v0.9 — release candidate hardening | planned |
| v1.0 — installable, benchmarked, stable | planned |

[Full roadmap](ROADMAP.md)

### The evidence rule

Rush Linux enforces a **Builder/Verifier separation**: no claim of correctness or performance is accepted without a literal command transcript. `✅ Verified` means a human or CI ran the command and the output is on record. Claims without transcripts are proposals, not facts. This applies to benchmarks, boot verification, and optimizer behavior equally. See [the testing doc](docs/testing-and-benchmarks.md) and the [agent protocol](docs/agent-protocol.md).

---

## Contributing

We are looking for kernel engineers, Rustaceans, and systems programmers who value verifiable claims over marketing copy.

- [How Rush is built](docs/how-rush-is-built.md)
- [Agent and contributor protocol](docs/agent-protocol.md)
- [Contributing guide](CONTRIBUTING.md)
- [Open a discussion](https://github.com/Nan0pk/Rush-linux/discussions)

---

## License

[Apache-2.0](LICENSE). Copyright the Rush Linux authors.
