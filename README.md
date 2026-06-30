<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/banner-dark.svg">
    <img src="docs/assets/banner-light.svg" alt="Rush Linux — adaptive power and performance for Arch" width="640">
  </picture>
</p>

<p align="center">
  <a href="VERSION"><img src="https://img.shields.io/badge/version-0.7.0--beta.1-blue" alt="version"/></a>
  <a href="ROADMAP.md"><img src="https://img.shields.io/badge/status-v0.6%20in%20progress-green" alt="status"/></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="license"/></a>
  <a href="https://github.com/Nan0pk/Rush-linux/actions"><img src="https://img.shields.io/github/actions/workflow/status/Nan0pk/Rush-linux/ci.yml?branch=main" alt="CI"/></a>
  <a href="https://github.com/Nan0pk/Rush-linux/releases"><img src="https://img.shields.io/github/v/release/Nan0pk/Rush-linux?include_prereleases" alt="release"/></a>
</p>

---

**Rush Linux is an Arch-based distribution that watches what you're doing and tunes the hardware to match.** Fast CPU when you're compiling. Tight latency when you're on a video call. Idle states when you're reading. No manual power profiles to flip between — every decision is logged, explained, and reversible.

It's early beta. The optimizer (`optid`) runs in safe dry-run mode, the boot path is verified end-to-end (UKI + systemd-boot + signed rollback), and a measurement harness (`rushbench`) is operational. The desktop and laptop editions are not yet buildable; a consumer-installable distribution is the long-term goal. What works today is real, verified, and committed.

---

## Try it on real hardware

The fastest way to see Rush Linux in action: download the prebuilt **testOS** image, write it to a USB stick, boot any x86_64 machine from it, run the benchmark suite, and pull the results back into your repo. About 5 minutes of actual work, no toolchain install required.

**You need:**

- A USB stick (≥ 1 GB)
- A test machine — any x86_64 PC that can boot from USB, ≥ 1 GB RAM, no OS prerequisites
- A workstation (Linux or macOS) to write the USB and collect results

### Step 1 — Download and write the image

Find your USB device with `lsblk`, then run:

```bash
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh \
  | sudo bash -s -- /dev/sdX
```

The installer:

1. Fetches the latest release from [Releases](https://github.com/Nan0pk/Rush-linux/releases).
2. Downloads the `.raw` image, the `testos-launcher` and `testos-ingest` binaries, and `SHA256SUMS`.
3. Verifies the checksums.
4. Refuses to write to a mounted device or anything that looks like your system disk.
5. Asks you to type the device name twice to confirm.
6. Writes the image with `dd`, syncs, and prints the next steps.

**No Linux workstation?** Download `testos-<version>.raw` from [Releases](https://github.com/Nan0pk/Rush-linux/releases) and write it with [Rufus](https://rufus.ie/) (Windows) or [balenaEtcher](https://etcher.balena.io/) (macOS / Windows / Linux). Then download `testos-ingest-<version>-linux-x86_64` separately for Step 5.

### Step 2 — Boot the test machine from the USB

1. Plug the USB into the test machine.
2. Reboot. Enter the boot menu (F12, F8, F11, or Esc — depends on the vendor).
3. Pick the USB from the list.
4. If it refuses to boot, disable Secure Boot in the firmware — testOS UKIs are unsigned for now.

testOS boots to a console menu on the screen. No login required.

### Step 3 — Pick benchmarks and let them run

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

### Step 4 — Pull the results into the repo

Plug the USB back into your workstation:

```bash
sudo testos-ingest pull /dev/sdX
testos-ingest format
testos-ingest commit
git push
```

- `pull` — mounts the USB, finds the latest run, copies results to `benchmarks/results/<UTC-date>/<host-fingerprint>/`.
- `format` — generates a `SUMMARY.md` Markdown table of all results, plus a failures section.
- `commit` — `git add` + `git commit` with a conventional message like `evidence(bench): testOS run 2026-06-30 host=ab12cd34 pass=9 fail=0 skip=0`.

That's it. Results are now in the repo and ready to push.

### Want to add a new benchmark?

Open `testos/bench-list.toml` and add a `[[benches]]` entry — `id`, `name`, `scenario`, `kind`, `command`, `estimated_seconds`. Rebuild the image (see Build from source below) and you're done. No code changes required. Full catalog spec: [`testos/README.md`](testos/README.md).

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

## How it works

```
  Sensors               Classify                Actuate  (--apply)
  ─────────────────     ───────────────────     ─────────────────────────
  /proc/pressure  ─┐    idle                    /dev/cpu_dma_latency
  /sys/power_supp  ├──► light               ──► energy_perf_preference
  /sys/thermal     │    interactive              acpi/platform_profile
  /proc/loadavg  ──┘    latency-critical         pci/*/pm_qos_resume_us
                        throughput
                              │
                         contracts.toml
                         (PM QoS floors per class)
```

`optid` polls sensors every 2 seconds, maps the current system state to one of five workload classes, looks up the PM QoS contract for that class, and either logs its intended actions (dry-run, the default) or applies them to the kernel.

`optctl pin <app> <class>` lets applications claim a class directly — a game can hold `latency-critical`, a build job can hold `throughput`.

Default mode is always **dry-run**. Kernel writes require explicit `--apply` on a supported host.

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

## License

[Apache-2.0](LICENSE). Copyright the Rush Linux authors.
