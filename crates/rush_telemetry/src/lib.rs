//! # rush_telemetry
//!
//! Zero-cost, non-invasive telemetry extraction for Rush Linux benchmarks.
//!
//! This crate provides three layers of telemetry collection:
//!
//! 1. **Hardware Layer** — Direct MSR RAPL energy reads, lockless PSI total extraction,
//!    HFI topology discovery
//! 2. **Kernel Transport Layer** — eBPF-based tracepoint attachment with ring buffer output
//! 3. **Post-Processing Layer** — Deferred serialization, compression, signing, and transport
//!
//! ## Design Constraints
//!
//! - No user-space polling during the measurement window (eliminates observer effect)
//! - No string formatting or float math in the data-collection hot path
//! - All scaling, parsing, and interpretation deferred to post-processing
//! - Graceful degradation through hardware fallback tiers
//! - Lockless, zero-allocation collection path using packed binary structs

pub mod collector;
pub mod ebpf;
pub mod hardware;
pub mod transport;

pub use collector::{Collector, CollectorBuilder, MeasurementResult, TelemetryHandle};
pub use hardware::EnergySource;
