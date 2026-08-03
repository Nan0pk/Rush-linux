//! F2 — Injectable kernel I/O, clock, and event boundaries.
//!
//! This is the mechanical implementation behind the production-facing
//! `kernel_io` facade. It centralizes permitted kernel writes and provides the
//! real, fault-injecting, and in-memory implementations used by production and
//! deterministic tests.

use std::cell::RefCell;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The single authority for which sysfs and procfs paths optid may write.
pub fn is_allowlisted_write_path(path: &Path) -> io::Result<()> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to write path with directory traversal: {}",
                path.display()
            ),
        ));
    }

    fn is_pm_qos_resume_latency(path: &Path) -> bool {
        path.file_name().and_then(|name| name.to_str()) == Some("pm_qos_resume_latency_us")
            && path
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                == Some("power")
    }

    fn is_runtime_pm_attr(path: &Path) -> bool {
        let name = path.file_name().and_then(|name| name.to_str());
        let parent_is_power = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some("power");
        parent_is_power && matches!(name, Some("control") | Some("autosuspend_delay_ms"))
    }

    fn is_storage_pm_attr(path: &Path) -> bool {
        let name = path.file_name().and_then(|name| name.to_str());
        let parent_is_link = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some("link");
        (parent_is_link && name == Some("l1_aspm")) || name == Some("link_power_management_policy")
    }

    fn is_backlight_attr(path: &Path) -> bool {
        path.file_name().and_then(|name| name.to_str()) == Some("brightness")
            && path
                .parent()
                .and_then(|parent| parent.parent())
                .and_then(|grandparent| grandparent.file_name())
                .and_then(|name| name.to_str())
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

    if allowed {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to write unallowlisted path {}", path.display()),
        ))
    }
}

/// Read-only kernel I/O used by procfs and sysfs discovery.
pub trait KernelRead {
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
    fn exists(&self, path: &Path) -> bool;

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

/// Write-side kernel and daemon-state I/O.
pub trait KernelWrite {
    fn write(&self, path: &Path, value: &str) -> io::Result<()>;
    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn append(&self, path: &Path, text: &str) -> io::Result<()>;

    fn sync_file(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    fn sync_dir(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }
}

/// Wall-clock source used for journal timestamps.
pub trait Clock {
    fn now_unix(&self) -> u64;
}

/// Event-wait boundary. F2 uses sleep; E1 supplies the real reactor.
pub trait EventSource {
    fn wait(&self, duration: Duration) -> bool;
}

/// Combined read, write, and clock boundary used by the actuator.
pub trait KernelIo: KernelRead + KernelWrite + Clock {}
impl<T: KernelRead + KernelWrite + Clock> KernelIo for T {}

/// Direct production implementation backed by `std`.
#[derive(Clone, Default)]
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
        std::fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect()
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

    fn sync_file(&self, path: &Path) -> io::Result<()> {
        std::fs::OpenOptions::new()
            .read(true)
            .open(path)?
            .sync_all()
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        std::fs::File::open(path)?.sync_all()
    }
}

impl Clock for RealKernel {
    fn now_unix(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default()
    }
}

impl EventSource for RealKernel {
    fn wait(&self, duration: Duration) -> bool {
        std::thread::sleep(duration);
        false
    }
}

#[derive(Clone, Debug)]
enum FaultRule {
    FailWrite {
        path: PathBuf,
        error: io::ErrorKind,
    },
    FailRead {
        path: PathBuf,
        error: io::ErrorKind,
    },
    HidePath {
        path: PathBuf,
    },
    MalformedContent {
        path: PathBuf,
        content: String,
    },
    ShortWrite {
        path: PathBuf,
        bytes: usize,
    },
    FailRename {
        from: PathBuf,
        to: PathBuf,
        error: io::ErrorKind,
    },
    FailRemove {
        path: PathBuf,
        error: io::ErrorKind,
    },
    FailCreateDir {
        path: PathBuf,
        error: io::ErrorKind,
    },
    #[allow(dead_code)]
    FailSyncFile {
        path: PathBuf,
        error: io::ErrorKind,
    },
    #[allow(dead_code)]
    FailSyncDir {
        path: PathBuf,
        error: io::ErrorKind,
    },
}

/// Deterministic one-shot and persistent fault injector.
pub struct FaultKernel {
    inner: Box<dyn KernelIo>,
    rules: RefCell<Vec<FaultRule>>,
}

impl FaultKernel {
    pub fn new(inner: Box<dyn KernelIo>) -> Self {
        Self {
            inner,
            rules: RefCell::new(Vec::new()),
        }
    }

    pub fn fail_next_write(&self, path: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailWrite { path, error });
        self
    }

    pub fn fail_next_write_short(&self, path: PathBuf, bytes: usize) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::ShortWrite { path, bytes });
        self
    }

    pub fn fail_next_read(&self, path: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailRead { path, error });
        self
    }

    pub fn hide_path(&self, path: PathBuf) -> &Self {
        self.rules.borrow_mut().push(FaultRule::HidePath { path });
        self
    }

    pub fn malform_content(&self, path: PathBuf, content: String) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::MalformedContent { path, content });
        self
    }

    pub fn fail_next_rename(&self, from: PathBuf, to: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailRename { from, to, error });
        self
    }

    pub fn fail_next_remove(&self, path: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailRemove { path, error });
        self
    }

    pub fn fail_next_create_dir(&self, path: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailCreateDir { path, error });
        self
    }

    #[allow(dead_code)]
    pub fn fail_next_sync_file(&self, path: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailSyncFile { path, error });
        self
    }

    #[allow(dead_code)]
    pub fn fail_next_sync_dir(&self, path: PathBuf, error: io::ErrorKind) -> &Self {
        self.rules
            .borrow_mut()
            .push(FaultRule::FailSyncDir { path, error });
        self
    }

    fn take_rule<T>(
        &self,
        matches: impl Fn(&FaultRule) -> bool,
        extract: impl FnOnce(FaultRule) -> T,
    ) -> Option<T> {
        let mut rules = self.rules.borrow_mut();
        let index = rules.iter().position(matches)?;
        Some(extract(rules.remove(index)))
    }

    fn take_read_fault(&self, path: &Path) -> Option<io::ErrorKind> {
        self.take_rule(
            |rule| matches!(rule, FaultRule::FailRead { path: candidate, .. } if candidate == path),
            |rule| match rule {
                FaultRule::FailRead { error, .. } => error,
                _ => unreachable!(),
            },
        )
    }

    fn take_write_fault(&self, path: &Path) -> Option<io::ErrorKind> {
        self.take_rule(
            |rule| matches!(rule, FaultRule::FailWrite { path: candidate, .. } if candidate == path),
            |rule| match rule {
                FaultRule::FailWrite { error, .. } => error,
                _ => unreachable!(),
            },
        )
    }

    fn take_short_write(&self, path: &Path) -> Option<usize> {
        self.take_rule(
            |rule| matches!(rule, FaultRule::ShortWrite { path: candidate, .. } if candidate == path),
            |rule| match rule {
                FaultRule::ShortWrite { bytes, .. } => bytes,
                _ => unreachable!(),
            },
        )
    }

    fn take_rename_fault(&self, from: &Path, to: &Path) -> Option<io::ErrorKind> {
        self.take_rule(
            |rule| {
                matches!(
                    rule,
                    FaultRule::FailRename {
                        from: candidate_from,
                        to: candidate_to,
                        ..
                    } if candidate_from == from && candidate_to == to
                )
            },
            |rule| match rule {
                FaultRule::FailRename { error, .. } => error,
                _ => unreachable!(),
            },
        )
    }

    fn take_remove_fault(&self, path: &Path) -> Option<io::ErrorKind> {
        self.take_rule(
            |rule| matches!(rule, FaultRule::FailRemove { path: candidate, .. } if candidate == path),
            |rule| match rule {
                FaultRule::FailRemove { error, .. } => error,
                _ => unreachable!(),
            },
        )
    }

    fn take_create_dir_fault(&self, path: &Path) -> Option<io::ErrorKind> {
        self.take_rule(
            |rule| matches!(rule, FaultRule::FailCreateDir { path: candidate, .. } if candidate == path),
            |rule| match rule {
                FaultRule::FailCreateDir { error, .. } => error,
                _ => unreachable!(),
            },
        )
    }

    fn take_sync_file_fault(&self, path: &Path) -> Option<io::ErrorKind> {
        self.take_rule(
            |rule| matches!(rule, FaultRule::FailSyncFile { path: candidate, .. } if candidate == path),
            |rule| match rule {
                FaultRule::FailSyncFile { error, .. } => error,
                _ => unreachable!(),
            },
        )
    }

    fn take_sync_dir_fault(&self, path: &Path) -> Option<io::ErrorKind> {
        self.take_rule(
            |rule| matches!(rule, FaultRule::FailSyncDir { path: candidate, .. } if candidate == path),
            |rule| match rule {
                FaultRule::FailSyncDir { error, .. } => error,
                _ => unreachable!(),
            },
        )
    }

    fn is_hidden(&self, path: &Path) -> bool {
        self.rules.borrow().iter().any(
            |rule| matches!(rule, FaultRule::HidePath { path: candidate } if candidate == path),
        )
    }

    fn malformed_content(&self, path: &Path) -> Option<String> {
        self.rules.borrow().iter().find_map(|rule| match rule {
            FaultRule::MalformedContent {
                path: candidate,
                content,
            } if candidate == path => Some(content.clone()),
            _ => None,
        })
    }
}

impl KernelRead for FaultKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if self.is_hidden(path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("FaultKernel: path hidden for test: {}", path.display()),
            ));
        }
        if let Some(content) = self.malformed_content(path) {
            return Ok(content);
        }
        if let Some(error) = self.take_read_fault(path) {
            return Err(io::Error::new(
                error,
                format!("FaultKernel: injected read fault for {}", path.display()),
            ));
        }
        self.inner.read_to_string(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        if self.is_hidden(path) {
            Ok(Vec::new())
        } else {
            self.inner.read_dir(path)
        }
    }

    fn exists(&self, path: &Path) -> bool {
        !self.is_hidden(path) && self.inner.exists(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        if self.is_hidden(path) {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("FaultKernel: path hidden for test: {}", path.display()),
            ))
        } else {
            self.inner.read_link(path)
        }
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        if self.is_hidden(path) {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("FaultKernel: path hidden for test: {}", path.display()),
            ))
        } else {
            self.inner.canonicalize(path)
        }
    }
}

impl KernelWrite for FaultKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        is_allowlisted_write_path(path)?;
        if let Some(error) = self.take_write_fault(path) {
            return Err(io::Error::new(
                error,
                format!("FaultKernel: injected write fault for {}", path.display()),
            ));
        }
        if let Some(bytes) = self.take_short_write(path) {
            if bytes == 0 {
                return Ok(());
            }
            let truncated = value.get(..bytes).unwrap_or(value);
            return self.inner.write(path, truncated);
        }
        self.inner.write(path, value)
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        if let Some(error) = self.take_write_fault(path) {
            return Err(io::Error::new(
                error,
                format!(
                    "FaultKernel: injected state-file write fault for {}",
                    path.display()
                ),
            ));
        }
        if let Some(bytes) = self.take_short_write(path) {
            if bytes == 0 {
                return Ok(());
            }
            let truncated = value.get(..bytes).unwrap_or(value);
            return self.inner.write_state_file(path, truncated);
        }
        self.inner.write_state_file(path, value)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        if let Some(error) = self.take_create_dir_fault(path) {
            Err(io::Error::new(
                error,
                format!(
                    "FaultKernel: injected create_dir_all fault for {}",
                    path.display()
                ),
            ))
        } else {
            self.inner.create_dir_all(path)
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if let Some(error) = self.take_rename_fault(from, to) {
            Err(io::Error::new(
                error,
                format!(
                    "FaultKernel: injected rename fault from {} to {}",
                    from.display(),
                    to.display()
                ),
            ))
        } else {
            self.inner.rename(from, to)
        }
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        if let Some(error) = self.take_remove_fault(path) {
            Err(io::Error::new(
                error,
                format!(
                    "FaultKernel: injected remove_file fault for {}",
                    path.display()
                ),
            ))
        } else {
            self.inner.remove_file(path)
        }
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        if let Some(error) = self.take_write_fault(path) {
            Err(io::Error::new(
                error,
                format!("FaultKernel: injected append fault for {}", path.display()),
            ))
        } else {
            self.inner.append(path, text)
        }
    }

    fn sync_file(&self, path: &Path) -> io::Result<()> {
        if let Some(error) = self.take_sync_file_fault(path) {
            Err(io::Error::new(
                error,
                format!(
                    "FaultKernel: injected file-sync fault for {}",
                    path.display()
                ),
            ))
        } else {
            self.inner.sync_file(path)
        }
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        if let Some(error) = self.take_sync_dir_fault(path) {
            Err(io::Error::new(
                error,
                format!(
                    "FaultKernel: injected directory-sync fault for {}",
                    path.display()
                ),
            ))
        } else {
            self.inner.sync_dir(path)
        }
    }
}

impl Clock for FaultKernel {
    fn now_unix(&self) -> u64 {
        self.inner.now_unix()
    }
}

/// In-memory implementation for deterministic tests.
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

    pub fn advance_clock(&self, seconds: u64) {
        self.clock
            .fetch_add(seconds, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn write_raw(&self, path: &Path, value: &str) {
        self.files
            .lock()
            .expect("MemoryKernel files mutex poisoned")
            .insert(path.to_path_buf(), value.to_string());
    }

    pub fn write_link(&self, path: &Path, target: &Path) {
        self.links
            .lock()
            .expect("MemoryKernel links mutex poisoned")
            .insert(path.to_path_buf(), target.to_path_buf());
    }

    pub fn add_dir(&self, parent: &Path, child: &Path) {
        self.add_dir_entry(parent, child);
    }

    pub fn add_dir_entry(&self, directory: &Path, entry: &Path) {
        let mut dirs = self.dirs.lock().expect("MemoryKernel dirs mutex poisoned");
        let entries = dirs.entry(directory.to_path_buf()).or_default();
        if !entries.iter().any(|candidate| candidate == entry) {
            entries.push(entry.to_path_buf());
        }
    }

    fn remove_dir_entry(&self, directory: &Path, entry: &Path) {
        if let Some(entries) = self
            .dirs
            .lock()
            .expect("MemoryKernel dirs mutex poisoned")
            .get_mut(directory)
        {
            entries.retain(|candidate| candidate != entry);
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl KernelRead for MemoryKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.files
            .lock()
            .expect("MemoryKernel files mutex poisoned")
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
            .expect("MemoryKernel dirs mutex poisoned")
            .get(path)
            .cloned()
            .unwrap_or_default())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files
            .lock()
            .expect("MemoryKernel files mutex poisoned")
            .contains_key(path)
            || self
                .dirs
                .lock()
                .expect("MemoryKernel dirs mutex poisoned")
                .contains_key(path)
            || self
                .links
                .lock()
                .expect("MemoryKernel links mutex poisoned")
                .contains_key(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.links
            .lock()
            .expect("MemoryKernel links mutex poisoned")
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
        if let Some(target) = self
            .links
            .lock()
            .expect("MemoryKernel links mutex poisoned")
            .get(path)
            .cloned()
        {
            return Ok(target);
        }
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::ParentDir => {
                    normalized.pop();
                }
                Component::CurDir => {}
                other => normalized.push(other.as_os_str()),
            }
        }
        if normalized.as_os_str().is_empty() {
            normalized.push("/");
        }
        Ok(normalized)
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl KernelWrite for MemoryKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        is_allowlisted_write_path(path)?;
        self.write_raw(path, value);
        Ok(())
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        self.write_raw(path, value);
        if let Some(parent) = path.parent() {
            self.add_dir_entry(parent, path);
        }
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.dirs
            .lock()
            .expect("MemoryKernel dirs mutex poisoned")
            .entry(path.to_path_buf())
            .or_default();
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut files = self
            .files
            .lock()
            .expect("MemoryKernel files mutex poisoned");
        let content = files.remove(from).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "MemoryKernel: rename source not found",
            )
        })?;
        files.insert(to.to_path_buf(), content);
        drop(files);
        if let Some(parent) = from.parent() {
            self.remove_dir_entry(parent, from);
        }
        if let Some(parent) = to.parent() {
            self.add_dir_entry(parent, to);
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files
            .lock()
            .expect("MemoryKernel files mutex poisoned")
            .remove(path)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "MemoryKernel: remove_file not found",
                )
            })?;
        if let Some(parent) = path.parent() {
            self.remove_dir_entry(parent, path);
        }
        Ok(())
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        self.files
            .lock()
            .expect("MemoryKernel files mutex poisoned")
            .entry(path.to_path_buf())
            .or_default()
            .push_str(text);
        if let Some(parent) = path.parent() {
            self.add_dir_entry(parent, path);
        }
        Ok(())
    }

    fn sync_file(&self, path: &Path) -> io::Result<()> {
        if self.exists(path) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("MemoryKernel: sync file not found: {}", path.display()),
            ))
        }
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        if self.exists(path) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("MemoryKernel: sync directory not found: {}", path.display()),
            ))
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_preserves_expected_boundaries() {
        assert!(is_allowlisted_write_path(Path::new("/proc/sys/vm/swappiness")).is_ok());
        assert!(is_allowlisted_write_path(Path::new("/tmp/not-allowlisted")).is_err());
        assert!(is_allowlisted_write_path(Path::new(
            "/sys/devices/system/cpu/cpu0/cpufreq/../../../../etc/passwd"
        ))
        .is_err());
    }

    #[test]
    fn memory_kernel_round_trip_and_clock() {
        let kernel = MemoryKernel::new();
        let path = Path::new("/proc/sys/vm/swappiness");
        kernel.write(path, "42").expect("write fixture");
        assert_eq!(kernel.read_to_string(path).expect("read fixture"), "42");
        kernel.advance_clock(7);
        assert_eq!(kernel.now_unix(), 7);
    }

    #[test]
    fn fault_rules_are_deterministic_and_one_shot() {
        let path = Path::new("/proc/sys/vm/swappiness");
        let inner = MemoryKernel::new();
        inner.write(path, "old").expect("write fixture");
        let kernel = FaultKernel::new(Box::new(inner));

        kernel.fail_next_write(path.to_path_buf(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            kernel
                .write(path, "blocked")
                .expect_err("fault must fire")
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        kernel.write(path, "new").expect("fault must be consumed");
        assert_eq!(kernel.read_to_string(path).expect("read fixture"), "new");
    }

    #[test]
    fn short_write_truncates_once() {
        let path = Path::new("/proc/sys/vm/swappiness");
        let inner = MemoryKernel::new();
        inner.write(path, "old").expect("write fixture");
        let kernel = FaultKernel::new(Box::new(inner));

        kernel.fail_next_write_short(path.to_path_buf(), 2);
        kernel.write(path, "100").expect("short write succeeds");
        assert_eq!(kernel.read_to_string(path).expect("read fixture"), "10");
        kernel.write(path, "200").expect("fault must be consumed");
        assert_eq!(kernel.read_to_string(path).expect("read fixture"), "200");
    }

    #[test]
    fn persistent_hide_and_malformed_rules_apply() {
        let path = Path::new("/virtual/value");
        let hidden = FaultKernel::new(Box::new(MemoryKernel::new()));
        hidden.hide_path(path.to_path_buf());
        assert!(!hidden.exists(path));
        assert_eq!(
            hidden
                .read_to_string(path)
                .expect_err("hidden path must fail")
                .kind(),
            io::ErrorKind::NotFound
        );

        let malformed = FaultKernel::new(Box::new(MemoryKernel::new()));
        malformed.malform_content(path.to_path_buf(), "bad".to_string());
        assert_eq!(
            malformed.read_to_string(path).expect("malformed fixture"),
            "bad"
        );
    }
}
