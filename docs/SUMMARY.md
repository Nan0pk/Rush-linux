# Summary

- [Project Brief](PROJECT_BRIEF.md)
- [Architecture](architecture.md)
- [Implementation Status](IMPLEMENTATION_STATUS.md)

## Core Concepts
- [Adaptive Engine](adaptive-engine.md)
- [Agent Protocol](agent-protocol.md)
- [Project Workflow](project-workflow.md)
- [Hardware Support](hardware-support.md)
- [Kernel Policy](kernel-policy.md)

## Design Decisions
- [ADR Index](decisions/README.md)
- [0001: systemd & cgroup v2](decisions/0001-systemd-cgroup-v2.md)
- [0002: Wayland & PipeWire](decisions/0002-wayland-pipewire.md)
- [0003: UKI & Rollback](decisions/0003-uki-rollback.md)
- [0004: Adaptive optid](decisions/0004-adaptive-optid.md)
- [0009: optid Security Boundary](decisions/0009-optid-security-boundary.md)
- [0012: Reproducible Build Discipline](decisions/0012-reproducible-build-discipline.md)
- [0018: Rush LiveDev Architecture Contract](decisions/0018-rush-livedev-architecture-contract.md)

## LiveDev Track
- [LiveDev Workspace Enablement](plans/livedev-workspace-enablement.md)
- [LiveDev Transition Plan](plans/livedev-transition-plan.md)
- [Automation–Human Interface Policy](automation-human-interface.md)
- [AI Interface Policy](ai-interface-policy.md)

## Research
- [0001: Apple Power Stack Analysis](research/0001-apple-power-stack-analysis.md)
- [0002: Rush Linux Architecture Review](research/0002-rush-linux-architecture-review.md)
- [0003: Unified Power Orchestrator](research/0003-unified-power-orchestrator-paper.md)
- [0004: Telemetry Fidelity](research/0004-telemetry-fidelity-rca-and-architecture.md)
- [0005: Focus vs Resource Pull](research/0005-focus-vs-resource-pull.md)
- [0006: Hardware Allowlist](research/0006-hw-allowlist-db-design.md)
- [0007: Display Power](research/0007-display-panel-backlight-psr-vrr-dpms.md)
- [0008: NVMe & PCIe Power Management](research/0008-nvme-apst-pcie-aspm-sata-alpm.md)
- [0009: Runtime PM Autosuspend](research/0009-runtime-pm-autosuspend-policy.md)
- [0010: PPD and GameMode Compatibility](research/0010-ppd-gamemode-dbus-shim.md)
- [0011: dGPU Runtime PM and MUX](research/0011-dgpu-runtime-pm-and-mux.md)
- [0012: Powercap Outer Loop](research/0012-dtpm-powercap-outer-loop.md)
- [0013: Thermal and Fan Coupling](research/0013-thermal-fan-budget-coupling.md)
- [0014: sched-ext Selector](research/0014-sched-ext-selector-per-workload-class.md)
- [0015: zram and MGLRU](research/0015-zram-mglru-tuning-per-ram-tier.md)
- [0016: mkosi Snapshot Pinning](research/0016-mkosi-ala-snapshot-pinning.md)
- [0017: UKI Signing and Secure Boot](research/0017-uki-signing-secure-boot-enrollment.md)
- [0018: Telemetry Runtime State](research/0018-telemetry-runtime-state-observability.md)
- [0019: GPU Upscaling and Ambient Light](research/0019-gpu-upscaling-resolution-scaling-als.md)
- [0020: Third-Pass Technical Debt Audit](research/0020-third-pass-tech-debt-audit.md)

## Guides
- [Build System](build-system.md)
- [How Rush is Built](how-rush-is-built.md)
- [Packaging and Builds](packaging-and-builds.md)
- [Testing and Benchmarks](testing-and-benchmarks.md)
- [Versioning](versioning.md)

## Project
- [Contributing](../CONTRIBUTING.md)
- [Release Plan](release-plan-v1.md)
- [Roadmap](../ROADMAP.md)
- [Security](../SECURITY.md)
