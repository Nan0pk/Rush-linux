//! F4 production desired-state reconciliation.
//!
//! One target-keyed state model owns transition restoration.  The model is
//! hydrated from the typed F4 state file and from the legacy
//! `original_<key>`/`applied_<key>` journals, captures baselines before writes,
//! claims ownership only after confirmed readback, and restores only when the
//! current value still equals the last value confirmed as written by optid.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::actuator::Actuator;
use crate::actuators::display;
use crate::envelope::{
    ActionOutcome, ErrorKindCode, GateEvaluation, GateReasonCode, GateStage, OutcomeReasonCode,
    OwnershipState, PipelineStage, ReadbackOutcome, ResponsibleSubsystem, RestoreOutcome,
    RestoreState, SupportState, TargetOutcome, WriteOutcome,
};
use crate::io_util::{atomic_write_state_file_with, clear_journal_with};
use crate::kernel_io::KernelIo;
use crate::policy::{Domain, DomainMode};
use crate::sensors::discover_cpu_epp_paths_with;
use crate::workload::{Mode, WorkloadClass};

const STATE_FILE: &str = "reconciliation-state.json";
const MODE_FILE: &str = "reconciler-mode";
pub(crate) const MAX_RESTORE_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReconcileMode {
    Shadow,
    V1,
}

impl ReconcileMode {
    fn load(io: &dyn KernelIo, state_dir: &Path) -> Self {
        match io
            .read_to_string(&state_dir.join(MODE_FILE))
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("shadow") => Self::Shadow,
            _ => Self::V1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Transition {
    AcChanged {
        from: Option<bool>,
        to: Option<bool>,
    },
    WorkloadChanged {
        from: WorkloadClass,
        to: WorkloadClass,
    },
    ModeChanged {
        from: Mode,
        to: Mode,
    },
    DomainDisabled {
        domain: Domain,
    },
}

impl Transition {
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::AcChanged { from, to } => format!("ac_changed: {from:?} -> {to:?}"),
            Self::WorkloadChanged { from, to } => format!("workload_changed: {from} -> {to}"),
            Self::ModeChanged { from, to } => format!("mode_changed: {from} -> {to}"),
            Self::DomainDisabled { domain } => {
                format!("domain_disabled: {}", domain.as_str())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TargetKind {
    KernelValue {
        path: PathBuf,
    },
    PmqosCpu,
    PmqosDevice {
        path: PathBuf,
    },
    RuntimePm {
        control_path: PathBuf,
        delay_path: Option<PathBuf>,
    },
    SystemdProperty {
        unit: String,
        property: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredValue {
    Scalar {
        value: String,
    },
    RuntimePm {
        control: String,
        delay: Option<String>,
    },
    Systemd {
        explicit: bool,
        value: String,
    },
}

impl StoredValue {
    fn public_value(&self) -> String {
        match self {
            Self::Scalar { value } => value.clone(),
            Self::RuntimePm { control, delay } => match delay {
                Some(delay) => format!("control={control};delay={delay}"),
                None => format!("control={control};delay=absent"),
            },
            Self::Systemd { explicit, value } => {
                format!("explicit={explicit};value={value}")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TargetState {
    target_id: String,
    domain: String,
    target: TargetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_journal_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline: Option<StoredValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    desired: Option<StoredValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_attempted: Option<StoredValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_confirmed: Option<StoredValue>,
    ownership: OwnershipState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ownership_reason: Option<String>,
    retries: u32,
    restore_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedState {
    schema_version: u32,
    targets: BTreeMap<String, TargetState>,
}

#[derive(Debug, Clone)]
struct DesiredTarget {
    target_id: String,
    domain: String,
    target: TargetKind,
    desired: StoredValue,
    legacy_journal_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RestorePlan {
    target_id: String,
    target: TargetKind,
    baseline: StoredValue,
    last_confirmed: StoredValue,
    legacy_journal_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SystemdPropertyState {
    explicit: bool,
    value: String,
}

pub(crate) trait SystemdIo {
    fn read_property(&self, unit: &str, property: &str) -> io::Result<SystemdPropertyState>;
    fn set_property(&self, unit: &str, property: &str, value: Option<&str>) -> io::Result<()>;
}

#[derive(Debug, Default)]
struct RealSystemd;

impl RealSystemd {
    fn output(args: &[&str]) -> io::Result<String> {
        let output = Command::new("systemctl").args(args).output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "systemctl exited with {}",
                output.status
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn runtime_property_is_explicit(unit: &str, property: &str) -> io::Result<bool> {
        let paths = Self::output(&["show", "--property=DropInPaths", "--value", unit])?;
        for raw in paths.split_whitespace() {
            let path = raw.trim_matches('"');
            if !path.starts_with("/run/systemd/system.control/") {
                continue;
            }
            let Ok(content) = fs::read_to_string(path) else {
                continue;
            };
            if content.lines().any(|line| {
                line.trim_start()
                    .strip_prefix(property)
                    .is_some_and(|suffix| suffix.starts_with('='))
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl SystemdIo for RealSystemd {
    fn read_property(&self, unit: &str, property: &str) -> io::Result<SystemdPropertyState> {
        let selector = format!("--property={property}");
        let value = Self::output(&["show", &selector, "--value", unit])?
            .trim()
            .to_string();
        Ok(SystemdPropertyState {
            explicit: Self::runtime_property_is_explicit(unit, property)?,
            value,
        })
    }

    fn set_property(&self, unit: &str, property: &str, value: Option<&str>) -> io::Result<()> {
        let assignment = format!("{property}={}", value.unwrap_or(""));
        let status = Command::new("systemctl")
            .args(["set-property", "--runtime", unit, &assignment])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("systemctl exited with {status}")))
        }
    }
}

pub(crate) struct Reconciler {
    state_dir: PathBuf,
    mode: ReconcileMode,
    targets: BTreeMap<String, TargetState>,
    previous_desired: BTreeSet<String>,
    last_ac: Option<bool>,
    last_workload: WorkloadClass,
    last_mode: Mode,
    last_domain_modes: HashMap<Domain, DomainMode>,
    systemd: Box<dyn SystemdIo>,
}

include!("state.rs");
include!("apply.rs");
include!("targets.rs");
include!("restore.rs");

#[cfg(test)]
mod tests;
