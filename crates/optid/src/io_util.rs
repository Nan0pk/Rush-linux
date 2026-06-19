//! Low-level I/O utilities for `optid`:
//!
//! - `guarded_write` — the single funnel for all sysfs/procfs writes. Enforces
//!   the allowlist from ADR 0009 and rejects directory traversal per the M1
//!   hardening (PR #101).
//! - `atomic_write_state_file` — write-then-rename so a SIGKILL between the
//!   write and the rename can never leave a truncated `original_*` or
//!   `intended_*` revert-journal entry.
//! - `revert_sysctls` / `revert_pm_qos` — restore journaled previous values on
//!   startup/shutdown so `optid` never leaves a host in a half-actuated state.
//! - `append_log`, `get_path_hash`, `now_unix` — small shared helpers.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

pub(crate) fn guarded_write(path: &Path, value: &str) -> io::Result<()> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to write path with directory traversal: {}",
                path.display()
            ),
        ));
    }

    // Structural check for the per-PCI-device PM QoS resume-latency file.
    // Must be exactly `…/power/pm_qos_resume_latency_us` — not a substring of
    // some other file name. Compare via Path::file_name() rather than
    // stringifying the path.
    fn is_pm_qos_resume_latency(path: &Path) -> bool {
        path.file_name().and_then(|n| n.to_str()) == Some("pm_qos_resume_latency_us")
            && path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                == Some("power")
    }

    // WP-N5 runtime-PM attributes: `…/power/control` and
    // `…/power/autosuspend_delay_ms`. Same structural check as above — the file
    // must be a direct child of a `power` directory, matched via file_name()
    // (never a substring of some other path). These are additional ADR-0009
    // write-allowlist entries; they do not relax any existing entry.
    fn is_runtime_pm_attr(path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str());
        let parent_is_power = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("power");
        parent_is_power && matches!(name, Some("control") | Some("autosuspend_delay_ms"))
    }

    // WP-N6 storage/link PM attributes. `…/link/l1_aspm` (per-device PCIe ASPM)
    // must be a direct child of a `link` directory; `link_power_management_policy`
    // (SATA ALPM) is a scsi_host attribute matched by file name. Additional
    // ADR-0009 write-allowlist entries; existing entries are untouched.
    fn is_storage_pm_attr(path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str());
        let parent_is_link = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("link");
        (parent_is_link && name == Some("l1_aspm")) || name == Some("link_power_management_policy")
    }

    // WP-N7 backlight: `…/backlight/<name>/brightness`. The file must be named
    // `brightness` and its grandparent directory must be `backlight`, so it can
    // never match an unrelated `brightness` file elsewhere in sysfs. Additional
    // ADR-0009 write-allowlist entry; existing entries are untouched.
    fn is_backlight_attr(path: &Path) -> bool {
        path.file_name().and_then(|n| n.to_str()) == Some("brightness")
            && path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|gp| gp.file_name())
                .and_then(|n| n.to_str())
                == Some("backlight")
    }

    let allowed = path == Path::new("/sys/firmware/acpi/platform_profile")
        || path.starts_with("/sys/devices/system/cpu/")
        || path == Path::new("/proc/sys/vm/swappiness")
        || path == Path::new("/proc/sys/vm/dirty_background_bytes")
        || path == Path::new("/proc/sys/vm/dirty_bytes")
        || (path.starts_with("/sys/") && is_pm_qos_resume_latency(path))
        || (path.starts_with("/sys/") && is_runtime_pm_attr(path))
        || (path.starts_with("/sys/") && is_storage_pm_attr(path))
        || (path.starts_with("/sys/") && is_backlight_attr(path))
        || (cfg!(test) && is_pm_qos_resume_latency(path))
        || (cfg!(test) && is_runtime_pm_attr(path))
        || (cfg!(test) && is_storage_pm_attr(path))
        || (cfg!(test) && is_backlight_attr(path));

    if !allowed {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to write unallowlisted path {}", path.display()),
        ));
    }

    fs::write(path, value)
}

pub(crate) fn revert_sysctls(state_dir: &Path) {
    let keys = [
        "vm_swappiness",
        "vm_dirty_background_bytes",
        "vm_dirty_bytes",
    ];
    for key in &keys {
        let orig_path = state_dir.join(format!("original_{key}"));
        if orig_path.exists() {
            if let Ok(orig_val) = fs::read_to_string(&orig_path) {
                let sysctl_name = key.replace('_', ".");
                let sysctl_path =
                    PathBuf::from(format!("/proc/sys/{}", sysctl_name.replace('.', "/")));
                if let Err(e) = guarded_write(&sysctl_path, orig_val.trim()) {
                    eprintln!("optid: failed to revert sysctl {sysctl_name}: {e}");
                } else {
                    println!("optid: reverted sysctl {sysctl_name} to {orig_val}");
                }
            }
            let _ = fs::remove_file(&orig_path);
            let _ = fs::remove_file(state_dir.join(format!("intended_{key}")));
        }
    }
}

pub(crate) fn revert_pm_qos(state_dir: &Path) {
    let Ok(entries) = fs::read_dir(state_dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("original_dev_") {
            let orig_path = entry.path();
            if let Ok(content) = fs::read_to_string(&orig_path) {
                let mut lines = content.lines();
                if let (Some(dev_path_str), Some(orig_val)) = (lines.next(), lines.next()) {
                    let dev_path = Path::new(dev_path_str);
                    if let Err(e) = guarded_write(dev_path, orig_val.trim()) {
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
                    }
                }
            }
            let _ = fs::remove_file(&orig_path);
            let hash = name_str.strip_prefix("original_dev_").unwrap_or("");
            if !hash.is_empty() {
                let intended_path = state_dir.join(format!("intended_dev_{hash}"));
                let _ = fs::remove_file(intended_path);
            }
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
    let Ok(entries) = fs::read_dir(state_dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("original_rpm_") {
            continue;
        }
        let orig_path = entry.path();
        if let Ok(content) = fs::read_to_string(&orig_path) {
            let mut lines = content.lines();
            if let (Some(dev_dir), Some(orig_control)) = (lines.next(), lines.next()) {
                let dev_dir = Path::new(dev_dir);
                let control_path = dev_dir.join("power").join("control");
                if let Err(e) = guarded_write(&control_path, orig_control.trim()) {
                    eprintln!(
                        "optid: failed to revert runtime PM control for {}: {e}",
                        dev_dir.display()
                    );
                } else {
                    println!(
                        "optid: reverted runtime PM control for {} to {}",
                        dev_dir.display(),
                        orig_control.trim()
                    );
                }
                if let Some(orig_delay) = lines.next() {
                    let orig_delay = orig_delay.trim();
                    if orig_delay != "n/a" {
                        let delay_path = dev_dir.join("power").join("autosuspend_delay_ms");
                        let _ = guarded_write(&delay_path, orig_delay);
                    }
                }
            }
        }
        let _ = fs::remove_file(&orig_path);
        let hash = name_str.strip_prefix("original_rpm_").unwrap_or("");
        if !hash.is_empty() {
            let _ = fs::remove_file(state_dir.join(format!("intended_rpm_{hash}")));
        }
    }
}

/// WP-N6: restore PCIe ASPM (`link/l1_aspm`) and SATA ALPM
/// (`link_power_management_policy`) to their journaled originals on
/// startup/shutdown. Each `original_aspm_<hash>` / `original_alpm_<hash>` file
/// holds two lines: the base directory and the original attribute value.
pub(crate) fn revert_storage(state_dir: &Path) {
    let Ok(entries) = fs::read_dir(state_dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let (prefix, rel): (&str, &[&str]) = if name_str.starts_with("original_aspm_") {
            ("original_aspm_", &["link", "l1_aspm"])
        } else if name_str.starts_with("original_alpm_") {
            ("original_alpm_", &["link_power_management_policy"])
        } else {
            continue;
        };
        let orig_path = entry.path();
        if let Ok(content) = fs::read_to_string(&orig_path) {
            let mut lines = content.lines();
            if let (Some(base), Some(orig_val)) = (lines.next(), lines.next()) {
                let mut target = Path::new(base).to_path_buf();
                for seg in rel {
                    target = target.join(seg);
                }
                if let Err(e) = guarded_write(&target, orig_val.trim()) {
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
                }
            }
        }
        let _ = fs::remove_file(&orig_path);
        let hash = name_str.strip_prefix(prefix).unwrap_or("");
        if !hash.is_empty() {
            let intended_prefix = if prefix == "original_aspm_" {
                "intended_aspm_"
            } else {
                "intended_alpm_"
            };
            let _ = fs::remove_file(state_dir.join(format!("{intended_prefix}{hash}")));
        }
    }
}

/// WP-N7: restore panel backlight `brightness` to its journaled original on
/// startup/shutdown. Each `original_bl_<hash>` file holds two lines: the
/// backlight device directory and the original raw brightness value.
pub(crate) fn revert_display(state_dir: &Path) {
    let Ok(entries) = fs::read_dir(state_dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("original_bl_") {
            continue;
        }
        let orig_path = entry.path();
        if let Ok(content) = fs::read_to_string(&orig_path) {
            let mut lines = content.lines();
            if let (Some(dev_dir), Some(orig_val)) = (lines.next(), lines.next()) {
                let target = Path::new(dev_dir).join("brightness");
                if let Err(e) = guarded_write(&target, orig_val.trim()) {
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
                }
            }
        }
        let _ = fs::remove_file(&orig_path);
        let hash = name_str.strip_prefix("original_bl_").unwrap_or("");
        if !hash.is_empty() {
            let _ = fs::remove_file(state_dir.join(format!("intended_bl_{hash}")));
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
pub(crate) fn atomic_write_state_file(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(tmp, path)
}

pub(crate) fn append_log(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(text.as_bytes())
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
}
