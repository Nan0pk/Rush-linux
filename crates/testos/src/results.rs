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
///
/// `provenance` binds the run to the run-intent, plan, benchmark catalog,
/// testOS image, source commit, and run_id. It is required for physical runs;
/// the strict evidence validator (`tools/validate-testos-evidence.py`)
/// rejects any manifest whose provenance block is missing, partial, or
/// contains placeholder values. The field is `Option` only so that older
/// manifests produced before this contract still parse; the validator
/// treats `None` as a hard failure for physical-run evidence.
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
    /// testOS image version (from /etc/testos/version, falling back to
    /// /etc/os-release VERSION). Must match `provenance.testos_version`.
    pub testos_version: String,
    /// Cryptographic provenance block copied from the run-intent. Required
    /// for physical runs; the validator rejects `None` and placeholder
    /// values. See `RunProvenance` for the field set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<RunProvenance>,
}

/// Provenance block recorded in `manifest.json`. Every field is copied from
/// the run-intent (or recomputed, for `intent_sha256`) so the evidence
/// validator can re-bind the run to the plan, catalog, image, source commit,
/// and run_id without trusting the runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunProvenance {
    pub run_id: String,
    pub source_commit: String,
    pub source_version: String,
    pub testos_version: String,
    /// `sha256:<64 hex>`.
    pub testos_image_digest: String,
    /// 64 hex chars (no prefix).
    pub plan_sha256: String,
    /// 64 hex chars (no prefix).
    pub benchmark_catalog_sha256: String,
    /// `generated_at` copied from the run intent.
    pub intent_generated_at: String,
    /// `dry_run` copied from the run intent. Must be false.
    pub intent_dry_run: bool,
    pub checkpoint_nonce: String,
    /// SHA-256 of the run-intent.json bytes the runner read from the USB.
    pub intent_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub campaign_id: Option<String>,
}

impl RunProvenance {
    /// Build the provenance block from a validated `RunIntent` and the raw
    /// bytes of the run-intent.json file (used to compute `intent_sha256`).
    pub fn from_intent(intent: &crate::run_intent::RunIntent, raw_bytes: &[u8]) -> Self {
        RunProvenance {
            run_id: intent.run_id.clone(),
            source_commit: intent.source_commit.clone(),
            source_version: intent.source_version.clone(),
            testos_version: intent.testos_version.clone(),
            testos_image_digest: intent.testos_image_digest.clone(),
            plan_sha256: intent.plan_sha256.clone(),
            benchmark_catalog_sha256: intent.benchmark_catalog_sha256.clone(),
            intent_generated_at: intent.generated_at.clone(),
            intent_dry_run: intent.dry_run,
            checkpoint_nonce: intent.checkpoint_nonce.clone(),
            intent_sha256: crate::run_intent::RunIntent::intent_sha256(intent, raw_bytes),
            campaign_id: intent.campaign_id.clone(),
        }
    }
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
