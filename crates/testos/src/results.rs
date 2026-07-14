//! Result schema — what testOS writes after each benchmark, and what the
//! ingest tool reads.
//!
//! Schema version is frozen at 1. Future versions must bump `SCHEMA_VERSION`
//! and the ingest tool must handle old versions gracefully (or refuse with a
//! clear error).

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

/// Per-benchmark result. One JSON file per benchmark, written to the USB stick
/// at `<mount>/testos-results/<timestamp>/<bench-id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResult {
    /// Schema version — always 1 for now.
    pub schema_version: u32,
    /// Bench id from the catalog.
    pub bench_id: String,
    /// Bench human-readable name (denormalized for readability).
    pub bench_name: String,
    /// Which manifest scenario this benchmark contributes to.
    pub scenario: String,
    /// Status: "pass" | "fail" | "aborted" | "skipped".
    pub status: String,
    /// ISO 8601 UTC timestamp when the benchmark started.
    pub started_at: String,
    /// ISO 8601 UTC timestamp when the benchmark finished.
    pub finished_at: String,
    /// Wall-clock seconds the benchmark actually took.
    pub elapsed_seconds: f64,
    /// The primary numeric result (e.g. IOPS, TPS, RPS, latency p95 in ms).
    /// None for ShellPassFail or when the benchmark failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// The unit of `value` (e.g. "iops", "tps", "rps", "ms", "gbps").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Raw stdout (truncated to 64 KiB) for forensics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Raw stderr (truncated to 64 KiB) for forensics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Exit code of the benchmark command (None if it was killed by signal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// The host fingerprint captured at runner start (same for every benchmark in a run).
    pub host: crate::host::HostFingerprint,
}

/// The top-level manifest written alongside the per-benchmark JSON files.
/// One per run. The ingest tool reads this first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub schema_version: u32,
    /// ISO 8601 UTC timestamp when the run started.
    pub started_at: String,
    /// ISO 8601 UTC timestamp when the run finished (or was aborted).
    pub finished_at: String,
    /// "all" | "selection:<comma-separated-ids>" | "aborted".
    pub mode: String,
    /// The ids of benchmarks that were attempted (regardless of pass/fail).
    pub attempted: Vec<String>,
    /// The ids of benchmarks that passed.
    pub passed: Vec<String>,
    /// The ids of benchmarks that failed.
    pub failed: Vec<String>,
    /// The ids of benchmarks that were skipped (e.g. user pressed Esc, or battery required but absent).
    pub skipped: Vec<String>,
    /// Host fingerprint (same as in each BenchResult; denormalized for convenience).
    pub host: crate::host::HostFingerprint,
    /// testOS image version (from /etc/os-release VERSION).
    pub testos_version: String,
}

impl BenchResult {
    pub fn stdout_truncated(raw: &str) -> String {
        const MAX: usize = 64 * 1024;
        if raw.len() <= MAX {
            raw.to_string()
        } else {
            let mut s = raw[..MAX].to_string();
            s.push_str("\n... [truncated by testOS, full output lost]");
            s
        }
    }
}
