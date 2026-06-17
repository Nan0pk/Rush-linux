//! Post-processing transport layer.
//!
//! Handles deferred serialization, compression, cryptographic signing,
//! and autonomous dispatch of telemetry payloads. All heavy processing
//! happens in a low-priority background thread after the benchmark
//! completes.

pub mod http;
pub mod serialize;
pub mod sign;

pub use serialize::{serialize_payload, TelemetryPayload};
pub use sign::PayloadSigner;
pub use http::TelemetryClient;
