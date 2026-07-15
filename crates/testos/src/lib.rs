//! testOS — shared library code.
//!
//! Used by the three binaries (`testos-launcher`, `testos-runner`, `testos-ingest`)
//! so that the bench catalog, result schema, run-intent contract, and host
//! fingerprinting are defined in exactly one place.
//!
//! Design notes:
//! - The bench catalog (`BenchList`) is loaded from `testos/bench-list.toml` on
//!   both the launcher side (to estimate runtime and show the menu before boot)
//!   and the runner side (to know what to execute after boot).
//! - The result schema (`BenchResult`) is a small, frozen JSON shape that the
//!   runner writes and the ingest tool reads. Schema version is pinned at 1.
//! - The run-intent contract (`RunIntent`) is the cryptographic association
//!   between the host planner and the runner. The host writes
//!   `run-intent.json` to the USB; the runner reads it on boot, refuses to
//!   run if it is missing/malformed/stale/dry-run/inconsistent, and copies
//!   every field into `manifest.json` so the evidence validator can re-bind
//!   the run to the plan, catalog, image, source commit, and run_id.
//! - Host fingerprinting reuses the same fields as `rushbench`'s `HostInfo`
//!   (kernel, cpu_model, dmi_board, battery_design_uwh) so results from both
//!   rigs can be joined later without translation.

pub mod catalog;
pub mod host;
pub mod results;
pub mod run_intent;

pub use catalog::{Bench, BenchKind, BenchList};
pub use host::HostFingerprint;
pub use results::{BenchResult, RunManifest, RunProvenance, SCHEMA_VERSION};
pub use run_intent::{RunIntent, RunIntentError, INTENT_FILENAME, INTENT_SCHEMA_VERSION};
