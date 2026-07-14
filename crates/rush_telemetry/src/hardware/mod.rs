//! Hardware abstraction layer.
//!
//! Provides direct MSR access for RAPL energy telemetry, lockless PSI total
//! extraction, and Intel HFI topology discovery. Each subsystem implements a
//! fallback chain: direct hardware → perf_event → sysfs.

pub mod hfi;
pub mod psi;
pub mod rapl;

pub use rapl::EnergySource;
pub use psi::PsiReader;
pub use hfi::HfiTopology;
