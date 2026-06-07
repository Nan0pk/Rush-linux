# Architecture

Rush Linux has four layers:

1. Source recipes that produce signed binary packages.
2. A modern base OS with systemd, cgroup v2, PSI, UKI boot, nftables, PipeWire,
   Wayland, and rollback support.
3. Hardware enablement packages for kernel, Mesa, firmware, and device policy.
4. `optid`, the only default runtime optimizer.

The project does not depend on a single static "performance" profile. It uses
live pressure and hardware signals to apply reversible policy changes.

## System Boundaries

`optid` owns runtime optimization policy. Other components may provide input,
compatibility APIs, or user intent, but they must not independently mutate the
same CPU, power, cgroup, I/O, or device knobs by default.

```text
kernel metrics -> optid sensors -> policy engine -> guarded action plan
     PSI              cgroups          hysteresis          systemd/sysfs
     thermal          power            cooldowns           decision logs
     CPUFreq          storage          allowlists          optctl explain
```

## Subsystems

- Adaptive engine: see `docs/adaptive-engine.md`.
- Kernel defaults: see `docs/kernel-policy.md`.
- Packaging and build model: see `docs/packaging-and-builds.md`.
- Boot and rollback: see `docs/boot-and-updates.md`.
- Hardware support: see `docs/hardware-support.md`.
- Testing and benchmarks: see `docs/testing-and-benchmarks.md`.
- Non-goals: see `docs/non-goals.md`.

## Compatibility Position

Legacy technology may be available later as a compatibility package when
necessary, but it must not be selected by default. This keeps the project
aligned with current upstream direction and avoids building the distro around
interfaces that are already being replaced.

## Documentation Rule

Architecture documentation is part of acceptance criteria. A change is not done
until the relevant document and ADR are updated.

