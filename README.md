<p align="center">
  <img src="docs/assets/rush-logo.svg" alt="Rush Linux" width="720"/>
</p>

<p align="center">
  <strong>One optimizer. Every decision explained and reversible.</strong>
</p>

<p align="center">
  <a href="VERSION"><img src="https://img.shields.io/badge/version-0.7.0--beta.1-blue" alt="version"/></a>
  <a href="ROADMAP.md"><img src="https://img.shields.io/badge/status-v0.6%20in%20progress-green" alt="status"/></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="license"/></a>
  <a href="https://github.com/Nan0pk/Rush-linux/actions"><img src="https://img.shields.io/github/actions/workflow/status/Nan0pk/Rush-linux/ci.yml?branch=main" alt="CI"/></a>
</p>

---

## What this project is for

Rush Linux is a Linux distribution that wants to give you **the best possible battery life and the snappiest feel at the same time** — without making you flip between "power saver", "balanced", and "performance" modes.

You shouldn't have to manage power profiles. The system should watch what you're actually doing — a long build, a video call, a game, an idle moment — and tune the hardware to match. When you're compiling, it should let the CPU run hot. When you're reading, it should let the disk spin down. When you're on a video call, it should hold a tight latency floor so you don't stutter. All of that, automatically, and with **a written record of every decision** so you can see what it did and why — and undo anything you don't like.

That's the goal. We're not there yet. Today Rush boots, runs its optimizer (`optid`) in safe dry-run mode, survives updates and rollbacks, and ships a measurement harness (`rushbench`) so any claim about performance can be backed by a real transcript. The next milestone is real-hardware benchmarks — see the **testOS Quick Start** below if you want to help with that.

> Contributors/agents: this project gates milestone claims on committed evidence. Run `python3 tools/dragnet.py --observe` and see `docs/dragnet-protocol.md` before relying on any "verified" status.

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

---

## What's built

- **`optid` daemon** — PSI + thermal + power supply → workload classifier → PM QoS enforcement via `contracts.toml`. Applies EPP, platform profile, and cgroup slices with `--apply`. Every decision is logged and explainable.
- **`optctl` CLI** — D-Bus client (`io.rushlinux.Optid1`): `status`, `explain`, `mode`, `pin`. Machine-readable output via `--json`.
- **`rushbench` harness** — Measures battery drain (`energy_now` or RAPL) and latency (PSI avg10, cyclictest, foreground launch) per workload class. Structured energy window, N-sample collection, and anomaly detection. No results published yet — this is the tool that generates them.
- **Bootable VM** — Arch-based rootfs boots to `multi-user.target` via UKI through OVMF/systemd-boot with `optid.service` active. Verified 2026-06-08; transcript committed 2026-06-23 (`release/evidence/v0.3.0-alpha.1/`).
- **Installable system** — `tools/rush-install.sh` stamps the mkosi-built image onto a blank disk via `systemd-repart`; installed system boots twice cleanly with `optid.service` active. Transcripts at `release/evidence/v0.5.0-beta.1/`.
- **Rollback + signing** — systemd-sysupdate descriptors, ≥3 retained boot entries, boot assessment service, Ed25519 update metadata signing. Bad-kernel rollback verified; transcript at `release/evidence/v0.4.0-alpha.1/c3-bad-kernel/`.

Default mode is always **dry-run**. Kernel writes require explicit `--apply` on a supported host.

---

## Technology choices

| Layer | Choice | Why |
|:------|:-------|:----|
| Optimizer | `optid` (Rust) | No GC pauses; direct sysfs access; auditable actuator path |
| Pressure sensing | PSI (`/proc/pressure`) | Kernel-native; quantifies actual CPU/IO/memory stall time |
| Latency enforcement | PM QoS (`/dev/cpu_dma_latency`) | Hard per-class latency floors, not soft hints |
| Image composition | mkosi + Arch Linux *(v0.5)* | Declarative, reproducible; no bespoke build scripts |
| Scheduling | sched_ext / scx_loader *(v0.5)* | BPF user-space scheduler; EEVDF as the verified fallback |
| Boot | UKI + systemd-boot | Atomic, signed, single-file boot entries |
| Updates | systemd-sysupdate | Structured, rollback-aware OTA |
| Firewall | nftables | Current kernel default; replaces iptables |
| Desktop | Wayland + PipeWire | Native compositor and audio stack |

---

## The Evidence Rule

Rush Linux enforces a **Builder/Verifier separation**: no claim of correctness or performance is accepted without a literal command transcript. `✅ Verified` means a human or CI ran the command and the output is on record. Claims without transcripts are proposals, not facts.

This applies to benchmarks, boot verification, and optimizer behavior equally. The measurement toolchain (`rushbench`) is built precisely to generate the transcripts — when results are ready, they will be committed alongside the runs that produced them.

→ [Agent and contributor protocol](docs/agent-protocol.md)

---

## Getting started

```bash
# Clone and build
git clone https://github.com/Nan0pk/Rush-linux.git
cd Rush-linux
cargo build --release

# One sensor read + classify cycle (dry-run, exits after one pass)
./target/release/optid --once

# Check current status from the state directory
./target/release/optctl status

# Run the full test suite
cargo test --workspace
```

Or open in VS Code Dev Containers or GitHub Codespaces — the checked-in [dev container](.devcontainer/devcontainer.json) provisions Rust stable, Python 3.11+, and PowerShell Core, then builds the workspace automatically.

---

## testOS Quick Start — benchmark a real machine

testOS is a temporary, self-contained Linux environment that boots from a USB stick, runs the Rush Linux benchmark suite on real hardware, writes the results back to the USB, and reboots back to your host OS. It exists so anyone can go from a fresh clone of this repo to real-hardware benchmark numbers in about 10 minutes of actual work.

You need two machines for this:
- **Your workstation** — Linux with `mkosi` and Rust installed (Arch is easiest; other distros work with `tools/env-setup.sh`).
- **A test machine** — any x86_64 PC that can boot from USB. At least 1 GB RAM. No OS prerequisites.

You also need a USB stick (≥ 1 GB).

### Step 1 — Build the testOS image on your workstation

```bash
git clone https://github.com/Nan0pk/Rush-linux.git
cd Rush-linux
cargo build --workspace --release
sudo bash testos/build-testos.sh
```

This takes ~10 minutes the first time. When it finishes, you'll have `build/testos.raw` (~500 MB).

### Step 2 — Write the image to a USB stick

Plug in the USB stick, find its device name, write the image:

```bash
lsblk                    # find your USB (look for the right size, e.g. /dev/sdX)
sudo ./target/release/testos-launcher write /dev/sdX
```

The launcher will refuse to write to a mounted device or to anything that looks like your system disk, and it will ask you to type the device name twice to confirm. **All data on the USB will be wiped.**

### Step 3 — Boot the test machine from the USB

1. Plug the USB into the test machine.
2. Reboot. Enter the boot menu (usually F12, F8, F11, or Esc — depends on the vendor).
3. Pick the USB from the list.
4. Disable Secure Boot if the test machine refuses to boot the USB (testOS UKIs are unsigned for now).

testOS boots into a console menu on the screen. No login required.

### Step 4 — Pick benchmarks and let them run

The menu shows:

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

Type `0` for all, or pick specific numbers separated by commas (e.g. `1,3,5`). Press Enter.

Progress is printed line by line, with per-test ETA. **Press Esc at any time to abort early** — partial results are saved.

When the run finishes, testOS syncs the USB, waits 5 seconds, and reboots back to the host OS. Pull the USB.

### Step 5 — Pull the results into the repo

Plug the USB back into your workstation:

```bash
sudo ./target/release/testos-ingest pull /dev/sdX
./target/release/testos-ingest format
./target/release/testos-ingest commit
git push
```

- `pull` — mounts the USB, finds the latest results, copies them to `benchmarks/results/<UTC-date>/<host-fingerprint>/`.
- `format` — generates a `SUMMARY.md` Markdown table of all results, plus a failures section if anything failed.
- `commit` — `git add` + `git commit` with a conventional commit message like `evidence(bench): testOS run 2026-06-30 host=ab12cd34 pass=9 fail=0 skip=0`.

That's it. Results are now in the repo and can be pushed.

### Want to add a new benchmark?

Open `testos/bench-list.toml` and add a new `[[benches]]` entry — id, name, scenario, kind, command, estimated_seconds. Re-run `testos-launcher build` and you're done. No code changes required. See [`testos/README.md`](testos/README.md) for the full catalog spec.

### Why this design

- **USB boot, RAM runtime** — host disk is never touched. The USB only participates in boot, never in benchmarks. Disk benchmarks hit the actual test disk.
- **Crash recovery is automatic** — if testOS hangs, a hard reset reboots the machine back into the host OS. No bricked machines.
- **One-shot boot** — testOS doesn't install a bootloader. Pull the USB, reboot, you're back where you started.
- **Full cold reboot** — every reboot goes through the BIOS, so hardware starts cold and benchmark numbers are fair.

Full design rationale and known limitations: [`testos/README.md`](testos/README.md).

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

→ [Full roadmap](ROADMAP.md)

---

## Contributing

We are looking for kernel engineers, Rustaceans, and systems programmers who value verifiable claims over marketing copy.

- [How Rush is built](docs/how-rush-is-built.md)
- [Agent and contributor protocol](docs/agent-protocol.md)
- [Contributing guide](CONTRIBUTING.md)
- [Open a discussion](https://github.com/Nan0pk/Rush-linux/discussions)
