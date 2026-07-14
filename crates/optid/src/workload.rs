//! Workload classification, mode enumeration, and hysteresis state machines.
//!
//! This module groups the "what is the system doing right now" types and the
//! two hysteresis filters that prevent classification flapping:
//!
//! - `WorkloadClass` — the SPEC §1 five-class taxonomy (`idle`, `light`,
//!   `interactive`, `latency-critical`, `throughput`) that selects the active
//!   latency contract.
//! - `Mode` — the coarse power-mode profile (`auto`, `battery`, `balanced`,
//!   `performance`, `realtime`) that selects the EPP / platform-profile /
//!   cgroup-weight action set.
//!
//! Both have separate hysteresis filters because workload class changes
//! faster than mode (3s vs 6s dwell), and mode hysteresis is bypassable
//! under critical thermal pressure.

use std::fmt;
use std::fs;
use std::path::Path;

use crate::args::DEFAULT_MODE_DWELL_WINDOW_SEC;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Auto,
    Battery,
    Balanced,
    Performance,
    Realtime,
}

impl Mode {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "battery" => Some(Self::Battery),
            "balanced" => Some(Self::Balanced),
            "performance" => Some(Self::Performance),
            "realtime" => Some(Self::Realtime),
            _ => None,
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Auto => "auto",
            Self::Battery => "battery",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
            Self::Realtime => "realtime",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum WorkloadClass {
    Idle,
    Light,
    Interactive,
    LatencyCritical,
    Throughput,
    /// v0.6 Phase C2: platform-forced class for VM guests. Selected by
    /// the classifier when DMI reports a hypervisor vendor. NOT
    /// user-selectable via `optctl pin`.
    #[serde(rename = "vm.guest")]
    VmGuest,
}

impl fmt::Display for WorkloadClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Idle => "idle",
            Self::Light => "light",
            Self::Interactive => "interactive",
            Self::LatencyCritical => "latency-critical",
            Self::Throughput => "throughput",
            Self::VmGuest => "vm.guest",
        };
        f.write_str(value)
    }
}

impl WorkloadClass {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "idle" => Some(Self::Idle),
            "light" => Some(Self::Light),
            "interactive" => Some(Self::Interactive),
            "latency-critical" => Some(Self::LatencyCritical),
            "throughput" => Some(Self::Throughput),
            // v0.6 Phase C2: accept "vm.guest" for state-file reads.
            "vm.guest" => Some(Self::VmGuest),
            _ => None,
        }
    }
}

pub(crate) fn read_pinned_class(state_dir: &Path, app_id: &str) -> Option<WorkloadClass> {
    let pin_file = state_dir.join("pins").join(app_id);
    let text = fs::read_to_string(pin_file).ok()?;
    WorkloadClass::parse(&text)
}

pub(crate) fn read_global_pinned_class(state_dir: &Path) -> Option<WorkloadClass> {
    let pin_file = state_dir.join("workload_class_pin");
    let text = fs::read_to_string(pin_file).ok()?;
    let parsed = WorkloadClass::parse(&text);
    if parsed.is_none() {
        eprintln!("optid: ignored invalid global class pin: '{}'", text.trim());
    }
    parsed
}

pub(crate) fn read_mode_override(state_dir: &Path) -> Option<Mode> {
    let text = fs::read_to_string(state_dir.join("mode")).ok()?;
    Mode::parse(&text)
}

#[derive(Debug, Clone)]
pub(crate) struct HysteresisState {
    pub(crate) committed_class: WorkloadClass,
    pub(crate) candidate_class: WorkloadClass,
    pub(crate) candidate_since: Option<u64>,
}

impl HysteresisState {
    pub(crate) fn new(initial_class: WorkloadClass) -> Self {
        Self {
            committed_class: initial_class,
            candidate_class: initial_class,
            candidate_since: None,
        }
    }

    pub(crate) fn update(
        &mut self,
        next_class: WorkloadClass,
        now: u64,
        dwell_window_sec: u64,
    ) -> (WorkloadClass, bool) {
        if next_class == self.committed_class {
            self.candidate_class = next_class;
            self.candidate_since = None;
            (self.committed_class, false)
        } else if next_class == self.candidate_class {
            match self.candidate_since {
                None => {
                    self.candidate_since = Some(now);
                    (self.committed_class, false)
                }
                Some(since) => {
                    if now >= since + dwell_window_sec {
                        self.committed_class = next_class;
                        self.candidate_since = None;
                        (self.committed_class, true)
                    } else {
                        (self.committed_class, false)
                    }
                }
            }
        } else {
            self.candidate_class = next_class;
            self.candidate_since = Some(now);
            (self.committed_class, false)
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ModeHysteresisState {
    pub(crate) committed_mode: Mode,
    pub(crate) candidate_mode: Mode,
    pub(crate) candidate_since: Option<u64>,
}

impl ModeHysteresisState {
    pub(crate) fn new(initial_mode: Mode) -> Self {
        Self {
            committed_mode: initial_mode,
            candidate_mode: initial_mode,
            candidate_since: None,
        }
    }

    pub(crate) fn force(&mut self, mode: Mode) {
        self.committed_mode = mode;
        self.candidate_mode = mode;
        self.candidate_since = None;
    }

    pub(crate) fn update(
        &mut self,
        next_mode: Mode,
        now: u64,
        dwell_window_sec: u64,
        bypass_hysteresis: bool,
    ) -> (Mode, bool, Option<String>) {
        if bypass_hysteresis {
            let changed = self.committed_mode != next_mode;
            self.force(next_mode);
            return (
                self.committed_mode,
                changed,
                Some(format!(
                    "mode hysteresis bypassed for safety: committed {} immediately",
                    self.committed_mode
                )),
            );
        }

        if next_mode == self.committed_mode {
            self.candidate_mode = next_mode;
            self.candidate_since = None;
            return (self.committed_mode, false, None);
        }

        if next_mode != self.candidate_mode {
            self.candidate_mode = next_mode;
            self.candidate_since = Some(now);
            return (
                self.committed_mode,
                false,
                Some(format!(
                    "mode hysteresis delaying transition: committed={}, candidate={}, elapsed=0s, required={}s",
                    self.committed_mode, self.candidate_mode, dwell_window_sec
                )),
            );
        }

        let since = self.candidate_since.unwrap_or(now);
        self.candidate_since = Some(since);
        if now >= since + dwell_window_sec {
            self.committed_mode = next_mode;
            self.candidate_since = None;
            return (
                self.committed_mode,
                true,
                Some(format!(
                    "mode hysteresis committed transition to {} after {}s dwell",
                    self.committed_mode,
                    now.saturating_sub(since)
                )),
            );
        }

        (
            self.committed_mode,
            false,
            Some(format!(
                "mode hysteresis delaying transition: committed={}, candidate={}, elapsed={}s, required={}s",
                self.committed_mode,
                self.candidate_mode,
                now.saturating_sub(since),
                dwell_window_sec
            )),
        )
    }

    pub(crate) fn explain_pending(&self, now: u64) -> Option<String> {
        self.candidate_since.map(|since| {
            format!(
                "mode hysteresis pending: committed={}, candidate={}, elapsed={}s, required={}s",
                self.committed_mode,
                self.candidate_mode,
                now.saturating_sub(since),
                DEFAULT_MODE_DWELL_WINDOW_SEC
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parse_round_trip() {
        for s in ["auto", "battery", "balanced", "performance", "realtime"] {
            let m = Mode::parse(s).unwrap_or_else(|| panic!("parse {s}"));
            assert_eq!(m.to_string(), s);
        }
        assert!(Mode::parse("nope").is_none());
    }

    #[test]
    fn workload_class_parse_round_trip() {
        for s in [
            "idle",
            "light",
            "interactive",
            "latency-critical",
            "throughput",
            "vm.guest",
        ] {
            let c = WorkloadClass::parse(s).unwrap_or_else(|| panic!("parse {s}"));
            assert_eq!(c.to_string(), s);
        }
        assert!(WorkloadClass::parse("nope").is_none());
    }

    #[test]
    fn test_n1_t3_hysteresis() {
        let mut hysteresis = HysteresisState::new(WorkloadClass::Idle);

        // Transition from Idle -> Interactive.
        // Sample at t=0, class remains Idle (candidate Interactive)
        let (class, _) = hysteresis.update(WorkloadClass::Interactive, 0, 3);
        assert_eq!(class, WorkloadClass::Idle);

        // Sample at t=2 (less than 3 seconds), class remains Idle
        let (class, _) = hysteresis.update(WorkloadClass::Interactive, 2, 3);
        assert_eq!(class, WorkloadClass::Idle);

        // Sample at t=3 (sustained 3 seconds), class transitions to Interactive
        let (class, changed) = hysteresis.update(WorkloadClass::Interactive, 3, 3);
        assert_eq!(class, WorkloadClass::Interactive);
        assert!(changed);

        // A single-sample blip at t=4 back to Idle does not immediately change committed class
        let (class, _) = hysteresis.update(WorkloadClass::Idle, 4, 3);
        assert_eq!(class, WorkloadClass::Interactive);
    }

    #[test]
    fn test_n1_t3b_mode_hysteresis_delays_auto_transition() {
        let mut hysteresis = ModeHysteresisState::new(Mode::Balanced);

        let (mode, changed, reason) = hysteresis.update(Mode::Performance, 0, 6, false);
        assert_eq!(mode, Mode::Balanced);
        assert!(!changed);
        assert!(reason.unwrap().contains("delaying transition"));

        let (mode, changed, _) = hysteresis.update(Mode::Performance, 5, 6, false);
        assert_eq!(mode, Mode::Balanced);
        assert!(!changed);

        let (mode, changed, reason) = hysteresis.update(Mode::Performance, 6, 6, false);
        assert_eq!(mode, Mode::Performance);
        assert!(changed);
        assert!(reason.unwrap().contains("committed transition"));
    }

    #[test]
    fn test_n1_t3c_mode_hysteresis_critical_thermal_bypasses_delay() {
        let mut hysteresis = ModeHysteresisState::new(Mode::Performance);

        let (mode, changed, reason) = hysteresis.update(Mode::Balanced, 10, 6, true);
        assert_eq!(mode, Mode::Balanced);
        assert!(changed);
        assert!(reason.unwrap().contains("bypassed for safety"));
    }
}
