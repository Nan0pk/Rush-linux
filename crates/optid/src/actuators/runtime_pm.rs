//! WP-N5 / D1 — runtime-PM device classification and safety predicates.
//!
//! The reconciler and actuator own desired-state tracking, durable transactions,
//! capability sealing, writes, readback, and rollback. This module owns the
//! device-local questions that must be answered before a runtime-PM target can
//! be considered safe to deepen.
//!
//! D1 deliberately separates *classification* from *policy*. Research 0009 has
//! proposed class-specific autosuspend delays, but several of those values are
//! explicitly hypotheses. This module therefore does not turn those numbers
//! into production policy. It provides deterministic typed discovery that later
//! D1 wiring can combine with verified latency, live-use, wakeup, and per-device
//! delay evidence. Unknown devices remain distinguishable so the final gate can
//! fail closed instead of treating "not identified" as "safe".

use std::path::{Path, PathBuf};

use crate::kernel_io::KernelRead;

/// Current conservative fallback used by the pre-D1 policy path. D1 must not
/// replace this with research-only class-specific guesses. The completed D1
/// path will select a per-device delay from accepted/evidence-backed policy.
pub(crate) const DEFAULT_AUTOSUSPEND_DELAY_MS: i32 = 2000;

/// Device classes whose live-use rules differ for runtime PM.
///
/// `Composite` is intentionally explicit: a USB device can expose, for example,
/// audio and HID interfaces at once, or combine one understood interface with a
/// vendor-specific one. Treating it as whichever interface was enumerated first
/// would make safety depend on directory order or hide an unmodelled function.
/// `Unknown` means there was no usable class evidence; the completed D1
/// actuation gate must deny unknown rather than infer a benign class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimePmDeviceClass {
    Network,
    Audio,
    Camera,
    Input,
    Storage,
    Composite,
    Other,
    Unknown,
}

/// Stable runtime-PM states in which a later D1 write gate may continue
/// evaluating the device. These are observations, not permission to actuate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimePmStableStatus {
    Active,
    Suspended,
}

/// A typed reason the D1 actuation precheck refuses to proceed.
///
/// This deliberately contains no guessed delay values. Audio and input
/// devices stay blocked until their live-use predicates are implemented and
/// accepted. Network has a hard live-use predicate (`carrier == 1`), storage
/// has one based on the kernel's own runtime-PM usage count (see
/// [`storage_live_use_block`]), and camera has one based on whether any
/// process holds its `/dev/videoN` node open (see [`camera_live_use_block`]).
/// Composite and other-classified devices remain denied outright: a composite
/// device mixes functions this module has not agreed a combined rule for, and
/// "other" carries no predicate at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimePmActuationBlock {
    UnknownClass,
    LiveUseGuardNotImplemented(RuntimePmDeviceClass),
    StorageLiveUseEvidenceUnavailable,
    StorageInUse,
    CameraLiveUseEvidenceUnavailable,
    CameraInUse,
    RuntimeStatusUnavailable,
    RuntimeStatusUnsupported,
    RuntimeStatusTransitioning,
    RuntimeStatusUnknown,
}

impl RuntimePmActuationBlock {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::UnknownClass => "device class is unknown",
            Self::LiveUseGuardNotImplemented(_) => {
                "device class has no accepted live-use guard yet"
            }
            Self::StorageLiveUseEvidenceUnavailable => {
                "power/runtime_usage is unavailable or unreadable for this storage device"
            }
            Self::StorageInUse => "storage device has a nonzero runtime-PM usage count",
            Self::CameraLiveUseEvidenceUnavailable => {
                "this camera has no readable video4linux/videoN mapping, or its open-file-descriptor scan could not be completed"
            }
            Self::CameraInUse => "a process currently holds this camera's video device node open",
            Self::RuntimeStatusUnavailable => "power/runtime_status is unavailable",
            Self::RuntimeStatusUnsupported => "runtime PM is unsupported for this device",
            Self::RuntimeStatusTransitioning => "runtime PM is currently transitioning",
            Self::RuntimeStatusUnknown => "power/runtime_status contains an unknown value",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimePmActuationReady {
    pub(crate) class: RuntimePmDeviceClass,
    pub(crate) runtime_status: RuntimePmStableStatus,
}

#[derive(Default)]
struct ClassFlags {
    network: bool,
    audio: bool,
    camera: bool,
    input: bool,
    storage: bool,
    other: bool,
}

impl ClassFlags {
    fn mark_usb_class(&mut self, value: u8) {
        match value {
            // USB Audio Device Class.
            0x01 => self.audio = true,
            // USB HID.
            0x03 => self.input = true,
            // USB Mass Storage.
            0x08 => self.storage = true,
            // USB Video Class (UVC).
            0x0e => self.camera = true,
            // 0x00 means class is defined by interfaces. By itself it is not a
            // usable classification; if no interface class can be read the
            // device remains Unknown and the later actuation gate fails closed.
            0x00 => {}
            _ => self.other = true,
        }
    }

    fn mark_pci_class(&mut self, value: u32) {
        let base = ((value >> 16) & 0xff) as u8;
        let subclass = ((value >> 8) & 0xff) as u8;
        match (base, subclass) {
            // Mass-storage controller.
            (0x01, _) => self.storage = true,
            // Network controller.
            (0x02, _) => self.network = true,
            // Multimedia video controller.
            (0x04, 0x00) => self.camera = true,
            // Multimedia audio / HD-audio controller.
            (0x04, 0x01 | 0x03) => self.audio = true,
            _ => self.other = true,
        }
    }

    fn finish(self) -> RuntimePmDeviceClass {
        let known_count = [
            self.network,
            self.audio,
            self.camera,
            self.input,
            self.storage,
        ]
        .into_iter()
        .filter(|present| *present)
        .count();

        // Any combination of understood classes, or an understood class plus
        // an unmodelled interface, stays composite. The final D1 gate can then
        // require every function to be understood instead of silently dropping
        // the extra interface from the safety model.
        if known_count > 1 || (known_count > 0 && self.other) {
            return RuntimePmDeviceClass::Composite;
        }
        if self.network {
            RuntimePmDeviceClass::Network
        } else if self.audio {
            RuntimePmDeviceClass::Audio
        } else if self.camera {
            RuntimePmDeviceClass::Camera
        } else if self.input {
            RuntimePmDeviceClass::Input
        } else if self.storage {
            RuntimePmDeviceClass::Storage
        } else if self.other {
            RuntimePmDeviceClass::Other
        } else {
            RuntimePmDeviceClass::Unknown
        }
    }
}

fn parse_hex_u8(value: &str) -> Option<u8> {
    let value = value.trim().trim_start_matches("0x");
    u8::from_str_radix(value, 16).ok()
}

fn parse_pci_class(value: &str) -> Option<u32> {
    let value = value.trim().trim_start_matches("0x");
    u32::from_str_radix(value, 16).ok()
}

fn usb_interface_classes(read: &dyn KernelRead, device_dir: &Path) -> Vec<u8> {
    let Ok(entries) = read.read_dir(device_dir) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| read.read_to_string(&entry.join("bInterfaceClass")).ok())
        .filter_map(|value| parse_hex_u8(&value))
        .collect()
}

/// Classify a runtime-PM candidate from stable sysfs class information.
///
/// Classification is deterministic and side-effect free. Network exposure via
/// a `net/` child takes part in the same result as USB interface classes and
/// PCI class codes, so a composite device cannot silently collapse to the
/// first directory entry returned by the kernel.
pub(crate) fn classify_device(read: &dyn KernelRead, device_dir: &Path) -> RuntimePmDeviceClass {
    let mut flags = ClassFlags::default();

    if read
        .read_dir(&device_dir.join("net"))
        .is_ok_and(|entries| !entries.is_empty())
    {
        flags.network = true;
    }

    // A USB device may carry a device-level class, per-interface classes, or
    // both. The common device-level value 00 means "look at interfaces".
    if let Ok(value) = read.read_to_string(&device_dir.join("bDeviceClass")) {
        if let Some(value) = parse_hex_u8(&value) {
            flags.mark_usb_class(value);
        }
    }
    for value in usb_interface_classes(read, device_dir) {
        flags.mark_usb_class(value);
    }

    // A USB interface node such as `1-1:1.0` is itself enumerated as a
    // runtime-PM candidate by `discover_runtime_pm_device_paths_with`, because
    // it exposes `power/control` like any other device. It carries no
    // `bDeviceClass` and has no interface children of its own; its class is the
    // `bInterfaceClass` in its own directory. Read it, so an interface is
    // classified from the evidence it does publish rather than refused as
    // unidentifiable.
    if let Ok(value) = read.read_to_string(&device_dir.join("bInterfaceClass")) {
        if let Some(value) = parse_hex_u8(&value) {
            flags.mark_usb_class(value);
        }
    }

    // PCI exposes a 24-bit class code as 0xBBSSPP (base, subclass,
    // programming interface). This is available without driver-specific I/O.
    if let Ok(value) = read.read_to_string(&device_dir.join("class")) {
        if let Some(value) = parse_pci_class(&value) {
            flags.mark_pci_class(value);
        }
    }

    flags.finish()
}

/// True when classification found no usable class evidence for this device.
///
/// This is the fail-closed half of the D1 gate: "not identified" must never be
/// treated as "safe to autosuspend". `Other` is deliberately excluded — an
/// understood bus class that simply falls outside the modelled categories still
/// carries evidence, whereas `Unknown` carries none at all. PCI devices expose
/// `class`, USB devices expose `bDeviceClass` with per-interface
/// `bInterfaceClass`, and a USB interface node exposes its own
/// `bInterfaceClass`, so a device reaching this predicate as `Unknown` is one
/// whose class could not be read at all rather than one merely uncategorised.
///
/// Devices on buses that publish no class attribute at all are refused here.
/// That is the intended direction, but it narrows runtime-PM coverage rather
/// than widening it, and it has not been measured on physical hardware.
pub(crate) fn class_evidence_missing(read: &dyn KernelRead, device_dir: &Path) -> bool {
    matches!(
        classify_device(read, device_dir),
        RuntimePmDeviceClass::Unknown
    )
}

/// Evaluate the D1 facts that are already settled enough to fail closed.
///
/// This is deliberately narrower than the final D1 gate. It does not choose a
/// delay or claim a device is safe to suspend. It only prevents later wiring
/// from treating an unknown class, an unimplemented live-use class, a missing
/// runtime status, or a transition/unknown runtime status as permission.
///
/// Network, storage, and camera are the ready classes for this slice: network
/// already had a hard carrier guard, storage has one based on the kernel's own
/// runtime-PM usage count (see [`storage_live_use_block`]), and camera has one
/// based on whether any process holds its `/dev/videoN` node open (see
/// [`camera_live_use_block`]). Audio, input, composite, and other devices
/// remain denied until a live-use predicate is implemented for them without
/// relying on the research-only timing hypotheses.
pub(crate) fn actuation_precheck(
    read: &dyn KernelRead,
    device_dir: &Path,
) -> Result<RuntimePmActuationReady, RuntimePmActuationBlock> {
    let class = classify_device(read, device_dir);
    match class {
        RuntimePmDeviceClass::Unknown => return Err(RuntimePmActuationBlock::UnknownClass),
        RuntimePmDeviceClass::Network => {}
        RuntimePmDeviceClass::Storage => {
            if let Some(block) = storage_live_use_block(read, device_dir) {
                return Err(block);
            }
        }
        RuntimePmDeviceClass::Camera => {
            if let Some(block) = camera_live_use_block(read, device_dir) {
                return Err(block);
            }
        }
        RuntimePmDeviceClass::Audio
        | RuntimePmDeviceClass::Input
        | RuntimePmDeviceClass::Composite
        | RuntimePmDeviceClass::Other => {
            return Err(RuntimePmActuationBlock::LiveUseGuardNotImplemented(class));
        }
    }

    let runtime_status = read
        .read_to_string(&device_dir.join("power").join("runtime_status"))
        .map_err(|_| RuntimePmActuationBlock::RuntimeStatusUnavailable)?;
    let runtime_status = match runtime_status.trim() {
        "active" => RuntimePmStableStatus::Active,
        "suspended" => RuntimePmStableStatus::Suspended,
        "unsupported" => return Err(RuntimePmActuationBlock::RuntimeStatusUnsupported),
        "suspending" | "resuming" => {
            return Err(RuntimePmActuationBlock::RuntimeStatusTransitioning);
        }
        _ => return Err(RuntimePmActuationBlock::RuntimeStatusUnknown),
    };

    Ok(RuntimePmActuationReady {
        class,
        runtime_status,
    })
}

/// True if any network interface backed by this device has its link up
/// (`carrier == 1`). Autosuspending a device with an active link would silently
/// drop packets, so the existing actuator hard-skips these.
pub(crate) fn network_carrier_up(read: &dyn KernelRead, device_dir: &Path) -> bool {
    if !matches!(
        classify_device(read, device_dir),
        RuntimePmDeviceClass::Network | RuntimePmDeviceClass::Composite
    ) {
        return false;
    }

    let net_dir = device_dir.join("net");
    let Ok(entries) = read.read_dir(&net_dir) else {
        return false;
    };
    entries.into_iter().any(|entry| {
        read.read_to_string(&entry.join("carrier"))
            .is_ok_and(|value| value.trim() == "1")
    })
}

/// Read this device's kernel-tracked runtime-PM usage count.
///
/// `power/runtime_usage` is the sysfs exposure of the runtime PM core's own
/// reference count (`Documentation/power/runtime_pm.rst`; kernel source
/// `drivers/base/power/sysfs.c` prints `atomic_read(&dev->power.usage_count)`
/// as a plain decimal integer). A positive count means some in-kernel user —
/// the owning driver mid-transfer, a child device, or another subsystem —
/// currently holds the device active; the runtime PM core itself refuses to
/// call `->runtime_suspend()` while the count is nonzero. This attribute is
/// gated behind `CONFIG_PM_ADVANCED_DEBUG`, which Rush's own kernel build
/// sets (`distro/kernel/default-adaptive.config`), but a consumer must still
/// treat a kernel without it as missing evidence rather than assuming the
/// setting.
///
/// Returns `Err(())` when the attribute is missing, unreadable, or does not
/// parse as an integer. Callers must treat that as "cannot tell", not as
/// "safe to suspend".
fn storage_runtime_usage(read: &dyn KernelRead, device_dir: &Path) -> Result<i64, ()> {
    let raw = read
        .read_to_string(&device_dir.join("power").join("runtime_usage"))
        .map_err(|_| ())?;
    raw.trim().parse::<i64>().map_err(|_| ())
}

/// The D1 live-use predicate for the storage class.
///
/// Storage devices reach D1's classification as PCI mass-storage controllers
/// (NVMe, AHCI/SATA) or USB mass-storage devices/interfaces. Those three
/// buses expose their block-layer I/O counters through different, driver
/// specific sysfs topologies below the classified device
/// (`nvme/nvmeN/nvmeNnM/stat` for NVMe; SCSI host/target/lun chains of
/// unpredictable depth for AHCI and USB mass storage) that this module does
/// not have verified, bus-independent traversal rules for. Rather than guess
/// that traversal, this predicate uses the one signal every classified
/// storage device already exposes directly in its own `power/` directory:
/// the kernel's runtime-PM usage count (see [`storage_runtime_usage`]).
///
/// Classifies the device itself, so it is safe to call on any runtime-PM
/// candidate — not only ones a caller has already confirmed are `Storage` —
/// the same self-contained style as [`network_carrier_up`]. Returns `None`
/// for every other class; it is not a substitute for the class match in
/// [`actuation_precheck`].
///
/// For a genuine storage device, returns `None` when the device may proceed
/// to the existing `runtime_status` check (usage count read as exactly `0`).
/// Returns `Some(block)` — fail closed — when the count is missing,
/// unreadable, unparseable, or nonzero. This never treats absent or
/// ambiguous evidence as "safe to suspend".
pub(crate) fn storage_live_use_block(
    read: &dyn KernelRead,
    device_dir: &Path,
) -> Option<RuntimePmActuationBlock> {
    if !matches!(
        classify_device(read, device_dir),
        RuntimePmDeviceClass::Storage
    ) {
        return None;
    }
    match storage_runtime_usage(read, device_dir) {
        Ok(0) => None,
        Ok(_) => Some(RuntimePmActuationBlock::StorageInUse),
        Err(()) => Some(RuntimePmActuationBlock::StorageLiveUseEvidenceUnavailable),
    }
}

/// Read the V4L2 character-device name this camera is bound to, from its
/// `video4linux/` child directory.
///
/// Classification reads only USB/PCI class codes; it says nothing about
/// whether a driver is actually bound. A camera device (USB Video Class or a
/// PCI multimedia-video controller) that has no `video4linux/` child, or
/// whose child is not a single `videoN` entry, has not published the mapping
/// this predicate needs — that is "cannot tell", not "not in use", so the
/// caller must fail closed rather than guess a device node. Exactly one
/// `videoN` entry is the only topology this predicate understands; more than
/// one (an unexpected topology this module has no verified rule for) is
/// refused the same way a missing directory is, rather than picking one
/// arbitrarily.
fn camera_video_device_node(read: &dyn KernelRead, device_dir: &Path) -> Result<PathBuf, ()> {
    let entries = read
        .read_dir(&device_dir.join("video4linux"))
        .map_err(|_| ())?;
    let mut names = entries.into_iter().filter_map(|entry| {
        let name = entry.file_name()?.to_str()?.to_string();
        let suffix = name.strip_prefix("video")?;
        (!suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())).then_some(name)
    });
    let name = names.next().ok_or(())?;
    if names.next().is_some() {
        return Err(());
    }
    Ok(Path::new("/dev").join(name))
}

/// Scan every process on the system for an open file descriptor that resolves
/// to `video_device`, reading process directories from `proc_dir`.
///
/// `proc_dir` exists as a parameter only so this module's own tests can point
/// it at a fixture tree instead of the real `/proc`; production always calls
/// this through [`camera_live_use_block`], which fixes it to `/proc`.
///
/// # Why an unreadable `/proc/<pid>/fd` fails closed, but a vanished `/proc/<pid>` does not
///
/// `/proc` is inherently racy: a process can exit at any point between this
/// function listing `/proc` and reading that one pid's own `fd` directory.
/// This predicate treats two failures there differently, on purpose:
///
/// - If reading `/proc/<pid>/fd` fails with `NotFound`, the process itself is
///   gone by the time this scan reached it. A process that no longer exists
///   holds no file descriptor at all, so moving on to the next pid cannot
///   miss anything real: there is nothing left there to miss.
/// - Any other failure — most importantly `PermissionDenied` — means the
///   process is still there but this predicate cannot see into it. That is a
///   genuine gap in the evidence, not a confirmed absence of use, so it must
///   deny the whole check the same way [`storage_live_use_block`] denies on
///   an unreadable `power/runtime_usage`: "cannot tell" is never "safe to
///   suspend". optid runs with the privilege to read any process's `fd`
///   directory in its ordinary deployment, so a permission failure here is
///   the unusual case, and treating it as permission to proceed would be
///   exactly the kind of guess this module exists to refuse.
///
/// The same reasoning applies one level down, to reading a single `fd`
/// entry's link target: `NotFound` there means that one descriptor was closed
/// between being listed and being read, which is a true statement about the
/// current instant (it is not open right now), not a missing observation; any
/// other read failure denies the whole check.
fn camera_in_use_by_any_process(
    read: &dyn KernelRead,
    proc_dir: &Path,
    video_device: &Path,
) -> Result<bool, ()> {
    let pids = read.read_dir(proc_dir).map_err(|_| ())?;
    for pid_dir in pids {
        let is_pid = pid_dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()));
        if !is_pid {
            continue;
        }

        let fds = match read.read_dir(&pid_dir.join("fd")) {
            Ok(fds) => fds,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(()),
        };

        for fd in fds {
            match read.read_link(&fd) {
                Ok(target) if target == video_device => return Ok(true),
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(()),
            }
        }
    }
    Ok(false)
}

/// The D1 live-use predicate for the camera class, parameterized on the proc
/// root so this module's own tests can exercise it against a fixture tree.
/// [`camera_live_use_block`] is the production entry point, fixed to `/proc`.
fn camera_live_use_block_under(
    read: &dyn KernelRead,
    device_dir: &Path,
    proc_dir: &Path,
) -> Option<RuntimePmActuationBlock> {
    if !matches!(
        classify_device(read, device_dir),
        RuntimePmDeviceClass::Camera
    ) {
        return None;
    }
    let video_device = match camera_video_device_node(read, device_dir) {
        Ok(path) => path,
        Err(()) => return Some(RuntimePmActuationBlock::CameraLiveUseEvidenceUnavailable),
    };
    match camera_in_use_by_any_process(read, proc_dir, &video_device) {
        Ok(false) => None,
        Ok(true) => Some(RuntimePmActuationBlock::CameraInUse),
        Err(()) => Some(RuntimePmActuationBlock::CameraLiveUseEvidenceUnavailable),
    }
}

/// The D1 live-use predicate for the camera class.
///
/// Camera devices reach D1's classification as USB Video Class (UVC, `0x0e`)
/// interfaces or PCI multimedia-video controllers. Both buses converge on the
/// same bus-independent signal once a V4L2 driver is bound: a
/// `video4linux/videoN` child directly under the classified device directory
/// (see [`camera_video_device_node`]), which names the `/dev/videoN`
/// character-device node. "Is this camera in use" is then answered the
/// standard, portable Linux way — enumerating every process's open file
/// descriptors under `/proc/<pid>/fd` and checking whether any resolves to
/// that node (see [`camera_in_use_by_any_process`]) — rather than by a
/// driver- or bus-specific activity counter this module does not have a
/// verified reading for, the same reasoning [`storage_live_use_block`]
/// documents for choosing `power/runtime_usage` over a bus-specific I/O
/// counter.
///
/// Classifies the device itself, so it is safe to call on any runtime-PM
/// candidate — not only ones a caller has already confirmed are `Camera` —
/// the same self-contained style as [`network_carrier_up`] and
/// [`storage_live_use_block`]. Returns `None` for every other class; it is
/// not a substitute for the class match in [`actuation_precheck`].
///
/// For a genuine camera device, returns `None` only when the `video4linux`
/// mapping resolves to exactly one device node and a full, error-free scan of
/// every process's `/proc/<pid>/fd` finds no match. Returns `Some(block)` —
/// fail closed — when the mapping is missing or ambiguous
/// (`CameraLiveUseEvidenceUnavailable`), when a matching open descriptor is
/// found (`CameraInUse`), or when the scan itself could not be completed
/// (`CameraLiveUseEvidenceUnavailable`; see
/// [`camera_in_use_by_any_process`] for which scan failures count as
/// "complete enough to prove absence" and which do not). This never treats
/// absent or ambiguous evidence as "safe to suspend".
pub(crate) fn camera_live_use_block(
    read: &dyn KernelRead,
    device_dir: &Path,
) -> Option<RuntimePmActuationBlock> {
    camera_live_use_block_under(read, device_dir, Path::new("/proc"))
}

/// Does this device expose a USB HID (interface class `03`) child?
/// Used to preserve the existing input wakeup diagnostic. Composite USB
/// devices still count as HID when any interface is HID.
pub(crate) fn is_hid_input(read: &dyn KernelRead, device_dir: &Path) -> bool {
    usb_interface_classes(read, device_dir)
        .into_iter()
        .any(|class| class == 0x03)
}

/// True if the device's `power/wakeup` attribute reads `disabled`. Absent
/// attribute ⇒ false (nothing to warn about).
pub(crate) fn wakeup_disabled(read: &dyn KernelRead, device_dir: &Path) -> bool {
    matches!(
        read.read_to_string(&device_dir.join("power").join("wakeup")),
        Ok(v) if v.trim() == "disabled"
    )
}

/// If autosuspending this device would be questionable for wakeup reasons
/// (it is an input device but wakeup is disabled), return a human-readable
/// warning. The actuator logs it but proceeds — this predicate never modifies
/// wakeup state.
pub(crate) fn wakeup_warning(read: &dyn KernelRead, device_dir: &Path) -> Option<String> {
    if is_hid_input(read, device_dir) && wakeup_disabled(read, device_dir) {
        Some(format!(
            "input device {} has power/wakeup=disabled; autosuspending control only (wakeup left untouched)",
            device_dir.display()
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel_io::RealKernel;
    use std::fs;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("optid_rpm_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn add_usb_interface(device: &Path, name: &str, class: &str) {
        let interface = device.join(name);
        fs::create_dir_all(&interface).unwrap();
        fs::write(interface.join("bInterfaceClass"), format!("{class}\n")).unwrap();
    }

    fn set_runtime_status(device: &Path, status: &str) {
        let power = device.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(power.join("runtime_status"), format!("{status}\n")).unwrap();
    }

    fn set_runtime_usage(device: &Path, usage: &str) {
        let power = device.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(power.join("runtime_usage"), format!("{usage}\n")).unwrap();
    }

    fn mark_storage_pci(device: &Path) {
        // Base class 0x01 = mass-storage controller (subclass irrelevant to
        // classification; e.g. 0x08 = NVMe, 0x06 = AHCI/SATA).
        fs::write(device.join("class"), "0x010802\n").unwrap();
    }

    fn mark_camera_usb(device: &Path) {
        // A UVC camera is discovered as a bare interface node (see
        // `d1_usb_interface_node_is_classified_from_its_own_class`): its own
        // sysfs directory carries `bInterfaceClass` directly, not nested
        // under a separate parent device directory. A bound V4L2 driver
        // publishes `video4linux/videoN` as that same directory's own child,
        // which is exactly what `set_video4linux_node(device, ...)` adds.
        // Using the nested-child shape here instead (as a composite parent
        // device's classification test legitimately does) would place
        // `video4linux` one level away from where it can ever actually be
        // found, so the "permits" test could never exercise a real topology.
        fs::write(device.join("bInterfaceClass"), "0e\n").unwrap();
    }

    /// Publish `video4linux/<node>` under a device directory, the way a bound
    /// V4L2 driver does.
    fn set_video4linux_node(device: &Path, node: &str) {
        fs::create_dir_all(device.join("video4linux").join(node)).unwrap();
    }

    /// Create a fake `/proc`-shaped tree usable as `camera_live_use_block_under`'s
    /// `proc_dir`, containing one process directory with an empty `fd/`.
    fn proc_with_pid(proc_dir: &Path, pid: &str) -> PathBuf {
        let fd_dir = proc_dir.join(pid).join("fd");
        fs::create_dir_all(&fd_dir).unwrap();
        fd_dir
    }

    fn symlink_fd(fd_dir: &Path, fd_num: &str, target: &Path) {
        std::os::unix::fs::symlink(target, fd_dir.join(fd_num)).unwrap();
    }

    #[test]
    fn d1_typed_classification_covers_required_device_classes() {
        let read = RealKernel::new();

        let network = tmp("class_network");
        fs::write(network.join("class"), "0x020000\n").unwrap();
        assert_eq!(
            classify_device(&read, &network),
            RuntimePmDeviceClass::Network
        );

        let audio = tmp("class_audio");
        add_usb_interface(&audio, "1-1:1.0", "01");
        assert_eq!(classify_device(&read, &audio), RuntimePmDeviceClass::Audio);

        let camera = tmp("class_camera");
        add_usb_interface(&camera, "1-2:1.0", "0e");
        assert_eq!(
            classify_device(&read, &camera),
            RuntimePmDeviceClass::Camera
        );

        let input = tmp("class_input");
        add_usb_interface(&input, "1-3:1.0", "03");
        assert_eq!(classify_device(&read, &input), RuntimePmDeviceClass::Input);

        let storage = tmp("class_storage");
        add_usb_interface(&storage, "1-4:1.0", "08");
        assert_eq!(
            classify_device(&read, &storage),
            RuntimePmDeviceClass::Storage
        );

        for dir in [&network, &audio, &camera, &input, &storage] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn d1_composite_device_never_hides_another_function() {
        let read = RealKernel::new();
        let dev = tmp("class_composite");
        add_usb_interface(&dev, "1-1:1.1", "03");
        add_usb_interface(&dev, "1-1:1.0", "01");
        assert_eq!(
            classify_device(&read, &dev),
            RuntimePmDeviceClass::Composite
        );

        let partially_known = tmp("class_partially_known");
        add_usb_interface(&partially_known, "2-1:1.0", "01");
        add_usb_interface(&partially_known, "2-1:1.1", "ff");
        assert_eq!(
            classify_device(&read, &partially_known),
            RuntimePmDeviceClass::Composite
        );
        let _ = fs::remove_dir_all(dev);
        let _ = fs::remove_dir_all(partially_known);
    }

    #[test]
    fn d1_unknown_and_other_are_distinct_fail_closed_inputs() {
        let read = RealKernel::new();
        let unknown = tmp("class_unknown");
        assert_eq!(
            classify_device(&read, &unknown),
            RuntimePmDeviceClass::Unknown
        );

        let per_interface_without_interfaces = tmp("class_zero_without_interfaces");
        fs::write(
            per_interface_without_interfaces.join("bDeviceClass"),
            "00\n",
        )
        .unwrap();
        assert_eq!(
            classify_device(&read, &per_interface_without_interfaces),
            RuntimePmDeviceClass::Unknown
        );

        let other = tmp("class_other");
        fs::write(other.join("class"), "0x030000\n").unwrap();
        assert_eq!(classify_device(&read, &other), RuntimePmDeviceClass::Other);
        let _ = fs::remove_dir_all(unknown);
        let _ = fs::remove_dir_all(per_interface_without_interfaces);
        let _ = fs::remove_dir_all(other);
    }

    #[test]
    fn d1_usb_interface_node_is_classified_from_its_own_class() {
        // Discovery enumerates interface nodes such as `1-1:1.0` alongside
        // whole devices, because they expose `power/control` too. Their class
        // lives in their own directory, not in a child.
        let read = RealKernel::new();
        let interface = tmp("class_interface_node");
        fs::write(interface.join("bInterfaceClass"), "03\n").unwrap();
        assert_eq!(
            classify_device(&read, &interface),
            RuntimePmDeviceClass::Input
        );
        assert!(!class_evidence_missing(&read, &interface));
        let _ = fs::remove_dir_all(interface);
    }

    #[test]
    fn d1_actuation_precheck_denies_unknown_and_unimplemented_live_use_classes() {
        let read = RealKernel::new();

        let unknown = tmp("precheck_unknown");
        set_runtime_status(&unknown, "active");
        assert_eq!(
            actuation_precheck(&read, &unknown),
            Err(RuntimePmActuationBlock::UnknownClass)
        );

        let audio = tmp("precheck_audio");
        add_usb_interface(&audio, "1-5:1.0", "01");
        set_runtime_status(&audio, "active");
        assert_eq!(
            actuation_precheck(&read, &audio),
            Err(RuntimePmActuationBlock::LiveUseGuardNotImplemented(
                RuntimePmDeviceClass::Audio
            ))
        );

        let composite = tmp("precheck_composite");
        add_usb_interface(&composite, "1-6:1.0", "01");
        add_usb_interface(&composite, "1-6:1.1", "03");
        set_runtime_status(&composite, "active");
        assert_eq!(
            actuation_precheck(&read, &composite),
            Err(RuntimePmActuationBlock::LiveUseGuardNotImplemented(
                RuntimePmDeviceClass::Composite
            ))
        );

        for dir in [&unknown, &audio, &composite] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn d1_actuation_precheck_requires_a_known_stable_runtime_status() {
        let read = RealKernel::new();
        let network = tmp("precheck_status");
        fs::write(network.join("class"), "0x020000\n").unwrap();

        assert_eq!(
            actuation_precheck(&read, &network),
            Err(RuntimePmActuationBlock::RuntimeStatusUnavailable)
        );

        for status in ["suspending", "resuming"] {
            set_runtime_status(&network, status);
            assert_eq!(
                actuation_precheck(&read, &network),
                Err(RuntimePmActuationBlock::RuntimeStatusTransitioning)
            );
        }

        set_runtime_status(&network, "unsupported");
        assert_eq!(
            actuation_precheck(&read, &network),
            Err(RuntimePmActuationBlock::RuntimeStatusUnsupported)
        );

        set_runtime_status(&network, "driver-specific-mystery");
        assert_eq!(
            actuation_precheck(&read, &network),
            Err(RuntimePmActuationBlock::RuntimeStatusUnknown)
        );

        set_runtime_status(&network, "active");
        assert_eq!(
            actuation_precheck(&read, &network),
            Ok(RuntimePmActuationReady {
                class: RuntimePmDeviceClass::Network,
                runtime_status: RuntimePmStableStatus::Active,
            })
        );

        set_runtime_status(&network, "suspended");
        assert_eq!(
            actuation_precheck(&read, &network),
            Ok(RuntimePmActuationReady {
                class: RuntimePmDeviceClass::Network,
                runtime_status: RuntimePmStableStatus::Suspended,
            })
        );

        let _ = fs::remove_dir_all(network);
    }

    #[test]
    fn carrier_up_detected() {
        let dev = tmp("carrier_up");
        let iface = dev.join("net").join("enp0s31f6");
        fs::create_dir_all(&iface).unwrap();
        fs::write(iface.join("carrier"), "1\n").unwrap();
        assert_eq!(
            classify_device(&RealKernel::new(), &dev),
            RuntimePmDeviceClass::Network
        );
        assert!(network_carrier_up(&RealKernel::new(), &dev));
        let _ = fs::remove_dir_all(dev);
    }

    #[test]
    fn carrier_down_or_absent_not_up() {
        let dev = tmp("carrier_down");
        let iface = dev.join("net").join("enp0s31f6");
        fs::create_dir_all(&iface).unwrap();
        fs::write(iface.join("carrier"), "0\n").unwrap();
        assert!(!network_carrier_up(&RealKernel::new(), &dev));

        let dev2 = tmp("carrier_none");
        assert!(!network_carrier_up(&RealKernel::new(), &dev2));
        let _ = fs::remove_dir_all(dev);
        let _ = fs::remove_dir_all(dev2);
    }

    #[test]
    fn hid_input_and_wakeup_warning() {
        let dev = tmp("hid");
        add_usb_interface(&dev, "1-1:1.0", "03");
        assert!(is_hid_input(&RealKernel::new(), &dev));
        assert_eq!(
            classify_device(&RealKernel::new(), &dev),
            RuntimePmDeviceClass::Input
        );

        let power = dev.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(power.join("wakeup"), "disabled\n").unwrap();
        assert!(wakeup_disabled(&RealKernel::new(), &dev));
        assert!(wakeup_warning(&RealKernel::new(), &dev).is_some());

        fs::write(power.join("wakeup"), "enabled\n").unwrap();
        assert!(!wakeup_disabled(&RealKernel::new(), &dev));
        assert!(wakeup_warning(&RealKernel::new(), &dev).is_none());
        let _ = fs::remove_dir_all(dev);
    }

    #[test]
    fn d1_storage_zero_usage_permits_the_runtime_status_check() {
        let read = RealKernel::new();
        let dev = tmp("storage_zero_usage");
        mark_storage_pci(&dev);
        set_runtime_usage(&dev, "0");
        assert_eq!(storage_live_use_block(&read, &dev), None);

        set_runtime_status(&dev, "active");
        assert_eq!(
            actuation_precheck(&read, &dev),
            Ok(RuntimePmActuationReady {
                class: RuntimePmDeviceClass::Storage,
                runtime_status: RuntimePmStableStatus::Active,
            })
        );
        let _ = fs::remove_dir_all(dev);
    }

    #[test]
    fn d1_storage_nonzero_usage_denies_regardless_of_runtime_status() {
        let read = RealKernel::new();
        let dev = tmp("storage_nonzero_usage");
        mark_storage_pci(&dev);
        set_runtime_usage(&dev, "1");
        // Even a stable, otherwise-safe-looking runtime_status must not
        // override a nonzero usage count.
        set_runtime_status(&dev, "active");
        assert_eq!(
            storage_live_use_block(&read, &dev),
            Some(RuntimePmActuationBlock::StorageInUse)
        );
        assert_eq!(
            actuation_precheck(&read, &dev),
            Err(RuntimePmActuationBlock::StorageInUse)
        );
        let _ = fs::remove_dir_all(dev);
    }

    #[test]
    fn d1_storage_negative_usage_is_treated_as_in_use_not_as_idle() {
        // A negative reference count should never occur on a healthy kernel,
        // but this predicate only ever permits an exact `0` reading; any
        // other value denies, including one that looks superficially "less
        // than in use".
        let read = RealKernel::new();
        let dev = tmp("storage_negative_usage");
        mark_storage_pci(&dev);
        set_runtime_usage(&dev, "-1");
        assert_eq!(
            storage_live_use_block(&read, &dev),
            Some(RuntimePmActuationBlock::StorageInUse)
        );
        let _ = fs::remove_dir_all(dev);
    }

    #[test]
    fn d1_storage_missing_usage_evidence_fails_closed() {
        let read = RealKernel::new();
        let dev = tmp("storage_missing_usage");
        mark_storage_pci(&dev);
        // No power/runtime_usage file at all.
        assert_eq!(
            storage_live_use_block(&read, &dev),
            Some(RuntimePmActuationBlock::StorageLiveUseEvidenceUnavailable)
        );
        assert_eq!(
            actuation_precheck(&read, &dev),
            Err(RuntimePmActuationBlock::StorageLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(dev);
    }

    #[test]
    fn d1_storage_unparseable_usage_evidence_fails_closed() {
        let read = RealKernel::new();
        let dev = tmp("storage_unparseable_usage");
        mark_storage_pci(&dev);
        set_runtime_usage(&dev, "not-a-number");
        assert_eq!(
            storage_live_use_block(&read, &dev),
            Some(RuntimePmActuationBlock::StorageLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(dev);
    }

    #[test]
    fn d1_storage_live_use_check_does_not_apply_to_other_classes() {
        // Self-contained class check, mirroring `network_carrier_up`: calling
        // this on a non-storage device must never produce a storage-shaped
        // block, even if that device happens to expose the same file name
        // for an unrelated reason.
        let read = RealKernel::new();
        let network = tmp("storage_check_on_network");
        fs::write(network.join("class"), "0x020000\n").unwrap();
        set_runtime_usage(&network, "5");
        assert_eq!(storage_live_use_block(&read, &network), None);
        let _ = fs::remove_dir_all(network);
    }

    #[test]
    fn d1_camera_zero_open_fds_with_valid_mapping_permits_the_runtime_status_check() {
        // Unlike storage, this predicate's evidence source is the real,
        // system-wide `/proc`, which this test cannot fully control (which
        // pids exist, and which of their `fd` directories this process can
        // read, both vary by environment and sandboxing). So the "permits"
        // path is proven through the injectable `camera_live_use_block_under`
        // against a controlled, empty proc fixture — no pids at all, hence
        // no open descriptor can exist — rather than through
        // `camera_live_use_block`/`actuation_precheck`, which are fixed to
        // the real `/proc` and are exercised by the deny-path tests below
        // instead (their failure mode does not depend on ambient process
        // state, because it is resolved before `/proc` is ever read).
        let read = RealKernel::new();
        let dev = tmp("camera_zero_fds");
        mark_camera_usb(&dev);
        set_video4linux_node(&dev, "video1");
        set_runtime_status(&dev, "active");

        let proc_dir = tmp("camera_zero_fds_proc");
        assert_eq!(camera_live_use_block_under(&read, &dev, &proc_dir), None);

        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_camera_matching_open_fd_denies_as_in_use() {
        let read = RealKernel::new();
        let dev = tmp("camera_in_use_device");
        mark_camera_usb(&dev);
        set_video4linux_node(&dev, "video3");
        set_runtime_status(&dev, "active");

        let proc_dir = tmp("camera_in_use_proc");
        let fd_dir = proc_with_pid(&proc_dir, "4242");
        symlink_fd(&fd_dir, "7", Path::new("/dev/video3"));

        assert_eq!(
            camera_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::CameraInUse)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_camera_missing_video4linux_mapping_denies_as_evidence_unavailable() {
        let read = RealKernel::new();
        let dev = tmp("camera_no_v4l_mapping");
        mark_camera_usb(&dev);
        set_runtime_status(&dev, "active");
        // No `video4linux/` directory at all: driver not bound, or not really
        // a V4L2 device despite classification.
        let proc_dir = tmp("camera_no_v4l_mapping_proc");

        assert_eq!(
            camera_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::CameraLiveUseEvidenceUnavailable)
        );
        assert_eq!(
            actuation_precheck(&read, &dev),
            Err(RuntimePmActuationBlock::CameraLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_camera_ambiguous_video4linux_mapping_denies_as_evidence_unavailable() {
        // Two `videoN` children is a topology this predicate has no verified
        // rule for; guessing which one is authoritative would be exactly the
        // kind of guess this module exists to refuse.
        let read = RealKernel::new();
        let dev = tmp("camera_ambiguous_v4l_mapping");
        mark_camera_usb(&dev);
        set_video4linux_node(&dev, "video0");
        set_video4linux_node(&dev, "video1");
        let proc_dir = tmp("camera_ambiguous_v4l_mapping_proc");

        assert_eq!(
            camera_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::CameraLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_camera_exited_process_between_listing_and_fd_read_is_skipped_not_denied() {
        // A pid directory that has vanished by the time its `fd` child is
        // read (`NotFound`) means that process is gone and holds nothing —
        // this must not deny the whole scan, or the check would become
        // unusable on any real, live system where processes are constantly
        // exiting during the scan.
        let read = RealKernel::new();
        let dev = tmp("camera_exited_pid");
        mark_camera_usb(&dev);
        set_video4linux_node(&dev, "video2");

        let proc_dir = tmp("camera_exited_pid_proc");
        // A pid directory whose `fd` child does not exist reproduces exactly
        // the `NotFound` a real scan gets when the process exits between
        // listing `/proc` and reading `/proc/<pid>/fd`.
        fs::create_dir_all(proc_dir.join("999")).unwrap();
        // A genuinely live pid, still with nothing open on this camera, so
        // the scan also proves it kept looking past the vanished one.
        let fd_dir = proc_with_pid(&proc_dir, "1000");
        symlink_fd(&fd_dir, "0", Path::new("/dev/null"));

        assert_eq!(camera_live_use_block_under(&read, &dev, &proc_dir), None);
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_camera_unreadable_proc_pid_fd_denies_the_whole_check() {
        // The documented fail-closed decision: an `fd` entry that exists but
        // cannot be read as a directory for a reason *other* than the process
        // having exited must deny the whole check, because it is a genuine
        // gap in the evidence rather than a confirmed absence of use.
        //
        // Tests in this repository run as root, and root bypasses ordinary
        // permission bits, so `chmod 000` cannot reproduce a real
        // `PermissionDenied` here. This instead makes `fd` a plain file where
        // a directory is expected, which fails with a distinct, genuine I/O
        // error (not-a-directory) rather than `NotFound` — the same "some
        // other read failure" branch a real permission denial would take.
        let read = RealKernel::new();
        let dev = tmp("camera_unreadable_fd_dir");
        mark_camera_usb(&dev);
        set_video4linux_node(&dev, "video4");

        let proc_dir = tmp("camera_unreadable_fd_dir_proc");
        let pid_dir = proc_dir.join("5555");
        fs::create_dir_all(&pid_dir).unwrap();
        fs::write(pid_dir.join("fd"), b"not a directory").unwrap();
        assert_ne!(
            fs::read_dir(pid_dir.join("fd")).err().map(|e| e.kind()),
            Some(std::io::ErrorKind::NotFound)
        );

        assert_eq!(
            camera_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::CameraLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_camera_live_use_check_does_not_apply_to_other_classes() {
        // Self-contained class check, mirroring
        // `d1_storage_live_use_check_does_not_apply_to_other_classes`: calling
        // this on a non-camera device must never produce a camera-shaped
        // block, even if that device happens to expose a `video4linux/videoN`
        // mapping that some process holds open.
        let read = RealKernel::new();
        let network = tmp("camera_check_on_network");
        fs::write(network.join("class"), "0x020000\n").unwrap();
        set_video4linux_node(&network, "video9");

        let proc_dir = tmp("camera_check_on_network_proc");
        let fd_dir = proc_with_pid(&proc_dir, "6666");
        symlink_fd(&fd_dir, "1", Path::new("/dev/video9"));

        assert_eq!(
            camera_live_use_block_under(&read, &network, &proc_dir),
            None
        );
        let _ = fs::remove_dir_all(&network);
        let _ = fs::remove_dir_all(&proc_dir);
    }
}
