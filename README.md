# Rush Linux

**Adaptive Linux. One optimizer. Every decision explained and reversible.**

[![Version](https://img.shields.io/badge/version-0.4.0--alpha.1-blue)](VERSION)
[![Status](https://img.shields.io/badge/status-pre--alpha-orange)](ROADMAP.md)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

Rush Linux is an architecture-first Linux distribution built around `optid`, a native Rust daemon that continuously reads kernel pressure signals and adjusts hardware policy in real time. No fixed power profiles. No manual tuning. One explainable, reversible optimizer that proves its work.

The project is pre-alpha. The optimizer runs, a VM boots via UKI with `optid.service` active, and the measurement harness is in place. A consumer-installable distribution is the long-term goal.

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
- **Bootable VM** — Arch-based rootfs boots to `multi-user.target` via UKI through OVMF/systemd-boot with `optid.service` active. Verified 2026-06-08.
- **Rollback + signing** — systemd-sysupdate descriptors, ≥3 retained boot entries, boot assessment service, Ed25519 update metadata signing.

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

## Status

| Milestone | State |
|:----------|:------|
| Phase 0 — repo, ADRs, CI | ✅ complete |
| v0.1 — compile-clean core, `optid --once` | ✅ complete |
| v0.2 — D-Bus control plane, `optctl` | ✅ complete |
| v0.3 — rootfs builder, VM boots | ✅ complete |
| **v0.4 — UKI boot, rollback, update signing** | ⚙ in progress |
| v0.5 — mkosi/Arch rebase, sched_ext integration | planned |
| v0.6 — hardware allowlist, PPD/GameMode shims | planned |
| v0.7 — desktop / laptop / realtime editions | planned |
| v0.8 — benchmark lab, published results | planned |
| v1.0 — installable, benchmarked, stable | planned |

→ [Full roadmap](ROADMAP.md)

---

## Contributing

We are looking for kernel engineers, Rustaceans, and systems programmers who value verifiable claims over marketing copy.

- [How Rush is built](docs/how-rush-is-built.md)
- [Agent and contributor protocol](docs/agent-protocol.md)
- [Contributing guide](CONTRIBUTING.md)
- [Open a discussion](https://github.com/Nan0pk/Rush-linux/discussions)
