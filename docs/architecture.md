# Architecture

Adaptive Linux separates the distribution into four layers:

1. Source recipes that produce signed binary packages.
2. A modern base OS with systemd, cgroup v2, PSI, UKI boot, nftables, PipeWire,
   Wayland, and rollback support.
3. Hardware enablement packages for the kernel, Mesa, firmware, and device
   policy.
4. `optid`, the only default runtime optimizer.

The key constraint is single ownership. `optid` must not fight TLP,
power-profiles-daemon, TuneD, ad-hoc shell scripts, or desktop widgets over the
same kernel and firmware knobs. Compatibility layers can expose familiar APIs,
but the default policy owner remains `optid`.

## Runtime Flow

```text
kernel metrics -> optid sensors -> policy engine -> guarded action plan
     PSI              cgroups          hysteresis          systemd/sysfs
     thermal          power            cooldowns           decision logs
     CPUFreq          storage          allowlists          optctl explain
```

The MVP implements deterministic policy first. ML-assisted tuning is explicitly
out of scope until deterministic policy has a benchmark history and rollback
guardrails.

## Compatibility Position

Legacy technology may be available as a compatibility package when necessary,
but it must not be selected by default:

- X11 is compatibility only; Wayland is default.
- PulseAudio and standalone JACK are compatibility only; PipeWire is default.
- iptables tooling is compatibility only; nftables is default.
- cgroup v1 is unsupported as a default boot mode.
- SysV/OpenRC/runit dual-init support is out of scope.

