//! Device-class actuator helpers.
//!
//! The core `Actuator::apply` funnel in `actuator.rs` stays the single place
//! that performs side effects and journaling. Per-domain *decision* logic that
//! is worth testing in isolation (e.g. "should this device be skipped?") lives
//! here so it can be unit-tested without touching real sysfs.

pub(crate) mod runtime_pm;
pub(crate) mod storage;
