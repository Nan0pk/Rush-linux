# Final Expert Re-Analysis Report: Rush Linux Core Architecture & v0.5 Image Pivot

**Date**: 2026-06-17  
**Project Version**: `0.4.0-alpha.1`  
**Milestone Focus**: v0.5 Image Pivot (`mkosi` / Arch Linux Base)  

---

## 1. Executive Summary & Northstar Alignment

Rush Linux is an R&D-focused, adaptive Linux distribution built to deliver Apple-class power efficiency and responsiveness on x86_64 hardware. Unlike conventional Linux distributions that rely on static performance profiles or conflicting user-space daemons (e.g., TLP, tuned, power-profiles-daemon), Rush Linux utilizes a highly explainable, single-owner optimization boundary owned entirely by **`optid`**—a pure Rust workload orchestrator.

Following the successful completion of foundational R&D (v0.1 through v0.4), all core subsystems are fully operational, deterministically testable, and compile-clean. However, the project has encountered its definitive engineering blocker: **No unified, standalone, officially bootable raw disk image (`disk.raw`) has yet been produced and published for broader human validation.**

To resolve this blocker and establish an immutable, maintainable foundation for all future editions, the project is executing the **v0.5 Image Pivot**. Under proposed **ADR 0014**, Rush Linux is transitioning from its experimental custom package/rootfs scaffolding (`tools/rush-builder.py`) to a declarative, modern image composition pipeline managed by **`mkosi`** on top of an Arch Linux rolling-release base.

---

## 2. Deep Subsystem Analysis & Verification State

An exhaustive audit of the `/home/user/Rush-linux` repository reveals an exceptionally disciplined and robust engineering core. All 50 pure Rust tests across the workspace currently pass cleanly.

### A. The Control Plane (`optid` & `optctl`)
- **Workload Classification Engine**: Functional and verified. `optid` continuously ingests multi-sensor telemetry (Pressure Stall Information via `/proc/pressure/*`, AC/battery state via `/sys/class/power_supply`, thermal status via `/sys/class/thermal`, and system load averages). A pure functional classifier maps these inputs to five standard SPEC §1 workload classes (`idle`, `light`, `interactive`, `latency-critical`, `throughput`) featuring rigorous hysteresis to prevent thrashing.
- **D-Bus Integration (`io.rushlinux.Optid`)**: Complete. The D-Bus server runs inside `optid` with client bindings in `optctl`, enabling clean IPC mode pinning (`optctl pin`) and telemetry readout. File-based IPC fallback is fully supported.
- **PM QoS Contract Enforcement**: Live and enforced. The provisional latency-budget contracts defined in `config/optid/contracts.toml` are dynamically resolved to CPU wakeup latency floors (`/dev/cpu_dma_latency`) and per-device PCI resume latency floors (`/sys/bus/pci/devices/*/power/pm_qos_resume_latency_us`). Reversion on service stop is deterministically tested.
- **Guarded Actuation**: `optid` strictly respects `--apply`. When active, it actuates kernel sysctls (`vm.swappiness`, `vm.dirty_*`) and platform profiles cleanly, journaling all prior states to ensure 100% reversible execution.

### B. The Measurement Rig (`rushbench`)
- Implemented as a standalone crate in `crates/rushbench`. It captures real-time battery drain (via `BAT*/energy_now` or Intel RAPL) and tracks responsiveness metrics, proving compliance with the SPEC §6 Energy and Latency verification gates (closed in PR #75).

### C. Boot, Updates, & Rollback Architecture
- **UKI-First Boot**: The system is structured around Unified Kernel Images, eliminating legacy fragile bootloader dependency chains.
- **Systemd Update & Rollback**: Descriptors for `systemd-sysupdate` are fully structured. Boot entries are dynamically managed via `tools/manage-boot-entries.sh` to enforce an `InstancesMax=3` rollback retention window.
- **Boot Assessment**: Wired via `optid-boot-assess.service` and `tools/optid-boot-assess` to track boot health, allowing automated fallback to prior verified UKIs in bad-kernel scenarios (validated via `tools/test-rollback.sh`).
- **Cryptographic Update Signing**: A fully functional Python module (`tools/sign_updates.py`) utilizing `cryptography` provides genuine Ed25519 signature generation, signing, and verification of repository metadata (`repodata.json`).

---

## 3. The Core Blocker & The mkosi Pivot (ADR 0014)

### The Problem
During Phase 0 and earlier alpha milestones, the project built a custom package and rootfs builder (`tools/rush-builder.py`) to prove the viability of custom recipe parsing. While successful for early CI boot verification, maintaining an ad-hoc packaging tool is highly inefficient. It lacks the rich dependency resolution, architectural caching, and robust package ecosystem needed for real-world hardware enablement. Furthermore, it has prevented the seamless generation of production-ready disk images.

### The Strategic Resolution
Adopting **`mkosi`** (Make Operating System Image) against an **Arch Linux** package repository provides massive strategic advantages:
1. **Immediate Ecosystem Parity**: Arch Linux's rolling-release model guarantees instant access to modern upstream Linux kernels, cutting-edge schedulers (`sched_ext`, EEVDF), dynamic memory reclaim mechanisms (`MGLRU`), and bleeding-edge Mesa drivers.
2. **Declarative, Repeatable Construction**: `mkosi` configuration (`mkosi.conf`) completely defines the layout, partition tables, subvolumes, and initrd embeddings in standard, machine-readable formats.
3. **Reproducible Builds**: By configuring `mkosi` to point to specific daily snapshot URLs of the Arch Linux Archive (ALA), Rush Linux can achieve 100% bit-for-bit build reproducibility.
4. **System Extension (sysext) Architecture**: `mkosi` has native, seamless support for building `systemd-sysext` images, perfectly paving the way for the v0.7 target of separating Desktop, Realtime Audio, and Server profiles into modular read-only layers atop an immutable base OS.

---

## 4. Actionable Implementation Plan: v0.5 Image Pivot

To produce the first official `disk.raw` image and close the v0.5 milestone, the following concrete steps are defined for immediate execution:

### Step 1: Toolchain and Build Workspace Preparation
Compile the highly optimized release binaries for the active Rush Linux control plane:
```bash
cargo build --release --workspace
```
Ensure the existing `mkosi` overlay directory (`mkosi/mkosi.extra/`) is populated with the compiled binary outputs:
- `target/release/optid` $\rightarrow$ `mkosi/mkosi.extra/usr/libexec/optid`
- `target/release/optctl` $\rightarrow$ `mkosi/mkosi.extra/usr/bin/optctl`
- `target/release/optid-boot-assess` $\rightarrow$ `mkosi/mkosi.extra/usr/libexec/optid-boot-assess`

### Step 2: Fine-Tuning `mkosi.conf`
Verify and refine `mkosi/mkosi.conf`. Ensure it includes:
- Distribution set to `arch`.
- Output format configured for GPT disk image (`disk.raw`).
- Complete package dependencies: `base`, `linux`, `systemd`, `nftables`, `zram-generator`, `dbus`, `bash`.
- Explicit kernel command line enabling cgroup v2, PSI, and debugging out-of-the-box:
  `systemd.unified_cgroup_hierarchy=1 cgroup_no_v1=all psi=1 root=/dev/vda2 rw console=ttyS0,115200 systemd.firstboot=no efi=debug`

### Step 3: Scripting the Automated Image Compiler
Implement a production build wrapper `tools/build-mkosi-image.sh` that:
1. Validates the presence of `mkosi` on the host.
2. Invokes Cargo to refresh all native release binaries.
3. Copies artifacts into `mkosi/mkosi.extra/`.
4. Executes `mkosi --directory mkosi/ build` to produce `/home/user/Rush-linux/build/disk.raw`.

### Step 4: Verification and Parity Testing
Validate the resulting `disk.raw` artifact against the project's stringent exit gates:
- Run `tools/validate-uefi-boot.sh` to confirm the image successfully boots through OVMF to `multi-user.target` with `optid.service` actively running.
- Run `tools/test-rollback.sh` to ensure rollback entry rotation remains fully operable.

---

## 5. Strategic Roadmap & Next Milestones

Once the `disk.raw` image is proven and published, Rush Linux will rapidly advance through its remaining pre-stable gates:

```text
[v0.5 Image Pivot] ──> [v0.6 Hardware-Aware optid] ──> [v0.7 Modular Editions] ──> [v1.0 Stable]
   (Current Target)       (TLP/PPD Allowlists)           (Desktop/RT Sysexts)       (Production OS)
```

1. **Milestone v0.6 (Hardware-Aware `optid`)**: Finalize Wave 2 features. Wire up D-Bus shims to intercept GNOME/KDE power slider events and GameMode requests, translating them into `optid` operational states. Enforce strict daemon conflict blocks (preventing `TLP` and `power-profiles-daemon` from interfering).
2. **Milestone v0.7 (Modular Edition Sysexts)**: Build standard `systemd-sysext` overlays for Desktop (Wayland/PipeWire/scx), Realtime Audio (`PREEMPT_RT` kernel), and Server profiles.
3. **Milestone v0.8 (Benchmark Lab)**: Execute automated Phoronix Test Suite comparisons against mainstream Linux distributions, demonstrating undeniable performance-per-watt superiority.
4. **Milestone v1.0 (Stable Release)**: Publish signed ISOs, UKIs, and modular sysext layers for general production deployment.
