//! O1 read-only runtime-state observability.
//!
//! This module reads stable kernel interfaces only. It owns no actuation,
//! opens no write path, and keeps counter history explicit so tests and the
//! daemon can distinguish real deltas from counter resets or stale samples.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::kernel_io::{Clock, KernelRead};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuntimeObservabilityMode {
    Off,
    #[default]
    Observe,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeObservabilityConfig {
    #[serde(default)]
    pub(crate) mode: RuntimeObservabilityMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservabilityConfig {
    #[serde(default)]
    pub(crate) runtime: RuntimeObservabilityConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeObservationStatus {
    Observed,
    Unsupported,
    PermissionDenied,
    Malformed,
    Stale,
    Disabled,
}

impl Default for RuntimeObservationStatus {
    fn default() -> Self {
        Self::Unsupported
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CounterDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) delta: Option<u64>,
    pub(crate) reset_or_wrap: bool,
}

impl CounterDelta {
    fn between(current: u64, previous: Option<u64>, stale: bool) -> Self {
        if stale {
            return Self::default();
        }
        let Some(previous) = previous else {
            return Self::default();
        };
        if current >= previous {
            Self {
                delta: Some(current - previous),
                reset_or_wrap: false,
            }
        } else {
            // Counter width and reset provenance are not exposed uniformly by
            // these ABIs. Never turn a wrap/reset into a huge false delta.
            Self {
                delta: None,
                reset_or_wrap: true,
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WakeupSourceObservation {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: RuntimeObservationStatus,
    pub(crate) event_count: Option<u64>,
    pub(crate) event_delta: CounterDelta,
    pub(crate) active_count: Option<u64>,
    pub(crate) active_delta: CounterDelta,
    pub(crate) wakeup_count: Option<u64>,
    pub(crate) wakeup_delta: CounterDelta,
    pub(crate) total_time_us: Option<u64>,
    pub(crate) total_time_delta_us: CounterDelta,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuntimePmObservation {
    pub(crate) id: String,
    pub(crate) status: RuntimeObservationStatus,
    pub(crate) runtime_status: Option<String>,
    pub(crate) control: Option<String>,
    pub(crate) active_time_us: Option<u64>,
    pub(crate) active_time_delta_us: CounterDelta,
    pub(crate) suspended_time_us: Option<u64>,
    pub(crate) suspended_time_delta_us: CounterDelta,
    pub(crate) pm_qos_resume_latency_us: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CpuIdleObservation {
    pub(crate) cpu: u32,
    pub(crate) state: String,
    pub(crate) status: RuntimeObservationStatus,
    pub(crate) time_us: Option<u64>,
    pub(crate) time_delta_us: CounterDelta,
    pub(crate) usage: Option<u64>,
    pub(crate) usage_delta: CounterDelta,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmQosObservation {
    pub(crate) status: RuntimeObservationStatus,
    pub(crate) effective_cpu_latency_us: Option<i64>,
    pub(crate) requestor_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StorageObservation {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) status: RuntimeObservationStatus,
    pub(crate) state: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BacklightObservation {
    pub(crate) id: String,
    pub(crate) status: RuntimeObservationStatus,
    pub(crate) brightness: Option<u64>,
    pub(crate) actual_brightness: Option<u64>,
    pub(crate) max_brightness: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuntimeObservabilitySnapshot {
    pub(crate) mode: RuntimeObservabilityMode,
    pub(crate) status: RuntimeObservationStatus,
    pub(crate) collected_at: u64,
    pub(crate) reads_attempted: u64,
    pub(crate) wakeup_sources: Vec<WakeupSourceObservation>,
    pub(crate) runtime_pm: Vec<RuntimePmObservation>,
    pub(crate) cpu_idle: Vec<CpuIdleObservation>,
    pub(crate) pm_qos: PmQosObservation,
    pub(crate) storage: Vec<StorageObservation>,
    pub(crate) backlights: Vec<BacklightObservation>,
}

impl RuntimeObservabilitySnapshot {
    pub(crate) fn collect(
        read: &dyn KernelRead,
        clock: &dyn Clock,
        mode: RuntimeObservabilityMode,
        previous: Option<&Self>,
    ) -> Self {
        let collected_at = clock.now_unix();
        if mode == RuntimeObservabilityMode::Off {
            return Self {
                mode,
                status: RuntimeObservationStatus::Disabled,
                collected_at,
                pm_qos: PmQosObservation {
                    status: RuntimeObservationStatus::Disabled,
                    ..PmQosObservation::default()
                },
                ..Self::default()
            };
        }

        let stale = previous.is_some_and(|previous| collected_at <= previous.collected_at);
        let mut budget = ReadCounter::default();
        let mut snapshot = Self {
            mode,
            status: if stale {
                RuntimeObservationStatus::Stale
            } else {
                RuntimeObservationStatus::Observed
            },
            collected_at,
            wakeup_sources: collect_wakeup_sources(read, &mut budget, previous, stale),
            runtime_pm: collect_runtime_pm(read, &mut budget, previous, stale),
            cpu_idle: collect_cpu_idle(read, &mut budget, previous, stale),
            pm_qos: collect_pm_qos(read, &mut budget),
            storage: collect_storage(read, &mut budget, previous),
            backlights: collect_backlights(read, &mut budget, previous),
            ..Self::default()
        };
        snapshot.reads_attempted = budget.reads;
        snapshot
    }

    pub(crate) fn render_summary(&self) -> String {
        if self.mode == RuntimeObservabilityMode::Off {
            return "observability.runtime=off\n".to_string();
        }

        let mut out = format!(
            "observability.runtime=observe status={} reads={} wakeups={} runtime_pm={} cpu_idle={} storage={} backlights={}\n",
            status_name(self.status),
            self.reads_attempted,
            self.wakeup_sources.len(),
            self.runtime_pm.len(),
            self.cpu_idle.len(),
            self.storage.len(),
            self.backlights.len(),
        );
        out.push_str(&format!(
            "pm_qos.cpu_latency_us={} requestors={} status={}\n",
            self.pm_qos
                .effective_cpu_latency_us
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_string()),
            self.pm_qos.requestor_count,
            status_name(self.pm_qos.status),
        ));
        for device in &self.runtime_pm {
            out.push_str(&format!(
                "runtime_pm.{}={} control={} active_delta_us={} suspended_delta_us={} status={}\n",
                device.id,
                device.runtime_status.as_deref().unwrap_or("unavailable"),
                device.control.as_deref().unwrap_or("unavailable"),
                render_delta(device.active_time_delta_us),
                render_delta(device.suspended_time_delta_us),
                status_name(device.status),
            ));
        }
        for storage in &self.storage {
            out.push_str(&format!(
                "storage.{}.{}={} status={}\n",
                storage.kind,
                storage.id,
                storage.state.as_deref().unwrap_or("unavailable"),
                status_name(storage.status),
            ));
        }
        for backlight in &self.backlights {
            out.push_str(&format!(
                "backlight.{}={}/{} actual={} status={}\n",
                backlight.id,
                backlight
                    .brightness
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unavailable".to_string()),
                backlight
                    .max_brightness
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unavailable".to_string()),
                backlight
                    .actual_brightness
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unavailable".to_string()),
                status_name(backlight.status),
            ));
        }
        out
    }
}

fn status_name(status: RuntimeObservationStatus) -> &'static str {
    match status {
        RuntimeObservationStatus::Observed => "observed",
        RuntimeObservationStatus::Unsupported => "unsupported",
        RuntimeObservationStatus::PermissionDenied => "permission_denied",
        RuntimeObservationStatus::Malformed => "malformed",
        RuntimeObservationStatus::Stale => "stale",
        RuntimeObservationStatus::Disabled => "disabled",
    }
}

fn render_delta(delta: CounterDelta) -> String {
    if delta.reset_or_wrap {
        "reset_or_wrap".to_string()
    } else {
        delta
            .delta
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unavailable".to_string())
    }
}

#[derive(Default)]
struct ReadCounter {
    reads: u64,
}

impl ReadCounter {
    fn read_to_string(&mut self, read: &dyn KernelRead, path: &Path) -> io::Result<String> {
        self.reads = self.reads.saturating_add(1);
        read.read_to_string(path)
    }

    fn read_dir(&mut self, read: &dyn KernelRead, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.reads = self.reads.saturating_add(1);
        read.read_dir(path)
    }
}

fn status_from_error(error: &io::Error) -> RuntimeObservationStatus {
    match error.kind() {
        io::ErrorKind::NotFound => RuntimeObservationStatus::Unsupported,
        io::ErrorKind::PermissionDenied => RuntimeObservationStatus::PermissionDenied,
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => {
            RuntimeObservationStatus::Malformed
        }
        _ => RuntimeObservationStatus::Malformed,
    }
}

fn basename(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(sanitize_id)
        .unwrap_or_else(|| "unknown".to_string())
}

fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn parse_u64(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

fn parse_i64(text: &str) -> Option<i64> {
    text.trim().parse().ok()
}

fn read_u64(
    read: &dyn KernelRead,
    budget: &mut ReadCounter,
    path: &Path,
) -> Result<u64, RuntimeObservationStatus> {
    budget
        .read_to_string(read, path)
        .map_err(|error| status_from_error(&error))
        .and_then(|text| parse_u64(&text).ok_or(RuntimeObservationStatus::Malformed))
}

fn read_i64(
    read: &dyn KernelRead,
    budget: &mut ReadCounter,
    path: &Path,
) -> Result<i64, RuntimeObservationStatus> {
    budget
        .read_to_string(read, path)
        .map_err(|error| status_from_error(&error))
        .and_then(|text| parse_i64(&text).ok_or(RuntimeObservationStatus::Malformed))
}

fn read_text(
    read: &dyn KernelRead,
    budget: &mut ReadCounter,
    path: &Path,
) -> Result<String, RuntimeObservationStatus> {
    budget
        .read_to_string(read, path)
        .map(|value| value.trim().to_string())
        .map_err(|error| status_from_error(&error))
}

fn merge_status(current: RuntimeObservationStatus, next: RuntimeObservationStatus) -> RuntimeObservationStatus {
    use RuntimeObservationStatus::*;
    let rank = |status| match status {
        Observed => 0,
        Unsupported => 1,
        Stale => 2,
        Malformed => 3,
        PermissionDenied => 4,
        Disabled => 5,
    };
    if rank(next) > rank(current) { next } else { current }
}

fn previous_wakeup<'a>(previous: Option<&'a RuntimeObservabilitySnapshot>, id: &str) -> Option<&'a WakeupSourceObservation> {
    previous?.wakeup_sources.iter().find(|entry| entry.id == id)
}

fn collect_wakeup_sources(
    read: &dyn KernelRead,
    budget: &mut ReadCounter,
    previous: Option<&RuntimeObservabilitySnapshot>,
    stale: bool,
) -> Vec<WakeupSourceObservation> {
    let root = Path::new("/sys/class/wakeup");
    if !read.exists(root) {
        return previous
            .map(|previous| {
                previous
                    .wakeup_sources
                    .iter()
                    .cloned()
                    .map(|mut entry| {
                        entry.status = RuntimeObservationStatus::Stale;
                        entry.event_delta = CounterDelta::default();
                        entry.active_delta = CounterDelta::default();
                        entry.wakeup_delta = CounterDelta::default();
                        entry.total_time_delta_us = CounterDelta::default();
                        entry
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    let Ok(mut entries) = budget.read_dir(read, root) else {
        return Vec::new();
    };
    entries.sort();
    let mut observed = Vec::new();
    let mut present = BTreeSet::new();
    for path in entries {
        let id = basename(&path);
        present.insert(id.clone());
        let name = read_text(read, budget, &path.join("name")).unwrap_or_else(|_| id.clone());
        let mut status = RuntimeObservationStatus::Observed;
        let event_count = read_u64(read, budget, &path.join("event_count"))
            .map_err(|failure| status = merge_status(status, failure))
            .ok();
        let active_count = read_u64(read, budget, &path.join("active_count"))
            .map_err(|failure| status = merge_status(status, failure))
            .ok();
        let wakeup_count = read_u64(read, budget, &path.join("wakeup_count"))
            .map_err(|failure| status = merge_status(status, failure))
            .ok();
        let total_time_us = read_u64(read, budget, &path.join("total_time"))
            .map_err(|failure| status = merge_status(status, failure))
            .ok();
        let prior = previous_wakeup(previous, &id);
        observed.push(WakeupSourceObservation {
            id,
            name,
            status: if stale { RuntimeObservationStatus::Stale } else { status },
            event_count,
            event_delta: event_count
                .map(|value| CounterDelta::between(value, prior.and_then(|p| p.event_count), stale))
                .unwrap_or_default(),
            active_count,
            active_delta: active_count
                .map(|value| CounterDelta::between(value, prior.and_then(|p| p.active_count), stale))
                .unwrap_or_default(),
            wakeup_count,
            wakeup_delta: wakeup_count
                .map(|value| CounterDelta::between(value, prior.and_then(|p| p.wakeup_count), stale))
                .unwrap_or_default(),
            total_time_us,
            total_time_delta_us: total_time_us
                .map(|value| CounterDelta::between(value, prior.and_then(|p| p.total_time_us), stale))
                .unwrap_or_default(),
        });
    }
    if let Some(previous) = previous {
        for prior in &previous.wakeup_sources {
            if present.contains(&prior.id) {
                continue;
            }
            let mut vanished = prior.clone();
            vanished.status = RuntimeObservationStatus::Stale;
            vanished.event_delta = CounterDelta::default();
            vanished.active_delta = CounterDelta::default();
            vanished.wakeup_delta = CounterDelta::default();
            vanished.total_time_delta_us = CounterDelta::default();
            observed.push(vanished);
        }
    }
    observed.sort_by(|left, right| left.id.cmp(&right.id));
    observed
}

fn previous_runtime_pm<'a>(previous: Option<&'a RuntimeObservabilitySnapshot>, id: &str) -> Option<&'a RuntimePmObservation> {
    previous?.runtime_pm.iter().find(|entry| entry.id == id)
}

fn collect_runtime_pm(
    read: &dyn KernelRead,
    budget: &mut ReadCounter,
    previous: Option<&RuntimeObservabilitySnapshot>,
    stale: bool,
) -> Vec<RuntimePmObservation> {
    let mut observed = Vec::new();
    let mut present = BTreeSet::new();
    for bus in ["pci", "usb", "platform", "i2c", "hid"] {
        let root = PathBuf::from(format!("/sys/bus/{bus}/devices"));
        if !read.exists(&root) {
            continue;
        }
        let Ok(mut entries) = budget.read_dir(read, &root) else {
            continue;
        };
        entries.sort();
        for path in entries {
            let status_path = path.join("power/runtime_status");
            if !read.exists(&status_path) {
                continue;
            }
            let id = format!("{}:{}", bus, basename(&path));
            present.insert(id.clone());
            let mut status = RuntimeObservationStatus::Observed;
            let runtime_status = read_text(read, budget, &status_path)
                .map_err(|failure| status = merge_status(status, failure))
                .ok()
                .and_then(|value| {
                    if matches!(value.as_str(), "active" | "suspended" | "suspending" | "resuming" | "error") {
                        Some(value)
                    } else {
                        status = merge_status(status, RuntimeObservationStatus::Malformed);
                        None
                    }
                });
            let control = read_text(read, budget, &path.join("power/control"))
                .map_err(|failure| status = merge_status(status, failure))
                .ok();
            let active_time_us = read_u64(read, budget, &path.join("power/runtime_active_time"))
                .map_err(|failure| status = merge_status(status, failure))
                .ok();
            let suspended_time_us = read_u64(read, budget, &path.join("power/runtime_suspended_time"))
                .map_err(|failure| status = merge_status(status, failure))
                .ok();
            let pm_qos_path = path.join("power/pm_qos_resume_latency_us");
            let pm_qos_resume_latency_us = if read.exists(&pm_qos_path) {
                read_i64(read, budget, &pm_qos_path).ok()
            } else {
                None
            };
            let prior = previous_runtime_pm(previous, &id);
            observed.push(RuntimePmObservation {
                id,
                status: if stale { RuntimeObservationStatus::Stale } else { status },
                runtime_status,
                control,
                active_time_us,
                active_time_delta_us: active_time_us
                    .map(|value| CounterDelta::between(value, prior.and_then(|p| p.active_time_us), stale))
                    .unwrap_or_default(),
                suspended_time_us,
                suspended_time_delta_us: suspended_time_us
                    .map(|value| CounterDelta::between(value, prior.and_then(|p| p.suspended_time_us), stale))
                    .unwrap_or_default(),
                pm_qos_resume_latency_us,
            });
        }
    }
    if let Some(previous) = previous {
        for prior in &previous.runtime_pm {
            if present.contains(&prior.id) {
                continue;
            }
            let mut vanished = prior.clone();
            vanished.status = RuntimeObservationStatus::Stale;
            vanished.active_time_delta_us = CounterDelta::default();
            vanished.suspended_time_delta_us = CounterDelta::default();
            observed.push(vanished);
        }
    }
    observed.sort_by(|left, right| left.id.cmp(&right.id));
    observed
}

fn previous_cpu_idle<'a>(previous: Option<&'a RuntimeObservabilitySnapshot>, cpu: u32, state: &str) -> Option<&'a CpuIdleObservation> {
    previous?.cpu_idle.iter().find(|entry| entry.cpu == cpu && entry.state == state)
}

fn collect_cpu_idle(
    read: &dyn KernelRead,
    budget: &mut ReadCounter,
    previous: Option<&RuntimeObservabilitySnapshot>,
    stale: bool,
) -> Vec<CpuIdleObservation> {
    let root = Path::new("/sys/devices/system/cpu");
    if !read.exists(root) {
        return Vec::new();
    }
    let Ok(mut cpus) = budget.read_dir(read, root) else {
        return Vec::new();
    };
    cpus.sort();
    let mut observed = Vec::new();
    let mut present = BTreeSet::new();
    for cpu_path in cpus {
        let cpu_name = basename(&cpu_path);
        let Some(cpu) = cpu_name.strip_prefix("cpu").and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let idle_root = cpu_path.join("cpuidle");
        if !read.exists(&idle_root) {
            continue;
        }
        let Ok(mut states) = budget.read_dir(read, &idle_root) else {
            continue;
        };
        states.sort();
        for state_path in states {
            let mut status = RuntimeObservationStatus::Observed;
            let state = read_text(read, budget, &state_path.join("name"))
                .map(sanitize_id)
                .map_err(|failure| status = merge_status(status, failure))
                .unwrap_or_else(|_| basename(&state_path));
            present.insert((cpu, state.clone()));
            let time_us = read_u64(read, budget, &state_path.join("time"))
                .map_err(|failure| status = merge_status(status, failure))
                .ok();
            let usage = read_u64(read, budget, &state_path.join("usage"))
                .map_err(|failure| status = merge_status(status, failure))
                .ok();
            let prior = previous_cpu_idle(previous, cpu, &state);
            observed.push(CpuIdleObservation {
                cpu,
                state,
                status: if stale { RuntimeObservationStatus::Stale } else { status },
                time_us,
                time_delta_us: time_us
                    .map(|value| CounterDelta::between(value, prior.and_then(|p| p.time_us), stale))
                    .unwrap_or_default(),
                usage,
                usage_delta: usage
                    .map(|value| CounterDelta::between(value, prior.and_then(|p| p.usage), stale))
                    .unwrap_or_default(),
            });
        }
    }
    if let Some(previous) = previous {
        for prior in &previous.cpu_idle {
            if present.contains(&(prior.cpu, prior.state.clone())) {
                continue;
            }
            let mut vanished = prior.clone();
            vanished.status = RuntimeObservationStatus::Stale;
            vanished.time_delta_us = CounterDelta::default();
            vanished.usage_delta = CounterDelta::default();
            observed.push(vanished);
        }
    }
    observed.sort_by(|left, right| (left.cpu, &left.state).cmp(&(right.cpu, &right.state)));
    observed
}

fn collect_pm_qos(read: &dyn KernelRead, budget: &mut ReadCounter) -> PmQosObservation {
    let path = Path::new("/sys/kernel/debug/pm_qos/cpu_latency_constraints");
    if !read.exists(path) {
        return PmQosObservation {
            status: RuntimeObservationStatus::Unsupported,
            ..PmQosObservation::default()
        };
    }
    let text = match budget.read_to_string(read, path) {
        Ok(text) => text,
        Err(error) => {
            return PmQosObservation {
                status: status_from_error(&error),
                ..PmQosObservation::default()
            }
        }
    };
    let mut values = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some(value) = line
            .split_whitespace()
            .rev()
            .find_map(|token| token.trim_end_matches(',').parse::<i64>().ok())
        else {
            return PmQosObservation {
                status: RuntimeObservationStatus::Malformed,
                ..PmQosObservation::default()
            };
        };
        values.push(value);
    }
    PmQosObservation {
        status: RuntimeObservationStatus::Observed,
        effective_cpu_latency_us: values.iter().copied().min(),
        requestor_count: values.len(),
    }
}

fn collect_storage(
    read: &dyn KernelRead,
    budget: &mut ReadCounter,
    previous: Option<&RuntimeObservabilitySnapshot>,
) -> Vec<StorageObservation> {
    let mut observed = Vec::new();
    let mut present = BTreeSet::new();

    let sata_root = Path::new("/sys/class/scsi_host");
    if read.exists(sata_root) {
        if let Ok(mut hosts) = budget.read_dir(read, sata_root) {
            hosts.sort();
            for host in hosts {
                let state_path = host.join("link_power_management_policy");
                if !read.exists(&state_path) {
                    continue;
                }
                let id = basename(&host);
                present.insert(("sata_alpm".to_string(), id.clone()));
                let (state, status) = match read_text(read, budget, &state_path) {
                    Ok(value) => (Some(value), RuntimeObservationStatus::Observed),
                    Err(status) => (None, status),
                };
                observed.push(StorageObservation {
                    id,
                    kind: "sata_alpm".to_string(),
                    status,
                    state,
                });
            }
        }
    }

    let pci_root = Path::new("/sys/bus/pci/devices");
    if read.exists(pci_root) {
        if let Ok(mut devices) = budget.read_dir(read, pci_root) {
            devices.sort();
            for device in devices {
                let state_path = device.join("link/l1_aspm");
                if !read.exists(&state_path) {
                    continue;
                }
                let id = basename(&device);
                present.insert(("pcie_aspm".to_string(), id.clone()));
                let (state, status) = match read_text(read, budget, &state_path) {
                    Ok(value) => (Some(value), RuntimeObservationStatus::Observed),
                    Err(status) => (None, status),
                };
                observed.push(StorageObservation {
                    id,
                    kind: "pcie_aspm".to_string(),
                    status,
                    state,
                });
            }
        }
    }

    let nvme_root = Path::new("/sys/class/nvme");
    if read.exists(nvme_root) {
        if let Ok(mut controllers) = budget.read_dir(read, nvme_root) {
            controllers.sort();
            for controller in controllers {
                let direct = controller.join("power/runtime_status");
                let device = controller.join("device/power/runtime_status");
                let state_path = if read.exists(&direct) {
                    Some(direct)
                } else if read.exists(&device) {
                    Some(device)
                } else {
                    None
                };
                let Some(state_path) = state_path else { continue };
                let id = basename(&controller);
                present.insert(("nvme_runtime".to_string(), id.clone()));
                let (state, status) = match read_text(read, budget, &state_path) {
                    Ok(value) => (Some(value), RuntimeObservationStatus::Observed),
                    Err(status) => (None, status),
                };
                observed.push(StorageObservation {
                    id,
                    kind: "nvme_runtime".to_string(),
                    status,
                    state,
                });
            }
        }
    }

    if let Some(previous) = previous {
        for prior in &previous.storage {
            if present.contains(&(prior.kind.clone(), prior.id.clone())) {
                continue;
            }
            let mut vanished = prior.clone();
            vanished.status = RuntimeObservationStatus::Stale;
            observed.push(vanished);
        }
    }
    observed.sort_by(|left, right| (&left.kind, &left.id).cmp(&(&right.kind, &right.id)));
    observed
}

fn collect_backlights(
    read: &dyn KernelRead,
    budget: &mut ReadCounter,
    previous: Option<&RuntimeObservabilitySnapshot>,
) -> Vec<BacklightObservation> {
    let root = Path::new("/sys/class/backlight");
    if !read.exists(root) {
        return previous
            .map(|previous| {
                previous
                    .backlights
                    .iter()
                    .cloned()
                    .map(|mut entry| {
                        entry.status = RuntimeObservationStatus::Stale;
                        entry
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    let Ok(mut entries) = budget.read_dir(read, root) else {
        return Vec::new();
    };
    entries.sort();
    let mut observed = Vec::new();
    let mut present = BTreeSet::new();
    for path in entries {
        let id = basename(&path);
        present.insert(id.clone());
        let mut status = RuntimeObservationStatus::Observed;
        let brightness = read_u64(read, budget, &path.join("brightness"))
            .map_err(|failure| status = merge_status(status, failure))
            .ok();
        let actual_path = path.join("actual_brightness");
        let actual_brightness = if read.exists(&actual_path) {
            read_u64(read, budget, &actual_path)
                .map_err(|failure| status = merge_status(status, failure))
                .ok()
        } else {
            brightness
        };
        let max_brightness = read_u64(read, budget, &path.join("max_brightness"))
            .map_err(|failure| status = merge_status(status, failure))
            .ok();
        observed.push(BacklightObservation {
            id,
            status,
            brightness,
            actual_brightness,
            max_brightness,
        });
    }
    if let Some(previous) = previous {
        for prior in &previous.backlights {
            if present.contains(&prior.id) {
                continue;
            }
            let mut vanished = prior.clone();
            vanished.status = RuntimeObservationStatus::Stale;
            observed.push(vanished);
        }
    }
    observed.sort_by(|left, right| left.id.cmp(&right.id));
    observed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel_io::{FaultKernel, MemoryKernel};

    fn add_file(kernel: &MemoryKernel, path: &str, value: &str) {
        kernel.write_raw(Path::new(path), value);
    }

    fn add_dir(kernel: &MemoryKernel, directory: &str, entry: &str) {
        kernel.add_dir_entry(Path::new(directory), Path::new(entry));
    }

    fn full_fixture() -> MemoryKernel {
        let kernel = MemoryKernel::new();
        add_dir(&kernel, "/sys/class/wakeup", "/sys/class/wakeup/wakeup0");
        add_file(&kernel, "/sys/class/wakeup/wakeup0/name", "XHC\n");
        add_file(&kernel, "/sys/class/wakeup/wakeup0/event_count", "10\n");
        add_file(&kernel, "/sys/class/wakeup/wakeup0/active_count", "2\n");
        add_file(&kernel, "/sys/class/wakeup/wakeup0/wakeup_count", "3\n");
        add_file(&kernel, "/sys/class/wakeup/wakeup0/total_time", "1200\n");

        add_dir(&kernel, "/sys/bus/pci/devices", "/sys/bus/pci/devices/0000:00:1f.0");
        add_file(&kernel, "/sys/bus/pci/devices/0000:00:1f.0/power/runtime_status", "suspended\n");
        add_file(&kernel, "/sys/bus/pci/devices/0000:00:1f.0/power/control", "auto\n");
        add_file(&kernel, "/sys/bus/pci/devices/0000:00:1f.0/power/runtime_active_time", "200\n");
        add_file(&kernel, "/sys/bus/pci/devices/0000:00:1f.0/power/runtime_suspended_time", "800\n");
        add_file(&kernel, "/sys/bus/pci/devices/0000:00:1f.0/power/pm_qos_resume_latency_us", "5000\n");
        add_file(&kernel, "/sys/bus/pci/devices/0000:00:1f.0/link/l1_aspm", "1\n");

        add_dir(&kernel, "/sys/devices/system/cpu", "/sys/devices/system/cpu/cpu0");
        add_dir(&kernel, "/sys/devices/system/cpu/cpu0/cpuidle", "/sys/devices/system/cpu/cpu0/cpuidle/state0");
        add_file(&kernel, "/sys/devices/system/cpu/cpu0/cpuidle/state0/name", "C6\n");
        add_file(&kernel, "/sys/devices/system/cpu/cpu0/cpuidle/state0/time", "1000\n");
        add_file(&kernel, "/sys/devices/system/cpu/cpu0/cpuidle/state0/usage", "20\n");

        add_file(&kernel, "/sys/kernel/debug/pm_qos/cpu_latency_constraints", "101 audio 100\n202 game 50\n");

        add_dir(&kernel, "/sys/class/scsi_host", "/sys/class/scsi_host/host0");
        add_file(&kernel, "/sys/class/scsi_host/host0/link_power_management_policy", "med_power_with_dipm\n");

        add_dir(&kernel, "/sys/class/nvme", "/sys/class/nvme/nvme0");
        add_file(&kernel, "/sys/class/nvme/nvme0/device/power/runtime_status", "suspended\n");

        add_dir(&kernel, "/sys/class/backlight", "/sys/class/backlight/intel_backlight");
        add_file(&kernel, "/sys/class/backlight/intel_backlight/brightness", "400\n");
        add_file(&kernel, "/sys/class/backlight/intel_backlight/actual_brightness", "390\n");
        add_file(&kernel, "/sys/class/backlight/intel_backlight/max_brightness", "1000\n");
        kernel
    }

    #[test]
    fn o1_runtime_mode_defaults_to_observe_and_parses_off() {
        assert_eq!(RuntimeObservabilityConfig::default().mode, RuntimeObservabilityMode::Observe);
        let parsed: RuntimeObservabilityConfig = toml::from_str("mode = \"off\"").unwrap();
        assert_eq!(parsed.mode, RuntimeObservabilityMode::Off);
    }

    #[test]
    fn o1_mock_full_state_snapshot_is_deterministic() {
        let kernel = full_fixture();
        let left = RuntimeObservabilitySnapshot::collect(&kernel, &kernel, RuntimeObservabilityMode::Observe, None);
        let right = RuntimeObservabilitySnapshot::collect(&kernel, &kernel, RuntimeObservabilityMode::Observe, None);
        assert_eq!(left, right);
        assert_eq!(left.wakeup_sources.len(), 1);
        assert_eq!(left.runtime_pm.len(), 1);
        assert_eq!(left.cpu_idle.len(), 1);
        assert_eq!(left.pm_qos.effective_cpu_latency_us, Some(50));
        assert_eq!(left.storage.len(), 3);
        assert_eq!(left.backlights.len(), 1);
    }

    #[test]
    fn o1_counter_reset_or_wrap_never_becomes_a_huge_delta() {
        let first = full_fixture();
        let previous = RuntimeObservabilitySnapshot::collect(&first, &first, RuntimeObservabilityMode::Observe, None);
        let second = full_fixture();
        second.advance_clock(2);
        add_file(&second, "/sys/class/wakeup/wakeup0/event_count", "1\n");
        add_file(&second, "/sys/devices/system/cpu/cpu0/cpuidle/state0/time", "5\n");
        let current = RuntimeObservabilitySnapshot::collect(&second, &second, RuntimeObservabilityMode::Observe, Some(&previous));
        assert!(current.wakeup_sources[0].event_delta.reset_or_wrap);
        assert_eq!(current.wakeup_sources[0].event_delta.delta, None);
        assert!(current.cpu_idle[0].time_delta_us.reset_or_wrap);
        assert_eq!(current.cpu_idle[0].time_delta_us.delta, None);
    }

    #[test]
    fn o1_monotonic_counters_produce_real_deltas() {
        let first = full_fixture();
        let previous = RuntimeObservabilitySnapshot::collect(&first, &first, RuntimeObservabilityMode::Observe, None);
        let second = full_fixture();
        second.advance_clock(2);
        add_file(&second, "/sys/class/wakeup/wakeup0/event_count", "14\n");
        add_file(&second, "/sys/bus/pci/devices/0000:00:1f.0/power/runtime_suspended_time", "900\n");
        let current = RuntimeObservabilitySnapshot::collect(&second, &second, RuntimeObservabilityMode::Observe, Some(&previous));
        assert_eq!(current.wakeup_sources[0].event_delta.delta, Some(4));
        assert_eq!(current.runtime_pm[0].suspended_time_delta_us.delta, Some(100));
    }

    #[test]
    fn o1_disappearing_devices_are_reported_stale() {
        let first = full_fixture();
        let previous = RuntimeObservabilitySnapshot::collect(&first, &first, RuntimeObservabilityMode::Observe, None);
        let second = MemoryKernel::new();
        second.advance_clock(2);
        add_dir(&second, "/sys/class/wakeup", "/sys/class/wakeup/wakeup1");
        add_file(&second, "/sys/class/wakeup/wakeup1/name", "NEW\n");
        add_file(&second, "/sys/class/wakeup/wakeup1/event_count", "1\n");
        add_file(&second, "/sys/class/wakeup/wakeup1/active_count", "0\n");
        add_file(&second, "/sys/class/wakeup/wakeup1/wakeup_count", "0\n");
        add_file(&second, "/sys/class/wakeup/wakeup1/total_time", "0\n");
        let current = RuntimeObservabilitySnapshot::collect(&second, &second, RuntimeObservabilityMode::Observe, Some(&previous));
        assert!(current.wakeup_sources.iter().any(|entry| entry.id == "wakeup0" && entry.status == RuntimeObservationStatus::Stale));
        assert!(current.runtime_pm.iter().any(|entry| entry.status == RuntimeObservationStatus::Stale));
        assert!(current.storage.iter().any(|entry| entry.status == RuntimeObservationStatus::Stale));
        assert!(current.backlights.iter().any(|entry| entry.status == RuntimeObservationStatus::Stale));
    }

    #[test]
    fn o1_permission_error_is_distinct_from_unsupported_and_malformed() {
        let memory = full_fixture();
        let fault = FaultKernel::new(Box::new(memory));
        fault.fail_next_read(
            PathBuf::from("/sys/class/wakeup/wakeup0/event_count"),
            io::ErrorKind::PermissionDenied,
        );
        let snapshot = RuntimeObservabilitySnapshot::collect(&fault, &fault, RuntimeObservabilityMode::Observe, None);
        assert_eq!(snapshot.wakeup_sources[0].status, RuntimeObservationStatus::PermissionDenied);

        let empty = MemoryKernel::new();
        let unsupported = RuntimeObservabilitySnapshot::collect(&empty, &empty, RuntimeObservabilityMode::Observe, None);
        assert_eq!(unsupported.pm_qos.status, RuntimeObservationStatus::Unsupported);

        let malformed = full_fixture();
        add_file(&malformed, "/sys/class/backlight/intel_backlight/brightness", "not-a-number\n");
        let malformed = RuntimeObservabilitySnapshot::collect(&malformed, &malformed, RuntimeObservabilityMode::Observe, None);
        assert_eq!(malformed.backlights[0].status, RuntimeObservationStatus::Malformed);
    }

    #[test]
    fn o1_nonadvancing_clock_marks_snapshot_stale_and_suppresses_deltas() {
        let first = full_fixture();
        let previous = RuntimeObservabilitySnapshot::collect(&first, &first, RuntimeObservabilityMode::Observe, None);
        let second = full_fixture();
        add_file(&second, "/sys/class/wakeup/wakeup0/event_count", "20\n");
        let current = RuntimeObservabilitySnapshot::collect(&second, &second, RuntimeObservabilityMode::Observe, Some(&previous));
        assert_eq!(current.status, RuntimeObservationStatus::Stale);
        assert_eq!(current.wakeup_sources[0].event_delta.delta, None);
    }

    #[test]
    fn o1_off_mode_performs_no_runtime_reads() {
        let kernel = full_fixture();
        let snapshot = RuntimeObservabilitySnapshot::collect(&kernel, &kernel, RuntimeObservabilityMode::Off, None);
        assert_eq!(snapshot.status, RuntimeObservationStatus::Disabled);
        assert_eq!(snapshot.reads_attempted, 0);
        assert!(snapshot.runtime_pm.is_empty());
        assert!(snapshot.wakeup_sources.is_empty());
    }
}
