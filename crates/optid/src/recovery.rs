//! S3D independent, one-shot recovery for S2D transaction records.
//!
//! This module deliberately contains no policy, classifier, D-Bus, session,
//! or async-runtime surface. It reads the closed S2D record schema, validates
//! canonical identity, restores only values still attributable to optid, and
//! records a durable outcome before compacting a resolved record.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "systemd_placeholder.rs"]
mod systemd_placeholder;
use systemd_placeholder::{assigns_a_value, is_unset_placeholder};

pub const DEFAULT_RECOVERY_DIR: &str = "/var/lib/optid/recovery";
pub const RECOVERY_FAILURE_EXIT: i32 = 78;
const TRANSACTION_SCHEMA_VERSION: u32 = 1;
const OUTCOME_LOG: &str = "recovery-outcomes.jsonl";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionPhase {
    Prepared,
    Committed,
    Compensating,
    Compensated,
    Relinquished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransactionRecord {
    schema_version: u32,
    generation: String,
    owner: String,
    domain: String,
    operation: Value,
    target_id: String,
    canonical_identity: String,
    target: TargetKind,
    #[serde(default)]
    legacy_journal_key: Option<String>,
    original: StoredValue,
    intended: StoredValue,
    rollback_method: String,
    stabilization_method: String,
    phase: TransactionPhase,
    created_at_unix: u64,
    updated_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDisposition {
    Restored,
    AlreadyRestored,
    RelinquishedDrift,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryEvent {
    pub target_id: String,
    pub disposition: RecoveryDisposition,
    pub detail: String,
    pub timestamp_unix: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecoverySummary {
    pub scanned: usize,
    pub restored: usize,
    pub already_restored: usize,
    pub relinquished: usize,
    pub failed: usize,
    pub events: Vec<RecoveryEvent>,
}

impl RecoverySummary {
    pub fn succeeded(&self) -> bool {
        self.failed == 0
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn read_trimmed(path: &Path) -> io::Result<String> {
    fs::read_to_string(path).map(|value| value.trim().to_string())
}

fn write_value(path: &Path, value: &str) -> io::Result<()> {
    fs::write(path, format!("{value}\n"))
}

fn systemd_output(args: &[&str]) -> io::Result<String> {
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

/// Normalize a stored systemd value so the placeholder and an empty value are
/// one thing. Applied to records on load, and matched by `read_target`, so a
/// comparison never turns on which build wrote the record.
fn canonicalize_stored(value: &mut StoredValue) {
    if let StoredValue::Systemd {
        explicit,
        value: text,
    } = value
    {
        if is_unset_placeholder(text) {
            *explicit = false;
            text.clear();
        }
    }
}

fn systemd_property_is_explicit(unit: &str, property: &str) -> io::Result<bool> {
    let paths = systemd_output(&["show", "--property=DropInPaths", "--value", unit])?;
    for raw in paths.split_whitespace() {
        let path = raw.trim_matches('"');
        if !path.starts_with("/run/systemd/system.control/") {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        if content.lines().any(|line| assigns_a_value(line, property)) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_target(target: &TargetKind) -> io::Result<StoredValue> {
    match target {
        TargetKind::KernelValue { path } | TargetKind::PmqosDevice { path } => {
            Ok(StoredValue::Scalar {
                value: read_trimmed(path)?,
            })
        }
        TargetKind::PmqosCpu => Ok(StoredValue::Scalar {
            value: "unconstrained".to_string(),
        }),
        TargetKind::RuntimePm {
            control_path,
            delay_path,
        } => Ok(StoredValue::RuntimePm {
            control: read_trimmed(control_path)?,
            delay: delay_path
                .as_ref()
                .map(|path| read_trimmed(path))
                .transpose()?,
        }),
        TargetKind::SystemdProperty { unit, property } => {
            let selector = format!("--property={property}");
            let value = systemd_output(&["show", &selector, "--value", unit])?
                .trim()
                .to_string();
            if is_unset_placeholder(&value) {
                // `systemctl show` prints the literal `[not set]` for a property
                // with no value. Storing that string as if it were a value
                // produced records that could never be restored: the restore
                // path sent `IOWeight=[not set]`, systemd answered "Failed to
                // parse IOWeight= value '[not set]': Invalid argument", the
                // record stayed pending, and the next daemon start refused to
                // touch the target at all with StaleGeneration.
                return Ok(StoredValue::Systemd {
                    explicit: false,
                    value: String::new(),
                });
            }
            Ok(StoredValue::Systemd {
                explicit: systemd_property_is_explicit(unit, property)?,
                value,
            })
        }
    }
}

fn write_target(target: &TargetKind, value: &StoredValue) -> io::Result<()> {
    match (target, value) {
        (TargetKind::KernelValue { path }, StoredValue::Scalar { value })
        | (TargetKind::PmqosDevice { path }, StoredValue::Scalar { value }) => {
            write_value(path, value)
        }
        (TargetKind::PmqosCpu, StoredValue::Scalar { value }) if value == "unconstrained" => Ok(()),
        (
            TargetKind::RuntimePm {
                control_path,
                delay_path,
            },
            StoredValue::RuntimePm { control, delay },
        ) => {
            write_value(control_path, "on")?;
            if let (Some(path), Some(delay)) = (delay_path, delay) {
                write_value(path, delay)?;
            }
            write_value(control_path, control)
        }
        (
            TargetKind::SystemdProperty { unit, property },
            StoredValue::Systemd { explicit, value },
        ) => {
            // Never send the placeholder back as a value, even if an older
            // record on disk still carries it.
            let restore_to = if *explicit && !is_unset_placeholder(value) {
                value.as_str()
            } else {
                ""
            };
            let assignment = format!("{property}={restore_to}");
            // Capture stderr: `systemctl exited with exit status: 1` on its own
            // is a dead end, and this path failing is what leaves a property
            // changed and its record pending, which then blocks the next
            // daemon start with StaleGeneration.
            let output = Command::new("systemctl")
                .args(["set-property", "--runtime", unit, &assignment])
                .output()?;
            if output.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(io::Error::other(format!(
                    "systemctl set-property --runtime {unit} {assignment} exited with {}: {}",
                    output.status,
                    if stderr.is_empty() {
                        "no stderr"
                    } else {
                        &stderr
                    }
                )))
            }
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "target/value kind mismatch",
        )),
    }
}

fn canonical_identity(target: &TargetKind) -> io::Result<String> {
    let canonical = |path: &Path| fs::canonicalize(path).map(|value| value.display().to_string());
    match target {
        TargetKind::KernelValue { path } => canonical(path).map(|path| format!("kernel:{path}")),
        TargetKind::PmqosCpu => Ok("pmqos:/dev/cpu_dma_latency".to_string()),
        TargetKind::PmqosDevice { path } => {
            canonical(path).map(|path| format!("device-pmqos:{path}"))
        }
        TargetKind::RuntimePm {
            control_path,
            delay_path,
        } => {
            let control = canonical(control_path)?;
            let delay = delay_path
                .as_ref()
                .map(|path| canonical(path))
                .transpose()?
                .unwrap_or_else(|| "absent".to_string());
            Ok(format!("runtime-pm:control={control};delay={delay}"))
        }
        TargetKind::SystemdProperty { unit, property } => Ok(format!("systemd:{unit}:{property}")),
    }
}

fn transaction_related(
    current: &StoredValue,
    original: &StoredValue,
    intended: &StoredValue,
) -> bool {
    if current == intended {
        return true;
    }
    match (current, original, intended) {
        (
            StoredValue::RuntimePm {
                control: current_control,
                delay: current_delay,
            },
            StoredValue::RuntimePm {
                control: original_control,
                delay: original_delay,
            },
            StoredValue::RuntimePm {
                control: intended_control,
                delay: intended_delay,
            },
        ) => {
            let control_related =
                current_control == original_control || current_control == intended_control;
            let delay_related = current_delay == original_delay || current_delay == intended_delay;
            let changed = current_control == intended_control || current_delay == intended_delay;
            control_related && delay_related && changed
        }
        _ => false,
    }
}

fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

fn append_event(root: &Path, event: &RecoveryEvent) -> io::Result<()> {
    fs::create_dir_all(root)?;
    let path = root.join(OUTCOME_LOG);
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut file, event)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    sync_dir(root)
}

fn compact(root: &Path, record_path: &Path) -> io::Result<()> {
    match fs::remove_file(record_path) {
        Ok(()) => sync_dir(root),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn record_event(summary: &mut RecoverySummary, event: RecoveryEvent) {
    match event.disposition {
        RecoveryDisposition::Restored => summary.restored += 1,
        RecoveryDisposition::AlreadyRestored => summary.already_restored += 1,
        RecoveryDisposition::RelinquishedDrift => summary.relinquished += 1,
        RecoveryDisposition::Failed => summary.failed += 1,
    }
    summary.events.push(event);
}

fn fail_event(target_id: impl Into<String>, detail: impl Into<String>) -> RecoveryEvent {
    RecoveryEvent {
        target_id: target_id.into(),
        disposition: RecoveryDisposition::Failed,
        detail: detail.into(),
        timestamp_unix: now_unix(),
    }
}

fn recover_record(root: &Path, path: &Path) -> Result<RecoveryEvent, RecoveryEvent> {
    let content = fs::read_to_string(path).map_err(|error| {
        fail_event(
            path.display().to_string(),
            format!("read transaction record: {error}"),
        )
    })?;
    let mut record: TransactionRecord = serde_json::from_str(&content).map_err(|error| {
        fail_event(
            path.display().to_string(),
            format!("parse transaction record: {error}"),
        )
    })?;
    // Records written by earlier builds stored systemd's `[not set]` placeholder
    // as a value. Canonicalize on load so those records compare equal to a
    // freshly-read unset property instead of failing their readback forever.
    canonicalize_stored(&mut record.original);
    canonicalize_stored(&mut record.intended);
    if record.schema_version != TRANSACTION_SCHEMA_VERSION {
        return Err(fail_event(
            record.target_id,
            format!("unsupported transaction schema {}", record.schema_version),
        ));
    }
    if record.owner != "optid" {
        return Err(fail_event(
            record.target_id,
            format!("unexpected transaction owner {}", record.owner),
        ));
    }

    let identity = canonical_identity(&record.target).map_err(|error| {
        fail_event(
            record.target_id.clone(),
            format!("validate canonical identity: {error}"),
        )
    })?;
    if identity != record.canonical_identity {
        return Err(fail_event(
            record.target_id,
            format!(
                "canonical identity changed from {} to {}",
                record.canonical_identity, identity
            ),
        ));
    }

    let current = read_target(&record.target).map_err(|error| {
        fail_event(
            record.target_id.clone(),
            format!("read current target value: {error}"),
        )
    })?;

    let (disposition, detail) = if current == record.original {
        (
            RecoveryDisposition::AlreadyRestored,
            "captured original already present".to_string(),
        )
    } else if transaction_related(&current, &record.original, &record.intended) {
        write_target(&record.target, &record.original).map_err(|error| {
            fail_event(
                record.target_id.clone(),
                format!("write captured original: {error}"),
            )
        })?;
        let readback = read_target(&record.target).map_err(|error| {
            fail_event(
                record.target_id.clone(),
                format!("read captured original after recovery: {error}"),
            )
        })?;
        if readback != record.original {
            return Err(fail_event(
                record.target_id,
                "recovery readback did not match captured original",
            ));
        }
        (
            RecoveryDisposition::Restored,
            format!("verified exact rollback via {}", record.rollback_method),
        )
    } else {
        (
            RecoveryDisposition::RelinquishedDrift,
            "current value no longer matches optid's intended state; ownership relinquished"
                .to_string(),
        )
    };

    let event = RecoveryEvent {
        target_id: record.target_id,
        disposition,
        detail,
        timestamp_unix: now_unix(),
    };
    append_event(root, &event).map_err(|error| {
        fail_event(
            event.target_id.clone(),
            format!("persist recovery outcome: {error}"),
        )
    })?;
    compact(root, path).map_err(|error| {
        fail_event(
            event.target_id.clone(),
            format!("compact resolved transaction: {error}"),
        )
    })?;
    Ok(event)
}

pub fn recover_directory(root: &Path) -> RecoverySummary {
    let mut summary = RecoverySummary::default();
    if let Err(error) = fs::create_dir_all(root) {
        record_event(
            &mut summary,
            fail_event(
                root.display().to_string(),
                format!("create recovery directory: {error}"),
            ),
        );
        return summary;
    }

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            record_event(
                &mut summary,
                fail_event(
                    root.display().to_string(),
                    format!("scan recovery directory: {error}"),
                ),
            );
            return summary;
        }
    };

    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort();

    for path in paths {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if name.starts_with('.') && name.ends_with(".tmp") {
            summary.scanned += 1;
            let event = fail_event(
                path.display().to_string(),
                "unpublished transaction temp file requires operator inspection",
            );
            let _ = append_event(root, &event);
            record_event(&mut summary, event);
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        summary.scanned += 1;
        match recover_record(root, &path) {
            Ok(event) => record_event(&mut summary, event),
            Err(event) => {
                let _ = append_event(root, &event);
                record_event(&mut summary, event);
            }
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_root(name: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rush-s3d-{name}-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create S3D test root");
        path
    }

    fn write_record(
        root: &Path,
        target: &Path,
        original: &str,
        intended: &str,
        phase: &str,
    ) -> PathBuf {
        let canonical = fs::canonicalize(target).expect("canonical target");
        let path = root.join("test-record.json");
        let record = json!({
            "schema_version": 1,
            "generation": "previous-generation",
            "owner": "optid",
            "domain": "vm",
            "operation": "vm_sysctl",
            "target_id": "vm-sysctl:test",
            "canonical_identity": format!("kernel:{}", canonical.display()),
            "target": {"kind": "kernel_value", "path": target},
            "original": {"kind": "scalar", "value": original},
            "intended": {"kind": "scalar", "value": intended},
            "rollback_method": "restore captured original",
            "stabilization_method": "none",
            "phase": phase,
            "created_at_unix": 1,
            "updated_at_unix": 1
        });
        fs::write(&path, serde_json::to_string_pretty(&record).unwrap()).unwrap();
        path
    }

    #[test]
    fn s3d_recovery_restores_intended_value_and_compacts() {
        let root = temp_root("restore");
        let target = root.join("target");
        fs::write(&target, "intended\n").unwrap();
        let record = write_record(&root, &target, "original", "intended", "committed");
        let summary = recover_directory(&root);
        assert!(summary.succeeded());
        assert_eq!(summary.restored, 1);
        assert_eq!(read_trimmed(&target).unwrap(), "original");
        assert!(!record.exists());
        assert!(root.join(OUTCOME_LOG).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn s3d_recovery_prepared_record_is_idempotent() {
        let root = temp_root("prepared");
        let target = root.join("target");
        fs::write(&target, "original\n").unwrap();
        let record = write_record(&root, &target, "original", "intended", "prepared");
        let summary = recover_directory(&root);
        assert!(summary.succeeded());
        assert_eq!(summary.already_restored, 1);
        assert_eq!(read_trimmed(&target).unwrap(), "original");
        assert!(!record.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn s3d_recovery_drift_relinquishes_without_overwrite() {
        let root = temp_root("drift");
        let target = root.join("target");
        fs::write(&target, "external-owner\n").unwrap();
        let record = write_record(&root, &target, "original", "intended", "committed");
        let summary = recover_directory(&root);
        assert!(summary.succeeded());
        assert_eq!(summary.relinquished, 1);
        assert_eq!(read_trimmed(&target).unwrap(), "external-owner");
        assert!(!record.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn s3d_recovery_refuses_identity_reuse() {
        let root = temp_root("identity");
        let first = root.join("first");
        let second = root.join("second");
        let link = root.join("link");
        fs::write(&first, "intended\n").unwrap();
        fs::write(&second, "replacement\n").unwrap();
        symlink(&first, &link).unwrap();
        let record = write_record(&root, &link, "original", "intended", "committed");
        fs::remove_file(&link).unwrap();
        symlink(&second, &link).unwrap();
        let summary = recover_directory(&root);
        assert!(!summary.succeeded());
        assert_eq!(summary.failed, 1);
        assert_eq!(read_trimmed(&second).unwrap(), "replacement");
        assert!(record.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn s3d_recovery_failure_retains_record() {
        let root = temp_root("failure");
        let target = root.join("target");
        fs::write(&target, "intended\n").unwrap();
        let record = write_record(&root, &target, "original", "intended", "committed");
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&target, permissions).unwrap();
        let summary = recover_directory(&root);
        assert!(!summary.succeeded());
        assert_eq!(summary.failed, 1);
        assert!(record.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn s3d_repeated_recovery_is_idempotent() {
        let root = temp_root("repeat");
        let first = recover_directory(&root);
        let second = recover_directory(&root);
        assert!(first.succeeded());
        assert!(second.succeeded());
        assert_eq!(first.scanned, 0);
        assert_eq!(second.scanned, 0);
        fs::remove_dir_all(root).unwrap();
    }

    // `is_unset_placeholder` and `assigns_a_value` are tested once, in
    // `systemd_placeholder.rs`, which both this module and the daemon's
    // `reconciler/mod.rs` include by `#[path]`.

    #[test]
    fn canonicalizing_collapses_the_placeholder_to_an_unset_value() {
        let mut v = StoredValue::Systemd {
            explicit: true,
            value: "[not set]".to_string(),
        };
        canonicalize_stored(&mut v);
        assert_eq!(
            v,
            StoredValue::Systemd {
                explicit: false,
                value: String::new()
            }
        );
    }

    #[test]
    fn canonicalizing_leaves_a_real_value_alone() {
        let mut v = StoredValue::Systemd {
            explicit: true,
            value: "150".to_string(),
        };
        canonicalize_stored(&mut v);
        assert_eq!(
            v,
            StoredValue::Systemd {
                explicit: true,
                value: "150".to_string()
            }
        );
    }

    #[test]
    fn canonicalizing_ignores_non_systemd_values() {
        let mut v = StoredValue::Scalar {
            value: "[not set]".to_string(),
        };
        canonicalize_stored(&mut v);
        assert_eq!(
            v,
            StoredValue::Scalar {
                value: "[not set]".to_string()
            }
        );
    }
}
