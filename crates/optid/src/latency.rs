//! C1 — exit-latency estimates with explicit provenance.
//!
//! The contract gate in `contracts.rs` answers one question: may a
//! depth-enabler put a device into a deeper state without breaking the
//! responsiveness floor of the committed workload class? Answering it
//! needs a *defensible* exit latency for the state being entered.
//!
//! ## What is not evidence
//!
//! - `autosuspend_delay_ms` is a policy timer, not exit latency.
//! - `pm_qos_resume_latency_us` is a constraint the OS *requests*, not a
//!   measurement of what the device does.
//!
//! Both were previously close enough to a latency number to be mistaken for
//! one. Neither may become a [`LatencyEstimate`].
//!
//! ## What is evidence in v1
//!
//! One source: a hardware-allowlist entry that carries a verified exit
//! latency for a `(domain, hwid)` pair, optionally pinned to a firmware
//! revision. Reading `PCI_EXP_LNKCAP` L1 exit latency and the NVMe `EXLAT`
//! power-state descriptor directly is the natural second and third source,
//! and both are explicitly deferred pending reference hardware — see
//! `docs/research/0008-nvme-apst-pcie-aspm-sata-alpm.md` §3 ("Deferred
//! (tracked; several need §4 hardware): reading `PCI_EXP_LNKCAP` / NVMe
//! `EXLAT` to enforce the exit-latency-vs-floor gate per device"). This
//! module is shaped so those land as additional [`LatencySource`] variants
//! without changing the gate.
//!
//! Anything else resolves to [`LatencyResolution::Unknown`], which denies
//! latency-sensitive depth actuation.

/// Where an exit-latency value came from.
///
/// v1 has exactly one variant on purpose. A variant that nothing produces
/// would be a claim the project cannot back (AGENTS.md §8 "Package
/// completion contract" item 5), so the register-read sources deferred by
/// research 0008 are absent until they are actually wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LatencySource {
    /// A hardware-allowlist entry carrying a verified exit latency for this
    /// `(domain, hwid)` pair. Pinned to a firmware revision when the entry
    /// records one.
    AllowlistVerified,
}

impl LatencySource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            LatencySource::AllowlistVerified => "allowlist_verified",
        }
    }
}

/// How much the estimate should be trusted. Composition keeps the weakest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LatencyConfidence {
    /// Verified on this hardware, but the value is not pinned to a firmware
    /// revision, so a firmware update could move it without being noticed.
    Medium,
    /// Verified on this hardware *and* pinned to the firmware revision the
    /// device is running.
    High,
}

impl LatencyConfidence {
    pub(crate) fn label(self) -> &'static str {
        match self {
            LatencyConfidence::Medium => "medium",
            LatencyConfidence::High => "high",
        }
    }
}

/// A defensible exit-latency value with the provenance needed to re-check it.
///
/// `measured_at` and `firmware_id` exist so a cached value can be invalidated
/// rather than trusted forever: see [`LatencyEstimate::revalidate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LatencyEstimate {
    /// Exit latency in **microseconds**. Never milliseconds.
    pub(crate) value_us: u64,
    pub(crate) source: LatencySource,
    pub(crate) confidence: LatencyConfidence,
    /// Unix seconds when the value was established, when the source records
    /// it. `None` for a compiled-in baseline entry that carries no date.
    pub(crate) measured_at: Option<u64>,
    /// The hwid the value is attributable to.
    pub(crate) hardware_id: String,
    /// The firmware revision the value was established against, when the
    /// source pins one. `None` means the value is not firmware-pinned.
    pub(crate) firmware_id: Option<String>,
}

/// The result of asking for a device's exit latency.
///
/// `Unknown` is a first-class answer, not an error: the gate must be able to
/// distinguish "this device's latency is 250us" from "nobody knows", because
/// only the first can authorize a deeper state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LatencyResolution {
    Known(LatencyEstimate),
    Unknown { reason: String },
}

impl LatencyResolution {
    pub(crate) fn unknown(reason: impl Into<String>) -> Self {
        LatencyResolution::Unknown {
            reason: reason.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn known(&self) -> Option<&LatencyEstimate> {
        match self {
            LatencyResolution::Known(estimate) => Some(estimate),
            LatencyResolution::Unknown { .. } => None,
        }
    }

    /// Compose several resolutions for one action into a single answer.
    ///
    /// A depth change that traverses more than one component (a link state
    /// plus a controller state, say) is only as fast as its slowest part, so
    /// the composed value is the **maximum** of the parts and the composed
    /// confidence is the **weakest** of the parts.
    ///
    /// Composition is strict: one `Unknown` part makes the whole `Unknown`.
    /// Averaging over a missing part, or silently dropping it, would invent
    /// a latency the project cannot defend. An empty set is `Unknown` for
    /// the same reason — composing nothing proves nothing.
    pub(crate) fn compose_max(parts: &[LatencyResolution]) -> LatencyResolution {
        if parts.is_empty() {
            return LatencyResolution::unknown("no exit-latency components to compose");
        }

        let mut worst: Option<&LatencyEstimate> = None;
        let mut weakest = LatencyConfidence::High;
        for part in parts {
            let estimate = match part {
                LatencyResolution::Known(estimate) => estimate,
                LatencyResolution::Unknown { reason } => {
                    return LatencyResolution::unknown(format!(
                        "composition is unknown because a component is unknown: {reason}"
                    ));
                }
            };
            weakest = weakest.min(estimate.confidence);
            worst = match worst {
                Some(current) if current.value_us >= estimate.value_us => Some(current),
                _ => Some(estimate),
            };
        }

        // `worst` is Some: `parts` is non-empty and every part was `Known`.
        let worst = match worst {
            Some(estimate) => estimate,
            None => return LatencyResolution::unknown("no exit-latency components to compose"),
        };
        LatencyResolution::Known(LatencyEstimate {
            value_us: worst.value_us,
            source: worst.source,
            confidence: weakest,
            measured_at: worst.measured_at,
            hardware_id: worst.hardware_id.clone(),
            firmware_id: worst.firmware_id.clone(),
        })
    }
}

impl LatencyEstimate {
    /// Re-check a cached estimate against the firmware the device is actually
    /// running.
    ///
    /// Exit latency is a property of silicon *and* controller firmware —
    /// research 0008 §1.5 treats PCIe/NVMe latencies as hardware-fixed, but
    /// pins them to "NVMe controller firmware tables", so a firmware update
    /// can move them. An estimate pinned to firmware `A` says nothing about
    /// the device once it is running firmware `B`, so it becomes `Unknown`
    /// rather than being carried forward.
    ///
    /// An estimate that pins no firmware is not invalidated by an observed
    /// revision; it is already carrying the lower confidence that reflects
    /// not being pinned.
    pub(crate) fn revalidate(self, observed_firmware_id: Option<&str>) -> LatencyResolution {
        match (self.firmware_id.as_deref(), observed_firmware_id) {
            (Some(pinned), Some(observed)) if pinned != observed => LatencyResolution::unknown(
                format!(
                    "cached exit latency for {hwid} was established on firmware {pinned:?} but the device reports {observed:?}; stale cache",
                    hwid = self.hardware_id,
                ),
            ),
            (Some(pinned), None) => LatencyResolution::unknown(format!(
                "cached exit latency for {hwid} is pinned to firmware {pinned:?} but the device's firmware revision could not be read; stale cache",
                hwid = self.hardware_id,
            )),
            _ => LatencyResolution::Known(self),
        }
    }

    /// One-line provenance for an operator-visible reason string.
    pub(crate) fn describe(&self) -> String {
        let firmware = self.firmware_id.as_deref().unwrap_or("-");
        format!(
            "{value}us (source={source} confidence={confidence} hwid={hwid} firmware={firmware})",
            value = self.value_us,
            source = self.source.label(),
            confidence = self.confidence.label(),
            hwid = self.hardware_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimate(value_us: u64, confidence: LatencyConfidence) -> LatencyEstimate {
        LatencyEstimate {
            value_us,
            source: LatencySource::AllowlistVerified,
            confidence,
            measured_at: None,
            hardware_id: "pci:v00008086d00009A0B".to_string(),
            firmware_id: None,
        }
    }

    fn firmware_pinned(value_us: u64, firmware: &str) -> LatencyEstimate {
        LatencyEstimate {
            firmware_id: Some(firmware.to_string()),
            ..estimate(value_us, LatencyConfidence::High)
        }
    }

    #[test]
    fn c1_compose_max_takes_the_slowest_component() {
        let composed = LatencyResolution::compose_max(&[
            LatencyResolution::Known(estimate(120, LatencyConfidence::High)),
            LatencyResolution::Known(estimate(900, LatencyConfidence::High)),
            LatencyResolution::Known(estimate(75, LatencyConfidence::High)),
        ]);
        let known = composed.known().expect("all components known");
        assert_eq!(
            known.value_us, 900,
            "a path is only as fast as its slowest component"
        );
    }

    #[test]
    fn c1_compose_max_keeps_the_weakest_confidence() {
        let composed = LatencyResolution::compose_max(&[
            LatencyResolution::Known(estimate(900, LatencyConfidence::High)),
            LatencyResolution::Known(estimate(120, LatencyConfidence::Medium)),
        ]);
        let known = composed.known().expect("all components known");
        // The value comes from the slowest part, the confidence from the
        // least trustworthy part — they need not be the same component.
        assert_eq!(known.value_us, 900);
        assert_eq!(known.confidence, LatencyConfidence::Medium);
    }

    #[test]
    fn c1_compose_max_is_unknown_if_any_component_is_unknown() {
        let composed = LatencyResolution::compose_max(&[
            LatencyResolution::Known(estimate(120, LatencyConfidence::High)),
            LatencyResolution::unknown("second link has no entry"),
        ]);
        assert!(
            composed.known().is_none(),
            "one unknown component must not be averaged away"
        );
        match composed {
            LatencyResolution::Unknown { reason } => {
                assert!(reason.contains("second link has no entry"))
            }
            LatencyResolution::Known(_) => unreachable!(),
        }
    }

    #[test]
    fn c1_compose_max_of_nothing_is_unknown() {
        // Composing an empty set proves nothing; it must not read as 0us,
        // which would fit every floor.
        assert!(LatencyResolution::compose_max(&[]).known().is_none());
    }

    #[test]
    fn c1_stale_firmware_cache_invalidates_the_estimate() {
        let resolution = firmware_pinned(250, "1.2.0").revalidate(Some("1.3.1"));
        match resolution {
            LatencyResolution::Unknown { reason } => {
                assert!(reason.contains("stale cache"), "reason was: {reason}");
                assert!(reason.contains("1.2.0") && reason.contains("1.3.1"));
            }
            LatencyResolution::Known(_) => {
                panic!("an estimate pinned to another firmware revision must not be reused")
            }
        }
    }

    #[test]
    fn c1_matching_firmware_keeps_the_estimate() {
        let resolution = firmware_pinned(250, "1.2.0").revalidate(Some("1.2.0"));
        assert_eq!(resolution.known().map(|e| e.value_us), Some(250));
    }

    #[test]
    fn c1_unreadable_firmware_invalidates_a_pinned_estimate() {
        // Fail-safe direction: if the estimate is pinned but the device's
        // revision cannot be read, the pin cannot be checked, so the value
        // is not evidence.
        let resolution = firmware_pinned(250, "1.2.0").revalidate(None);
        assert!(resolution.known().is_none());
    }

    #[test]
    fn c1_unpinned_estimate_survives_an_unknown_firmware() {
        // An estimate that never claimed a firmware revision is not
        // invalidated by one; its lower confidence already reflects that.
        let resolution = estimate(250, LatencyConfidence::Medium).revalidate(Some("9.9.9"));
        assert_eq!(resolution.known().map(|e| e.value_us), Some(250));
    }

    #[test]
    fn c1_describe_names_the_provenance() {
        let text = firmware_pinned(250, "1.2.0").describe();
        assert!(text.contains("250us"));
        assert!(text.contains("source=allowlist_verified"));
        assert!(text.contains("confidence=high"));
        assert!(text.contains("firmware=1.2.0"));
    }
}
