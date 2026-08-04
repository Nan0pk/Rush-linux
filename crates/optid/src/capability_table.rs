//! S4D — typed, pre-opened kernel capabilities and irreversible write sealing.
//!
//! Runtime policy continues to nominate typed `Action`s, but the production
//! kernel boundary no longer reopens their paths for writes. Startup discovers
//! the exact supported attributes, validates their operation type and current
//! identity, opens them with `CLOEXEC`, and installs Landlock before any worker
//! thread or D-Bus input is started. Later writes must resolve to one of those
//! exact descriptors and revalidate the path identity before use.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::capability::Capability;
use crate::kernel_io::{
    is_allowlisted_write_path, Clock, EventSource, KernelRead, KernelWrite, RealKernel,
};
use crate::sensors::{discover_cpu_epp_paths_with, Snapshot};

#[path = "capability_seal_test/landlock_syscall.rs"]
mod landlock;

pub(crate) const EXIT_TOPOLOGY_REBUILD: i32 = 75;
const DEFAULT_TOPOLOGY_DEBOUNCE_OBSERVATIONS: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CapabilityKind {
    CpuEpp,
    PlatformProfile,
    VmSysctl,
    DeviceResumeLatency,
    RuntimePmControl,
    RuntimePmAutosuspendDelay,
    PcieAspm,
    SataAlpm,
    BacklightBrightness,
}

impl CapabilityKind {
    fn validator(self) -> Capability {
        match self {
            Self::CpuEpp => Capability::CpuEpp,
            Self::PlatformProfile => Capability::PlatformProfile,
            Self::VmSysctl => Capability::VmSysctl,
            Self::DeviceResumeLatency => Capability::DeviceResumeLatency,
            Self::RuntimePmControl | Self::RuntimePmAutosuspendDelay => Capability::RuntimePm,
            Self::PcieAspm => Capability::PcieAspm,
            Self::SataAlpm => Capability::SataAlpm,
            Self::BacklightBrightness => Capability::Backlight,
        }
    }

    fn validate(self, path: &Path) -> io::Result<()> {
        self.validator().validate_target(path)?;
        let file_name = path.file_name().and_then(|name| name.to_str());
        let exact = match self {
            Self::RuntimePmControl => file_name == Some("control"),
            Self::RuntimePmAutosuspendDelay => file_name == Some("autosuspend_delay_ms"),
            _ => true,
        };
        if exact {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "operation/type mismatch: {:?} cannot own {}",
                    self,
                    path.display()
                ),
            ))
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::CpuEpp => "cpu_epp",
            Self::PlatformProfile => "platform_profile",
            Self::VmSysctl => "vm_sysctl",
            Self::DeviceResumeLatency => "device_resume_latency",
            Self::RuntimePmControl => "runtime_pm_control",
            Self::RuntimePmAutosuspendDelay => "runtime_pm_delay",
            Self::PcieAspm => "pcie_aspm",
            Self::SataAlpm => "sata_alpm",
            Self::BacklightBrightness => "backlight",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CapabilitySpec {
    pub(crate) kind: CapabilityKind,
    pub(crate) path: PathBuf,
}

impl CapabilitySpec {
    pub(crate) fn new(kind: CapabilityKind, path: impl Into<PathBuf>) -> Self {
        Self {
            kind,
            path: path.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapabilityIdentity {
    canonical_path: PathBuf,
    device: u64,
    inode: u64,
}

#[derive(Debug)]
struct CapabilityEntry {
    kind: CapabilityKind,
    requested_path: PathBuf,
    identity: CapabilityIdentity,
    file: Mutex<File>,
}

impl CapabilityEntry {
    fn open(spec: &CapabilitySpec) -> io::Result<Self> {
        spec.kind.validate(&spec.path)?;
        let canonical_path = fs::canonicalize(&spec.path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&canonical_path)?;
        let metadata = file.metadata()?;
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if flags & libc::FD_CLOEXEC == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("capability descriptor lacks CLOEXEC: {}", spec.path.display()),
            ));
        }
        Ok(Self {
            kind: spec.kind,
            requested_path: spec.path.clone(),
            identity: CapabilityIdentity {
                canonical_path,
                device: metadata.dev(),
                inode: metadata.ino(),
            },
            file: Mutex::new(file),
        })
    }

    fn verify_current_identity(&self, requested: &Path) -> io::Result<()> {
        self.kind.validate(requested)?;
        let canonical = fs::canonicalize(requested)?;
        if canonical != self.identity.canonical_path {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "capability path replacement detected for {}",
                    requested.display()
                ),
            ));
        }
        let metadata = fs::metadata(requested)?;
        if metadata.dev() != self.identity.device || metadata.ino() != self.identity.inode {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("stale capability identity for {}", requested.display()),
            ));
        }
        Ok(())
    }

    fn read(&self, requested: &Path) -> io::Result<String> {
        self.verify_current_identity(requested)?;
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("capability descriptor lock poisoned"))?;
        file.seek(SeekFrom::Start(0))?;
        let mut output = String::new();
        file.read_to_string(&mut output)?;
        Ok(output)
    }

    fn write(&self, requested: &Path, value: &str) -> io::Result<()> {
        self.verify_current_identity(requested)?;
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("capability descriptor lock poisoned"))?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(value.as_bytes())?;
        file.flush()
    }

    fn cloexec(&self) -> io::Result<bool> {
        let file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("capability descriptor lock poisoned"))?;
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        if flags < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(flags & libc::FD_CLOEXEC != 0)
        }
    }
}

#[derive(Debug)]
pub(crate) struct CapabilityTable {
    aliases: BTreeMap<PathBuf, Arc<CapabilityEntry>>,
    inventory: BTreeSet<String>,
}

impl CapabilityTable {
    pub(crate) fn build(specs: impl IntoIterator<Item = CapabilitySpec>) -> io::Result<Self> {
        let mut aliases = BTreeMap::new();
        let mut inventory = BTreeSet::new();
        let mut unique = BTreeSet::new();
        for spec in specs {
            if !unique.insert((spec.kind, spec.path.clone())) {
                continue;
            }
            let entry = Arc::new(CapabilityEntry::open(&spec)?);
            inventory.insert(format!(
                "{}:{}",
                spec.kind.label(),
                entry.identity.canonical_path.display()
            ));
            aliases.insert(entry.requested_path.clone(), Arc::clone(&entry));
            aliases.insert(entry.identity.canonical_path.clone(), entry);
        }
        Ok(Self { aliases, inventory })
    }

    pub(crate) fn from_snapshot(read: &dyn KernelRead, snapshot: &Snapshot) -> io::Result<Self> {
        Self::build(specs_from_snapshot(read, snapshot))
    }

    pub(crate) fn len(&self) -> usize {
        self.inventory.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.inventory.is_empty()
    }

    pub(crate) fn inventory(&self) -> &BTreeSet<String> {
        &self.inventory
    }

    fn entry(&self, path: &Path) -> io::Result<&Arc<CapabilityEntry>> {
        self.aliases.get(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("no pre-opened capability for {}", path.display()),
            )
        })
    }

    fn read(&self, path: &Path) -> io::Result<String> {
        self.entry(path)?.read(path)
    }

    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        self.entry(path)?.write(path, value)
    }

    pub(crate) fn all_descriptors_cloexec(&self) -> io::Result<bool> {
        let mut seen = BTreeSet::new();
        for entry in self.aliases.values() {
            let key = Arc::as_ptr(entry) as usize;
            if seen.insert(key) && !entry.cloexec()? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) fn seal(
        &self,
        allowed_state_roots: &[PathBuf],
    ) -> io::Result<CapabilitySealReport> {
        if self.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "cannot enforce capability sealing with an empty table",
            ));
        }
        for root in allowed_state_roots {
            fs::create_dir_all(root)?;
        }
        let abi = landlock::detect_landlock_abi()?;
        let handled_rights =
            landlock::install_landlock_restrictions_with_write_roots(abi, allowed_state_roots)?;
        if !landlock::no_new_privs_is_set()? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "no_new_privs was not retained after Landlock sealing",
            ));
        }

        let probe = self
            .aliases
            .values()
            .next()
            .expect("non-empty table checked above");
        let new_open_denied = matches!(
            OpenOptions::new()
                .write(true)
                .open(&probe.identity.canonical_path),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::Other
                )
        );
        if !new_open_denied {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "negative self-test failed: a new hardware write open succeeded after sealing",
            ));
        }

        let state_probe = allowed_state_roots[0].join(format!(
            ".s4d-seal-self-test-{}",
            std::process::id()
        ));
        fs::write(&state_probe, b"sealed-state-write\n")?;
        fs::remove_file(&state_probe)?;

        Ok(CapabilitySealReport {
            landlock_abi: abi,
            handled_rights,
            capability_count: self.len(),
            new_hardware_write_open_denied: true,
            state_write_allowed: true,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapabilitySealReport {
    pub(crate) landlock_abi: u32,
    pub(crate) handled_rights: u64,
    pub(crate) capability_count: usize,
    pub(crate) new_hardware_write_open_denied: bool,
    pub(crate) state_write_allowed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct CapabilityKernel {
    table: Arc<CapabilityTable>,
    fallback: RealKernel,
}

impl CapabilityKernel {
    pub(crate) fn new(table: Arc<CapabilityTable>) -> Self {
        Self {
            table,
            fallback: RealKernel::new(),
        }
    }

    pub(crate) fn table(&self) -> &Arc<CapabilityTable> {
        &self.table
    }
}

impl KernelRead for CapabilityKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if self.table.aliases.contains_key(path) {
            self.table.read(path)
        } else {
            self.fallback.read_to_string(path)
        }
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.fallback.read_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.table.aliases.contains_key(path) || self.fallback.exists(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.fallback.read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.fallback.canonicalize(path)
    }
}

impl KernelWrite for CapabilityKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        is_allowlisted_write_path(path)?;
        self.table.write(path, value)
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        self.fallback.write_state_file(path, value)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.fallback.create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.fallback.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.fallback.remove_file(path)
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        self.fallback.append(path, text)
    }
}

impl Clock for CapabilityKernel {
    fn now_unix(&self) -> u64 {
        self.fallback.now_unix()
    }
}

impl EventSource for CapabilityKernel {
    fn wait(&self, duration: Duration) -> bool {
        self.fallback.wait(duration)
    }
}

pub(crate) fn specs_from_snapshot(
    read: &dyn KernelRead,
    snapshot: &Snapshot,
) -> Vec<CapabilitySpec> {
    let mut specs = Vec::new();
    for path in discover_cpu_epp_paths_with(read) {
        specs.push(CapabilitySpec::new(CapabilityKind::CpuEpp, path));
    }
    let platform = PathBuf::from("/sys/firmware/acpi/platform_profile");
    if read.exists(&platform) {
        specs.push(CapabilitySpec::new(
            CapabilityKind::PlatformProfile,
            platform,
        ));
    }
    for path in [
        "/proc/sys/vm/swappiness",
        "/proc/sys/vm/dirty_background_bytes",
        "/proc/sys/vm/dirty_bytes",
    ] {
        let path = PathBuf::from(path);
        if read.exists(&path) {
            specs.push(CapabilitySpec::new(CapabilityKind::VmSysctl, path));
        }
    }
    for path in &snapshot.pm_qos_device_paths {
        specs.push(CapabilitySpec::new(
            CapabilityKind::DeviceResumeLatency,
            path.clone(),
        ));
    }
    for device in &snapshot.runtime_pm_device_paths {
        let control = device.join("power/control");
        if read.exists(&control) {
            specs.push(CapabilitySpec::new(
                CapabilityKind::RuntimePmControl,
                control,
            ));
        }
        let delay = device.join("power/autosuspend_delay_ms");
        if read.exists(&delay) {
            specs.push(CapabilitySpec::new(
                CapabilityKind::RuntimePmAutosuspendDelay,
                delay,
            ));
        }
    }
    for device in &snapshot.pcie_aspm_device_paths {
        specs.push(CapabilitySpec::new(
            CapabilityKind::PcieAspm,
            device.join("link/l1_aspm"),
        ));
    }
    for host in &snapshot.sata_alpm_host_paths {
        specs.push(CapabilitySpec::new(
            CapabilityKind::SataAlpm,
            host.join("link_power_management_policy"),
        ));
    }
    if let Some(backlight) = &snapshot.selected_backlight {
        specs.push(CapabilitySpec::new(
            CapabilityKind::BacklightBrightness,
            backlight.join("brightness"),
        ));
    }
    specs
}

pub(crate) fn topology_fingerprint(
    read: &dyn KernelRead,
    snapshot: &Snapshot,
) -> BTreeSet<String> {
    specs_from_snapshot(read, snapshot)
        .into_iter()
        .map(|spec| {
            let canonical = read.canonicalize(&spec.path).unwrap_or(spec.path);
            format!("{}:{}", spec.kind.label(), canonical.display())
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TopologyDecision {
    Stable,
    Pending { observations: u8 },
    Rebuild,
}

#[derive(Debug, Clone)]
pub(crate) struct TopologyDebouncer {
    baseline: BTreeSet<String>,
    pending: Option<(BTreeSet<String>, u8)>,
    required_observations: u8,
}

impl TopologyDebouncer {
    pub(crate) fn new(baseline: BTreeSet<String>) -> Self {
        Self::with_required_observations(
            baseline,
            DEFAULT_TOPOLOGY_DEBOUNCE_OBSERVATIONS,
        )
    }

    fn with_required_observations(
        baseline: BTreeSet<String>,
        required_observations: u8,
    ) -> Self {
        Self {
            baseline,
            pending: None,
            required_observations: required_observations.max(1),
        }
    }

    pub(crate) fn observe(&mut self, current: BTreeSet<String>) -> TopologyDecision {
        if current == self.baseline {
            self.pending = None;
            return TopologyDecision::Stable;
        }
        let observations = match &self.pending {
            Some((candidate, count)) if candidate == &current => count.saturating_add(1),
            _ => 1,
        };
        self.pending = Some((current, observations));
        if observations >= self.required_observations {
            TopologyDecision::Rebuild
        } else {
            TopologyDecision::Pending { observations }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "optid-s4d-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create temp root");
        root
    }

    fn runtime_control(root: &Path) -> PathBuf {
        let path = root.join("device/power/control");
        fs::create_dir_all(path.parent().expect("parent")).expect("create power dir");
        fs::write(&path, "on\n").expect("write control");
        path
    }

    #[test]
    fn s4d_operation_type_mismatch_is_rejected() {
        let root = temp_root("type-mismatch");
        let path = runtime_control(&root);
        let error = CapabilityTable::build([CapabilitySpec::new(
            CapabilityKind::RuntimePmAutosuspendDelay,
            path,
        )])
        .expect_err("mismatched operation must fail");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn s4d_preopened_descriptor_survives_permission_tightening() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("preopened");
        let path = runtime_control(&root);
        let table = CapabilityTable::build([CapabilitySpec::new(
            CapabilityKind::RuntimePmControl,
            path.clone(),
        )])
        .expect("build table");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).expect("tighten mode");
        table.write(&path, "auto\n").expect("descriptor write");
        assert_eq!(fs::read_to_string(path).expect("read"), "auto\n");
    }

    #[test]
    fn s4d_symlink_path_replacement_is_rejected() {
        let root = temp_root("symlink-replacement");
        let path = runtime_control(&root);
        let table = CapabilityTable::build([CapabilitySpec::new(
            CapabilityKind::RuntimePmControl,
            path.clone(),
        )])
        .expect("build table");
        let old_device = root.join("device-old");
        fs::rename(root.join("device"), &old_device).expect("move original");
        let replacement = root.join("replacement/power");
        fs::create_dir_all(&replacement).expect("replacement dir");
        fs::write(replacement.join("control"), "on\n").expect("replacement value");
        symlink(root.join("replacement"), root.join("device")).expect("replacement symlink");
        let error = table.write(&path, "auto\n").expect_err("replacement denied");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn s4d_stale_identity_is_rejected() {
        let root = temp_root("stale-identity");
        let path = runtime_control(&root);
        let table = CapabilityTable::build([CapabilitySpec::new(
            CapabilityKind::RuntimePmControl,
            path.clone(),
        )])
        .expect("build table");
        fs::remove_file(&path).expect("remove old file");
        fs::write(&path, "on\n").expect("replace inode");
        let error = table.write(&path, "auto\n").expect_err("stale identity denied");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn s4d_removed_device_fails_closed() {
        let root = temp_root("removed");
        let path = runtime_control(&root);
        let table = CapabilityTable::build([CapabilitySpec::new(
            CapabilityKind::RuntimePmControl,
            path.clone(),
        )])
        .expect("build table");
        fs::remove_file(&path).expect("remove target");
        let error = table.write(&path, "auto\n").expect_err("removed target denied");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn s4d_capability_descriptors_are_cloexec() {
        let root = temp_root("cloexec");
        let path = runtime_control(&root);
        let table = CapabilityTable::build([CapabilitySpec::new(
            CapabilityKind::RuntimePmControl,
            path,
        )])
        .expect("build table");
        assert!(table.all_descriptors_cloexec().expect("query flags"));
    }

    #[test]
    fn s4d_topology_change_is_debounced() {
        let baseline = BTreeSet::from(["runtime_pm:/device/a".to_string()]);
        let changed = BTreeSet::from(["runtime_pm:/device/b".to_string()]);
        let mut tracker = TopologyDebouncer::with_required_observations(baseline.clone(), 2);
        assert_eq!(
            tracker.observe(changed.clone()),
            TopologyDecision::Pending { observations: 1 }
        );
        assert_eq!(tracker.observe(baseline), TopologyDecision::Stable);
        assert_eq!(
            tracker.observe(changed.clone()),
            TopologyDecision::Pending { observations: 1 }
        );
        assert_eq!(tracker.observe(changed), TopologyDecision::Rebuild);
    }

    #[test]
    fn s4d_cold_rebuild_opens_fresh_identity() {
        let root = temp_root("cold-rebuild");
        let path = runtime_control(&root);
        let first = CapabilityTable::build([CapabilitySpec::new(
            CapabilityKind::RuntimePmControl,
            path.clone(),
        )])
        .expect("first table");
        fs::remove_file(&path).expect("remove old target");
        fs::write(&path, "on\n").expect("fresh target");
        assert!(first.write(&path, "auto\n").is_err());
        let second = CapabilityTable::build([CapabilitySpec::new(
            CapabilityKind::RuntimePmControl,
            path.clone(),
        )])
        .expect("fresh table");
        second.write(&path, "auto\n").expect("fresh descriptor");
        assert_eq!(fs::read_to_string(path).expect("read"), "auto\n");
    }
}
