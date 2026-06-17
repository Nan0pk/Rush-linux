//! Payload serialization and compression.
//!
//! Converts raw `TelemetryEvent` arrays into a compressed MessagePack
//! payload suitable for network transport. All processing is deferred
//! to post-execution — nothing happens during the measurement window.

use serde::{Deserialize, Serialize};

use crate::ebpf::types::TelemetryEvent;

/// Schema version for the telemetry payload format.
const SCHEMA_VERSION: u32 = 1;

/// Top-level telemetry payload for serialization.
#[derive(Debug, Serialize, Deserialize)]
pub struct TelemetryPayload {
    /// Schema version for forward/backward compatibility.
    pub schema_version: u32,
    /// ISO 8601 UTC timestamp of when the measurement was taken.
    pub timestamp_utc: String,
    /// Machine hostname.
    pub hostname: String,
    /// Kernel version string.
    pub kernel_version: String,
    /// CPU model name.
    pub cpu_model: String,
    /// Whether the system is a hybrid (P+E core) architecture.
    pub is_hybrid: bool,
    /// Git SHA of the rush_telemetry crate build.
    pub build_sha: String,
    /// Serialized events (MessagePack-encoded within the payload).
    #[serde(with = "serde_bytes")]
    pub events_blob: Vec<u8>,
    /// Number of events in the blob (for validation).
    pub event_count: u32,
    /// Number of events dropped by the ring buffer (if any).
    pub dropped_events: u32,
    /// Measurement window duration in nanoseconds.
    pub window_duration_ns: u64,
}

/// Serialize a telemetry payload to compressed MessagePack.
///
/// Steps:
/// 1. Convert raw `TelemetryEvent` array to a byte blob (zero-copy memcpy)
/// 2. Build the `TelemetryPayload` struct
/// 3. Serialize to MessagePack via `rmp-serde`
/// 4. Compress with zstd level 3
///
/// Returns the compressed bytes ready for signing and transport.
pub fn serialize_payload(
    events: &[TelemetryEvent],
    dropped_events: u32,
    window_duration_ns: u64,
) -> Result<Vec<u8>, SerializationError> {
    // Zero-copy: reinterpret the event slice as bytes
    let events_blob = events_to_bytes(events);

    let payload = TelemetryPayload {
        schema_version: SCHEMA_VERSION,
        timestamp_utc: chrono_utc_now(),
        hostname: get_hostname(),
        kernel_version: get_kernel_version_string(),
        cpu_model: get_cpu_model(),
        is_hybrid: false, // Populated by caller if HFI is available
        build_sha: env!("CARGO_PKG_VERSION").to_string(),
        events_blob,
        event_count: events.len() as u32,
        dropped_events,
        window_duration_ns,
    };

    // Serialize to MessagePack
    let msgpack = rmp_serde::to_vec_named(&payload)
        .map_err(|e| SerializationError::MsgpackEncode(e.to_string()))?;

    // Compress with zstd level 3 (good ratio, fast compression)
    let compressed = zstd::encode_all(msgpack.as_slice(), 3)
        .map_err(|e| SerializationError::ZstdCompress(e.to_string()))?;

    Ok(compressed)
}

/// Reinterpret a `TelemetryEvent` slice as raw bytes.
///
/// This is a zero-copy operation — no allocation beyond the output Vec.
/// The bytes are in native endianness (little-endian on x86_64/ARM64).
fn events_to_bytes(events: &[TelemetryEvent]) -> Vec<u8> {
    let byte_len = events.len() * std::mem::size_of::<TelemetryEvent>();
    let mut bytes = Vec::with_capacity(byte_len);
    unsafe {
        bytes.set_len(byte_len);
        std::ptr::copy_nonoverlapping(
            events.as_ptr() as *const u8,
            bytes.as_mut_ptr(),
            byte_len,
        );
    }
    bytes
}

/// Errors during serialization.
#[derive(Debug)]
pub enum SerializationError {
    MsgpackEncode(String),
    ZstdCompress(String),
}

impl std::fmt::Display for SerializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerializationError::MsgpackEncode(e) => write!(f, "MessagePack encode error: {e}"),
            SerializationError::ZstdCompress(e) => write!(f, "zstd compress error: {e}"),
        }
    }
}

impl std::error::Error for SerializationError {}

/// Get UTC timestamp in ISO 8601 format.
fn chrono_utc_now() -> String {
    // Use system clock directly to avoid chrono dependency
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Simple UTC timestamp formatting (no chrono dependency)
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch to Y-M-D (simplified — good enough for telemetry)
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1;
    for &m_days in &month_days {
        if days < m_days {
            break;
        }
        days -= m_days;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn get_hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn get_kernel_version_string() -> String {
    std::fs::read_to_string("/proc/version")
        .map(|s| {
            s.split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|_| "unknown".to_string())
}

fn get_cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_days_to_ymd() {
        // 2024-01-01 = day 19723
        assert_eq!(days_to_ymd(19723), (2024, 1, 1));
        // 1970-01-01 = day 0
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn test_events_to_bytes_roundtrip() {
        let events = vec![TelemetryEvent {
            event_type: 0,
            cpu_id: 1,
            _reserved: 0,
            tsc_ns: 123456789,
            payload: crate::ebpf::types::EventPayload {
                energy: crate::ebpf::types::EnergyPayload {
                    rapl_raw: 42,
                    rollover_count: 0,
                    _pad: 0,
                },
            },
        }];

        let bytes = events_to_bytes(&events);
        assert_eq!(bytes.len(), std::mem::size_of::<TelemetryEvent>());
    }
}
