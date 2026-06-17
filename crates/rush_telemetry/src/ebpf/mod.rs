//! eBPF-based telemetry collection.
//!
//! Provides kernel-side tracepoint attachment with `BPF_MAP_TYPE_RINGBUF`
//! output for zero-polling, zero-overhead telemetry extraction during
//! benchmark execution.
//!
//! ## Architecture
//!
//! Three BPF programs are attached to scheduler tracepoints:
//!
//! 1. `handle_sched_stat_wait` — captures task wait times
//! 2. `handle_sched_switch` — captures context switches for tracked PIDs
//! 3. `handle_energy_timer` — periodic RAPL energy sampling (10ms timer)
//!
//! All events are emitted as packed binary structs to a ring buffer.
//! User-space drains the buffer only after the benchmark completes.

pub mod loader;
pub mod types;

pub use loader::{EbpfCollector, EbpfConfig};
pub use types::{TelemetryEvent, EventType};
