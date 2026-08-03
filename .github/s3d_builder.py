#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def write(path: str, content: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")

def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    if text.count(old) != 1:
        raise SystemExit(f"{path}: expected one occurrence, found {text.count(old)}: {old[:80]!r}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")

recovery_rs = r'''//! S3D independent, one-shot recovery for S2D transaction records.
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

pub const DEFAULT_RECOVERY_DIR: &str = "/var/lib/optid/recovery";
pub const RECOVERY_FAILURE_EXIT: i32 = 78;
const TRANSACTION_SCHEMA_VERSION: u32 = 1;
const OUTCOME_LOG: &str = "recovery-outcomes.jsonl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TargetKind {
    KernelValue { path: PathBuf },
    PmqosCpu,
    PmqosDevice { path: PathBuf },
    RuntimePm {
        control_path: PathBuf,
        delay_path: Option<PathBuf>,
    },
    SystemdProperty { unit: String, property: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredValue {
    Scalar { value: String },
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
        (TargetKind::PmqosCpu, StoredValue::Scalar { value })
            if value == "unconstrained" =>
        {
            Ok(())
        }
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
            let assignment = format!(
                "{property}={}",
                if *explicit { value.as_str() } else { "" }
            );
            let status = Command::new("systemctl")
                .args(["set-property", "--runtime", unit, &assignment])
                .status()?;
            if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!(
                    "systemctl exited with {status}"
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
        TargetKind::SystemdProperty { unit, property } => {
            Ok(format!("systemd:{unit}:{property}"))
        }
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
    let record: TransactionRecord = serde_json::from_str(&content).map_err(|error| {
        fail_event(
            path.display().to_string(),
            format!("parse transaction record: {error}"),
        )
    })?;
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
            fail_event(root.display().to_string(), format!("create recovery directory: {error}")),
        );
        return summary;
    }

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            record_event(
                &mut summary,
                fail_event(root.display().to_string(), format!("scan recovery directory: {error}")),
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
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
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
        let path = std::env::temp_dir().join(format!(
            "rush-s3d-{name}-{}-{id}",
            std::process::id()
        ));
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
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&target, permissions).unwrap();
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
}
'''

recover_bin = r'''//! `optid-recover` — S3D one-shot recovery executable.

#[path = "../recovery.rs"]
mod recovery;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use recovery::{recover_directory, DEFAULT_RECOVERY_DIR, RECOVERY_FAILURE_EXIT};

fn usage() {
    eprintln!("Usage: optid-recover [--recovery-dir PATH] [--status-file PATH]");
}

fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("tmp");
    fs::write(&temp, content)?;
    fs::File::open(&temp)?.sync_all()?;
    fs::rename(&temp, path)?;
    fs::File::open(parent)?.sync_all()
}

fn main() {
    let mut recovery_dir = PathBuf::from(DEFAULT_RECOVERY_DIR);
    let mut status_file = PathBuf::from("/run/optid/recovery-status.json");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--recovery-dir" => {
                let Some(value) = args.next() else {
                    usage();
                    std::process::exit(2);
                };
                recovery_dir = PathBuf::from(value);
            }
            "--status-file" => {
                let Some(value) = args.next() else {
                    usage();
                    std::process::exit(2);
                };
                status_file = PathBuf::from(value);
            }
            "--help" | "-h" => {
                usage();
                return;
            }
            _ => {
                eprintln!("optid-recover: unknown argument {arg}");
                usage();
                std::process::exit(2);
            }
        }
    }

    let summary = recover_directory(&recovery_dir);
    let rendered =
        serde_json::to_string_pretty(&summary).expect("RecoverySummary serialization is infallible");
    if let Err(error) = atomic_write(&status_file, &rendered) {
        eprintln!("optid-recover: cannot write recovery status: {error}");
        std::process::exit(RECOVERY_FAILURE_EXIT);
    }
    println!("{rendered}");
    if !summary.succeeded() {
        std::process::exit(RECOVERY_FAILURE_EXIT);
    }
}
'''

supervision_rs = r'''//! S3D systemd notification and transaction-journal health gate.

use std::env;
use std::ffi::OsStr;
use std::io;
#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static READY_SENT: AtomicBool = AtomicBool::new(false);

fn verify_journal_health(
    transactions: &TransactionEngine,
    io: &dyn KernelIo,
) -> Result<(), TransactionError> {
    if !io.exists(&transactions.root) {
        return Ok(());
    }
    for path in io
        .read_dir(&transactions.root)
        .map_err(|error| TransactionError::io("scan recovery directory", error))?
    {
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
        if name.starts_with('.') && name.ends_with(".tmp") {
            return Err(TransactionError::new(
                TransactionErrorKind::InvalidRecord,
                format!("unpublished transaction temp file remains: {}", path.display()),
            ));
        }
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let record = transactions.load_record(io, &path)?;
        transactions.validate_generation_and_identity(io, &record)?;
        if record.phase != TransactionPhase::Committed {
            return Err(TransactionError::new(
                TransactionErrorKind::PhaseConflict,
                format!(
                    "{} remains in {:?} after a completed cycle",
                    record.target_id, record.phase
                ),
            ));
        }
    }
    Ok(())
}

fn notify_socket(message: &str) -> io::Result<()> {
    let Some(raw) = env::var_os("NOTIFY_SOCKET") else {
        return Ok(());
    };
    let socket = UnixDatagram::unbound()?;
    let bytes = OsStr::new(&raw).as_bytes();
    if bytes.first() == Some(&b'@') {
        #[cfg(target_os = "linux")]
        {
            let address = SocketAddr::from_abstract_name(&bytes[1..])?;
            socket.send_to_addr(message.as_bytes(), &address)?;
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "abstract systemd notification sockets require Linux",
            ));
        }
    }
    socket.send_to(message.as_bytes(), Path::new(&raw))?;
    Ok(())
}

#[cfg(test)]
thread_local! {
    static TEST_NOTIFICATIONS: std::cell::RefCell<Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>> =
        const { std::cell::RefCell::new(None) };
}

fn emit(message: &str) -> io::Result<()> {
    #[cfg(test)]
    {
        let captured = TEST_NOTIFICATIONS.with(|slot| slot.borrow().clone());
        if let Some(captured) = captured {
            captured
                .lock()
                .expect("S3D notification capture mutex poisoned")
                .push(message.to_string());
            return Ok(());
        }
    }
    notify_socket(message)
}

fn notify_cycle_complete(
    transactions: &TransactionEngine,
    io: &dyn KernelIo,
) -> Result<(), TransactionError> {
    verify_journal_health(transactions, io)?;
    let first = READY_SENT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok();
    let message = if first {
        "READY=1\nSTATUS=optid control cycle complete\nWATCHDOG=1"
    } else {
        "WATCHDOG=1"
    };
    emit(message).map_err(|error| TransactionError::io("notify systemd watchdog", error))
}

#[cfg(test)]
fn capture_notifications<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    let capture = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    READY_SENT.store(false, Ordering::SeqCst);
    TEST_NOTIFICATIONS.with(|slot| {
        slot.replace(Some(capture.clone()));
    });
    let result = run();
    TEST_NOTIFICATIONS.with(|slot| {
        slot.replace(None);
    });
    let messages = capture
        .lock()
        .expect("S3D notification capture mutex poisoned")
        .clone();
    (result, messages)
}
'''

s3d_tests = r'''use super::*;

#[test]
fn s3d_complete_cycle_emits_ready_and_watchdog() {
    let kernel = MemoryKernel::new();
    let root = PathBuf::from("/state/s3d-recovery");
    kernel.add_dir(Path::new("/state"), &root);
    let transactions = TransactionEngine::new(root, "generation".to_string());

    let (result, messages) =
        capture_notifications(|| notify_cycle_complete(&transactions, &kernel));

    result.expect("healthy completed cycle must notify systemd");
    assert_eq!(
        messages,
        vec!["READY=1\nSTATUS=optid control cycle complete\nWATCHDOG=1"]
    );
}

#[test]
fn s3d_journal_failure_withholds_watchdog() {
    let kernel = MemoryKernel::new();
    let root = PathBuf::from("/state/s3d-recovery");
    kernel.add_dir(Path::new("/state"), &root);
    let temp = root.join(".unpublished.json.generation.tmp");
    kernel.write_raw(&temp, "partial");
    kernel.add_dir_entry(&root, &temp);
    let transactions = TransactionEngine::new(root, "generation".to_string());

    let (result, messages) =
        capture_notifications(|| notify_cycle_complete(&transactions, &kernel));

    assert!(result.is_err());
    assert!(messages.is_empty());
}
'''

recovery_cli_test = r'''use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn temp_root(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "rush-s3d-cli-{name}-{}-{id}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("create S3D CLI root");
    path
}

fn write_record(root: &Path, target: &Path) {
    let canonical = fs::canonicalize(target).expect("canonical target");
    let record = json!({
        "schema_version": 1,
        "generation": "crashed-generation",
        "owner": "optid",
        "domain": "vm",
        "operation": "vm_sysctl",
        "target_id": "vm-sysctl:cli",
        "canonical_identity": format!("kernel:{}", canonical.display()),
        "target": {"kind": "kernel_value", "path": target},
        "original": {"kind": "scalar", "value": "original"},
        "intended": {"kind": "scalar", "value": "intended"},
        "rollback_method": "restore captured original",
        "stabilization_method": "none",
        "phase": "committed",
        "created_at_unix": 1,
        "updated_at_unix": 1
    });
    fs::write(
        root.join("cli-record.json"),
        serde_json::to_string_pretty(&record).unwrap(),
    )
    .unwrap();
}

#[test]
fn s3d_recovery_cli_recovers_before_success_exit() {
    let root = temp_root("success");
    let target = root.join("target");
    let status = root.join("status.json");
    fs::write(&target, "intended\n").unwrap();
    write_record(&root, &target);

    let output = Command::new(env!("CARGO_BIN_EXE_optid-recover"))
        .args([
            "--recovery-dir",
            root.to_str().unwrap(),
            "--status-file",
            status.to_str().unwrap(),
        ])
        .output()
        .expect("run optid-recover");

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&target).unwrap().trim(), "original");
    assert!(status.exists());
    assert!(!root.join("cli-record.json").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn s3d_recovery_binary_has_no_policy_or_async_surface() {
    let source = include_str!("../src/bin/optid-recover.rs");
    let recovery = include_str!("../src/recovery.rs");
    for forbidden in ["Policy", "classif", "zbus", "tokio", "D-Bus", "session"] {
        assert!(
            !source.contains(forbidden),
            "recovery binary contains forbidden surface {forbidden}"
        );
    }
    for forbidden in ["zbus", "tokio"] {
        assert!(
            !recovery.contains(forbidden),
            "recovery core contains forbidden dependency {forbidden}"
        );
    }
}
'''

systemd_test = r'''use std::fs;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn s3d_apply_unit_orders_recovery_before_daemon() {
    let root = repository_root();
    let apply = fs::read_to_string(root.join("packaging/systemd/optid-apply.service"))
        .expect("read optid-apply.service");
    let recover = fs::read_to_string(root.join("packaging/systemd/optid-recover.service"))
        .expect("read optid-recover.service");

    assert!(apply.contains("Type=notify"));
    assert!(apply.contains("NotifyAccess=main"));
    assert!(apply.contains("WatchdogSec="));
    assert!(apply.contains("ExecStartPre=/usr/libexec/optid-recover"));
    assert!(apply.contains("RestartPreventExitStatus=78"));
    assert!(recover.contains("Type=oneshot"));
    assert!(recover.contains("Before=optid-apply.service"));
    assert!(recover.contains("ExecStart=/usr/libexec/optid-recover"));
}

#[test]
fn s3d_failed_recovery_prevents_automatic_actuation_restart_loop() {
    let root = repository_root();
    let apply = fs::read_to_string(root.join("packaging/systemd/optid-apply.service"))
        .expect("read optid-apply.service");
    assert!(apply.contains("StartLimitIntervalSec="));
    assert!(apply.contains("StartLimitBurst="));
    assert!(apply.contains("RestartPreventExitStatus=78"));
}
'''

arch_doc = r'''# S3D Independent Recovery and Watchdog Supervision

**Status:** Builder candidate for package S3D
**Architecture:** D2 fail-passive capability sealing

## Scope

S3D adds two bounded mechanisms without adding a permanent broker or a
steady-state IPC hop:

1. `optid-recover`, a one-shot executable that consumes S2D recovery records
   before automatic actuation starts or restarts; and
2. systemd notification from the synchronous reconciler path only after a
   complete transaction/readback/reconciliation cycle and a healthy journal.

The recovery executable contains no policy parser, classifier, D-Bus server,
session bridge, or async runtime.

## Recovery ordering

`optid-apply.service` runs `optid-recover` as `ExecStartPre` on every initial
start and every supervisor-managed restart. A failed recovery exits with status
78, prevents automatic actuation, and is excluded from the normal restart loop.
The separate `optid-recover.service` provides an explicit boot/manual one-shot
unit and is ordered before the apply service.

Recovery is idempotent:

- a prepared record whose original is still present is recorded as already
  restored and compacted;
- an intended or transaction-partial value is rolled back to the exact captured
  original and verified;
- external drift is relinquished without overwrite;
- canonical identity mismatch, malformed evidence, write failure, or readback
  mismatch leaves the transaction record in place and fails closed; and
- every outcome is appended durably before a resolved record is removed.

## Watchdog semantics

The watchdog message is emitted synchronously from `Reconciler::reconcile`
after apply/readback/compensation, transition handback, state persistence, and
journal validation all complete. The first healthy cycle sends `READY=1` and
`WATCHDOG=1`; later healthy cycles send only `WATCHDOG=1`.

An unpublished temp record, malformed record, stale generation, identity
mismatch, non-committed residual phase, or notification failure prevents the
heartbeat and returns an error to the daemon. There is no independent heartbeat
thread that could falsely report health while the control path is stuck.

## Boundaries

S3D does not pre-open hardware descriptors or install Landlock; that is S4D.
It does not persist domain/HWID circuit breakers or controlled canary re-entry;
that is S5D. It does not implement new topology discovery. The apply unit is
prepared for systemd-managed cold restart, while actual hotplug-triggered
topology rebuilding remains coupled to the later sealed-capability lifecycle.
'''

recover_service = r'''[Unit]
Description=Rush Linux one-shot optid transaction recovery
Documentation=man:optid(8)
DefaultDependencies=no
After=local-fs.target systemd-udevd.service
Before=optid-apply.service
ConditionPathExists=/proc/pressure/cpu

[Service]
Type=oneshot
ExecStart=/usr/libexec/optid-recover --recovery-dir /var/lib/optid/recovery --status-file /run/optid/recovery-status.json
StateDirectory=optid
RuntimeDirectory=optid
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=no
ProtectKernelTunables=yes
ReadWritePaths=/var/lib/optid /run/optid \
    /sys/devices/system/cpu \
    /sys/firmware/acpi/platform_profile \
    /sys/devices \
    /sys/bus/pci \
    /sys/bus/usb \
    /sys/class/backlight \
    /sys/class/scsi_host \
    /proc/sys/vm \
    /dev/cpu_dma_latency
RestrictAddressFamilies=AF_UNIX
SystemCallArchitectures=native
SystemCallFilter=@system-service
SystemCallFilter=~@privileged @obsolete
ProtectKernelModules=yes
ProtectKernelLogs=yes
RestrictNamespaces=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
RemoveIPC=yes
ProcSubset=pid
ProtectProc=invisible

[Install]
WantedBy=multi-user.target
'''

write("crates/optid/src/recovery.rs", recovery_rs)
write("crates/optid/src/bin/optid-recover.rs", recover_bin)
write("crates/optid/src/reconciler/supervision.rs", supervision_rs)
write("crates/optid/src/reconciler/tests/s3d.rs", s3d_tests)
write("crates/optid/tests/recovery_cli.rs", recovery_cli_test)
write("crates/optid/tests/s3d_systemd.rs", systemd_test)
write("docs/architecture/optid-s3d-recovery-watchdog.md", arch_doc)
write("packaging/systemd/optid-recover.service", recover_service)

replace_once(
    "crates/optid/Cargo.toml",
    '[[bin]]\nname = "optid-lever-contracts"\npath = "src/bin/optid-lever-contracts.rs"\n',
    '[[bin]]\nname = "optid-lever-contracts"\npath = "src/bin/optid-lever-contracts.rs"\n\n'
    '# S3D — Independent one-shot recovery; no policy, D-Bus, or async runtime.\n'
    '[[bin]]\nname = "optid-recover"\npath = "src/bin/optid-recover.rs"\n',
)

replace_once(
    "crates/optid/src/reconciler/mod.rs",
    'include!("transaction.rs");\n',
    'include!("transaction.rs");\ninclude!("supervision.rs");\n',
)

replace_once(
    "crates/optid/src/reconciler/apply.rs",
    '        self.persist(actuator.kernel.as_ref())?;\n        Ok(outcomes)\n',
    '        self.persist(actuator.kernel.as_ref())?;\n'
    '        notify_cycle_complete(&self.transactions, actuator.kernel.as_ref())\n'
    '            .map_err(io::Error::from)?;\n'
    '        Ok(outcomes)\n',
)

replace_once(
    "crates/optid/src/reconciler/tests.rs",
    'mod systemd;\nmod unit;\n',
    'mod systemd;\nmod unit;\nmod s3d;\n',
)

apply_path = ROOT / "packaging/systemd/optid-apply.service"
apply = apply_path.read_text(encoding="utf-8")
apply = apply.replace(
    "After=multi-user.target systemd-udevd.service\n",
    "After=multi-user.target systemd-udevd.service optid-recover.service\n"
    "StartLimitIntervalSec=60s\n"
    "StartLimitBurst=3\n",
    1,
)
apply = apply.replace(
    "[Service]\nType=simple\nExecStart=/usr/libexec/optid --apply --interval-sec 2 --state-dir /run/optid\n",
    "[Service]\n"
    "Type=notify\n"
    "NotifyAccess=main\n"
    "WatchdogSec=10s\n"
    "TimeoutStartSec=30s\n"
    "ExecStartPre=/usr/libexec/optid-recover --recovery-dir /var/lib/optid/recovery --status-file /run/optid/recovery-status.json\n"
    "ExecStart=/usr/libexec/optid --apply --interval-sec 2 --state-dir /run/optid\n",
    1,
)
apply = apply.replace(
    "RestartSec=2\n",
    "RestartSec=2\nRestartPreventExitStatus=78\n",
    1,
)
apply_path.write_text(apply, encoding="utf-8")

recipe_path = ROOT / "recipes/core/optid.toml"
recipe = recipe_path.read_text(encoding="utf-8")
recipe = recipe.replace(
    '  ["target/release/optid", "/usr/libexec/optid"],\n',
    '  ["target/release/optid", "/usr/libexec/optid"],\n'
    '  ["target/release/optid-recover", "/usr/libexec/optid-recover"],\n',
    1,
)
recipe = recipe.replace(
    '  "packaging/systemd/optid-apply.service",\n',
    '  "packaging/systemd/optid-apply.service",\n'
    '  "packaging/systemd/optid-recover.service",\n',
    1,
)
recipe_path.write_text(recipe, encoding="utf-8")
