//! Low-level I/O utilities for `optid`:
//!
//! - `guarded_write` — the single funnel for all sysfs/procfs writes. Enforces
//!   the allowlist from ADR 0009 and rejects directory traversal per the M1
//!   hardening (PR #101). F2: the allowlist logic now lives in
//!   `kernel_io::is_allowlisted_write_path` (centralized); this function
//!   delegates to `RealKernel::new().write(...)`.
//! - `atomic_write_state_file` — write-then-rename so a SIGKILL between the
//!   write and the rename can never leave a truncated `original_*` or
//!   `intended_*` revert-journal entry.
//! - `revert_sysctls` / `revert_pm_qos` — restore journaled previous values on
//!   startup/shutdown so `optid` never leaves a host in a half-actuated state.
//! - `append_log`, `get_path_hash`, `now_unix` — small shared helpers.
//!
//! F2: every function has a `*_with(io)` form that accepts an injected
//! `KernelWrite` (and `KernelRead` for the revert functions, which read
//! journal files). The legacy no-argument form delegates to
//! `RealKernel::new()`. This preserves bit-for-bit behavior while
//! making fault-injection deterministic from tests.

use std::io;
use std::path::{Path, PathBuf};

use crate::kernel_io::{KernelIo, KernelRead, KernelWrite, RealKernel};

#[cfg(test)]
use std::fs;

/// F2: production path — delegates to `RealKernel::new().write()`,
/// which calls `kernel_io::is_allowlisted_write_path` (the centralized
/// allowlist + traversal check) and then `std::fs::write`.
#[cfg(test)]
pub(crate) fn guarded_write(path: &Path, value: &str) -> io::Result<()> {
    guarded_write_with(&RealKernel::new(), path, value)
}

/// F2: injectable guarded write for tests. The `KernelWrite` trait's
/// `write` method enforces the allowlist via
/// `is_allowlisted_write_path` before delegating to the underlying
/// filesystem.
pub(crate) fn guarded_write_with(
    write: &dyn KernelWrite,
    path: &Path,
    value: &str,
) -> io::Result<()> {
    write.write(path, value)
}

pub(crate) fn revert_sysctls(state_dir: &Path) {
    revert_sysctls_with(&RealKernel::new(), state_dir)
}

/// F2: injectable sysctl recovery.
pub(crate) fn revert_sysctls_with(io: &dyn KernelIo, state_dir: &Path) {
    let keys = [
        "vm_swappiness",
        "vm_dirty_background_bytes",
        "vm_dirty_bytes",
    ];
    for key in &keys {
        match actuation_state_with(io, state_dir, key) {
            None => continue,
            Some(true) => {
                // Clean-shutdown revert: actuation landed, marker present.
            }
            Some(false) => {
                // Crash recovery: original_<key> exists but no applied marker.
                // The sysfs write may or may not have landed; restore to be safe.
                eprintln!(
                    "optid: crash recovery for sysctl {key} — applied marker absent, \
                     restoring journaled original"
                );
            }
        }
        let orig_path = state_dir.join(format!("original_{key}"));
        let mut restored = false;
        if let Ok(orig_val) = io.read_to_string(&orig_path) {
            let sysctl_name = key.replace('_', ".");
            let sysctl_path = PathBuf::from(format!("/proc/sys/{}", sysctl_name.replace('.', "/")));
            if let Err(e) = guarded_write_with(io, &sysctl_path, orig_val.trim()) {
                eprintln!("optid: failed to revert sysctl {sysctl_name}: {e}");
            } else {
                println!("optid: reverted sysctl {sysctl_name} to {orig_val}");
                restored = true;
            }
        }
        if restored {
            clear_journal_with(io, state_dir, key);
        } else {
            eprintln!("optid: retaining journal for {key}; restore did not complete");
        }
    }
}

pub(crate) fn revert_pm_qos(state_dir: &Path) {
    revert_pm_qos_with(&RealKernel::new(), state_dir)
}

/// F2: injectable PM-QoS recovery.
pub(crate) fn revert_pm_qos_with(io: &dyn KernelIo, state_dir: &Path) {
    let Ok(entries) = io.read_dir(state_dir) else {
        return;
    };
    for entry in entries {
        let name = entry.file_name().unwrap_or_default();
        let name_str = name.to_string_lossy();
        let hash = match name_str.strip_prefix("original_dev_") {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        let key = format!("dev_{hash}");
        match actuation_state_with(io, state_dir, &key) {
            None => continue,
            Some(true) => {}
            Some(false) => {
                eprintln!(
                    "optid: crash recovery for PM QoS dev_{hash} — applied marker absent, \
                     restoring journaled original"
                );
            }
        }
        let orig_path = entry;
        let mut restored = false;
        if let Ok(content) = io.read_to_string(&orig_path) {
            let mut lines = content.lines();
            if let (Some(dev_path_str), Some(orig_val)) = (lines.next(), lines.next()) {
                let dev_path = Path::new(dev_path_str);
                if let Err(e) = guarded_write_with(io, dev_path, orig_val.trim()) {
                    eprintln!(
                        "optid: failed to revert PM QoS for {}: {e}",
                        dev_path.display()
                    );
                } else {
                    println!(
                        "optid: reverted PM QoS for {} to {}",
                        dev_path.display(),
                        orig_val.trim()
                    );
                    restored = true;
                }
            }
        }
        if restored {
            clear_journal_with(io, state_dir, &key);
        } else {
            eprintln!("optid: retaining journal for {key}; restore did not complete");
        }
    }
}

/// WP-N5: restore runtime-PM `power/control` and `power/autosuspend_delay_ms`
/// to their journaled originals on startup/shutdown. Mirrors `revert_pm_qos`.
///
/// Each `original_rpm_<hash>` file holds three lines: the device directory, the
/// original `control` value, and the original `autosuspend_delay_ms` value (or
/// the literal `n/a` when the device had no `autosuspend_delay_ms` attribute).
pub(crate) fn revert_runtime_pm(state_dir: &Path) {
    revert_runtime_pm_with(&RealKernel::new(), state_dir)
}

/// F2: injectable runtime-PM recovery.
pub(crate) fn revert_runtime_pm_with(io: &dyn KernelIo, state_dir: &Path) {
    let Ok(entries) = io.read_dir(state_dir) else {
        return;
    };
    for entry in entries {
        let name = entry.file_name().unwrap_or_default();
        let name_str = name.to_string_lossy();
        let hash = match name_str.strip_prefix("original_rpm_") {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        let key = format!("rpm_{hash}");
        match actuation_state_with(io, state_dir, &key) {
            None => continue,
            Some(true) => {}
            Some(false) => {
                eprintln!(
                    "optid: crash recovery for runtime PM {hash} — applied marker absent, \
                     restoring journaled original"
                );
            }
        }
        let orig_path = entry;
        let mut restored = false;
        if let Ok(content) = io.read_to_string(&orig_path) {
            let mut lines = content.lines();
            if let (Some(dev_dir), Some(orig_control)) = (lines.next(), lines.next()) {
                let dev_dir = Path::new(dev_dir);
                let control_path = dev_dir.join("power").join("control");
                let control_restored =
                    if let Err(e) = guarded_write_with(io, &control_path, orig_control.trim()) {
                        eprintln!(
                            "optid: failed to revert runtime PM control for {}: {e}",
                            dev_dir.display()
                        );
                        false
                    } else {
                        println!(
                            "optid: reverted runtime PM control for {} to {}",
                            dev_dir.display(),
                            orig_control.trim()
                        );
                        true
                    };
                if control_restored {
                    if let Some(orig_delay) = lines.next() {
                        let orig_delay = orig_delay.trim();
                        if orig_delay != "n/a" {
                            let delay_path = dev_dir.join("power").join("autosuspend_delay_ms");
                            match guarded_write_with(io, &delay_path, orig_delay) {
                                Ok(()) => restored = true,
                                Err(e) => eprintln!(
                                    "optid: failed to revert runtime PM delay for {}: {e}",
                                    dev_dir.display()
                                ),
                            }
                        } else {
                            restored = true;
                        }
                    }
                }
            }
        }
        if restored {
            clear_journal_with(io, state_dir, &key);
        } else {
            eprintln!("optid: retaining journal for {key}; restore did not complete");
        }
    }
}

/// WP-N6: restore PCIe ASPM (`link/l1_aspm`) and SATA ALPM
/// (`link_power_management_policy`) to their journaled originals on
/// startup/shutdown. Each `original_aspm_<hash>` / `original_alpm_<hash>` file
/// holds two lines: the base directory and the original attribute value.
pub(crate) fn revert_storage(state_dir: &Path) {
    revert_storage_with(&RealKernel::new(), state_dir)
}

/// F2: injectable storage-power recovery.
pub(crate) fn revert_storage_with(io: &dyn KernelIo, state_dir: &Path) {
    let Ok(entries) = io.read_dir(state_dir) else {
        return;
    };
    for entry in entries {
        let name = entry.file_name().unwrap_or_default();
        let name_str = name.to_string_lossy();
        let (prefix, rel, journal_key): (&str, &[&str], &str) =
            if name_str.starts_with("original_aspm_") {
                ("original_aspm_", &["link", "l1_aspm"], "aspm")
            } else if name_str.starts_with("original_alpm_") {
                ("original_alpm_", &["link_power_management_policy"], "alpm")
            } else {
                continue;
            };
        let hash = name_str.strip_prefix(prefix).unwrap_or("");
        if hash.is_empty() {
            continue;
        }
        let key = format!("{journal_key}_{hash}");
        match actuation_state_with(io, state_dir, &key) {
            None => continue,
            Some(true) => {}
            Some(false) => {
                eprintln!(
                    "optid: crash recovery for storage PM {journal_key}_{hash} — applied marker absent, \
                     restoring journaled original"
                );
            }
        }
        let orig_path = entry;
        let mut restored = false;
        if let Ok(content) = io.read_to_string(&orig_path) {
            let mut lines = content.lines();
            if let (Some(base), Some(orig_val)) = (lines.next(), lines.next()) {
                let mut target = Path::new(base).to_path_buf();
                for seg in rel {
                    target = target.join(seg);
                }
                if let Err(e) = guarded_write_with(io, &target, orig_val.trim()) {
                    eprintln!(
                        "optid: failed to revert storage PM for {}: {e}",
                        target.display()
                    );
                } else {
                    println!(
                        "optid: reverted storage PM for {} to {}",
                        target.display(),
                        orig_val.trim()
                    );
                    restored = true;
                }
            }
        }
        if restored {
            clear_journal_with(io, state_dir, &key);
        } else {
            eprintln!("optid: retaining journal for {key}; restore did not complete");
        }
    }
}

/// WP-N7: restore panel backlight `brightness` to its journaled original on
/// startup/shutdown. Each `original_bl_<hash>` file holds two lines: the
/// backlight device directory and the original raw brightness value.
pub(crate) fn revert_display(state_dir: &Path) {
    revert_display_with(&RealKernel::new(), state_dir)
}

/// F2: injectable backlight recovery.
pub(crate) fn revert_display_with(io: &dyn KernelIo, state_dir: &Path) {
    let Ok(entries) = io.read_dir(state_dir) else {
        return;
    };
    for entry in entries {
        let name = entry.file_name().unwrap_or_default();
        let name_str = name.to_string_lossy();
        let hash = match name_str.strip_prefix("original_bl_") {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        let key = format!("bl_{hash}");
        match actuation_state_with(io, state_dir, &key) {
            None => continue,
            Some(true) => {}
            Some(false) => {
                eprintln!(
                    "optid: crash recovery for backlight {hash} — applied marker absent, \
                     restoring journaled original"
                );
            }
        }
        let orig_path = entry;
        let mut restored = false;
        if let Ok(content) = io.read_to_string(&orig_path) {
            let mut lines = content.lines();
            if let (Some(dev_dir), Some(orig_val)) = (lines.next(), lines.next()) {
                let target = Path::new(dev_dir).join("brightness");
                if let Err(e) = guarded_write_with(io, &target, orig_val.trim()) {
                    eprintln!(
                        "optid: failed to revert backlight for {}: {e}",
                        target.display()
                    );
                } else {
                    println!(
                        "optid: reverted backlight for {} to {}",
                        target.display(),
                        orig_val.trim()
                    );
                    restored = true;
                }
            }
        }
        if restored {
            clear_journal_with(io, state_dir, &key);
        } else {
            eprintln!("optid: retaining journal for {key}; restore did not complete");
        }
    }
}

/// Atomic write of a state file in `/run/optid`.
///
/// Writes to `<path>.tmp` first, then renames into place. The rename is
/// atomic on POSIX, so a SIGKILL between the write and the rename leaves
/// either the previous contents (if any) or no file at all — never a
/// truncated `original_*` or `intended_*` file that the next-boot revert
/// would interpret as a real backup.
#[cfg(test)]
pub(crate) fn atomic_write_state_file(path: &Path, content: &str) -> io::Result<()> {
    atomic_write_state_file_with(&RealKernel::new(), path, content)
}

/// F2: injectable atomic write for tests.
pub(crate) fn atomic_write_state_file_with(
    write: &dyn KernelWrite,
    path: &Path,
    content: &str,
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        write.create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    write.write_state_file(&tmp, content)?;
    write.rename(&tmp, path)
}

/// optid-safety: write an `applied_<key>` marker after a successful sysfs
/// mutation, so the next-boot revert can distinguish "clean shutdown"
/// (marker present → normal revert) from "crash mid-actuation" (marker
/// absent but `original_<key>` present → crash recovery).
///
/// The marker is written atomically (write-then-rename) so a crash during
/// the marker write itself cannot leave a stale marker. The marker's
/// contents are the timestamp and the written value, for forensic use.
///
/// Returns `Ok(())` on success. A failure to write the marker is logged to
/// stderr but does NOT propagate — the sysfs write already succeeded, and
/// failing here would leave the system in the new state without a marker,
/// which the revert path treats as crash recovery (conservative).
#[cfg(test)]
pub(crate) fn mark_applied(state_dir: &Path, key: &str, value: &str) {
    mark_applied_with(&RealKernel::new(), state_dir, key, value)
}

/// F2: injectable mark_applied for tests. Takes `&dyn KernelIo` because
/// it needs both `KernelWrite` (for the atomic write) and `Clock` (for
/// the timestamp).
pub(crate) fn mark_applied_with(io: &dyn KernelIo, state_dir: &Path, key: &str, value: &str) {
    let marker_path = state_dir.join(format!("applied_{key}"));
    let content = format!("{}\n{}", io.now_unix(), value);
    if let Err(e) = atomic_write_state_file_with(io, &marker_path, &content) {
        eprintln!(
            "optid: failed to write applied marker for {key}: {e} \
             (next-boot revert will treat this as crash recovery)"
        );
    }
}

/// optid-safety: detect incomplete actuation by checking whether the
/// `applied_<key>` marker exists. Returns:
/// - `Some(true)` — marker present; the actuation landed cleanly.
/// - `Some(false)` — `original_<key>` exists but no marker; crash recovery
///   needed (the sysfs write may or may not have landed; revert to be safe).
/// - `None` — neither file exists; nothing to revert.
#[cfg(test)]
pub(crate) fn actuation_state(state_dir: &Path, key: &str) -> Option<bool> {
    actuation_state_with(&RealKernel::new(), state_dir, key)
}

/// F2: injectable actuation_state for tests.
pub(crate) fn actuation_state_with(
    read: &dyn KernelRead,
    state_dir: &Path,
    key: &str,
) -> Option<bool> {
    let orig = state_dir.join(format!("original_{key}"));
    let applied = state_dir.join(format!("applied_{key}"));
    if read.exists(&applied) {
        Some(true)
    } else if read.exists(&orig) {
        Some(false)
    } else {
        None
    }
}

/// optid-safety: remove the `applied_<key>` marker and the `original_<key>`
/// journal after a successful revert. Called by the revert functions once
/// they have restored the original value. Best-effort; a failure to remove
/// the marker is logged but does not propagate (the next revert will simply
/// re-restore from `original_<key>`, which is idempotent).
#[cfg(test)]
pub(crate) fn clear_journal(state_dir: &Path, key: &str) {
    clear_journal_with(&RealKernel::new(), state_dir, key)
}

/// F2: injectable clear_journal for tests.
pub(crate) fn clear_journal_with(write: &dyn KernelWrite, state_dir: &Path, key: &str) {
    let _ = write.remove_file(&state_dir.join(format!("applied_{key}")));
    let _ = write.remove_file(&state_dir.join(format!("original_{key}")));
    let _ = write.remove_file(&state_dir.join(format!("intended_{key}")));
}

pub(crate) fn append_log(path: &Path, text: &str) -> io::Result<()> {
    append_log_with(&RealKernel::new(), path, text)
}

/// F2: injectable append_log for tests.
pub(crate) fn append_log_with(write: &dyn KernelWrite, path: &Path, text: &str) -> io::Result<()> {
    write.append(path, text)
}

pub(crate) fn get_path_hash(path: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel_io::{FaultKernel, MemoryKernel};

    #[test]
    fn guarded_write_rejects_directory_traversal() {
        let tmp = std::env::temp_dir().join("optid_guarded_write_test");
        let _ = fs::create_dir_all(&tmp);
        let target = tmp.join("..").join("optid_traversal_target");
        let res = guarded_write(&target, "x");
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn guarded_write_rejects_unallowlisted_paths() {
        let tmp = std::env::temp_dir().join("optid_unallowlisted_test");
        let _ = fs::create_dir_all(&tmp);
        let target = tmp.join("evil.conf");
        let res = guarded_write(&target, "x");
        assert!(res.is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn atomic_write_persists_content() {
        let tmp = std::env::temp_dir().join(format!("optid_atomic_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);
        let target = tmp.join("state.txt");
        atomic_write_state_file(&target, "hello").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn append_log_creates_and_appends() {
        let tmp = std::env::temp_dir().join(format!("optid_log_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);
        let log = tmp.join("actions.log");
        append_log(&log, "line1\n").unwrap();
        append_log(&log, "line2\n").unwrap();
        let content = fs::read_to_string(&log).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn atomic_state_write_fault_does_not_publish_target() {
        let state_dir = Path::new("/run/optid-f2-atomic");
        let target = state_dir.join("original_vm_swappiness");
        let tmp = target.with_extension("tmp");
        let fault = FaultKernel::new(Box::new(MemoryKernel::new()));
        fault.fail_next_write(tmp, io::ErrorKind::PermissionDenied);

        let error = atomic_write_state_file_with(&fault, &target, "60").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(!fault.exists(&target), "failed atomic write must not publish");
    }

    #[test]
    fn append_log_fault_is_injected_through_kernel_seam() {
        let path = Path::new("/run/optid-f2-log/actions.log");
        let fault = FaultKernel::new(Box::new(MemoryKernel::new()));
        fault.fail_next_write(path.to_path_buf(), io::ErrorKind::Other);

        let error = append_log_with(&fault, path, "action\n").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(!fault.exists(path));
    }

    #[test]
    fn injected_sysctl_recovery_restores_and_clears_journal() {
        let state_dir = Path::new("/run/optid-f2-recovery");
        let original = state_dir.join("original_vm_swappiness");
        let applied = state_dir.join("applied_vm_swappiness");
        let target = Path::new("/proc/sys/vm/swappiness");
        let memory = MemoryKernel::new();
        memory.write_raw(&original, "60\n");
        memory.write_raw(&applied, "marker");
        memory.write_raw(target, "100");
        let fault = FaultKernel::new(Box::new(memory));

        revert_sysctls_with(&fault, state_dir);

        assert_eq!(fault.read_to_string(target).unwrap(), "60");
        assert!(!fault.exists(&original));
        assert!(!fault.exists(&applied));
    }

    #[test]
    fn injected_sysctl_recovery_failure_keeps_journal_for_retry() {
        let state_dir = Path::new("/run/optid-f2-recovery-failure");
        let original = state_dir.join("original_vm_swappiness");
        let applied = state_dir.join("applied_vm_swappiness");
        let target = Path::new("/proc/sys/vm/swappiness");
        let memory = MemoryKernel::new();
        memory.write_raw(&original, "60\n");
        memory.write_raw(&applied, "marker");
        memory.write_raw(target, "100");
        let fault = FaultKernel::new(Box::new(memory));
        fault.fail_next_write(target.to_path_buf(), io::ErrorKind::PermissionDenied);

        revert_sysctls_with(&fault, state_dir);

        assert_eq!(fault.read_to_string(target).unwrap(), "100");
        assert!(fault.exists(&original));
        assert!(fault.exists(&applied));
    }

    #[test]
    fn get_path_hash_is_stable() {
        let p = Path::new("/sys/bus/pci/devices/0000:00:00.0/power/pm_qos_resume_latency_us");
        assert_eq!(get_path_hash(p), get_path_hash(p));
    }

    #[test]
    fn get_path_hash_differs_for_different_paths() {
        let a = Path::new("/sys/bus/pci/devices/0000:00:00.0/power/pm_qos_resume_latency_us");
        let b = Path::new("/sys/bus/pci/devices/0000:00:01.0/power/pm_qos_resume_latency_us");
        assert_ne!(get_path_hash(a), get_path_hash(b));
    }

    #[test]
    fn failed_revert_keeps_journal_for_retry() {
        let state = std::env::temp_dir().join(format!("optid_revert_{}", std::process::id()));
        fs::create_dir_all(&state).unwrap();
        let key = "dev_failed";
        let original = state.join(format!("original_{key}"));
        let applied = state.join(format!("applied_{key}"));
        fs::write(&original, "/tmp/not-an-allowed-optid-path\n42\n").unwrap();
        fs::write(&applied, "test marker").unwrap();

        revert_pm_qos(&state);

        assert!(original.exists(), "original value must remain retryable");
        assert!(applied.exists(), "applied marker must remain retryable");
        let _ = fs::remove_dir_all(&state);
    }
}
