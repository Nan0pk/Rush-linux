//! WP-N3 focused core: one-shot, read-only energy and sleep diagnostics.
//!
//! The long-running telemetry design lives in research 0018. This module is the
//! deliberately smaller public front door: it reads stable sysfs attributes once,
//! explains likely blockers, and never writes policy or device state.

use serde::Serialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
pub(crate) struct DoctorReport {
    generated_at_unix: u64,
    status: ReportStatus,
    coverage: Coverage,
    summary: Summary,
    findings: Vec<Finding>,
    wakeup_sources: Vec<WakeupSource>,
    runtime_pm_devices: Vec<RuntimePmDevice>,
    changed_settings: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ReportStatus {
    Healthy,
    NeedsAttention,
    LimitedVisibility,
}

impl ReportStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::NeedsAttention => "needs attention",
            Self::LimitedVisibility => "limited visibility",
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct Coverage {
    wakeup_sources_available: bool,
    runtime_pm_available: bool,
    notes: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    wakeup_source_count: usize,
    wakeup_sources_active_now: usize,
    runtime_pm_device_count: usize,
    runtime_pm_suspended: usize,
    runtime_pm_active: usize,
    runtime_pm_errors: usize,
    runtime_pm_forced_on_idle: usize,
}

#[derive(Debug, Serialize)]
struct WakeupSource {
    name: String,
    active_count: Option<u64>,
    event_count: Option<u64>,
    wakeup_count: Option<u64>,
    active_since: Option<u64>,
    prevent_suspend_time: Option<u64>,
}

impl WakeupSource {
    fn active_now(&self) -> bool {
        self.active_since.is_some_and(|value| value > 0)
    }
}

#[derive(Debug, Serialize)]
struct RuntimePmDevice {
    bus: String,
    device: String,
    status: String,
    control: Option<String>,
    runtime_usage: Option<i64>,
    runtime_active_time_us: Option<u64>,
    runtime_suspended_time_us: Option<u64>,
    autosuspend_delay_ms: Option<i64>,
    wakeup: Option<String>,
}

impl RuntimePmDevice {
    fn subject(&self) -> String {
        format!("{}/{}", self.bus, self.device)
    }

    fn forced_on_while_idle(&self) -> bool {
        self.control.as_deref() == Some("on") && self.runtime_usage == Some(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Severity {
    Critical,
    Warning,
    Info,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Serialize)]
struct Finding {
    severity: Severity,
    kind: &'static str,
    subject: String,
    detail: String,
    recommendation: String,
}

pub(crate) fn run(sysfs_root: &Path, json: bool) -> io::Result<()> {
    let report = collect(sysfs_root);
    if json {
        let output = serde_json::to_string_pretty(&report)
            .map_err(|err| io::Error::other(format!("serialize doctor report: {err}")))?;
        println!("{output}");
    } else {
        let output = report.render_text();
        print!("{output}");
    }
    Ok(())
}

fn collect(sysfs_root: &Path) -> DoctorReport {
    let (mut wakeup_sources, wakeup_note) = collect_wakeup_sources(sysfs_root);
    let (mut runtime_pm_devices, runtime_pm_note) = collect_runtime_pm_devices(sysfs_root);

    wakeup_sources.sort_by(|left, right| {
        right
            .prevent_suspend_time
            .unwrap_or(0)
            .cmp(&left.prevent_suspend_time.unwrap_or(0))
            .then_with(|| {
                right
                    .event_count
                    .unwrap_or(0)
                    .cmp(&left.event_count.unwrap_or(0))
            })
            .then_with(|| left.name.cmp(&right.name))
    });
    runtime_pm_devices.sort_by(|left, right| {
        left.bus
            .cmp(&right.bus)
            .then_with(|| left.device.cmp(&right.device))
    });

    let mut coverage = Coverage {
        wakeup_sources_available: wakeup_note.is_none(),
        runtime_pm_available: runtime_pm_note.is_none(),
        notes: Vec::new(),
    };
    coverage.notes.extend(wakeup_note);
    coverage.notes.extend(runtime_pm_note);

    let summary = Summary {
        wakeup_source_count: wakeup_sources.len(),
        wakeup_sources_active_now: wakeup_sources
            .iter()
            .filter(|source| source.active_now())
            .count(),
        runtime_pm_device_count: runtime_pm_devices.len(),
        runtime_pm_suspended: runtime_pm_devices
            .iter()
            .filter(|device| device.status == "suspended")
            .count(),
        runtime_pm_active: runtime_pm_devices
            .iter()
            .filter(|device| device.status == "active")
            .count(),
        runtime_pm_errors: runtime_pm_devices
            .iter()
            .filter(|device| device.status == "error")
            .count(),
        runtime_pm_forced_on_idle: runtime_pm_devices
            .iter()
            .filter(|device| device.forced_on_while_idle())
            .count(),
    };

    let mut findings = build_findings(&wakeup_sources, &runtime_pm_devices);
    findings.sort_by_key(|finding| match finding.severity {
        Severity::Critical => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    });

    let status = if findings
        .iter()
        .any(|finding| finding.severity != Severity::Info)
    {
        ReportStatus::NeedsAttention
    } else if !coverage.wakeup_sources_available && !coverage.runtime_pm_available {
        ReportStatus::LimitedVisibility
    } else {
        ReportStatus::Healthy
    };

    DoctorReport {
        generated_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        status,
        coverage,
        summary,
        findings,
        wakeup_sources,
        runtime_pm_devices,
        changed_settings: false,
    }
}

fn collect_wakeup_sources(sysfs_root: &Path) -> (Vec<WakeupSource>, Option<String>) {
    let root = sysfs_root.join("class/wakeup");
    let Ok(entries) = fs::read_dir(&root) else {
        return (
            Vec::new(),
            Some(format!(
                "wakeup-source telemetry unavailable at {}",
                root.display()
            )),
        );
    };

    let sources: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            WakeupSource {
                name: read_trimmed(path.join("name"))
                    .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned()),
                active_count: read_u64(path.join("active_count")),
                event_count: read_u64(path.join("event_count")),
                wakeup_count: read_u64(path.join("wakeup_count")),
                active_since: read_u64(path.join("active_since")),
                prevent_suspend_time: read_u64(path.join("prevent_suspend_time")),
            }
        })
        .collect();
    if sources.is_empty() {
        (
            sources,
            Some(format!(
                "no wakeup sources were exposed under {}",
                root.display()
            )),
        )
    } else {
        (sources, None)
    }
}

fn collect_runtime_pm_devices(sysfs_root: &Path) -> (Vec<RuntimePmDevice>, Option<String>) {
    let bus_root = sysfs_root.join("bus");
    let Ok(buses) = fs::read_dir(&bus_root) else {
        return (
            Vec::new(),
            Some(format!(
                "runtime-PM telemetry unavailable at {}",
                bus_root.display()
            )),
        );
    };

    let mut devices = Vec::new();
    for bus in buses.filter_map(Result::ok) {
        let bus_name = bus.file_name().to_string_lossy().into_owned();
        let Ok(entries) = fs::read_dir(bus.path().join("devices")) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let power = entry.path().join("power");
            let Some(status) = read_trimmed(power.join("runtime_status")) else {
                continue;
            };
            devices.push(RuntimePmDevice {
                bus: bus_name.clone(),
                device: entry.file_name().to_string_lossy().into_owned(),
                status,
                control: read_trimmed(power.join("control")),
                runtime_usage: read_i64(power.join("runtime_usage")),
                runtime_active_time_us: read_u64(power.join("runtime_active_time")),
                runtime_suspended_time_us: read_u64(power.join("runtime_suspended_time")),
                autosuspend_delay_ms: read_i64(power.join("autosuspend_delay_ms")),
                wakeup: read_trimmed(power.join("wakeup")),
            });
        }
    }
    if devices.is_empty() {
        (
            devices,
            Some(format!(
                "no runtime-PM devices were exposed under {}",
                bus_root.display()
            )),
        )
    } else {
        (devices, None)
    }
}

fn build_findings(
    wakeup_sources: &[WakeupSource],
    runtime_pm_devices: &[RuntimePmDevice],
) -> Vec<Finding> {
    let mut findings = Vec::new();

    for source in wakeup_sources.iter().filter(|source| source.active_now()) {
        findings.push(Finding {
            severity: Severity::Warning,
            kind: "wakeup-source",
            subject: source.name.clone(),
            detail: format!(
                "currently active; cumulative prevent-suspend time is {}",
                source.prevent_suspend_time.unwrap_or(0)
            ),
            recommendation: "Identify the owning driver or service before changing wake policy."
                .to_string(),
        });
    }

    for device in runtime_pm_devices
        .iter()
        .filter(|device| device.status == "error")
    {
        findings.push(Finding {
            severity: Severity::Critical,
            kind: "runtime-pm",
            subject: device.subject(),
            detail: "the kernel reports a runtime-PM error".to_string(),
            recommendation:
                "Keep this device out of automatic power transitions until the driver failure is understood."
                    .to_string(),
        });
    }

    for device in runtime_pm_devices
        .iter()
        .filter(|device| device.forced_on_while_idle())
        .take(10)
    {
        findings.push(Finding {
            severity: Severity::Info,
            kind: "runtime-pm-opportunity",
            subject: device.subject(),
            detail: "power/control=on while runtime_usage=0".to_string(),
            recommendation: "Review device and driver safety evidence before enabling runtime PM."
                .to_string(),
        });
    }

    findings
}

impl DoctorReport {
    fn render_text(&self) -> String {
        let mut output = String::new();
        output.push_str("Rush Doctor — read-only energy & sleep diagnosis\n");
        output.push_str(&format!("Status: {}\n\n", self.status.label()));
        output.push_str(&format!(
            "Wakeup sources: {} observed, {} active now\n",
            self.summary.wakeup_source_count, self.summary.wakeup_sources_active_now
        ));
        output.push_str(&format!(
            "Runtime PM: {} devices, {} suspended, {} active, {} errors, {} forced on while idle\n",
            self.summary.runtime_pm_device_count,
            self.summary.runtime_pm_suspended,
            self.summary.runtime_pm_active,
            self.summary.runtime_pm_errors,
            self.summary.runtime_pm_forced_on_idle
        ));

        if self.findings.is_empty() {
            output.push_str("\nNo immediate blockers were visible in the available sysfs data.\n");
        } else {
            output.push_str("\nFindings:\n");
            for finding in self.findings.iter().take(15) {
                output.push_str(&format!(
                    "- [{}] {}: {}\n  Next: {}\n",
                    finding.severity.label(),
                    finding.subject,
                    finding.detail,
                    finding.recommendation
                ));
            }
            if self.findings.len() > 15 {
                output.push_str(&format!(
                    "- {} additional informational findings are available with --json.\n",
                    self.findings.len() - 15
                ));
            }
        }

        if !self.coverage.notes.is_empty() {
            output.push_str("\nVisibility notes:\n");
            for note in &self.coverage.notes {
                output.push_str(&format!("- {note}\n"));
            }
        }

        output.push_str("\nNo settings were changed.\n");
        output
    }
}

fn read_trimmed(path: PathBuf) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
}

fn read_u64(path: PathBuf) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

fn read_i64(path: PathBuf) -> Option<i64> {
    read_trimmed(path)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rush_doctor_{name}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(path: impl AsRef<Path>, value: &str) {
        let path = path.as_ref();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }

    #[test]
    fn reports_active_wakeup_and_runtime_pm_error() {
        let root = fixture_root("attention");
        let wake = root.join("class/wakeup/wakeup0");
        write(wake.join("name"), "XHC\n");
        write(wake.join("active_count"), "4\n");
        write(wake.join("event_count"), "12\n");
        write(wake.join("wakeup_count"), "3\n");
        write(wake.join("active_since"), "42\n");
        write(wake.join("prevent_suspend_time"), "9000\n");

        let power = root.join("bus/pci/devices/0000:00:14.0/power");
        write(power.join("runtime_status"), "error\n");
        write(power.join("control"), "auto\n");
        write(power.join("runtime_usage"), "0\n");

        let report = collect(&root);
        assert_eq!(report.status, ReportStatus::NeedsAttention);
        assert_eq!(report.summary.wakeup_sources_active_now, 1);
        assert_eq!(report.summary.runtime_pm_errors, 1);
        assert_eq!(report.findings[0].severity, Severity::Critical);
        assert!(report.render_text().contains("No settings were changed."));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_forced_on_idle_as_information_not_failure() {
        let root = fixture_root("opportunity");
        fs::create_dir_all(root.join("class/wakeup")).unwrap();
        let power = root.join("bus/usb/devices/1-1/power");
        write(power.join("runtime_status"), "active\n");
        write(power.join("control"), "on\n");
        write(power.join("runtime_usage"), "0\n");

        let report = collect(&root);
        assert_eq!(report.status, ReportStatus::Healthy);
        assert_eq!(report.summary.runtime_pm_forced_on_idle, 1);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].severity, Severity::Info);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn degrades_cleanly_when_sysfs_is_unavailable() {
        let root = fixture_root("missing");
        let report = collect(&root);
        assert_eq!(report.status, ReportStatus::LimitedVisibility);
        assert_eq!(report.coverage.notes.len(), 2);
        assert!(!report.changed_settings);
        let _ = fs::remove_dir_all(root);
    }
}
