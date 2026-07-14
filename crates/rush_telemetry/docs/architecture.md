# rush_telemetry — Technical Reference

> **Primary document:** `docs/research/0004-telemetry-fidelity-rca-and-architecture.md`
> on `main`. This file is a supplementary technical reference for developers working
> on the crate itself. For root cause analysis, architectural rationale, evidence gaps,
> and integration guidance, see the research document.

## Wire Format

```
struct telemetry_event {       // 40 bytes, #[repr(C, packed)]
    u8  event_type;            // 0=ENERGY, 1=PSI, 2=SCHED_WAIT, 3=SCHED_SWITCH, 4=MARKER
    u8  cpu_id;
    u16 _reserved;
    u64 tsc_ns;                // CLOCK_MONOTONIC nanoseconds
    union payload;             // 16 bytes, interpreted by event_type
};
```

## MSR Addresses

| Register | Address | Bits | Meaning |
|----------|---------|------|---------|
| MSR_RAPL_POWER_UNIT | 0x606 | [12:8] | ESU: energy unit = 2^-ESU J |
| MSR_PKG_ENERGY_STATUS | 0x611 | [31:0] | Rolling energy counter |
| MSR_PP0_ENERGY_STATUS | 0x639 | [31:0] | Core domain energy |
| MSR_IA32_HW_FEEDBACK_PTR | 0x17D | [63:0] | HFI table physical address |

## Fallback Chain

```
MSR direct (/dev/cpu/0/msr pread)  →  sysfs RAPL  →  sysfs battery  →  unavailable
    ~200ns/read                         ~5μs/read      ~10μs/read
```

## BPF Programs

| Program | Tracepoint | Emits |
|---------|-----------|-------|
| `handle_sched_stat_wait` | `tp/sched/sched_stat_wait` | SCHED_WAIT (pid, wait_ns) |
| `handle_sched_switch` | `tp/sched/sched_switch` | SCHED_SWITCH (prev_pid, next_pid, state) |
| `handle_psi_group_change` | `kprobe/psi_group_change` | PSI_SAMPLE (timestamp) |

## Status

Prototype. See research doc §3.3 for implemented vs. stubbed breakdown and §4 for
evidence gaps that must be closed before adoption.
