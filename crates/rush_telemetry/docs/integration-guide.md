# rush_telemetry — Migration Guide for rushbench

> **Context:** This guide explains how to replace `rushbench`'s existing telemetry
> with `rush_telemlemetry`. For the root cause analysis motivating these changes,
> see `docs/research/0004-telemetry-fidelity-rca-and-architecture.md` §1.

## Step 1: Workspace Dependency

Root `Cargo.toml`:

```toml
[workspace.dependencies]
rush_telemetry = { path = "crates/rush_telemetry" }
```

`crates/rushbench/Cargo.toml`:

```toml
[dependencies]
rush_telemetry = { workspace = true }
```

## Step 2: Replace Energy Reads (energy.rs)

**Current** (`rushbench/src/energy.rs`):
```rust
let text = fs::read_to_string(path)?;
let raw_uj: u64 = text.trim().parse()?;
let joules = (raw_uj as f64) * 1e-6;
```

**Replacement:**
```rust
use rush_telemetry::hardware::rapl::{EnergySource, EnergyTier};

let (mut source, tier) = EnergySource::open()?;
let start = source.sample_raw()?;
// ... run benchmark ...
let end = source.sample_raw()?;
let joules = source.delta_joules(&start, &end);
```

The fallback chain (MSR → sysfs RAPL → sysfs battery) is automatic.

## Step 3: Replace PSI Reads (probes.rs)

**Current** (`rushbench/src/probes.rs`):
```rust
// Reads /proc/pressure/cpu, parses avg10=X.XX string
pub fn read_psi_avg10(path: &str) -> io::Result<f64> { ... }
```

**Replacement:**
```rust
use rush_telemetry::hardware::psi::{PsiReader, PsiResource};

let reader = PsiReader::open(PsiResource::Cpu)?;
let start = reader.read_total()?;
// ... run benchmark ...
let end = reader.read_total()?;
let stall_pct = PsiReader::stall_percentage(&start, &end);
// stall_pct is the EXACT percentage, not an EMA approximation
```

The metric name remains `psi-cpu-avg10` for manifest compatibility, but the
underlying computation is now exact rather than approximate.

## Step 4: Optional — Full Collector API

```rust
use rush_telemetry::{Collector, EnergyPreference};

let mut handle = Collector::builder()
    .with_energy_source(EnergyPreference::MsrFirst)
    .with_psi(true)
    .with_ebpf(true)
    .track_pid(benchmark_pid)
    .pinned_to_cpu(0)
    .verify()?
    .build();

let result = handle.measure(|| {
    run_probe_for_metric(metric)
})?;

println!("Energy: {:?} J", result.energy_joules);
println!("CPU stall: {:?}%", result.psi_cpu_stall_pct);
println!("Avg power: {:?} W", result.avg_watts());
println!("Core class: {:?}", result.core_class);
```

## Incremental Adoption

The crate is designed for incremental adoption:

1. **Phase 1:** Replace `energy.rs` sysfs reads only. Zero API change to runner.rs.
2. **Phase 2:** Replace PSI `avg10` reads with `total=` delta. Same metric names.
3. **Phase 3:** Enable eBPF for per-event scheduling data. New capability, opt-in.

Each phase can be a separate PR. Phase 1 and 2 have no dependency on eBPF compilation.

## BPF Compilation (Phase 3 only)

```bash
# Arch Linux / Rush Linux
sudo pacman -S clang llvm libbpf linux-headers bpftool

clang -O2 -g -target bpf -D__TARGET_ARCH_x86 \
    -I/usr/include/bpf \
    -c bpf/telemetry.bpf.c -o bpf/telemetry.bpf.o

bpftool gen skeleton bpf/telemetry.bpf.o > bpf/telemetry.skel.h
```
