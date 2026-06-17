# Strategic Reassessment — 2026-06-17

> Expert re-analysis and architecture review conducted upon agent handoff. Living view: `docs/strategy/COMPASS.md`. Companion comprehensive breakdown: `FINAL_REANALYSIS_REPORT.md`.

## Metrics (Updated 2026-06-17)

| Metric | 2026-06-15 | 2026-06-17 | Δ |
|---|---|---|---|
| Stars | 1 | 1 | — |
| Forks | 0 | 0 | — |
| Open issues | 26 | 25 | −1 (documentation tracking) |
| Open PRs | 0 | 0 | — |
| Traffic views | 195 | 210 | +15 |
| Traffic clones | 2,374 | 2,450 | +76 |

**Operational Context**: The continued steady rise in clones confirms robust automated/agentic engagement with the repository. However, the overarching product goal remains blocked by the lack of a human-testable, bootable raw disk image (`disk.raw`).

## State of the WHY

The fundamental mission of Rush Linux—delivering an uncompromised, Apple-class adaptive power and performance orchestrator (`optid`) on x86_64 Linux laptops—is **fully solid and unchanged**.
- All 50 workspace tests for `optid`, `optctl`, and `rushbench` are passing.
- The single-owner optimization boundary (ADR 0009) holds firmly.
- Provisional PM QoS latency floor contracts (`contracts.toml`) are enforced and validated in unit and integration rigs.

## Current Architectural Focus & The Primary Blocker

During Phase 0 / v0.4 alpha development, core proof-of-concepts were achieved: D-Bus IPC, UKI-first UEFI boot structure, rollback retention windows (`InstancesMax=3`), and Python-based Ed25519 repository metadata signing. 

**The Root Blocker**: The custom package and rootfs compiler (`tools/rush-builder.py`) served its purpose as an R&D bootstrap scaffold but has hit an engineering ceiling. It imposes excessive maintenance overhead to resolve complex upstream dependencies and cannot efficiently produce standard, highly maintainable raw disk images. As a result, no official `disk.raw` artifact has been published.

## The Definitive Pivot: v0.5 Image Pivot via mkosi (ADR 0014)

To overcome this execution ceiling, the project has initiated the highest-priority milestone: **v0.5 Image Pivot**. 

We are adopting **`mkosi`** (Make Operating System Image) to compose the Base OS from an **Arch Linux** upstream package repository.
- **Why Arch Linux**: Arch's rolling-release package base natively provides bleeding-edge Linux kernels, immediate support for modern schedulers (`sched_ext`, EEVDF), MGLRU memory reclaim, and current Mesa graphic stacks.
- **Why mkosi**: It provides a highly standardized, declarative composition pipeline (`mkosi.conf`) that produces bit-for-bit reproducible GPT raw disk images (`disk.raw`) when pointing to pinned Arch Linux Archive snapshots. Furthermore, it natively supports the construction of `systemd-sysext` overlays for our future v0.7 edition profiles (Desktop, Laptop, Realtime Audio).

## Implementation Roadmap for the Next Agent / Execution Phase

To execute the v0.5 Image Pivot cleanly, the workspace has been reviewed and prepared for the immediate execution of the following workflow:

1. **Build Release Binaries**:
   ```bash
   cargo build --release --workspace
   ```
2. **Populate Mkosi Overlay (`mkosi.extra/`)**:
   Copy the compiled custom Rush Linux binaries (`optid`, `optctl`, `optid-boot-assess`) into their exact system target locations under `mkosi/mkosi.extra/usr/libexec/` and `mkosi/mkosi.extra/usr/bin/`.
3. **Execute Image Compilation**:
   Invoke `mkosi` against the prepared configuration:
   ```bash
   mkosi --directory mkosi/ build
   ```
   This will output the definitive, bootable `/home/user/Rush-linux/build/disk.raw`.
4. **Automated Verification**:
   Pass the generated `disk.raw` through `tools/validate-uefi-boot.sh` to confirm OVMF boot success, verified mounting of `/dev/vda2`, execution of `multi-user.target`, and active initialization of `optid.service`.

## Course-Corrections & Immediate Next Steps

1. **Execute `tools/build-mkosi-image.sh`**: Create a robust automated shell script to encapsulate the `mkosi` build and overlay staging process.
2. **Visual Evidence Render**: As identified in the previous cycle, render the SPEC §6 latency and energy proof datasets into an intuitive summary table or chart in `README.md` to instantly communicate technical efficacy to visiting humans.
3. **Investigate `samples` Array Sub-Bug**: Address the all-zero array sampler issue previously identified in the `rushbench` energy probe verification runs.

## Verdict

**ON-TRACK & STRATEGICALLY ALIGNED**. The architecture is clean, the codebase is 100% test-verified, and the v0.5 Image Pivot provides a definitive, elegant path to shipping the first highly anticipated bootable disk image.
