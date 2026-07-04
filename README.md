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

## Try it on real hardware

The fastest way to see Rush Linux in action: download the prebuilt **testOS** image, write it to a USB stick, boot any x86_64 machine from it, run the benchmark suite, and pull the results back into your repo. About 5 minutes of actual work, no toolchain install required.

> **Latest release: [v0.7.0-beta.2](https://github.com/Nan0pk/Rush-linux/releases/tag/v0.7.0-beta.2)** (prerelease — the installer fetches it automatically)
>
> Browse all releases: [github.com/Nan0pk/Rush-linux/releases](https://github.com/Nan0pk/Rush-linux/releases)

**You need:**

- A USB stick (≥ 1 GB)
- A test machine — any x86_64 PC that can boot from USB, ≥ 1 GB RAM, no OS prerequisites
- A workstation to write the USB and collect results

Pick your workstation OS:

<details>
<summary><strong>Linux</strong> — one-liner or download-then-run</summary>

```bash
# Recommended: download, inspect, then run.
wget https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh
less install.sh    # inspect it if you like
sudo bash install.sh /dev/sdX
```

Or, if you trust the source and want a one-liner:

```bash
wget -qO- https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh | sudo bash -s -- /dev/sdX
```

Find your USB device first with `lsblk` (look for `RM=1` — removable). Safety checks:

- Refuses to write to the host's root disk.
- Refuses non-removable disks (`RM=0`) unless `--force` (catches accidental targeting of internal SATA/NVMe disks).
- Refuses mounted devices.
- Warns if the target disk is much larger than the image (suspicious — wrong disk?).
- Shows the disk's VENDOR, MODEL, SIZE, TRAN, RM and asks `yes` before writing.

</details>

<details>
<summary><strong>Windows</strong> — native PowerShell, no WSL, no Rufus</summary>

Open **PowerShell as Administrator** (right-click PowerShell → "Run as Administrator"), then run:

```powershell
# Step 1: Download the installer:
curl.exe -L -o install.ps1 https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.ps1

# Step 2: Run it (bypass execution policy for this process only — Windows
#         blocks downloaded scripts by default):
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

The installer scans for USB disks automatically. If exactly one USB stick is plugged in, it uses it. If multiple USB sticks are plugged in, it shows a numbered list and asks you to pick. You can also pass `-Device \\.\PhysicalDrive<N>` explicitly if you prefer (find the number with `Get-Disk | Format-Table Number, FriendlyName, BusType`).

If Step 2 still fails with "cannot be loaded because running scripts is disabled," unblock the file first (Windows marks downloaded files with a "Mark of the Web"):

```powershell
Unblock-File .\install.ps1
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

The installer uses native Windows APIs (`CreateFile` + `WriteFile` via P/Invoke) to write the image directly to the raw disk — no Rufus, no Etcher, no WSL. Safety checks:

- Refuses to write to the Windows system disk.
- Refuses non-USB bus types unless `-Force` (catches accidental targeting of internal SATA/NVMe disks).
- Auto-clears any existing partitions on the USB (Windows auto-mounts every USB stick, so the script handles this for you — no `-Force` needed for a fresh USB).
- Shows the disk's FriendlyName, BusType, Size, PartitionStyle and asks `yes` before writing.

</details>

<details>
<summary><strong>macOS</strong> — download-then-run</summary>

```bash
# Find your USB device:
diskutil list

# Download and run:
curl -fsSL -o install.sh https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh
sudo bash install.sh /dev/diskN
```

The installer uses `dd` on macOS. The `testos-launcher` and `testos-ingest` binaries are Linux-only — on macOS, you'll need to collect results from a Linux machine (or build the binaries from source).

</details>

### What happens after you write the USB

1. Plug the USB into the test machine.
2. Reboot. Enter the boot menu (F12, F8, F11, or Esc — depends on the vendor).
3. Pick the USB from the list.
4. If it refuses to boot, disable Secure Boot in the firmware — testOS UKIs are unsigned for now.

testOS boots to a console menu on the screen. No login required.

```
Available benchmarks:
  [0] Run all (estimated 3m 40s)
  [1] fio — sequential read IOPS (30s)
  [2] fio — sequential write IOPS (30s)
  [3] iperf3 — TCP throughput (loopback) (20s)
  [4] PostgreSQL — pgbench TPS (1m)
  [5] nginx — requests per second (30s)
  [6] PSI — CPU pressure avg10 (5s)
  [7] PSI — IO pressure avg10 (5s)
  [8] cyclictest — max latency (µs) (30s)
  [9] foreground launch latency (ms) (10s)

Select (comma-separated numbers, or 0 for all, or 'q' to quit):
```

Type `0` for all, or pick specific numbers separated by commas (e.g. `1,3,5`). Progress is printed line by line with per-test ETA. **Press Esc at any time to abort early** — partial results are saved. When the run finishes, testOS syncs the USB, waits 5 seconds, and reboots back to the host OS.

### Pull the results into the repo

Plug the USB back into your workstation.

**On Windows (one command, fully automated):**

```powershell
# Set your GitHub token (needs repo scope for push + PR merge):
$env:GITHUB_TOKEN = "github_pat_xxx..."

# Download and run the collector - it does EVERYTHING:
curl.exe -L -o collect-results.ps1 https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/collect-results.ps1
powershell -ExecutionPolicy Bypass -File .\collect-results.ps1
```

The collector automatically:
1. Finds the USB, mounts the ESP partition
2. Copies `testos-results\` + install logs
3. Reads `manifest.json` for pass/fail counts
4. Clones the repo, creates a branch, commits the results
5. Pushes the branch, opens a PR
6. Waits for CI checks to pass (up to 10 min)
7. Auto-merges the PR to main
8. Cleans up the temp clone and unmounts the USB

No manual git, no manual mount, no manual PR. One command, done.

**On Linux:**

```bash
sudo testos-ingest pull /dev/sdX
testos-ingest format
testos-ingest commit
git push
```

Run `.\collect-results.ps1 -Diagnose` to see all disks/partitions if something goes wrong. Run with `-DryRun` to do everything except push (useful for testing).

Results land in `benchmarks/results/<UTC-date>/<host-fingerprint>/`.

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

---

## Power profile comparison

What `optid` actually does per workload class, when run with `--apply` on supported hardware:

| Workload class | CPU governor | EPP | Platform profile | PM QoS CPU latency | Use case |
|:---------------|:-------------|:----|:------------------|:-------------------|:---------|
| `idle` | powersave | power | low-power | 100 ms | Screen off, no foreground app |
| `light` | powersave | balance_performance | balanced | 50 ms | Reading, browsing, light editor |
| `interactive` | performance | performance | balanced | 1 ms | Typing, scrolling, UI interaction |
| `latency-critical` | performance | performance | performance | 10 µs | Video call, game, audio session |
| `throughput` | performance | performance | performance | 10 ms | Compile, render, batch job |

PM QoS CPU latency is the hard floor — the kernel will not let any CPU enter a C-state deeper than that floor allows. A `latency-critical` class holds a 10 µs floor, so the CPU stays in shallow C-states and wakes fast. A `throughput` class relaxes to 10 ms because raw throughput doesn't care about wake latency.

EPP (`energy_perf_preference`) is the hint the CPU scheduler uses to trade frequency vs. efficiency. Platform profile is the ACPI-level hint that drives fan curves, dGPU power, USB autosuspend, etc.

---

## What's built

- **`optid` daemon** — PSI + thermal + power-supply sensor polling, workload classification, PM QoS contract enforcement. Applies EPP, platform profile, and cgroup slices when run with `--apply`. Every decision is logged and explainable.
- **`optctl` CLI** — D-Bus client (`io.rushlinux.Optid1`): `status`, `explain`, `mode`, `pin`. Machine-readable output via `--json`.
- **`rushbench` harness** — measures battery drain (`energy_now` or RAPL) and latency (PSI avg10, cyclictest, foreground launch) per workload class. Structured energy windows, N-sample collection, anomaly detection.
- **testOS** — bootable USB image for real-hardware benchmarking. See [Try it on real hardware](#try-it-on-real-hardware) above.
- **Bootable VM** — Arch-based rootfs boots to `multi-user.target` via UKI through OVMF/systemd-boot with `optid.service` active. Verified; transcript at `release/evidence/v0.3.0-alpha.1/`.
- **Installable system** — `tools/rush-install.sh` stamps the mkosi-built image onto a blank disk via `systemd-repart`; installed system boots twice cleanly with `optid.service` active. Transcripts at `release/evidence/v0.5.0-beta.1/`.
- **Rollback + signing** — systemd-sysupdate descriptors, ≥3 retained boot entries, boot assessment service, Ed25519 update metadata signing. Bad-kernel rollback verified; transcript at `release/evidence/v0.4.0-alpha.1/c3-bad-kernel/`.

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

For developers who want to modify Rush itself, build the testOS image locally, or run the optimizer in dry-run mode:

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
- [Benchmark methodology](docs/decisions/0011-benchmark-methodology.md) — how claims are measured
- [Testing strategy](docs/testing-and-benchmarks.md) — release gates and tiers
- [testOS README](testos/README.md) — full design rationale and known limitations for the USB benchmark environment
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

## Current LiveDev operator path

```sh
python3 tools/livedev-next
python3 tools/livedev-next --mock
python3 tools/livedev-next --plan
```

For the full runbook, see [docs/livedev/OPERATOR_RUNBOOK.md](docs/livedev/OPERATOR_RUNBOOK.md).

---

## License

[Apache-2.0](LICENSE). Copyright the Rush Linux authors.
