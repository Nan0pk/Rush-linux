//! Device-class actuator helpers.
//!
//! The core `Actuator::apply` funnel in `actuator.rs` stays the single place
//! that performs side effects and journaling. Per-domain *decision* logic that
//! is worth testing in isolation (e.g. "should this device be skipped?") lives
//! here so it can be unit-tested without touching real sysfs.

pub(crate) mod display;
// D2 — NVMe Identify/PSD parsing and APST table construction. This is a
// standalone, narrowly-scoped slice of the storage depth control package:
// nothing in production calls it yet (see the module's own doc comment for
// exactly what is deliberately not wired up). Gating it on `#[cfg(test)]`
// rather than suppressing the unused-code lint keeps `clippy -D warnings`
// meaningful, matching the same choice already made in `capability.rs` for
// `Capability::required_paths`. A later D2 slice that wires this into a
// real ioctl call site and/or the reconciler removes this gate.
#[cfg(test)]
pub(crate) mod nvme_apst;
pub(crate) mod runtime_pm;
pub(crate) mod storage;
