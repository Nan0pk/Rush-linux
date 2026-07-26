//! F2 — Injectable kernel I/O, clock, and event boundaries.
//!
//! This module is the **test seam** that the OPTID-COMPLETION-PLAN F2 package
//! calls for. Sensors, actuators, and the recovery/revert path previously
//! called `std::fs`, `std::time`, and `std::thread` directly, which made
//! fault-injection and deterministic simulation impossible outside the
//! production kernel. The four traits below narrow those call sites to a
//! small surface that has both a production implementation (`RealKernel`)
//! and a fault-injecting wrapper (`FaultKernel`).
//!
//! ## Design rules (F2 plan)
//!
//! 1. **No behavior change.** Every existing free function in `sensors.rs`
//!    and `io_util.rs` keeps its signature and delegates to `RealKernel`.
//!    The trait methods are exercised by production code through `*_with(io)`
//!    variants; the old entry points call those variants with
//!    `&RealKernel::new()`.
//! 2. **Path canonicalization and permitted roots are centralized here.**
//!    `is_allowlisted_write_path` is the single authority for which sysfs /
//!    procfs paths optid may write. It is extracted verbatim from the
//!    former `io_util::guarded_write` so the allowlist semantics are
//!    unchanged.
//! 3. **No policy redesign.** The traits are mechanical I/O seams. Decision
//!    logic stays in `policy.rs`.
//! 4. **`EventSource` is defined but not yet wired into the main loop.**
//!    Replacing the fixed `thread::sleep` poll with a real event reactor
//!    is package E1. F2 only provides the trait and a sleep-based
//!    production impl so E1 has a stable seam to fill in.
//!
//! ## Trait surface
//!
//! - [`KernelRead`] — `read_to_string`, `read_dir`, `exists`.
//! - [`KernelWrite`] — `write` (allowlist-enforced), `create_dir_all`,
//!   `rename`, `remove_file`, `append`.
//! - [`Clock`] — `now_unix`.
//! - [`EventSource`] — `wait(duration) -> bool` (true = event arrived
//!   before the deadline; false = full duration elapsed).
//! - [`KernelIo`] — combined trait (read + write + clock) for the
//!   actuator, which needs all three.
//!
//! ## Fault injection
//!
//! [`FaultKernel`] wraps any `KernelIo` and injects configurable failures:
//! fail the Nth write to a specific path, make a path disappear after K
//! reads, return malformed content, or deny permission. This is what makes
//! the F2 fault-injection tests deterministic — they exercise the real
//! production code path through `Actuator` with a `FaultKernel` that
//! simulates missing files, EBUSY, short writes, and hot-unplug.

use std::cell::RefCell;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ─────────────────────────────────────────────────────────────────────
// Path canonicalization and permitted roots (centralized here, F2)
// ─────────────────────────────────────────────────────────────────────

/// The single authority for which sysfs / procfs paths optid may write.
///
/// Extracted verbatim from the former `io_util::guarded_write` so the
/// allowlist semantics are bit-for-bit unchanged. The `cfg!(test)` branches
/// are preserved so existing tests that exercise the structural checks
/// against temp-dir paths continue to behave identically.
///
/// Returns `Ok(())` if the path is allowlisted and free of directory
/// traversal; otherwise returns `io::ErrorKind::PermissionDenied`.
///
/// F2: Made `pub` so integration tests in other workspace crates can
/// use this function for test fixtures.
pub fn is_allowlisted_write_path(path: &Path) -> io::Result<()> {
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
    fn is_pm_qos_resume_latency(path: &Path) -> bool {
        path.file_name().and_then(|n| n.to_str()) == Some("pm_qos_resume_latency_us")
            && path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                == Some("power")
    }

    fn is_runtime_pm_attr(path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str());
        let parent_is_power = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("power");
        parent_is_power && matches!(name, Some("control") | Some("autosuspend_delay_ms"))
    }

    fn is_storage_pm_attr(path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str());
        let parent_is_link = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("link");
        (parent_is_link && name == Some("l1_aspm")) || name == Some("link_power_management_policy")
    }

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
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────
// Traits
// ─────────────────────────────────────────────────────────────────────

/// Read-only kernel I/O. Narrow surface for procfs / sysfs reads.
///
/// `read_dir` returns a `Vec<PathBuf>` rather than `fs::ReadDir` so the
/// mock impl can synthesize directory listings without touching the
/// filesystem.
///
/// `read_link` / `canonicalize` are a narrow extension for stable device
/// identity (T1 thermal sensors) without a device database.
///
/// F2: Made `pub` so integration tests in other workspace crates can
/// construct test kernels.
pub trait KernelRead {
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
    fn exists(&self, path: &Path) -> bool;

    /// Read a symlink target (relative or absolute). Used for hwmon `device`
    /// links so sensor identity does not depend on volatile `hwmonN` names.
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::read_link(path)
    }

    /// Canonicalize a path. Default uses `std::fs::canonicalize`.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

/// Write-side kernel I/O. The `write` method enforces the centralized
/// allowlist via [`is_allowlisted_write_path`] before delegating to the
/// underlying filesystem, so every implementation (production and mock)
/// applies the same structural defence.
///
/// F2: Made `pub` so integration tests in other workspace crates can
/// construct test kernels.
pub trait KernelWrite {
    /// Allowlist-enforced write. Returns `PermissionDenied` for paths
    /// outside the permitted roots or containing directory traversal.
    fn write(&self, path: &Path, value: &str) -> io::Result<()>;
    /// Write a daemon-owned state file without the kernel-attribute allowlist.
    ///
    /// Callers are responsible for constraining `path` to the configured
    /// state directory. Keeping this separate from [`KernelWrite::write`]
    /// preserves the sysfs/procfs safety boundary while making journal and
    /// marker writes fault-injectable.
    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    /// Append `text` to `path`, creating the file (and parent dirs) if
    /// absent. Equivalent to `io_util::append_log`.
    fn append(&self, path: &Path, text: &str) -> io::Result<()>;
}

/// Monotonic-ish wall clock for timestamps. Production uses
/// `SystemTime::now().duration_since(UNIX_EPOCH)`.
///
/// F2: Made `pub` so integration tests in other workspace crates can
/// construct test kernels.
pub trait Clock {
    fn now_unix(&self) -> u64;
}

/// Event reactor seam. F2's production impl is a plain `thread::sleep`
/// that always returns `false` (full duration elapsed, no event). E1
/// replaces this with a real reactor (PSI poll, udev hotplug, D-Bus
/// signal) that returns `true` when an event arrives before the deadline.
///
/// F2: Made `pub` so integration tests in other workspace crates can
/// construct test kernels.
#[allow(dead_code)]
pub trait EventSource {
    /// Block for up to `duration`. Return `true` if an event arrived
    /// before the deadline, `false` if the full duration elapsed.
    fn wait(&self, duration: Duration) -> bool;
}

/// Combined trait for the actuator, which needs read + write + clock.
/// Blanket-implemented for any `T: KernelRead + KernelWrite + Clock`.
///
/// F2: Made `pub` so integration tests in other workspace crates can
/// construct test kernels.
pub trait KernelIo: KernelRead + KernelWrite + Clock {}
impl<T: KernelRead + KernelWrite + Clock> KernelIo for T {}

// ─────────────────────────────────────────────────────────────────────
// RealKernel — production implementation
// ─────────────────────────────────────────────────────────────────────

/// Production kernel I/O. Delegates to `std::fs`, `std::time`, and
/// `std::thread`. The default constructor is used by every legacy free
/// function in `sensors.rs` and `io_util.rs` so existing behavior is
/// preserved bit-for-bit.
///
/// F2: Made `pub` so integration tests in other workspace crates can
/// construct test kernels.
#[derive(Default, Clone)]
pub struct RealKernel;

impl RealKernel {
    pub fn new() -> Self {
        Self
    }
}

impl KernelRead for RealKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(path)? {
            out.push(entry?.path());
        }
        Ok(out)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

impl KernelWrite for RealKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        is_allowlisted_write_path(path)?;
        std::fs::write(path, value)
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        std::fs::write(path, value)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        file.write_all(text.as_bytes())
    }
}

impl Clock for RealKernel {
    fn now_unix(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default()
    }
}

impl EventSource for RealKernel {
    fn wait(&self, duration: Duration) -> bool {
        // F2: plain sleep. E1 replaces this with a real event reactor.
        std::thread::sleep(duration);
        false
    }
}

// ─────────────────────────────────────────────────────────────────────
// FaultKernel — fault-injecting wrapper for deterministic tests
// ─────────────────────────────────────────────────────────────────────

/// A fault rule. When it fires, the wrapped I/O call returns the configured
/// error instead of delegating to the inner kernel.
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum FaultRule {
    /// Fail the next `write` to `path` with `error`. Fires once.
    FailWrite { path: PathBuf, error: io::ErrorKind },
    /// Fail the next `read_to_string` of `path` with `error`. Fires once.
    FailRead { path: PathBuf, error: io::ErrorKind },
    /// Make `path` report as non-existent for `exists` / `read_to_string`.
    /// Persists until cleared.
    HidePath { path: PathBuf },
    /// Return `content` instead of the real file contents for
    /// `read_to_string(path)`. Persists until cleared.
    MalformedContent { path: PathBuf, content: String },
    /// Write only the first `n` bytes of the value on the next `write`
    /// to `path`, then return `Ok(())` as if the full write succeeded.
    /// Fires once. Simulates a kernel sysfs attribute that truncates
    /// input (e.g., accepts only the first byte of a multi-byte value).
    ShortWrite { path: PathBuf, n: usize },
    /// Fail the next `rename` from `from` to `to` with `error`. Fires once.
    /// F2: Extended fault injection for recovery/journal path testing.
    FailRename {
        from: PathBuf,
        to: PathBuf,
        error: io::ErrorKind,
    },
    /// Fail the next `remove_file` of `path` with `error`. Fires once.
    /// F2: Extended fault injection for journal cleanup testing.
    FailRemove { path: PathBuf, error: io::ErrorKind },
    /// Fail the next `create_dir_all` of `path` with `error`. Fires once.
    /// F2: Extended fault injection for state directory creation testing.
    FailCreateDir { path: PathBuf, error: io::ErrorKind },
}

/// A fault-injecting wrapper around any `KernelIo`. Used by the F2
/// fault-injection tests to simulate missing files, permission-denied,
/// short writes, and disappearing paths deterministically — without
/// poking at the real kernel.
///
/// Rules are evaluated in insertion order. The first matching rule fires
/// (for one-shot rules) or applies (for persistent rules). One-shot rules
/// are consumed after firing.
///
/// **Not `Clone`** because `io::Error` is not `Clone`. Tests construct a
/// fresh `FaultKernel` per scenario.
///
/// The rules vector is wrapped in a `RefCell` so that one-shot rules can
/// be consumed from `&self` trait methods. `FaultKernel` is `!Sync` by
/// virtue of `RefCell` and is intended for single-threaded test use only.
///
/// F2: Made `pub` so integration tests in other workspace crates can
/// construct test kernels.
#[allow(dead_code)]
pub struct FaultKernel {
    inner: Box<dyn KernelIo>,
    rules: RefCell<Vec<FaultRule>>,
}

#[allow(dead_code)]
impl FaultKernel {
    /// Wrap an inner kernel (typically `Box::new(RealKernel::new())`).
    pub fn new(inner: Box<dyn KernelIo>) -> Self {
        Self {
            inner,
            rules: RefCell::new(Vec::new()),
        }
    }

    /// Fail the next `write` to `path` with the given error kind.
    pub fn fail_next_write(&self, path: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailWrite { path, error });
        self
    }

    /// Make the next `write` to `path` write only the first `n` bytes
    /// of the value, then return `Ok(())` as if the full write
    /// succeeded. Fires once. Simulates a kernel sysfs attribute that
    /// truncates input (e.g., accepts only `"1"` when `"1\n"` is
    /// written, or drops the newline from `"100\n"`).
    ///
    /// If `n` is 0, the write is a no-op (zero bytes written) but
    /// still returns `Ok(())` — simulating a kernel that accepts the
    /// ioctl but stores nothing.
    pub fn fail_next_write_short(&self, path: PathBuf, n: usize) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::ShortWrite { path, n });
        self
    }

    /// Fail the next `read_to_string` of `path` with the given error kind.
    #[allow(dead_code)]
    pub fn fail_next_read(&self, path: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailRead { path, error });
        self
    }

    /// Make `path` report as non-existent (exists = false, read = NotFound).
    pub fn hide_path(&self, path: PathBuf) -> &Self {
        self.rules.borrow_mut().push(FaultRule::HidePath { path });
        self
    }

    /// Return `content` for `read_to_string(path)` instead of the real
    /// file contents.
    pub fn malform_content(&self, path: PathBuf, content: String) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::MalformedContent { path, content });
        self
    }

    /// Fail the next `rename` from `from_path` to `to_path` with the given
    /// error kind. Fires once.
    pub fn fail_next_rename(
        &self,
        from_path: PathBuf,
        to_path: PathBuf,
        error: io::ErrorKind,
    ) -> &Self {
        self.rules.borrow_mut().push(FaultRule::FailRename {
            from: from_path,
            to: to_path,
            error,
        });
        self
    }

    /// Fail the next `remove_file` of `path` with the given error kind.
    /// Fires once.
    pub fn fail_next_remove(&self, path: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailRemove { path, error });
        self
    }

    /// Fail the next `create_dir_all` of `path` with the given error kind.
    /// Fires once.
    pub fn fail_next_create_dir(&self, path: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailCreateDir { path, error });
        self
    }

    /// Take the first matching one-shot read fault for `path`, or None.
    fn take_one_shot_read_fault(&self, path: &Path) -> Option<io::ErrorKind> {
        let mut rules = self.rules.borrow_mut();
        let found_idx = rules
            .iter()
            .position(|r| matches!(r, FaultRule::FailRead { path: p, .. } if p == path));
        found_idx.map(|i| match rules.remove(i) {
            FaultRule::FailRead { error, .. } => error,
            _ => unreachable!(),
        })
    }

    /// Take the first matching one-shot write fault for `path`, or None.
    fn take_one_shot_write_fault(&self, path: &Path) -> Option<io::ErrorKind> {
        let mut rules = self.rules.borrow_mut();
        let found_idx = rules
            .iter()
            .position(|r| matches!(r, FaultRule::FailWrite { path: p, .. } if p == path));
        found_idx.map(|i| match rules.remove(i) {
            FaultRule::FailWrite { error, .. } => error,
            _ => unreachable!(),
        })
    }

    /// Take the first matching one-shot short-write rule for `path`, or None.
    fn take_one_shot_short_write(&self, path: &Path) -> Option<usize> {
        let mut rules = self.rules.borrow_mut();
        let found_idx = rules
            .iter()
            .position(|r| matches!(r, FaultRule::ShortWrite { path: p, .. } if p == path));
        found_idx.map(|i| match rules.remove(i) {
            FaultRule::ShortWrite { n, .. } => n,
            _ => unreachable!(),
        })
    }

    fn path_is_hidden(&self, path: &Path) -> bool {
        self.rules
            .borrow()
            .iter()
            .any(|r| matches!(r, FaultRule::HidePath { path: p } if p == path))
    }

    fn malformed_content_for(&self, path: &Path) -> Option<String> {
        self.rules.borrow().iter().find_map(|r| {
            if let FaultRule::MalformedContent { path: p, content } = r {
                (p == path).then_some(content.clone())
            } else {
                None
            }
        })
    }

    /// Take the first matching one-shot rename fault for `from` -> `to`, or None.
    fn take_one_shot_rename_fault(&self, from: &Path, to: &Path) -> Option<io::ErrorKind> {
        let mut rules = self.rules.borrow_mut();
        let found_idx = rules.iter().position(|r| {
            matches!(
                r,
                FaultRule::FailRename { from: f, to: t, .. } if f == from && t == to
            )
        });
        found_idx.map(|i| match rules.remove(i) {
            FaultRule::FailRename { error, .. } => error,
            _ => unreachable!(),
        })
    }

    /// Take the first matching one-shot remove fault for `path`, or None.
    fn take_one_shot_remove_fault(&self, path: &Path) -> Option<io::ErrorKind> {
        let mut rules = self.rules.borrow_mut();
        let found_idx = rules
            .iter()
            .position(|r| matches!(r, FaultRule::FailRemove { path: p, .. } if p == path));
        found_idx.map(|i| match rules.remove(i) {
            FaultRule::FailRemove { error, .. } => error,
            _ => unreachable!(),
        })
    }

    /// Take the first matching one-shot create_dir fault for `path`, or None.
    fn take_one_shot_create_dir_fault(&self, path: &Path) -> Option<io::ErrorKind> {
        let mut rules = self.rules.borrow_mut();
        let found_idx = rules
            .iter()
            .position(|r| matches!(r, FaultRule::FailCreateDir { path: p, .. } if p == path));
        found_idx.map(|i| match rules.remove(i) {
            FaultRule::FailCreateDir { error, .. } => error,
            _ => unreachable!(),
        })
    }
}

impl KernelRead for FaultKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        // Persistent rules first.
        if self.path_is_hidden(path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("FaultKernel: path hidden for test: {}", path.display()),
            ));
        }
        if let Some(content) = self.malformed_content_for(path) {
            return Ok(content);
        }
        // One-shot rule.
        if let Some(kind) = self.take_one_shot_read_fault(path) {
            return Err(io::Error::new(
                kind,
                format!("FaultKernel: injected read fault for {}", path.display()),
            ));
        }
        self.inner.read_to_string(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        // For hidden paths, return an empty directory listing (simulates
        // hot-unplug: the bus directory still exists but is empty).
        if self.path_is_hidden(path) {
            return Ok(Vec::new());
        }
        self.inner.read_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        if self.path_is_hidden(path) {
            return false;
        }
        self.inner.exists(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        if self.path_is_hidden(path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("FaultKernel: path hidden for test: {}", path.display()),
            ));
        }
        self.inner.read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        if self.path_is_hidden(path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("FaultKernel: path hidden for test: {}", path.display()),
            ));
        }
        self.inner.canonicalize(path)
    }
}

impl KernelWrite for FaultKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        // Allowlist check first (same as RealKernel).
        is_allowlisted_write_path(path)?;
        // One-shot write fault (hard error).
        if let Some(kind) = self.take_one_shot_write_fault(path) {
            return Err(io::Error::new(
                kind,
                format!("FaultKernel: injected write fault for {}", path.display()),
            ));
        }
        // One-shot short-write fault: write only the first `n` bytes,
        // then return Ok(()) as if the full write succeeded. This
        // simulates a kernel that truncates input silently. When n=0,
        // the write is a no-op (zero bytes written) but still returns
        // Ok(()) — simulating a kernel that accepts the ioctl but
        // stores nothing.
        if let Some(n) = self.take_one_shot_short_write(path) {
            if n == 0 {
                return Ok(());
            }
            let truncated = if value.len() <= n { value } else { &value[..n] };
            return self.inner.write(path, truncated);
        }
        self.inner.write(path, value)
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        if let Some(kind) = self.take_one_shot_write_fault(path) {
            return Err(io::Error::new(
                kind,
                format!(
                    "FaultKernel: injected state-file write fault for {}",
                    path.display()
                ),
            ));
        }
        if let Some(n) = self.take_one_shot_short_write(path) {
            if n == 0 {
                return Ok(());
            }
            let truncated = if value.len() <= n { value } else { &value[..n] };
            return self.inner.write_state_file(path, truncated);
        }
        self.inner.write_state_file(path, value)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.take_one_shot_create_dir_fault(path) {
            return Err(io::Error::new(
                kind,
                format!(
                    "FaultKernel: injected create_dir_all fault for {}",
                    path.display()
                ),
            ));
        }
        self.inner.create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if let Some(kind) = self.take_one_shot_rename_fault(from, to) {
            return Err(io::Error::new(
                kind,
                format!(
                    "FaultKernel: injected rename fault from {} to {}",
                    from.display(),
                    to.display()
                ),
            ));
        }
        self.inner.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.take_one_shot_remove_fault(path) {
            return Err(io::Error::new(
                kind,
                format!(
                    "FaultKernel: injected remove_file fault for {}",
                    path.display()
                ),
            ));
        }
        self.inner.remove_file(path)
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        if let Some(kind) = self.take_one_shot_write_fault(path) {
            return Err(io::Error::new(
                kind,
                format!("FaultKernel: injected append fault for {}", path.display()),
            ));
        }
        self.inner.append(path, text)
    }
}

impl Clock for FaultKernel {
    fn now_unix(&self) -> u64 {
        self.inner.now_unix()
    }
}

// Note: FaultKernel does NOT implement EventSource because KernelIo
// (the combined trait used by the actuator) is Read + Write + Clock only.
// The main-loop event reactor is package E1's job.

// ─────────────────────────────────────────────────────────────────────
// MemoryKernel — in-memory KernelIo for unit tests (crate-wide under cfg(test))
// ─────────────────────────────────────────────────────────────────────

/// Minimal in-memory kernel. Stores file contents and directory listings.
/// Available to all optid unit tests (thermal, sensors, F2).
///
/// **Note:** `write` still enforces the allowlist. Tests that need to
/// populate arbitrary sysfs fixtures should use `write_raw` / `add_dir`.
///
/// F2: This struct is enabled via the `test-utils` feature for integration
/// tests that need to construct a MemoryKernel from outside the optid crate,
/// or automatically during `cargo test` for in-crate tests.
#[cfg(any(test, feature = "test-utils"))]
pub struct MemoryKernel {
    files: std::sync::Mutex<std::collections::HashMap<PathBuf, String>>,
    dirs: std::sync::Mutex<std::collections::HashMap<PathBuf, Vec<PathBuf>>>,
    links: std::sync::Mutex<std::collections::HashMap<PathBuf, PathBuf>>,
    clock: std::sync::atomic::AtomicU64,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for MemoryKernel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl MemoryKernel {
    pub fn new() -> Self {
        Self {
            files: std::sync::Mutex::new(std::collections::HashMap::new()),
            dirs: std::sync::Mutex::new(std::collections::HashMap::new()),
            links: std::sync::Mutex::new(std::collections::HashMap::new()),
            clock: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Advance the in-memory clock by `secs`.
    pub fn advance_clock(&self, secs: u64) {
        self.clock
            .fetch_add(secs, std::sync::atomic::Ordering::Relaxed);
    }

    /// Test-only raw write that bypasses the allowlist.
    pub fn write_raw(&self, path: &Path, value: &str) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), value.to_string());
    }

    /// Record a symlink at `path` pointing to `target`.
    pub fn write_link(&self, path: &Path, target: &Path) {
        self.links
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), target.to_path_buf());
    }

    /// Register `child` as an entry of directory `parent` (creates parent listing).
    pub fn add_dir(&self, parent: &Path, child: &Path) {
        self.dirs
            .lock()
            .unwrap()
            .entry(parent.to_path_buf())
            .or_default()
            .push(child.to_path_buf());
    }

    /// Register a file path as a directory listing entry under its parent
    /// directory already present, or under `dir` explicitly.
    pub fn add_dir_entry(&self, dir: &Path, entry: &Path) {
        self.dirs
            .lock()
            .unwrap()
            .entry(dir.to_path_buf())
            .or_default()
            .push(entry.to_path_buf());
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl KernelRead for MemoryKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("MemoryKernel: no such file: {}", path.display()),
                )
            })
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        Ok(self
            .dirs
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or_default())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains_key(path)
            || self.dirs.lock().unwrap().contains_key(path)
            || self.links.lock().unwrap().contains_key(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.links
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("MemoryKernel: no such link: {}", path.display()),
                )
            })
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        // Normalize `..` and `.` without touching the real filesystem.
        let mut out = PathBuf::new();
        for c in path.components() {
            match c {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                other => out.push(other.as_os_str()),
            }
        }
        if out.as_os_str().is_empty() {
            out.push("/");
        }
        Ok(out)
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl KernelWrite for MemoryKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        is_allowlisted_write_path(path)?;
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), value.to_string());
        Ok(())
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), value.to_string());
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.dirs
            .lock()
            .unwrap()
            .entry(path.to_path_buf())
            .or_default();
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        if let Some(content) = files.remove(from) {
            files.insert(to.to_path_buf(), content);
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "MemoryKernel: rename source not found",
            ))
        }
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files
            .lock()
            .unwrap()
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "MemoryKernel: remove_file not found",
                )
            })
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        let entry = files.entry(path.to_path_buf()).or_default();
        entry.push_str(text);
        Ok(())
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl Clock for MemoryKernel {
    fn now_unix(&self) -> u64 {
        self.clock.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
impl EventSource for MemoryKernel {
    fn wait(&self, _duration: Duration) -> bool {
        false
    }
}

// ─────────────────────────────────────────────────────────────────────
// Tests for the kernel_io seam itself
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// The allowlist check is the centralized path-validation authority.
    /// Verify it rejects traversal and unallowlisted paths identically to
    /// the former `io_util::guarded_write`.
    #[test]
    fn allowlist_rejects_directory_traversal() {
        let p = Path::new("/sys/devices/system/cpu/cpu0/cpufreq/../../../../etc/passwd");
        assert!(is_allowlisted_write_path(p).is_err());
    }

    #[test]
    fn allowlist_accepts_cpu_epp_path() {
        let p = Path::new("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference");
        assert!(is_allowlisted_write_path(p).is_ok());
    }

    #[test]
    fn allowlist_accepts_vm_swappiness() {
        assert!(is_allowlisted_write_path(Path::new("/proc/sys/vm/swappiness")).is_ok());
    }

    #[test]
    fn allowlist_rejects_random_temp_path_in_test_build() {
        // The cfg!(test) branches only relax the *structural* checks for
        // pm_qos / runtime_pm / storage / backlight attrs — they do NOT
        // allow arbitrary temp paths.
        let p = Path::new("/tmp/definitely-not-allowlisted");
        assert!(is_allowlisted_write_path(p).is_err());
    }

    #[test]
    fn real_kernel_write_enforces_allowlist() {
        let k = RealKernel::new();
        let tmp = std::env::temp_dir().join("optid_kernel_io_real_write_test");
        let _ = std::fs::create_dir_all(&tmp);
        let target = tmp.join("evil.conf");
        let res = k.write(&target, "x");
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn real_kernel_read_dir_returns_entries() {
        let tmp =
            std::env::temp_dir().join(format!("optid_kernel_io_readdir_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("a"), "1").unwrap();
        std::fs::write(tmp.join("b"), "2").unwrap();
        let k = RealKernel::new();
        let mut entries = k.read_dir(&tmp).unwrap();
        entries.sort();
        assert_eq!(entries.len(), 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn real_kernel_clock_advances() {
        let k = RealKernel::new();
        let t1 = k.now_unix();
        std::thread::sleep(Duration::from_millis(1100));
        let t2 = k.now_unix();
        assert!(t2 > t1, "clock must advance: t1={t1} t2={t2}");
    }

    #[test]
    fn real_kernel_event_source_sleeps_full_duration() {
        let k = RealKernel::new();
        let start = SystemTime::now();
        let interrupted = k.wait(Duration::from_millis(100));
        let elapsed = start.elapsed().unwrap();
        assert!(!interrupted, "F2 RealKernel.wait always returns false");
        assert!(elapsed >= Duration::from_millis(90));
    }

    #[test]
    fn fault_kernel_hides_path() {
        let tmp = std::env::temp_dir().join(format!("optid_fault_hide_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let target = tmp.join("real_file");
        std::fs::write(&target, "real").unwrap();

        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        fk.hide_path(target.clone());

        assert!(!fk.exists(&target), "hidden path must report not-exists");
        let res = fk.read_to_string(&target);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), io::ErrorKind::NotFound);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fault_kernel_malforms_content() {
        let tmp = std::env::temp_dir().join(format!("optid_fault_malform_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let target = tmp.join("psi");
        std::fs::write(&target, "some avg10=1.00 avg60=1.00 avg300=1.00 total=1\n").unwrap();

        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        fk.malform_content(target.clone(), "garbage not a psi line at all".to_string());

        let content = fk.read_to_string(&target).unwrap();
        assert_eq!(content, "garbage not a psi line at all");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fault_kernel_fail_next_write_fires_once() {
        // Use an allowlisted path so the allowlist check passes and the
        // fault rule is the thing that fires.
        let path = Path::new("/proc/sys/vm/swappiness");
        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        fk.fail_next_write(path.to_path_buf(), io::ErrorKind::Other);

        // First write: fault fires.
        let res1 = fk.write(path, "60");
        assert!(res1.is_err());
        assert_eq!(res1.unwrap_err().kind(), io::ErrorKind::Other);

        // Second write: no fault rule left. The write will likely fail
        // with PermissionDenied (we're not root in tests), but the
        // important thing is it's NOT the injected Other error.
        let res2 = fk.write(path, "60");
        if let Err(e) = res2 {
            assert_ne!(
                e.kind(),
                io::ErrorKind::Other,
                "second write must not fire the consumed fault rule"
            );
        }
    }

    #[test]
    fn fault_kernel_fail_next_read_fires_once() {
        let tmp = std::env::temp_dir().join(format!("optid_fault_read_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let target = tmp.join("data");
        std::fs::write(&target, "real").unwrap();

        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        fk.fail_next_read(target.clone(), io::ErrorKind::PermissionDenied);

        let res1 = fk.read_to_string(&target);
        assert!(res1.is_err());
        assert_eq!(res1.unwrap_err().kind(), io::ErrorKind::PermissionDenied);

        // Second read: fault consumed, real content returned.
        let res2 = fk.read_to_string(&target).unwrap();
        assert_eq!(res2, "real");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fault_kernel_hidden_dir_returns_empty_listing() {
        let tmp = std::env::temp_dir().join(format!("optid_fault_dir_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("dev1"), "x").unwrap();
        std::fs::write(tmp.join("dev2"), "x").unwrap();

        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        fk.hide_path(tmp.clone());

        let entries = fk.read_dir(&tmp).unwrap();
        assert!(entries.is_empty(), "hidden dir must return empty listing");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Verify the blanket `KernelIo` impl covers `RealKernel`.
    #[test]
    fn blanket_kernel_io_covers_real_kernel() {
        let k = RealKernel::new();
        // If this compiles, the blanket impl works.
        fn accepts_kernel_io(_: &dyn KernelIo) {}
        accepts_kernel_io(&k);
    }

    /// Verify the blanket `KernelIo` impl covers `FaultKernel`.
    #[test]
    fn blanket_kernel_io_covers_fault_kernel() {
        let k = FaultKernel::new(Box::new(RealKernel::new()));
        fn accepts_kernel_io(_: &dyn KernelIo) {}
        accepts_kernel_io(&k);
    }

    /// A simple in-memory kernel for pure unit tests (no filesystem).
    #[test]
    fn memory_kernel_round_trip() {
        let k = MemoryKernel::new();
        // Use an allowlisted path so the MemoryKernel's write() (which
        // enforces the allowlist) accepts it.
        let path = Path::new("/proc/sys/vm/swappiness");
        k.write(path, "42").unwrap();
        assert_eq!(k.read_to_string(path).unwrap(), "42");
        assert!(k.exists(path));
        k.write(path, "100").unwrap();
        assert_eq!(k.read_to_string(path).unwrap(), "100");
    }

    #[test]
    fn memory_kernel_advance_clock() {
        let k = MemoryKernel::new();
        assert_eq!(k.now_unix(), 0);
        k.advance_clock(42);
        assert_eq!(k.now_unix(), 42);
        k.advance_clock(8);
        assert_eq!(k.now_unix(), 50);
    }

    // ── Short-write injection (F2 blocker: "short-write injection is
    // missing") ──────────────────────────────────────────────────────

    /// `fail_next_write_short` writes only the first `n` bytes and
    /// returns `Ok(())`, simulating a kernel that truncates input
    /// silently. The inner kernel receives the truncated value.
    #[test]
    fn fault_kernel_short_write_truncates_and_succeeds() {
        let inner = MemoryKernel::new();
        let path = Path::new("/proc/sys/vm/swappiness");
        // Write "100\n" but truncate to 3 bytes ("100").
        inner.write(path, "0").unwrap(); // populate so read works
        let fk = FaultKernel::new(Box::new(inner));
        fk.fail_next_write_short(path.to_path_buf(), 3);

        // The short write returns Ok(()) — the caller thinks it succeeded.
        let result = fk.write(path, "100\n");
        assert!(result.is_ok(), "short write must return Ok(())");

        // The inner MemoryKernel stored only "100" (3 bytes), not "100\n".
        let stored = fk.read_to_string(path).unwrap();
        assert_eq!(stored, "100", "short write must truncate to 3 bytes");
    }

    /// A short write with `n=0` writes nothing but returns `Ok(())`,
    /// simulating a kernel that accepts the ioctl but stores nothing.
    #[test]
    fn fault_kernel_short_write_zero_bytes_writes_nothing() {
        let inner = MemoryKernel::new();
        let path = Path::new("/proc/sys/vm/swappiness");
        inner.write(path, "old").unwrap();
        let fk = FaultKernel::new(Box::new(inner));
        fk.fail_next_write_short(path.to_path_buf(), 0);

        let result = fk.write(path, "new");
        assert!(result.is_ok(), "zero-byte short write must return Ok(())");

        // The inner kernel still has "old" — the short write stored nothing.
        let stored = fk.read_to_string(path).unwrap();
        assert_eq!(
            stored, "old",
            "zero-byte short write must not change the value"
        );
    }

    /// `fail_next_write_short` fires once: the second write is a normal
    /// full write.
    #[test]
    fn fault_kernel_short_write_fires_once() {
        let inner = MemoryKernel::new();
        let path = Path::new("/proc/sys/vm/swappiness");
        inner.write(path, "init").unwrap();
        let fk = FaultKernel::new(Box::new(inner));
        fk.fail_next_write_short(path.to_path_buf(), 2);

        // First write: short (2 bytes of "100" = "10").
        fk.write(path, "100").unwrap();
        assert_eq!(fk.read_to_string(path).unwrap(), "10");

        // Second write: full ("200").
        fk.write(path, "200").unwrap();
        assert_eq!(fk.read_to_string(path).unwrap(), "200");
    }

    /// `fail_next_write_short` with `n` >= value length writes the full
    /// value (no truncation needed).
    #[test]
    fn fault_kernel_short_write_n_ge_value_writes_full() {
        let inner = MemoryKernel::new();
        let path = Path::new("/proc/sys/vm/swappiness");
        inner.write(path, "init").unwrap();
        let fk = FaultKernel::new(Box::new(inner));
        // n=100 is larger than "42" (2 bytes), so no truncation.
        fk.fail_next_write_short(path.to_path_buf(), 100);

        fk.write(path, "42").unwrap();
        assert_eq!(fk.read_to_string(path).unwrap(), "42");
    }

    /// `fail_next_rename` injects a fault that fires once.
    #[test]
    fn fault_kernel_fail_next_rename() {
        let inner = MemoryKernel::new();
        let from = Path::new("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
        let to = Path::new("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor.bak");
        inner.write_raw(from, "powersave");
        let fk = FaultKernel::new(Box::new(inner));
        fk.fail_next_rename(
            from.to_path_buf(),
            to.to_path_buf(),
            io::ErrorKind::PermissionDenied,
        );

        // First rename: fault fires.
        let res = fk.rename(from, to);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), io::ErrorKind::PermissionDenied);

        // Second rename: no fault left, succeeds (MemoryKernel allows it).
        let res2 = fk.rename(from, to);
        assert!(
            res2.is_ok(),
            "second rename must not fire the consumed fault rule"
        );
    }

    /// `fail_next_remove` injects a fault that fires once.
    #[test]
    fn fault_kernel_fail_next_remove() {
        let inner = MemoryKernel::new();
        let path = Path::new("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
        inner.write_raw(path, "powersave");
        let fk = FaultKernel::new(Box::new(inner));
        fk.fail_next_remove(path.to_path_buf(), io::ErrorKind::NotFound);

        // First remove: fault fires.
        let res = fk.remove_file(path);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), io::ErrorKind::NotFound);

        // Second remove: no fault left, succeeds (MemoryKernel allows it).
        let res2 = fk.remove_file(path);
        assert!(
            res2.is_ok(),
            "second remove must not fire the consumed fault rule"
        );
    }

    /// `fail_next_create_dir` injects a fault that fires once.
    #[test]
    fn fault_kernel_fail_next_create_dir() {
        let inner = MemoryKernel::new();
        let path = Path::new("/sys/devices/system/cpu/cpu0/cpufreq/new_dir");
        let fk = FaultKernel::new(Box::new(inner));
        fk.fail_next_create_dir(path.to_path_buf(), io::ErrorKind::AlreadyExists);

        // First create_dir_all: fault fires.
        let res = fk.create_dir_all(path);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), io::ErrorKind::AlreadyExists);

        // Second create_dir_all: no fault left, succeeds (MemoryKernel allows it).
        let res2 = fk.create_dir_all(path);
        assert!(
            res2.is_ok(),
            "second create_dir_all must not fire the consumed fault rule"
        );
    }

    /// `fail_next_rename` fires once: the second rename succeeds.
    #[test]
    fn fault_kernel_rename_fires_once() {
        let inner = MemoryKernel::new();
        let from = Path::new("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
        let to = Path::new("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor.bak");
        inner.write_raw(from, "powersave");
        let fk = FaultKernel::new(Box::new(inner));
        fk.fail_next_rename(from.to_path_buf(), to.to_path_buf(), io::ErrorKind::Other);

        // First rename: fault fires.
        let res1 = fk.rename(from, to);
        assert!(res1.is_err());
        assert_eq!(res1.unwrap_err().kind(), io::ErrorKind::Other);

        // Second rename: no fault rule left.
        let res2 = fk.rename(from, to);
        assert!(
            res2.is_ok(),
            "second rename must not fire the consumed fault rule"
        );
    }
}
