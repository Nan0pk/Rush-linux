//! testOS — shared library code.
//!
//! Used by the three binaries (`testos-launcher`, `testos-runner`, `testos-ingest`)
//! so that the bench catalog, result schema, and host fingerprinting are defined
//! in exactly one place.
//!
//! Design notes:
//! - The bench catalog (`BenchList`) is loaded from `testos/bench-list.toml` on
//!   both the launcher side (to estimate runtime and show the menu before boot)
//!   and the runner side (to know what to execute after boot).
//! - The result schema (`BenchResult`) is a small, frozen JSON shape that the
//!   runner writes and the ingest tool reads. Schema version is pinned at 1.
//! - Host fingerprinting reuses the same fields as `rushbench`'s `HostInfo`
//!   (kernel, cpu_model, dmi_board, battery_design_uwh) so results from both
//!   rigs can be joined later without translation.

pub mod catalog;
pub mod host;
pub mod results;

pub use catalog::{Bench, BenchKind, BenchList};
pub use host::HostFingerprint;
pub use results::{BenchResult, RunManifest, SCHEMA_VERSION};
