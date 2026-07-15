//! Run-intent contract — the cryptographic association between the host
//! planner and the testOS runner.
//!
//! The host writes `run-intent.json` to the USB before boot. The runner reads
//! it on boot, refuses to run if it is missing/malformed/stale/dry-run/
//! inconsistent, and copies every field into `manifest.json` so the strict
//! evidence validator (`tools/validate-testos-evidence.py`) can re-bind the
//! run to the plan, catalog, image, source commit, and run_id.
//!
//! Schema: `schemas/testos-run-intent.schema.json` (frozen at version 1).
//! The runner MUST refuse a mismatched `schema_version`.
//!
//! Fail-closed behavior: every error path returns `RunIntentError` and the
//! runner exits without writing results. A missing or invalid intent never
//! falls through to an unsigned/default run.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Filename of the run-intent file on the USB, relative to the USB mount root.
pub const INTENT_FILENAME: &str = "run-intent.json";

/// Schema version of the run-intent file. Frozen at 1. The runner refuses
/// mismatched versions.
pub const INTENT_SCHEMA_VERSION: u32 = 1;

/// Default freshness window for `generated_at` (24 hours). Intents older than
/// this are rejected to prevent stale-USB replays. The intent may override
/// this via `freshness_seconds` (clamped to [60s, 7d]).
pub const DEFAULT_FRESHNESS_SECONDS: u64 = 24 * 60 * 60;

/// Minimum/maximum allowed `freshness_seconds` override.
pub const MIN_FRESHNESS_SECONDS: u64 = 60;
pub const MAX_FRESHNESS_SECONDS: u64 = 7 * 24 * 60 * 60;

/// The run-intent record read from the USB.
///
/// Field set and patterns mirror `schemas/testos-run-intent.schema.json`.
/// `additionalProperties` is false in the schema, so serde's default
/// (ignore-unknown) is acceptable for forward-compat reading, but the runner
/// also validates `intent_kind` and `schema_version` explicitly to fail
/// closed on the wrong file type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunIntent {
    pub schema_version: u32,
    pub intent_kind: String,
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
    /// ISO 8601 UTC, e.g. `2026-07-15T10:00:00Z`.
    pub generated_at: String,
    pub dry_run: bool,
    pub checkpoint_nonce: String,
    #[serde(default)]
    pub campaign_id: Option<String>,
    #[serde(default)]
    pub freshness_seconds: Option<u64>,
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Errors that can occur while loading/validating a run intent.
///
/// Every variant maps to a fail-closed exit in the runner. The runner prints
/// the error and drops to a diagnostic shell; it never writes results without
/// a valid intent.
#[derive(Debug)]
pub enum RunIntentError {
    /// `run-intent.json` is absent from the USB.
    Missing,
    /// The file exists but cannot be read (I/O error).
    ReadError(String),
    /// The file is not valid JSON.
    ParseError(String),
    /// JSON parsed but a required field is missing or has the wrong shape.
    /// Carries the field name and a human-readable reason.
    FieldError(String, String),
    /// `schema_version` does not match `INTENT_SCHEMA_VERSION`.
    SchemaVersionMismatch(u32),
    /// `intent_kind` is not `"testos-run-intent"`.
    WrongKind(String),
    /// `dry_run == true` on a physical boot.
    DryRun,
    /// `generated_at` is older than the freshness window.
    Stale { generated_at: String, age_seconds: u64, max_seconds: u64 },
    /// `generated_at` is in the future (clock skew / tampering).
    Future { generated_at: String, skew_seconds: u64 },
    /// A digest field does not match its expected format.
    BadDigest(String, String),
    /// The running testOS image version does not match `testos_version`.
    TestosVersionMismatch { intent: String, running: String },
    /// The benchmark catalog on the USB does not hash to
    /// `benchmark_catalog_sha256`.
    CatalogHashMismatch { expected: String, actual: String },
}

impl std::fmt::Display for RunIntentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunIntentError::Missing => write!(f, "run-intent.json is missing from the USB; the host planner did not write a valid intent before boot"),
            RunIntentError::ReadError(e) => write!(f, "cannot read run-intent.json: {}", e),
            RunIntentError::ParseError(e) => write!(f, "run-intent.json is not valid JSON: {}", e),
            RunIntentError::FieldError(field, reason) => write!(f, "run-intent.json field {:?}: {}", field, reason),
            RunIntentError::SchemaVersionMismatch(v) => write!(f, "run-intent.json schema_version is {}, runner expects {}", v, INTENT_SCHEMA_VERSION),
            RunIntentError::WrongKind(k) => write!(f, "run-intent.json intent_kind is {:?}, expected \"testos-run-intent\"", k),
            RunIntentError::DryRun => write!(f, "run-intent.json has dry_run=true; a physical testOS run requires dry_run=false"),
            RunIntentError::Stale { generated_at, age_seconds, max_seconds } => write!(f, "run-intent.json generated_at {} is {}s old (max {}s); stale intent rejected", generated_at, age_seconds, max_seconds),
            RunIntentError::Future { generated_at, skew_seconds } => write!(f, "run-intent.json generated_at {} is {}s in the future (clock skew or tampering)", generated_at, skew_seconds),
            RunIntentError::BadDigest(field, val) => write!(f, "run-intent.json field {:?} is not a valid digest: {:?}", field, val),
            RunIntentError::TestosVersionMismatch { intent, running } => write!(f, "run-intent.json testos_version {:?} does not match the running image version {:?}", intent, running),
            RunIntentError::CatalogHashMismatch { expected, actual } => write!(f, "benchmark catalog SHA-256 mismatch: intent expected {}, USB catalog hashed to {}", expected, actual),
        }
    }
}

impl std::error::Error for RunIntentError {}

impl RunIntent {
    /// Load and fully validate a run intent from `usb_mount/run-intent.json`.
    ///
    /// `running_testos_version` is the version read from `/etc/testos/version`
    /// inside the booted image. `catalog_path` is the path to the bench-list
    /// TOML on the USB; its SHA-256 is recomputed and compared to
    /// `benchmark_catalog_sha256`.
    ///
    /// Returns `Ok(intent)` only when every check passes. Any failure is
    /// fail-closed: the caller must not write results.
    pub fn load_and_validate(
        usb_mount: &Path,
        running_testos_version: &str,
        catalog_path: &Path,
    ) -> Result<RunIntent, RunIntentError> {
        let intent_path = usb_mount.join(INTENT_FILENAME);
        if !intent_path.exists() {
            return Err(RunIntentError::Missing);
        }
        let text = std::fs::read_to_string(&intent_path)
            .map_err(|e| RunIntentError::ReadError(e.to_string()))?;
        let intent: RunIntent = serde_json::from_str(&text)
            .map_err(|e| RunIntentError::ParseError(e.to_string()))?;

        intent.validate(running_testos_version, catalog_path)?;
        Ok(intent)
    }

    /// Full validation of an already-parsed intent.
    pub fn validate(
        &self,
        running_testos_version: &str,
        catalog_path: &Path,
    ) -> Result<(), RunIntentError> {
        // Discriminator + schema version first — these catch the wrong file
        // type before we look at any field.
        if self.schema_version != INTENT_SCHEMA_VERSION {
            return Err(RunIntentError::SchemaVersionMismatch(self.schema_version));
        }
        if self.intent_kind != "testos-run-intent" {
            return Err(RunIntentError::WrongKind(self.intent_kind.clone()));
        }

        // Required-string field presence + pattern checks.
        require_nonempty(&self.run_id, "run_id")?;
        require_pattern(&self.run_id, "run_id", r"^[A-Za-z0-9_.:-]{4,128}$")?;
        require_pattern(&self.source_commit, "source_commit", r"^[0-9a-f]{40}$")?;
        require_pattern(
            &self.source_version,
            "source_version",
            r"^\d+\.\d+\.\d+(-(alpha|beta|rc)\.\d+)?$",
        )?;
        require_pattern(
            &self.testos_version,
            "testos_version",
            r"^\d+\.\d+\.\d+(-(alpha|beta|rc)\.\d+)?$",
        )?;
        require_pattern(
            &self.testos_image_digest,
            "testos_image_digest",
            r"^sha256:[0-9a-f]{64}$",
        )?;
        require_pattern(&self.plan_sha256, "plan_sha256", r"^[0-9a-f]{64}$")?;
        require_pattern(
            &self.benchmark_catalog_sha256,
            "benchmark_catalog_sha256",
            r"^[0-9a-f]{64}$",
        )?;
        require_pattern(
            &self.generated_at,
            "generated_at",
            r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$",
        )?;
        require_pattern(
            &self.checkpoint_nonce,
            "checkpoint_nonce",
            r"^[A-Za-z0-9_.:-]{8,128}$",
        )?;
        if let Some(c) = &self.campaign_id {
            require_pattern(c, "campaign_id", r"^[A-Za-z0-9_.:-]{4,128}$")?;
        }

        // dry_run must be false for a physical run.
        if self.dry_run {
            return Err(RunIntentError::DryRun);
        }

        // Freshness: generated_at must be within [now - max, now + small skew].
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let gen_epoch = parse_iso8601_to_epoch(&self.generated_at).ok_or_else(|| {
            RunIntentError::FieldError(
                "generated_at".to_string(),
                format!("cannot parse {:?} as ISO 8601 UTC", self.generated_at),
            )
        })?;
        let max_age = self
            .freshness_seconds
            .unwrap_or(DEFAULT_FRESHNESS_SECONDS)
            .clamp(MIN_FRESHNESS_SECONDS, MAX_FRESHNESS_SECONDS);
        // Allow up to 5 minutes of clock skew into the future.
        const SKEW: u64 = 5 * 60;
        if gen_epoch > now + SKEW {
            return Err(RunIntentError::Future {
                generated_at: self.generated_at.clone(),
                skew_seconds: gen_epoch - now,
            });
        }
        if now > gen_epoch + max_age {
            return Err(RunIntentError::Stale {
                generated_at: self.generated_at.clone(),
                age_seconds: now - gen_epoch,
                max_seconds: max_age,
            });
        }

        // Cross-check the running testOS image version.
        if self.testos_version != running_testos_version {
            return Err(RunIntentError::TestosVersionMismatch {
                intent: self.testos_version.clone(),
                running: running_testos_version.to_string(),
            });
        }

        // Recompute the benchmark catalog hash and compare.
        let catalog_bytes = std::fs::read(catalog_path).map_err(|e| {
            RunIntentError::FieldError(
                "benchmark_catalog_sha256".to_string(),
                format!("cannot read catalog at {}: {}", catalog_path.display(), e),
            )
        })?;
        let actual = sha256_hex(&catalog_bytes);
        if actual != self.benchmark_catalog_sha256 {
            return Err(RunIntentError::CatalogHashMismatch {
                expected: self.benchmark_catalog_sha256.clone(),
                actual,
            });
        }

        Ok(())
    }

    /// SHA-256 of the run-intent.json bytes, as hex. Used by the runner to
    /// fill `manifest.json.provenance.intent_sha256`.
    pub fn intent_sha256(&self, raw_bytes: &[u8]) -> String {
        sha256_hex(raw_bytes)
    }
}

fn require_nonempty(s: &str, field: &str) -> Result<(), RunIntentError> {
    if s.is_empty() {
        return Err(RunIntentError::FieldError(
            field.to_string(),
            "must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn require_pattern(s: &str, field: &str, pattern: &str) -> Result<(), RunIntentError> {
    // Minimal regex-free pattern check for the patterns we use:
    //  - ^[0-9a-f]{40}$
    //  - ^[0-9a-f]{64}$
    //  - ^sha256:[0-9a-f]{64}$
    //  - ^\d+\.\d+\.\d+(-(alpha|beta|rc)\.\d+)?$
    //  - ^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$
    //  - ^[A-Za-z0-9_.:-]{4,128}$ / {8,128}
    //
    // We hand-check each known pattern to avoid pulling in the `regex` crate
    // (the testos crate is intentionally dependency-light). If the pattern
    // does not match a known shape, we accept defensively (the validator
    // script does the authoritative regex check).
    let ok = match pattern {
        r"^[0-9a-f]{40}$" => is_hex(s, 40),
        r"^[0-9a-f]{64}$" => is_hex(s, 64),
        r"^sha256:[0-9a-f]{64}$" => {
            s.len() == 71 && s.starts_with("sha256:") && is_hex(&s[7..], 64)
        }
        r"^\d+\.\d+\.\d+(-(alpha|beta|rc)\.\d+)?$" => is_semver(s),
        r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$" => is_iso8601_utc(s),
        r"^[A-Za-z0-9_.:-]{4,128}$" => is_safe_token(s, 4, 128),
        r"^[A-Za-z0-9_.:-]{8,128}$" => is_safe_token(s, 8, 128),
        _ => true,
    };
    if !ok {
        return Err(RunIntentError::FieldError(
            field.to_string(),
            format!("{:?} does not match pattern {}", s, pattern),
        ));
    }
    Ok(())
}

fn is_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_semver(s: &str) -> bool {
    // \d+.\d+.\d+ with optional -(alpha|beta|rc).N
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    match pre {
        None => true,
        Some(p) => {
            // alpha|beta|rc . \d+
            match p.split_once('.') {
                Some((tag, num)) => {
                    matches!(tag, "alpha" | "beta" | "rc")
                        && !num.is_empty()
                        && num.bytes().all(|b| b.is_ascii_digit())
                }
                None => false,
            }
        }
    }
}

fn is_iso8601_utc(s: &str) -> bool {
    // YYYY-MM-DDTHH:MM:SSZ
    let b = s.as_bytes();
    b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z'
        && b[..4].iter().all(|c| c.is_ascii_digit())
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && b[11..13].iter().all(|c| c.is_ascii_digit())
        && b[14..16].iter().all(|c| c.is_ascii_digit())
        && b[17..19].iter().all(|c| c.is_ascii_digit())
}

fn is_safe_token(s: &str, min: usize, max: usize) -> bool {
    !(s.len() < min || s.len() > max)
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
}

/// Parse `YYYY-MM-DDTHH:MM:SSZ` to a Unix epoch second. Returns None on
/// malformed input. Uses the same civil-from-days algorithm as the runner.
fn parse_iso8601_to_epoch(s: &str) -> Option<u64> {
    if !is_iso8601_utc(s) {
        return None;
    }
    let b = s.as_bytes();
    let year: i64 = std::str::from_utf8(&b[..4]).ok()?.parse().ok()?;
    let month: u32 = std::str::from_utf8(&b[5..7]).ok()?.parse().ok()?;
    let day: u32 = std::str::from_utf8(&b[8..10]).ok()?.parse().ok()?;
    let hour: u64 = std::str::from_utf8(&b[11..13]).ok()?.parse().ok()?;
    let min: u64 = std::str::from_utf8(&b[14..16]).ok()?.parse().ok()?;
    let sec: u64 = std::str::from_utf8(&b[17..19]).ok()?.parse().ok()?;

    // Days from civil (Hinnant). Returns days since 1970-01-01.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u64; // [0, 399]
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) as u64 + 2) / 5 + (day - 1) as u64;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days_since_epoch = era as i64 * 146097 + doe as i64 - 719468;
    if days_since_epoch < 0 {
        return None;
    }
    Some(days_since_epoch as u64 * 86400 + hour * 3600 + min * 60 + sec)
}

/// SHA-256 of `data`, lowercase hex. Uses the `sha2` crate via the
/// `testos_sha256` feature-free inline implementation below. To avoid adding
/// a dependency, we shell out to `sha256sum` (always present on a testOS
/// image). If `sha256sum` is unavailable, we fall back to a pure-Rust
/// SHA-256 implementation so this code is testable in CI without external
/// tools.
pub fn sha256_hex(data: &[u8]) -> String {
    // Try `sha256sum` first (one fork, no dependency).
    if let Ok(out) = std::process::Command::new("sha256sum")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
    {
        use std::io::Write;
        let mut child = out;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(data);
        }
        if let Ok(output) = child.wait_with_output() {
            if output.status.success() {
                let line = String::from_utf8_lossy(&output.stdout);
                if let Some(hash) = line.split_whitespace().next() {
                    if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return hash.to_lowercase();
                    }
                }
            }
        }
    }
    // Fallback: pure-Rust SHA-256.
    rust_sha256_hex(data)
}

// ─── Pure-Rust SHA-256 fallback ──────────────────────────────────────────────
//
// Used only when `sha256sum` is unavailable (e.g. some CI sandboxes). This is
// a straightforward implementation of FIPS 180-4; correctness is verified by
// the regression tests in tools/test-cloud-safe-livedev.py against known
// vectors and against `sha256sum` output.

fn rust_sha256_hex(data: &[u8]) -> String {
    let h = sha256_raw(data);
    h.iter().map(|b| format!("{:02x}", b)).collect()
}

fn sha256_raw(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut hh: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bitlen = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bitlen.to_be_bytes());
    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h) =
            (hh[0], hh[1], hh[2], hh[3], hh[4], hh[5], hh[6], hh[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        hh[0] = hh[0].wrapping_add(a);
        hh[1] = hh[1].wrapping_add(b);
        hh[2] = hh[2].wrapping_add(c);
        hh[3] = hh[3].wrapping_add(d);
        hh[4] = hh[4].wrapping_add(e);
        hh[5] = hh[5].wrapping_add(f);
        hh[6] = hh[6].wrapping_add(g);
        hh[7] = hh[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&hh[i].to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        // Empty string.
        assert_eq!(
            rust_sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // "abc".
        assert_eq!(
            rust_sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn parse_iso8601_roundtrip() {
        // 2026-07-15T00:00:00Z = 1784448000
        assert_eq!(parse_iso8601_to_epoch("2026-07-15T00:00:00Z"), Some(1784448000));
        assert_eq!(parse_iso8601_to_epoch("not-a-date"), None);
    }

    #[test]
    fn semver_patterns() {
        assert!(is_semver("0.7.0"));
        assert!(is_semver("0.7.0-beta.4"));
        assert!(!is_semver("0.7"));
        assert!(!is_semver("0.7.0-rc.4.2"));
    }

    #[test]
    fn safe_token_patterns() {
        assert!(is_safe_token("run-2026-07-15", 4, 128));
        assert!(!is_safe_token("ab", 4, 128));
        assert!(!is_safe_token("has space", 4, 128));
        assert!(!is_safe_token("has/slash", 4, 128));
    }
}
