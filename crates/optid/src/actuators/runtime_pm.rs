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
/// This deliberately contains no guessed delay values. Network has a hard
/// live-use predicate (`carrier == 1`), storage has one based on the kernel's
/// own runtime-PM usage count (see [`storage_live_use_block`]), camera has
/// one based on whether any process holds its `/dev/videoN` node open (see
/// [`camera_live_use_block`]), audio has one based on whether any process
/// holds any of its published `/dev/snd/*` nodes open (see
/// [`audio_live_use_block`]), and input has one based on whether any process
/// holds any of its `/dev/input/*`, `/dev/hidrawN` or `/dev/usb/hiddevN`
/// nodes open (see [`input_live_use_block`]). Composite and
/// other-classified devices remain denied outright: a composite device mixes
/// functions this module has not agreed a combined rule for, and "other"
/// carries no predicate at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimePmActuationBlock {
    UnknownClass,
    LiveUseGuardNotImplemented(RuntimePmDeviceClass),
    StorageLiveUseEvidenceUnavailable,
    StorageInUse,
    CameraLiveUseEvidenceUnavailable,
    CameraInUse,
    AudioLiveUseEvidenceUnavailable,
    AudioInUse,
    InputLiveUseEvidenceUnavailable,
    InputInUse,
    InputInUseByKernelHandler,
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
            Self::AudioLiveUseEvidenceUnavailable => {
                "this audio device has no readable sound/cardN mapping with at least one controlC or pcmC device node, or its open-file-descriptor scan could not be completed"
            }
            Self::AudioInUse => {
                "a process currently holds one of this audio device's control or PCM device nodes open"
            }
            Self::InputLiveUseEvidenceUnavailable => {
                "this input device has no readable input/inputN mapping with an event, mouse, or js device node for every input device, its handler list in /proc/bus/input/devices is unreadable, missing, or ambiguous, or its open-file-descriptor scan could not be completed"
            }
            Self::InputInUse => {
                "a process currently holds one of this input device's event, mouse, js, hidraw, or hiddev device nodes open"
            }
            Self::InputInUseByKernelHandler => {
                "a kernel-internal input handler (such as the console keyboard, keyboard-light, or sysrq handler) holds this input device open"
            }
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
/// Network, storage, camera, audio, and input are the ready classes for this
/// slice: network already had a hard carrier guard, storage has one based on
/// the kernel's own runtime-PM usage count (see [`storage_live_use_block`]),
/// camera has one based on whether any process holds its `/dev/videoN` node
/// open (see [`camera_live_use_block`]), audio has one based on whether any
/// process holds any of its published `/dev/snd/*` nodes open (see
/// [`audio_live_use_block`]), and input has one based on whether any process
/// holds any of its input, hidraw, or hiddev nodes open (see
/// [`input_live_use_block`]). Composite and other devices remain denied until
/// a live-use predicate is implemented for them without relying on the
/// research-only timing hypotheses.
pub(crate) fn actuation_precheck(
    read: &dyn KernelRead,
    device_dir: &Path,
) -> Result<RuntimePmActuationReady, RuntimePmActuationBlock> {
    let class = classify_device(read, device_dir);
    let live_use_block = match class {
        RuntimePmDeviceClass::Unknown => return Err(RuntimePmActuationBlock::UnknownClass),
        RuntimePmDeviceClass::Network => None,
        RuntimePmDeviceClass::Storage => storage_live_use_block(read, device_dir),
        RuntimePmDeviceClass::Camera => camera_live_use_block(read, device_dir),
        RuntimePmDeviceClass::Audio => audio_live_use_block(read, device_dir),
        RuntimePmDeviceClass::Input => input_live_use_block(read, device_dir),
        RuntimePmDeviceClass::Composite | RuntimePmDeviceClass::Other => {
            return Err(RuntimePmActuationBlock::LiveUseGuardNotImplemented(class));
        }
    };
    finish_actuation_precheck(read, device_dir, class, live_use_block)
}

/// Test-only mirror of [`actuation_precheck`], parameterized on the proc root
/// so this module's own tests can exercise the camera, audio, and input arms
/// — the only ones that consult `/proc` — against a controlled fixture instead of
/// the real, system-wide `/proc`. This exists only so the "device is
/// classified correctly and, with no evidence of use, proceeds to the
/// `runtime_status` check" claim can be tested for camera, audio, and input at the
/// same integration level as [`actuation_precheck`] itself, without that test
/// depending on ambient process state on whatever machine runs the test suite
/// (see the audio and camera "permits" tests' own comments for why scanning
/// the real `/proc` is not a safe basis for a deterministic test). Kept as a
/// thin duplicate of the dispatch in [`actuation_precheck`] — rather than
/// making `actuation_precheck` itself take a `proc_dir` parameter — so the
/// production entry point keeps a single, always-compiled call to
/// [`camera_live_use_block`], [`audio_live_use_block`], and
/// [`input_live_use_block`]; both functions
/// share the same [`finish_actuation_precheck`] tail, so they cannot disagree
/// about anything past the live-use decision itself.
#[cfg(test)]
fn actuation_precheck_under(
    read: &dyn KernelRead,
    device_dir: &Path,
    proc_dir: &Path,
) -> Result<RuntimePmActuationReady, RuntimePmActuationBlock> {
    let class = classify_device(read, device_dir);
    let live_use_block = match class {
        RuntimePmDeviceClass::Unknown => return Err(RuntimePmActuationBlock::UnknownClass),
        RuntimePmDeviceClass::Network => None,
        RuntimePmDeviceClass::Storage => storage_live_use_block(read, device_dir),
        RuntimePmDeviceClass::Camera => camera_live_use_block_under(read, device_dir, proc_dir),
        RuntimePmDeviceClass::Audio => audio_live_use_block_under(read, device_dir, proc_dir),
        RuntimePmDeviceClass::Input => input_live_use_block_under(read, device_dir, proc_dir),
        RuntimePmDeviceClass::Composite | RuntimePmDeviceClass::Other => {
            return Err(RuntimePmActuationBlock::LiveUseGuardNotImplemented(class));
        }
    };
    finish_actuation_precheck(read, device_dir, class, live_use_block)
}

/// The shared tail of [`actuation_precheck`] and [`actuation_precheck_under`]:
/// apply the already-decided live-use verdict, then apply the `runtime_status`
/// check that is identical for every class.
fn finish_actuation_precheck(
    read: &dyn KernelRead,
    device_dir: &Path,
    class: RuntimePmDeviceClass,
    live_use_block: Option<RuntimePmActuationBlock>,
) -> Result<RuntimePmActuationReady, RuntimePmActuationBlock> {
    if let Some(block) = live_use_block {
        return Err(block);
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
/// to any path in `device_nodes`, reading process directories from
/// `proc_dir`.
///
/// Shared by the camera, audio, and input live-use predicates. All answer "is this
/// device in use" the same portable, bus-independent way: does any process
/// hold an open file descriptor on one of the device's own character-device
/// nodes under `/dev`. Camera only ever has one such node (`/dev/videoN`);
/// audio can have several for one card (one `controlC<N>` plus one
/// `pcmC<N>D<M>{p,c}` per substream), so this takes a set of paths rather
/// than a single one (input, likewise, can have several). A match on any
/// element denies the whole check the same
/// way — this function does not distinguish which node in the set was open,
/// only whether the device as a whole has an open handle.
///
/// `proc_dir` exists as a parameter only so this module's own tests can point
/// it at a fixture tree instead of the real `/proc`; production always calls
/// this through [`camera_live_use_block`], [`audio_live_use_block`], or
/// [`input_live_use_block`], each of which fixes it to `/proc`.
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
fn device_in_use_by_any_process(
    read: &dyn KernelRead,
    proc_dir: &Path,
    device_nodes: &[PathBuf],
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
                Ok(target) if device_nodes.contains(&target) => {
                    return Ok(true);
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(()),
            }
        }
    }
    Ok(false)
}

/// [`device_in_use_by_any_process`] specialised to the camera predicate's
/// single video device node, kept as its own name so the camera call sites
/// read as "is this one node open" rather than "is any of this set open".
fn camera_in_use_by_any_process(
    read: &dyn KernelRead,
    proc_dir: &Path,
    video_device: &Path,
) -> Result<bool, ()> {
    let video_device = video_device.to_path_buf();
    device_in_use_by_any_process(read, proc_dir, std::slice::from_ref(&video_device))
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

/// Parse a sysfs entry name as `card<N>` and return the digit suffix `N`.
fn parse_card_name(name: &str) -> Option<&str> {
    let number = name.strip_prefix("card")?;
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then_some(number)
}

/// True when `name` is one substream entry of card `card_number`:
/// `pcmC<card_number>D<M>p` (playback) or `pcmC<card_number>D<M>c` (capture),
/// for any digit string `M`.
fn is_pcm_substream_node(name: &str, card_number: &str) -> bool {
    let Some(after_prefix) = name.strip_prefix(&format!("pcmC{card_number}D")) else {
        return false;
    };
    let Some(substream_index) = after_prefix
        .strip_suffix('p')
        .or_else(|| after_prefix.strip_suffix('c'))
    else {
        return false;
    };
    !substream_index.is_empty() && substream_index.bytes().all(|b| b.is_ascii_digit())
}

/// Enumerate the `/dev/snd/*` character-device nodes this audio device's bound
/// ALSA driver has published, from its `sound/cardN` child directory.
///
/// # The sysfs shape this relies on
///
/// A bound ALSA driver calls `snd_card_register()`, which registers the
/// card's own `struct device` under the kernel's `sound` device class
/// (`sound_class = class_create(..., "sound")`, `sound/core/sound.c`) with
/// that classified device as its parent, and then registers each component
/// device node (`controlC<N>` from `sound/core/control.c`, and one
/// `pcmC<N>D<M>p`/`pcmC<N>D<M>c` node per PCM substream from
/// `sound/core/pcm.c`) the same way, still parented to the card. The Linux
/// driver core's own "glue directory" behavior — used whenever a device
/// belongs to both a class and a bus/parent device, so that sysfs does not
/// have to place a class-named device directly under an unrelated parent's
/// directory — nests these under the parent as `<device_dir>/sound/cardN/...`
/// rather than as siblings of `<device_dir>` itself. This is the same
/// mechanism [`camera_video_device_node`] already relies on for
/// `video4linux/videoN` (V4L2 registers under the `video4linux` class the
/// same way), so this predicate trusts it for the same reason. This has been
/// checked against kernel source and the ALSA driver-registration
/// documentation, cross-checked against real `udevadm`/`aplay` output showing
/// `/sys/class/sound/controlCN` resolving through a physical device's own
/// ancestry, but this module has no environment with real ALSA sound
/// hardware to read the live directory from, so it has not been confirmed by
/// directly listing `/sys/devices/.../sound/cardN/` on running kernel.
///
/// # Which entries this counts, and why only these
///
/// `sound/cardN/` can contain other entries this predicate does not
/// recognise: informational attributes such as `id` and `number`, and
/// subdirectories such as `pcm0p`/`pcm0c` that describe PCM component state
/// rather than being the character-device nodes themselves. Only entries
/// whose name exactly matches `controlC<N>` (the mixer/control device) or
/// `pcmC<N>D<M>p`/`pcmC<N>D<M>c` (a playback/capture substream, for any `M`)
/// are treated as device nodes, matched against the same card number `N`
/// this `cardN` directory is named for. `midiC<N>D<M>` (rawmidi) and the
/// card-independent `seq`/`timer` nodes are deliberately out of scope: the
/// task this predicate exists for only asked about control and PCM nodes,
/// and a device that turns out to expose only recognised-but-out-of-scope
/// nodes still fails closed below (an empty result is treated as unavailable
/// evidence, not as "nothing to check").
///
/// More than one `cardN` child under `sound/` is a topology this predicate
/// has no verified rule for — the same reasoning
/// [`camera_video_device_node`] applies to more than one `videoN` child — so
/// it is refused the same way a missing `sound/` directory is, rather than
/// picking one card arbitrarily.
///
/// Returns `Err(())` when `sound/` is missing or unreadable, when it does not
/// contain exactly one `cardN` child, or when that card directory contains no
/// entry this predicate recognises as a control or PCM device node. Callers
/// must treat that as "cannot tell", not as "safe to suspend".
fn audio_device_nodes(read: &dyn KernelRead, device_dir: &Path) -> Result<Vec<PathBuf>, ()> {
    let sound_dir = device_dir.join("sound");
    let card_entries = read.read_dir(&sound_dir).map_err(|_| ())?;
    let mut card_numbers = card_entries.into_iter().filter_map(|entry| {
        let name = entry.file_name()?.to_str()?.to_string();
        parse_card_name(&name).map(|number| number.to_string())
    });
    let card_number = card_numbers.next().ok_or(())?;
    if card_numbers.next().is_some() {
        // More than one cardN child: ambiguous topology, refuse rather than guess.
        return Err(());
    }

    let card_dir = sound_dir.join(format!("card{card_number}"));
    let node_entries = read.read_dir(&card_dir).map_err(|_| ())?;
    let control_name = format!("controlC{card_number}");
    let nodes: Vec<PathBuf> = node_entries
        .into_iter()
        .filter_map(|entry| {
            let name = entry.file_name()?.to_str()?.to_string();
            let is_device_node = name == control_name || is_pcm_substream_node(&name, &card_number);
            is_device_node.then(|| Path::new("/dev/snd").join(name))
        })
        .collect();

    if nodes.is_empty() {
        return Err(());
    }
    Ok(nodes)
}

/// The D1 live-use predicate for the audio class, parameterized on the proc
/// root so this module's own tests can exercise it against a fixture tree.
/// [`audio_live_use_block`] is the production entry point, fixed to `/proc`.
fn audio_live_use_block_under(
    read: &dyn KernelRead,
    device_dir: &Path,
    proc_dir: &Path,
) -> Option<RuntimePmActuationBlock> {
    if !matches!(
        classify_device(read, device_dir),
        RuntimePmDeviceClass::Audio
    ) {
        return None;
    }
    let device_nodes = match audio_device_nodes(read, device_dir) {
        Ok(nodes) => nodes,
        Err(()) => return Some(RuntimePmActuationBlock::AudioLiveUseEvidenceUnavailable),
    };
    match device_in_use_by_any_process(read, proc_dir, &device_nodes) {
        Ok(false) => None,
        Ok(true) => Some(RuntimePmActuationBlock::AudioInUse),
        Err(()) => Some(RuntimePmActuationBlock::AudioLiveUseEvidenceUnavailable),
    }
}

/// The D1 live-use predicate for the audio class.
///
/// Audio devices reach D1's classification as USB Audio Device Class
/// (`0x01`) interfaces or PCI multimedia-audio/HD-audio controllers (PCI base
/// class `0x04`, subclass `0x01` or `0x03`). Once an ALSA driver is bound,
/// both buses converge on the same bus-independent signal: a `sound/cardN`
/// child directly under the classified device directory (see
/// [`audio_device_nodes`]), which names every `/dev/snd/*` character-device
/// node this card publishes. "Is this audio device in use" is then answered
/// the same standard, portable Linux way camera uses — enumerating every
/// process's open file descriptors under `/proc/<pid>/fd` and checking
/// whether any resolves to one of those nodes (see
/// [`device_in_use_by_any_process`]) — rather than by a driver- or
/// bus-specific activity counter this module does not have a verified
/// reading for, the same reasoning [`storage_live_use_block`] documents for
/// choosing `power/runtime_usage` over a bus-specific I/O counter.
///
/// # Control node counts the same as a PCM node
///
/// A card can have its mixer/control node (`controlC<N>`) open — for example
/// a mixer application only querying or setting volume — without any PCM
/// substream open at all, so no actual audio is streaming. This predicate
/// treats that the same as an open PCM node: both deny as `AudioInUse`. This
/// module has no verified way to tell, from sysfs or `/proc` alone, whether a
/// held-open control descriptor reflects a momentary query or an application
/// that intends to keep issuing mixer commands while the device is
/// autosuspended; guessing that a control-only open is harmless would be
/// exactly the kind of guess the storage and camera predicates already
/// refuse to make. The fail-closed choice costs a plausibly-unnecessary
/// denial in the mixer-query case; the alternative risks autosuspending a
/// device a process still expects to control.
///
/// Classifies the device itself, so it is safe to call on any runtime-PM
/// candidate — not only ones a caller has already confirmed are `Audio` —
/// the same self-contained style as [`network_carrier_up`],
/// [`storage_live_use_block`], and [`camera_live_use_block`]. Returns `None`
/// for every other class; it is not a substitute for the class match in
/// [`actuation_precheck`].
///
/// For a genuine audio device, returns `None` only when `sound/` resolves to
/// exactly one `cardN` directory containing at least one recognised control
/// or PCM device node, and a full, error-free scan of every process's
/// `/proc/<pid>/fd` finds no match on any of them. Returns `Some(block)` —
/// fail closed — when the mapping is missing, ambiguous, or empty of
/// recognised nodes (`AudioLiveUseEvidenceUnavailable`), when a matching open
/// descriptor is found on any node (`AudioInUse`), or when the scan itself
/// could not be completed (`AudioLiveUseEvidenceUnavailable`; see
/// [`device_in_use_by_any_process`] for which scan failures count as
/// "complete enough to prove absence" and which do not). This never treats
/// absent or ambiguous evidence as "safe to suspend".
pub(crate) fn audio_live_use_block(
    read: &dyn KernelRead,
    device_dir: &Path,
) -> Option<RuntimePmActuationBlock> {
    audio_live_use_block_under(read, device_dir, Path::new("/proc"))
}

/// Parse `name` as `<prefix><N>` for a non-empty decimal `N`.
fn has_numbered_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// True when `name` has the shape the HID core gives every HID device it
/// registers: `BBBB:VVVV:PPPP.IIII` (bus, vendor, product, instance), each
/// group hexadecimal (`drivers/hid/hid-core.c`, `hid_add_device()`:
/// `dev_set_name(&hdev->dev, "%04X:%04X:%04X.%04X", ...)`). Groups are
/// accepted at any non-zero width because `%04X` is a minimum, not a maximum.
fn is_hid_device_name(name: &str) -> bool {
    let is_hex_group =
        |group: &str| !group.is_empty() && group.bytes().all(|b| b.is_ascii_hexdigit());
    let Some((ids, instance)) = name.split_once('.') else {
        return false;
    };
    let groups: Vec<&str> = ids.split(':').collect();
    groups.len() == 3 && groups.into_iter().all(is_hex_group) && is_hex_group(instance)
}

/// List `dir`, treating a directory that does not exist as empty and every
/// other failure as unavailable evidence.
///
/// Used only for the optional glue directories (`input/`, `hidraw/`,
/// `usbmisc/`) that a device publishes only when the matching driver or
/// handler is bound. Their absence is a real, ordinary observation ("nothing
/// of that kind is registered here"); an unreadable one is not.
fn read_optional_dir(read: &dyn KernelRead, dir: &Path) -> Result<Vec<PathBuf>, ()> {
    match read.read_dir(dir) {
        Ok(entries) => Ok(entries),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(()),
    }
}

/// What sysfs says about one input-class device: the device nodes a process
/// could be using it through, the `inputN` names found, and whether any
/// `inputN` carries a keyboard-light folder.
#[derive(Default)]
struct InputMapping {
    nodes: Vec<PathBuf>,
    input_names: Vec<String>,
    led_folder_seen: bool,
}

/// Collect the `/dev/input/*` nodes published under one `input/` glue
/// directory into `mapping`. Returns `Err(())` if any `inputN` there is
/// unreadable, publishes no recognised node, or publishes more than one node
/// of the same kind.
///
/// Also records whether any `inputN` has an `inputN::<led>` child. The
/// keyboard-light handler creates those LED class devices, parented to the
/// input device, only after it has opened the input device inside the kernel
/// (`input_leds_connect()` in `drivers/input/input-leds.c` calls
/// `input_open_device()` and then names each LED `"%s::%s"` from the input
/// device's name), so one is direct evidence of a kernel-held open.
fn collect_input_class_nodes(
    read: &dyn KernelRead,
    input_glue_dir: &Path,
    mapping: &mut InputMapping,
) -> Result<(), ()> {
    for input_dir in read_optional_dir(read, input_glue_dir)? {
        let Some(input_name) = input_dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !has_numbered_name(input_name, "input") {
            continue;
        }
        mapping.input_names.push(input_name.to_string());
        let led_prefix = format!("{input_name}::");

        let mut event = Vec::new();
        let mut mouse = Vec::new();
        let mut joystick = Vec::new();
        for entry in read.read_dir(&input_dir).map_err(|_| ())? {
            let Some(name) = entry.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with(&led_prefix) {
                mapping.led_folder_seen = true;
            } else if has_numbered_name(name, "event") {
                event.push(name.to_string());
            } else if has_numbered_name(name, "mouse") {
                mouse.push(name.to_string());
            } else if has_numbered_name(name, "js") {
                joystick.push(name.to_string());
            }
        }
        if event.len() > 1 || mouse.len() > 1 || joystick.len() > 1 {
            // evdev, mousedev and joydev each create at most one node per
            // input device. More than one of a kind is a topology this
            // predicate has no verified rule for.
            return Err(());
        }
        if event.is_empty() && mouse.is_empty() && joystick.is_empty() {
            // No handler that publishes a device node is bound, so there is
            // no node whose open descriptors could show use.
            return Err(());
        }
        if !mouse.is_empty() {
            // mousedev's shared `/dev/input/mice` opens every device that has
            // a `mouseN` node while it is held open (`mixdev_open_devices()`
            // in `drivers/input/mousedev.c`), so a process holding it uses
            // this device too.
            mapping.nodes.push(PathBuf::from("/dev/input/mice"));
        }
        for name in event.into_iter().chain(mouse).chain(joystick) {
            mapping.nodes.push(Path::new("/dev/input").join(name));
        }
    }
    Ok(())
}

/// Enumerate the character-device nodes through which a process can be using
/// this input device, from the sysfs children its bound drivers publish.
///
/// # The sysfs shape this relies on
///
/// Checked against the mainline kernel source, not against a live machine:
///
/// - The input core registers every input device as `inputN` in the `input`
///   class (`drivers/input/input.c`). A class device whose parent is not
///   itself a class device is placed in a "glue" directory named after its
///   class under that parent (`get_device_parent()` in
///   `drivers/base/core.c`), so it appears as `<parent>/input/inputN`.
/// - The event handlers — evdev (`eventN`, `drivers/input/evdev.c`), mousedev
///   (`mouseN`, `drivers/input/mousedev.c`) and joydev (`jsN`,
///   `drivers/input/joydev.c`) — register their nodes in the same `input`
///   class with the input device as parent. A class device whose parent is a
///   class device is placed directly under it, so they appear as
///   `<parent>/input/inputN/eventN` and so on. The input class names their
///   device nodes `/dev/input/<name>` (`input_devnode()`).
/// - For an ordinary USB HID device, the parent of the input device is not
///   the USB interface but the HID device the HID core creates for it:
///   `usbhid` sets `hid->dev.parent = &intf->dev`
///   (`drivers/hid/usbhid/hid-core.c`), `hid-input` sets
///   `input_dev->dev.parent = &hid->dev` (`drivers/hid/hid-input.c`), and the
///   HID device is named `BBBB:VVVV:PPPP.IIII` (see [`is_hid_device_name`]).
///   So the real path is `<interface>/<hid-device>/input/inputN/eventN`, one
///   level deeper than a driver that registers its input device directly on
///   the interface (`<interface>/input/inputN`). Both shapes are read.
/// - The same HID device can also publish a raw node, `hidraw/hidrawN`
///   (`device_create(&hidraw_class, &hid->dev, ...)` in
///   `drivers/hid/hidraw.c`, node `/dev/hidrawN`), and the interface can
///   publish `usbmisc/hiddevN` (`usb_register_dev()` in
///   `drivers/usb/core/file.c` with `hiddev_devnode()` naming it
///   `/dev/usb/hiddevN`, `drivers/hid/usbhid/hiddev.c`). Programs that talk to
///   the device without going through the input layer — vendor
///   configuration tools, game-controller libraries, UPS monitors — hold
///   these open instead, so they are checked too.
///
/// A single HID device commonly registers several input devices (a keyboard
/// with separate media keys, a receiver serving several peripherals), so
/// several `inputN` children are normal, not ambiguous: every node from every
/// one of them is checked, which can only deny more, never less.
///
/// # When this refuses
///
/// Returns `Err(())` — "cannot tell", never "safe to suspend" — when:
///
/// - the device directory, or any `input/`, `inputN`, `hidraw/` or
///   `usbmisc/` directory that exists, cannot be read;
/// - no `inputN` device is found at all (no driver bound, or a node shape
///   this function does not read, such as a whole USB device whose HID
///   interfaces sit one level further down — that device is refused, and its
///   interface nodes are evaluated on their own);
/// - an `inputN` publishes no `eventN`, `mouseN` or `jsN` node, so there is
///   nothing whose open descriptors could show use;
/// - an `inputN` publishes more than one node of the same kind, a topology
///   this module has no verified rule for.
///
/// A `hidrawN` or `hiddevN` node alone does not make evidence available: it
/// is an extra place to look, not a substitute for the input mapping.
fn input_device_nodes(read: &dyn KernelRead, device_dir: &Path) -> Result<InputMapping, ()> {
    let mut mapping = InputMapping::default();
    collect_input_class_nodes(read, &device_dir.join("input"), &mut mapping)?;

    for entry in read_optional_dir(read, &device_dir.join("usbmisc"))? {
        if let Some(name) = entry.file_name().and_then(|n| n.to_str()) {
            if has_numbered_name(name, "hiddev") {
                mapping.nodes.push(Path::new("/dev/usb").join(name));
            }
        }
    }

    for child in read.read_dir(device_dir).map_err(|_| ())? {
        let Some(name) = child.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_hid_device_name(name) {
            continue;
        }
        collect_input_class_nodes(read, &child.join("input"), &mut mapping)?;
        for entry in read_optional_dir(read, &child.join("hidraw"))? {
            if let Some(name) = entry.file_name().and_then(|n| n.to_str()) {
                if has_numbered_name(name, "hidraw") {
                    mapping.nodes.push(Path::new("/dev").join(name));
                }
            }
        }
    }

    if mapping.input_names.is_empty() {
        return Err(());
    }
    Ok(mapping)
}

/// True for the handlers whose use is visible as a `/dev/input/*` node, and
/// therefore already covered by the open-descriptor scan: evdev, mousedev and
/// joydev name their handle after their node (`eventN`, `mouseN`, `jsN`).
fn is_device_node_handler(handler: &str) -> bool {
    has_numbered_name(handler, "event")
        || has_numbered_name(handler, "mouse")
        || has_numbered_name(handler, "js")
}

/// Does a kernel-internal input handler hold any of `input_names` open?
///
/// `/proc/bus/input/devices` lists every input device as a block of lines
/// ending in a blank line, including `S: Sysfs=<path of the inputN device>`
/// and `H: Handlers=<name> <name> ...` naming every handler bound to it
/// (`input_devices_seq_show()` in `drivers/input/input.c`). `inputN` names
/// are unique system-wide, so a block belongs to one of this device's input
/// devices when the last component of its sysfs path is that `inputN`.
///
/// Handlers other than evdev, mousedev and joydev open the device inside the
/// kernel as soon as they bind, with no file descriptor anywhere in `/proc`:
/// the console keyboard handler `kbd` (`kbd_connect()` in
/// `drivers/tty/vt/keyboard.c`), the keyboard-light handler `leds`
/// (`drivers/input/input-leds.c`), `sysrq` (`drivers/tty/sysrq.c`), `rfkill`
/// (`net/rfkill/input.c`) and `apm-power` (`drivers/input/apm-power.c`) all
/// call `input_open_device()` in their connect function. Any such handler, or
/// any handler name this module does not recognise, counts as use.
///
/// Returns `Err(())` when the file cannot be read, when an input device does
/// not appear in it exactly once, or when its block has no `H:` line.
fn input_held_by_kernel_handler(
    read: &dyn KernelRead,
    proc_dir: &Path,
    input_names: &[String],
) -> Result<bool, ()> {
    let listing = read
        .read_to_string(&proc_dir.join("bus").join("input").join("devices"))
        .map_err(|_| ())?;

    let mut blocks: Vec<(Option<&str>, Option<&str>)> = Vec::new();
    let mut current: (Option<&str>, Option<&str>) = (None, None);
    for line in listing.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if current != (None, None) {
                blocks.push(current);
            }
            current = (None, None);
        } else if let Some(path) = line.strip_prefix("S: Sysfs=") {
            current.0 = Some(path.trim());
        } else if let Some(handlers) = line.strip_prefix("H: Handlers=") {
            current.1 = Some(handlers);
        }
    }

    let mut held = false;
    for input_name in input_names {
        let mut matching = blocks.iter().filter(|(sysfs, _)| {
            sysfs.and_then(|path| path.rsplit('/').next()) == Some(input_name.as_str())
        });
        let (_, handlers) = matching.next().ok_or(())?;
        if matching.next().is_some() {
            return Err(());
        }
        let handlers = handlers.ok_or(())?;
        if handlers
            .split_whitespace()
            .any(|handler| !is_device_node_handler(handler))
        {
            held = true;
        }
    }
    Ok(held)
}

/// The D1 live-use predicate for the input class, parameterized on the proc
/// root so this module's own tests can exercise it against a fixture tree.
/// [`input_live_use_block`] is the production entry point, fixed to `/proc`.
fn input_live_use_block_under(
    read: &dyn KernelRead,
    device_dir: &Path,
    proc_dir: &Path,
) -> Option<RuntimePmActuationBlock> {
    if !matches!(
        classify_device(read, device_dir),
        RuntimePmDeviceClass::Input
    ) {
        return None;
    }
    let mapping = match input_device_nodes(read, device_dir) {
        Ok(mapping) => mapping,
        Err(()) => return Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable),
    };
    if mapping.led_folder_seen {
        return Some(RuntimePmActuationBlock::InputInUseByKernelHandler);
    }
    match input_held_by_kernel_handler(read, proc_dir, &mapping.input_names) {
        Ok(false) => {}
        Ok(true) => return Some(RuntimePmActuationBlock::InputInUseByKernelHandler),
        Err(()) => return Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable),
    }
    match device_in_use_by_any_process(read, proc_dir, &mapping.nodes) {
        Ok(false) => None,
        Ok(true) => Some(RuntimePmActuationBlock::InputInUse),
        Err(()) => Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable),
    }
}

/// The D1 live-use predicate for the input class.
///
/// Input devices reach D1's classification only as USB HID (`0x03`)
/// interfaces or devices; PCI has no input class, and PS/2, I2C-HID and other
/// buses publish no class attribute this module reads, so they stay
/// `Unknown` and are denied before reaching here. Once drivers are bound, the
/// device's `/dev/input/*`, `/dev/hidrawN` and `/dev/usb/hiddevN` nodes are
/// found from sysfs (see [`input_device_nodes`]), and "is this input device
/// in use" is answered the same way camera and audio answer it: does any
/// process hold an open file descriptor on one of those nodes (see
/// [`device_in_use_by_any_process`]).
///
/// # Kernel-held opens count as use too
///
/// Unlike a camera or a sound card, an input device can be held open inside
/// the kernel with no file descriptor anywhere: the text-console keyboard
/// handler, the keyboard-light handler and sysrq open every keyboard they
/// match. A keyboard someone is typing on at a text or recovery console would
/// otherwise look unused. So before the descriptor scan, this predicate
/// denies with `InputInUseByKernelHandler` when any `inputN` has an
/// `inputN::<led>` folder (see [`collect_input_class_nodes`]) or when
/// `/proc/bus/input/devices` lists a handler other than evdev, mousedev or
/// joydev for any of this device's input devices (see
/// [`input_held_by_kernel_handler`]). When the kernel is built with virtual
/// terminal support, the console keyboard handler binds to every input
/// device that reports at least one ordinary key code below `BTN_MISC`, or
/// sound events (`kbd_match()` in `drivers/tty/vt/keyboard.c`). That includes
/// every keyboard and many receivers and multi-function mice, so on a normal
/// desktop or console machine most keyboards are refused by this rule alone,
/// whatever the session is doing. A plain mouse that reports only button and
/// motion events is not matched by it.
///
/// # An open node counts as use, even though the kernel might allow suspend
///
/// The `usbhid` driver itself lets an opened HID interface autosuspend when
/// remote wakeup is available (`usbhid_open()` sets `needs_remote_wakeup`),
/// so an open descriptor does not by itself mean the kernel would refuse.
/// This predicate still denies: whether remote wakeup actually works on a
/// given device, and how long its resume takes before the first key press or
/// pointer motion is delivered, is not something this module has verified
/// evidence for. A desktop session normally holds every keyboard and mouse
/// open, so on a typical machine this predicate will deny those devices. That
/// is the intended fail-closed result until per-device evidence exists, not
/// a defect.
///
/// Classifies the device itself, so it is safe to call on any runtime-PM
/// candidate, in the same self-contained style as the other class
/// predicates. Returns `None` for every other class; it is not a substitute
/// for the class match in [`actuation_precheck`].
///
/// For a genuine input device, returns `None` only when at least one
/// `inputN` device with recognised nodes is found, none of them has a
/// keyboard-light folder or a kernel-internal handler, and a full, error-free
/// scan of every process's `/proc/<pid>/fd` finds none of their nodes open.
/// Returns `Some(InputLiveUseEvidenceUnavailable)` when the mapping is
/// missing, unreadable or ambiguous, when the handler list is unreadable or
/// does not list each input device exactly once with an `H:` line, or when
/// the scan could not be completed; `Some(InputInUseByKernelHandler)` for a
/// kernel-held open; and `Some(InputInUse)` when any node is open.
pub(crate) fn input_live_use_block(
    read: &dyn KernelRead,
    device_dir: &Path,
) -> Option<RuntimePmActuationBlock> {
    input_live_use_block_under(read, device_dir, Path::new("/proc"))
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

    fn mark_audio_usb(device: &Path) {
        // Same reasoning as `mark_camera_usb`: a bare USB interface node
        // carries its own `bInterfaceClass` directly, which is where a bound
        // ALSA driver's `sound/cardN` child directory (see
        // `add_sound_card_node`) actually appears in a real topology.
        fs::write(device.join("bInterfaceClass"), "01\n").unwrap();
    }

    fn mark_audio_pci(device: &Path) {
        // PCI base class 0x04 = multimedia controller; subclass 0x03 = HD
        // Audio, 0x01 = multimedia audio (see `ClassFlags::mark_pci_class`).
        fs::write(device.join("class"), "0x040300\n").unwrap();
    }

    /// Publish one `sound/card<card_number>/<node_name>` entry under a device
    /// directory, the way a bound ALSA driver's card and component device
    /// registration does (see `audio_device_nodes`'s doc comment for the
    /// sysfs mechanism this fixture stands in for).
    fn add_sound_card_node(device: &Path, card_number: &str, node_name: &str) {
        fs::create_dir_all(
            device
                .join("sound")
                .join(format!("card{card_number}"))
                .join(node_name),
        )
        .unwrap();
    }

    fn mark_input_usb(device: &Path) {
        // Same reasoning as `mark_camera_usb`: a bare USB HID interface node
        // (`1-1:1.0`) carries its own `bInterfaceClass` directly, and the HID
        // device the HID core creates for it is that directory's own child.
        fs::write(device.join("bInterfaceClass"), "03\n").unwrap();
    }

    /// A HID device name of the shape `hid_add_device()` gives it.
    const HID_DEVICE: &str = "0003:046D:C52B.0001";

    /// Publish `<device>/<hid_device>/input/<input_name>/<node_name>`, the
    /// shape `usbhid` + `hid-input` + an input handler produce for a USB HID
    /// interface (see `input_device_nodes`'s doc comment).
    fn add_hid_input_node(device: &Path, hid_device: &str, input_name: &str, node_name: &str) {
        fs::create_dir_all(
            device
                .join(hid_device)
                .join("input")
                .join(input_name)
                .join(node_name),
        )
        .unwrap();
    }

    /// Write a `/proc/bus/input/devices` fixture under `proc_dir`, one block
    /// per `(inputN, handlers)` pair, in the format `input_devices_seq_show()`
    /// prints (including the trailing space after each handler name).
    fn write_proc_input_devices(proc_dir: &Path, devices: &[(&str, &str)]) {
        let dir = proc_dir.join("bus").join("input");
        fs::create_dir_all(&dir).unwrap();
        let mut listing = String::new();
        for (input_name, handlers) in devices {
            let handlers: String = handlers
                .split_whitespace()
                .map(|h| format!("{h} "))
                .collect();
            listing.push_str(&format!(
                "I: Bus=0003 Vendor=046d Product=c52b Version=0111\n\
                 N: Name=\"Fixture\"\n\
                 P: Phys=usb-0000:00:14.0-1/input0\n\
                 S: Sysfs=/devices/pci0000:00/0000:00:14.0/usb1/1-1/1-1:1.0/{HID_DEVICE}/input/{input_name}\n\
                 U: Uniq=\n\
                 H: Handlers={handlers}\n\
                 B: PROP=0\n\
                 B: EV=17\n\n"
            ));
        }
        fs::write(dir.join("devices"), listing).unwrap();
    }

    /// Publish `<device>/input/<input_name>/<node_name>`, the shape a driver
    /// that registers its input device directly on the interface produces.
    fn add_direct_input_node(device: &Path, input_name: &str, node_name: &str) {
        fs::create_dir_all(device.join("input").join(input_name).join(node_name)).unwrap();
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

        // Vendor-specific USB interface class 0xff classifies as Other,
        // which carries no live-use predicate at all.
        let other = tmp("precheck_other");
        add_usb_interface(&other, "1-5:1.0", "ff");
        set_runtime_status(&other, "active");
        assert_eq!(
            actuation_precheck(&read, &other),
            Err(RuntimePmActuationBlock::LiveUseGuardNotImplemented(
                RuntimePmDeviceClass::Other
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

        for dir in [&unknown, &other, &composite] {
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

    #[test]
    fn d1_audio_zero_open_fds_with_valid_mapping_permits_the_runtime_status_check() {
        // Same reasoning as the camera equivalent: `/proc` is real and
        // system-wide, so the "permits" path is proven through the
        // injectable `audio_live_use_block_under` against a controlled,
        // empty proc fixture rather than through `audio_live_use_block`.
        let read = RealKernel::new();
        let dev = tmp("audio_zero_fds");
        mark_audio_usb(&dev);
        add_sound_card_node(&dev, "0", "controlC0");
        add_sound_card_node(&dev, "0", "pcmC0D0p");
        add_sound_card_node(&dev, "0", "pcmC0D0c");
        set_runtime_status(&dev, "active");

        let proc_dir = tmp("audio_zero_fds_proc");
        assert_eq!(audio_live_use_block_under(&read, &dev, &proc_dir), None);

        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_audio_matching_open_fd_on_pcm_node_denies_as_in_use() {
        let read = RealKernel::new();
        let dev = tmp("audio_pcm_in_use_device");
        mark_audio_pci(&dev);
        add_sound_card_node(&dev, "1", "controlC1");
        add_sound_card_node(&dev, "1", "pcmC1D0p");
        set_runtime_status(&dev, "active");

        let proc_dir = tmp("audio_pcm_in_use_proc");
        let fd_dir = proc_with_pid(&proc_dir, "7000");
        symlink_fd(&fd_dir, "9", Path::new("/dev/snd/pcmC1D0p"));

        assert_eq!(
            audio_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::AudioInUse)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_audio_matching_open_fd_on_control_node_alone_also_denies_as_in_use() {
        // The documented fail-closed choice: a mixer holding only the
        // control node open, with no PCM substream open at all, still
        // denies. See `audio_live_use_block`'s doc comment for why a
        // control-only open is not treated as harmless.
        let read = RealKernel::new();
        let dev = tmp("audio_control_only_in_use_device");
        mark_audio_usb(&dev);
        add_sound_card_node(&dev, "2", "controlC2");
        add_sound_card_node(&dev, "2", "pcmC2D0p");
        set_runtime_status(&dev, "active");

        let proc_dir = tmp("audio_control_only_in_use_proc");
        let fd_dir = proc_with_pid(&proc_dir, "7001");
        // Only the control node is open; the pcm node is not referenced by
        // any fd at all.
        symlink_fd(&fd_dir, "3", Path::new("/dev/snd/controlC2"));

        assert_eq!(
            audio_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::AudioInUse)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_audio_missing_sound_directory_denies_as_evidence_unavailable() {
        let read = RealKernel::new();
        let dev = tmp("audio_no_sound_dir");
        mark_audio_usb(&dev);
        set_runtime_status(&dev, "active");
        // No `sound/` directory at all: driver not bound, or not really an
        // ALSA-backed device despite classification.
        let proc_dir = tmp("audio_no_sound_dir_proc");

        assert_eq!(
            audio_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::AudioLiveUseEvidenceUnavailable)
        );
        assert_eq!(
            actuation_precheck(&read, &dev),
            Err(RuntimePmActuationBlock::AudioLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_audio_ambiguous_sound_card_mapping_denies_as_evidence_unavailable() {
        // Two `cardN` children is a topology this predicate has no verified
        // rule for; guessing which one is authoritative would be exactly the
        // kind of guess this module exists to refuse.
        let read = RealKernel::new();
        let dev = tmp("audio_ambiguous_card_mapping");
        mark_audio_usb(&dev);
        add_sound_card_node(&dev, "0", "controlC0");
        add_sound_card_node(&dev, "1", "controlC1");
        let proc_dir = tmp("audio_ambiguous_card_mapping_proc");

        assert_eq!(
            audio_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::AudioLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_audio_card_directory_with_no_recognised_device_node_denies_as_evidence_unavailable() {
        // The `cardN` directory exists, but publishes only entries this
        // predicate does not recognise (e.g. `id`, or a `midiC0D0` rawmidi
        // node) — no `controlCN` and no `pcmCND Mp/c` substream. An empty
        // result must fail closed, not be read as "nothing to check".
        let read = RealKernel::new();
        let dev = tmp("audio_no_recognised_node");
        mark_audio_usb(&dev);
        fs::create_dir_all(dev.join("sound").join("card0")).unwrap();
        fs::write(dev.join("sound").join("card0").join("id"), "Generic\n").unwrap();
        add_sound_card_node(&dev, "0", "midiC0D0");
        let proc_dir = tmp("audio_no_recognised_node_proc");

        assert_eq!(
            audio_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::AudioLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_audio_exited_process_between_listing_and_fd_read_is_skipped_not_denied() {
        let read = RealKernel::new();
        let dev = tmp("audio_exited_pid");
        mark_audio_usb(&dev);
        add_sound_card_node(&dev, "0", "controlC0");

        let proc_dir = tmp("audio_exited_pid_proc");
        // A pid directory whose `fd` child does not exist reproduces exactly
        // the `NotFound` a real scan gets when the process exits between
        // listing `/proc` and reading `/proc/<pid>/fd`.
        fs::create_dir_all(proc_dir.join("999")).unwrap();
        // A genuinely live pid, still with nothing open on this audio
        // device, so the scan also proves it kept looking past the vanished
        // one.
        let fd_dir = proc_with_pid(&proc_dir, "1000");
        symlink_fd(&fd_dir, "0", Path::new("/dev/null"));

        assert_eq!(audio_live_use_block_under(&read, &dev, &proc_dir), None);
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_audio_unreadable_proc_pid_fd_denies_the_whole_check() {
        // The same documented fail-closed decision as the camera equivalent:
        // an `fd` entry that exists but cannot be read as a directory for a
        // reason other than the process having exited must deny the whole
        // check.
        let read = RealKernel::new();
        let dev = tmp("audio_unreadable_fd_dir");
        mark_audio_usb(&dev);
        add_sound_card_node(&dev, "0", "controlC0");

        let proc_dir = tmp("audio_unreadable_fd_dir_proc");
        let pid_dir = proc_dir.join("5556");
        fs::create_dir_all(&pid_dir).unwrap();
        fs::write(pid_dir.join("fd"), b"not a directory").unwrap();
        assert_ne!(
            fs::read_dir(pid_dir.join("fd")).err().map(|e| e.kind()),
            Some(std::io::ErrorKind::NotFound)
        );

        assert_eq!(
            audio_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::AudioLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_audio_live_use_check_does_not_apply_to_other_classes() {
        // Self-contained class check, mirroring
        // `d1_camera_live_use_check_does_not_apply_to_other_classes`: calling
        // this on a non-audio device must never produce an audio-shaped
        // block, even if that device happens to expose a `sound/cardN`
        // mapping that some process holds open.
        let read = RealKernel::new();
        let network = tmp("audio_check_on_network");
        fs::write(network.join("class"), "0x020000\n").unwrap();
        add_sound_card_node(&network, "0", "controlC0");

        let proc_dir = tmp("audio_check_on_network_proc");
        let fd_dir = proc_with_pid(&proc_dir, "6667");
        symlink_fd(&fd_dir, "1", Path::new("/dev/snd/controlC0"));

        assert_eq!(audio_live_use_block_under(&read, &network, &proc_dir), None);
        let _ = fs::remove_dir_all(&network);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_actuation_precheck_permits_audio_device_with_no_evidence_of_use() {
        // Integration-level proof that `actuation_precheck` now reaches the
        // `runtime_status` check for audio, instead of denying it outright
        // via `LiveUseGuardNotImplemented` the way it still does for
        // composite and other-classified devices.
        //
        // This goes through `actuation_precheck_under` with a controlled,
        // empty proc fixture rather than through `actuation_precheck` itself
        // (fixed to the real, system-wide `/proc`). A real machine's `/proc`
        // holds processes this test does not own or control, and scanning it
        // can fail closed for reasons that have nothing to do with this
        // predicate's own logic: on the sandboxed host this change was
        // developed on, `/proc/1/fd/*` belongs to a process outside this
        // container's user namespace, and reading its file-descriptor
        // symlinks returns a genuine `PermissionDenied`, which
        // `device_in_use_by_any_process` correctly (and intentionally) turns
        // into a deny — the exact same "cannot tell" failure mode
        // `d1_audio_unreadable_proc_pid_fd_denies_the_whole_check` and
        // `d1_camera_unreadable_proc_pid_fd_denies_the_whole_check` prove on
        // purpose. That makes the real `/proc` path correct but
        // environment-dependent, not a safe basis for an assertion that must
        // hold on every machine that runs this suite. `actuation_precheck_under`
        // exists so this "permits" claim can still be checked at the same
        // integration level, against a fixture this test fully controls.
        let read = RealKernel::new();
        let dev = tmp("precheck_audio_permits");
        mark_audio_pci(&dev);
        add_sound_card_node(&dev, "3", "controlC3");
        add_sound_card_node(&dev, "3", "pcmC3D0p");
        set_runtime_status(&dev, "active");

        let proc_dir = tmp("precheck_audio_permits_proc");
        assert_eq!(
            actuation_precheck_under(&read, &dev, &proc_dir),
            Ok(RuntimePmActuationReady {
                class: RuntimePmDeviceClass::Audio,
                runtime_status: RuntimePmStableStatus::Active,
            })
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_name_matchers_accept_only_kernel_shaped_names() {
        assert!(is_hid_device_name("0003:046D:C52B.0001"));
        assert!(is_hid_device_name("0003:046D:C52B.10000"));
        assert!(!is_hid_device_name("1-1:1.0"));
        assert!(!is_hid_device_name("ep_81"));
        assert!(!is_hid_device_name("0003:046D.0001"));
        assert!(!is_hid_device_name("0003:046D:C52B:0001.0001"));
        assert!(!is_hid_device_name("0003:046D:C52B."));
        assert!(has_numbered_name("event12", "event"));
        assert!(!has_numbered_name("event", "event"));
        assert!(!has_numbered_name("input3::capslock", "input"));
        assert!(!has_numbered_name("js0x", "js"));
    }

    #[test]
    fn d1_input_zero_open_fds_with_valid_hid_mapping_permits_the_runtime_status_check() {
        // Same reasoning as the camera and audio equivalents: the "permits"
        // path is proven against a controlled, empty proc fixture.
        let read = RealKernel::new();
        let dev = tmp("input_zero_fds");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");
        add_hid_input_node(&dev, HID_DEVICE, "input5", "mouse0");
        // Non-node entries an input device really publishes must be ignored.
        fs::write(
            dev.join(HID_DEVICE)
                .join("input")
                .join("input5")
                .join("name"),
            "Mouse\n",
        )
        .unwrap();
        fs::create_dir_all(dev.join(HID_DEVICE).join("hidraw").join("hidraw0")).unwrap();
        set_runtime_status(&dev, "active");

        // No keyboard-light folder and only device-node handlers: the one
        // shape in which no kernel-internal handler holds the device open.
        let proc_dir = tmp("input_zero_fds_proc");
        write_proc_input_devices(&proc_dir, &[("input5", "mouse0 event5")]);
        let fd_dir = proc_with_pid(&proc_dir, "8000");
        symlink_fd(&fd_dir, "0", Path::new("/dev/null"));
        assert_eq!(input_live_use_block_under(&read, &dev, &proc_dir), None);

        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_direct_input_mapping_on_the_interface_is_also_read() {
        // A driver that registers its input device directly on the USB
        // interface (no HID device in between) publishes `input/inputN` on
        // the interface itself.
        let read = RealKernel::new();
        let dev = tmp("input_direct_mapping");
        mark_input_usb(&dev);
        add_direct_input_node(&dev, "input9", "event9");

        let proc_dir = tmp("input_direct_mapping_proc");
        write_proc_input_devices(&proc_dir, &[("input9", "event9")]);
        assert_eq!(input_live_use_block_under(&read, &dev, &proc_dir), None);

        let fd_dir = proc_with_pid(&proc_dir, "8001");
        symlink_fd(&fd_dir, "4", Path::new("/dev/input/event9"));
        assert_eq!(
            input_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::InputInUse)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_matching_open_fd_on_event_node_denies_as_in_use() {
        let read = RealKernel::new();
        let dev = tmp("input_event_in_use");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");
        set_runtime_status(&dev, "active");

        let proc_dir = tmp("input_event_in_use_proc");
        write_proc_input_devices(&proc_dir, &[("input5", "event5")]);
        let fd_dir = proc_with_pid(&proc_dir, "8002");
        symlink_fd(&fd_dir, "7", Path::new("/dev/input/event5"));

        assert_eq!(
            input_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::InputInUse)
        );
        assert_eq!(
            actuation_precheck_under(&read, &dev, &proc_dir),
            Err(RuntimePmActuationBlock::InputInUse)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_open_shared_mice_node_denies_only_a_device_with_a_mouse_node() {
        // `/dev/input/mice` opens every device that has a `mouseN` node, so
        // holding it counts as use of a mouse, but not of a keyboard that
        // has only an `eventN` node.
        let read = RealKernel::new();
        let proc_dir = tmp("input_mice_proc");
        write_proc_input_devices(
            &proc_dir,
            &[("input6", "mouse1 event6"), ("input7", "event7")],
        );
        let fd_dir = proc_with_pid(&proc_dir, "8003");
        symlink_fd(&fd_dir, "3", Path::new("/dev/input/mice"));

        let mouse = tmp("input_mice_mouse");
        mark_input_usb(&mouse);
        add_hid_input_node(&mouse, HID_DEVICE, "input6", "event6");
        add_hid_input_node(&mouse, HID_DEVICE, "input6", "mouse1");
        assert_eq!(
            input_live_use_block_under(&read, &mouse, &proc_dir),
            Some(RuntimePmActuationBlock::InputInUse)
        );

        let keyboard = tmp("input_mice_keyboard");
        mark_input_usb(&keyboard);
        add_hid_input_node(&keyboard, HID_DEVICE, "input7", "event7");
        assert_eq!(
            input_live_use_block_under(&read, &keyboard, &proc_dir),
            None
        );

        for dir in [&proc_dir, &mouse, &keyboard] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn d1_input_open_hidraw_or_hiddev_node_denies_as_in_use() {
        // Programs that bypass the input layer hold the raw HID node or the
        // legacy hiddev node open instead; both count as use.
        let read = RealKernel::new();
        let dev = tmp("input_raw_in_use");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");
        fs::create_dir_all(dev.join(HID_DEVICE).join("hidraw").join("hidraw2")).unwrap();
        fs::create_dir_all(dev.join("usbmisc").join("hiddev0")).unwrap();

        let hidraw_proc = tmp("input_raw_in_use_hidraw_proc");
        write_proc_input_devices(&hidraw_proc, &[("input5", "event5")]);
        let fd_dir = proc_with_pid(&hidraw_proc, "8004");
        symlink_fd(&fd_dir, "5", Path::new("/dev/hidraw2"));
        assert_eq!(
            input_live_use_block_under(&read, &dev, &hidraw_proc),
            Some(RuntimePmActuationBlock::InputInUse)
        );

        let hiddev_proc = tmp("input_raw_in_use_hiddev_proc");
        write_proc_input_devices(&hiddev_proc, &[("input5", "event5")]);
        let fd_dir = proc_with_pid(&hiddev_proc, "8005");
        symlink_fd(&fd_dir, "5", Path::new("/dev/usb/hiddev0"));
        assert_eq!(
            input_live_use_block_under(&read, &dev, &hiddev_proc),
            Some(RuntimePmActuationBlock::InputInUse)
        );

        for dir in [&dev, &hidraw_proc, &hiddev_proc] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn d1_input_every_input_device_under_one_hid_device_is_checked() {
        // A keyboard with separate media keys registers several input
        // devices under one HID device. That is normal, not ambiguous, and
        // an open node on any of them must deny.
        let read = RealKernel::new();
        let dev = tmp("input_several_inputs");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input10", "event10");
        add_hid_input_node(&dev, HID_DEVICE, "input11", "event11");
        add_hid_input_node(&dev, HID_DEVICE, "input12", "js0");

        let proc_dir = tmp("input_several_inputs_proc");
        write_proc_input_devices(
            &proc_dir,
            &[
                ("input10", "event10"),
                ("input11", "event11"),
                ("input12", "js0"),
            ],
        );
        assert_eq!(input_live_use_block_under(&read, &dev, &proc_dir), None);

        let fd_dir = proc_with_pid(&proc_dir, "8006");
        symlink_fd(&fd_dir, "9", Path::new("/dev/input/js0"));
        assert_eq!(
            input_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::InputInUse)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_missing_mapping_denies_as_evidence_unavailable() {
        let read = RealKernel::new();
        let dev = tmp("input_no_mapping");
        mark_input_usb(&dev);
        set_runtime_status(&dev, "active");
        // A HID device with no input devices under it (driver bound, but no
        // input handler) is still a missing mapping.
        fs::create_dir_all(dev.join(HID_DEVICE)).unwrap();
        let proc_dir = tmp("input_no_mapping_proc");

        assert_eq!(
            input_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );
        // The mapping fails before `/proc` is consulted, so the production
        // entry point gives the same deterministic answer.
        assert_eq!(
            actuation_precheck(&read, &dev),
            Err(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_raw_nodes_alone_do_not_make_evidence_available() {
        // A vendor-defined HID interface can publish only `hidrawN` and
        // `hiddevN`, with no input device. Those are extra places to look,
        // not a substitute for the input mapping.
        let read = RealKernel::new();
        let dev = tmp("input_raw_only");
        mark_input_usb(&dev);
        fs::create_dir_all(dev.join(HID_DEVICE).join("hidraw").join("hidraw3")).unwrap();
        fs::create_dir_all(dev.join("usbmisc").join("hiddev1")).unwrap();
        let proc_dir = tmp("input_raw_only_proc");

        assert_eq!(
            input_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_device_with_no_recognised_node_denies_as_evidence_unavailable() {
        // `input5` exists but no evdev, mousedev or joydev node is bound to
        // it, so no open descriptor could ever show use. One such input
        // device denies even when a sibling input device is fine.
        let read = RealKernel::new();
        let dev = tmp("input_no_recognised_node");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input4", "event4");
        add_hid_input_node(&dev, HID_DEVICE, "input5", "input5::numlock");
        fs::write(
            dev.join(HID_DEVICE)
                .join("input")
                .join("input5")
                .join("name"),
            "Keyboard\n",
        )
        .unwrap();
        let proc_dir = tmp("input_no_recognised_node_proc");

        assert_eq!(
            input_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_ambiguous_mapping_denies_as_evidence_unavailable() {
        // evdev creates exactly one `eventN` per input device. Two under one
        // `inputN` is a topology this predicate has no verified rule for.
        let read = RealKernel::new();
        let dev = tmp("input_ambiguous");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event6");
        let proc_dir = tmp("input_ambiguous_proc");

        assert_eq!(
            input_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_unreadable_mapping_directory_denies_as_evidence_unavailable() {
        // A mapping directory that exists but cannot be listed is a gap in
        // the evidence, unlike one that does not exist. Tests run as root,
        // which bypasses permission bits, so a regular file stands in for
        // "exists but cannot be read as a directory".
        let read = RealKernel::new();
        let proc_dir = tmp("input_unreadable_mapping_proc");

        let bad_input = tmp("input_unreadable_input_glue");
        mark_input_usb(&bad_input);
        add_hid_input_node(&bad_input, HID_DEVICE, "input5", "event5");
        fs::write(bad_input.join("input"), b"not a directory").unwrap();
        assert_eq!(
            input_live_use_block_under(&read, &bad_input, &proc_dir),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );

        let bad_hidraw = tmp("input_unreadable_hidraw_glue");
        mark_input_usb(&bad_hidraw);
        add_hid_input_node(&bad_hidraw, HID_DEVICE, "input5", "event5");
        fs::write(bad_hidraw.join(HID_DEVICE).join("hidraw"), b"x").unwrap();
        assert_eq!(
            input_live_use_block_under(&read, &bad_hidraw, &proc_dir),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );

        let bad_usbmisc = tmp("input_unreadable_usbmisc_glue");
        mark_input_usb(&bad_usbmisc);
        add_hid_input_node(&bad_usbmisc, HID_DEVICE, "input5", "event5");
        fs::write(bad_usbmisc.join("usbmisc"), b"x").unwrap();
        assert_eq!(
            input_live_use_block_under(&read, &bad_usbmisc, &proc_dir),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );

        let bad_input_device = tmp("input_unreadable_input_device");
        mark_input_usb(&bad_input_device);
        fs::create_dir_all(bad_input_device.join(HID_DEVICE).join("input")).unwrap();
        fs::write(
            bad_input_device
                .join(HID_DEVICE)
                .join("input")
                .join("input5"),
            b"x",
        )
        .unwrap();
        assert_eq!(
            input_live_use_block_under(&read, &bad_input_device, &proc_dir),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );

        for dir in [
            &proc_dir,
            &bad_input,
            &bad_hidraw,
            &bad_usbmisc,
            &bad_input_device,
        ] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn d1_input_keyboard_light_folder_denies_as_kernel_held() {
        // input-leds creates `inputN::<led>` only after opening the device
        // inside the kernel, so the folder alone denies, even when the
        // handler list shown here omits `leds`.
        let read = RealKernel::new();
        let dev = tmp("input_led_folder");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");
        add_hid_input_node(&dev, HID_DEVICE, "input5", "input5::capslock");
        let proc_dir = tmp("input_led_folder_proc");
        write_proc_input_devices(&proc_dir, &[("input5", "event5")]);

        assert_eq!(
            input_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::InputInUseByKernelHandler)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_kernel_internal_handler_denies_as_kernel_held() {
        // Each handler that opens the device inside the kernel, and any
        // handler name this module does not recognise, denies on its own,
        // with no file descriptor open anywhere. Checked on the second of two
        // input devices so every input device's entry is proven to be read.
        // `evbug` stands in for a handler name this module does not
        // otherwise list.
        let read = RealKernel::new();
        let dev = tmp("input_kernel_handler");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");
        add_hid_input_node(&dev, HID_DEVICE, "input6", "event6");

        for handler in ["kbd", "leds", "sysrq", "rfkill", "apm-power", "evbug"] {
            let proc_dir = tmp(&format!("input_kernel_handler_{handler}_proc"));
            let second = format!("{handler} event6");
            write_proc_input_devices(&proc_dir, &[("input5", "event5"), ("input6", &second)]);
            assert_eq!(
                input_live_use_block_under(&read, &dev, &proc_dir),
                Some(RuntimePmActuationBlock::InputInUseByKernelHandler),
                "handler {handler}"
            );
            let _ = fs::remove_dir_all(&proc_dir);
        }

        // The ordinary desktop keyboard line, through the precheck.
        set_runtime_status(&dev, "active");
        let proc_dir = tmp("input_kernel_handler_desktop_proc");
        write_proc_input_devices(
            &proc_dir,
            &[("input5", "sysrq kbd leds event5"), ("input6", "event6")],
        );
        assert_eq!(
            actuation_precheck_under(&read, &dev, &proc_dir),
            Err(RuntimePmActuationBlock::InputInUseByKernelHandler)
        );
        let _ = fs::remove_dir_all(&proc_dir);
        let _ = fs::remove_dir_all(&dev);
    }

    #[test]
    fn d1_input_missing_or_unreadable_handler_list_denies_as_evidence_unavailable() {
        let read = RealKernel::new();
        let dev = tmp("input_handler_list_unavailable");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");

        // No /proc/bus/input/devices at all.
        let missing = tmp("input_handler_list_missing_proc");
        assert_eq!(
            input_live_use_block_under(&read, &dev, &missing),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );

        // Present but unreadable as a file (a directory stands in, because
        // tests run as root and root bypasses permission bits).
        let unreadable = tmp("input_handler_list_unreadable_proc");
        fs::create_dir_all(unreadable.join("bus").join("input").join("devices")).unwrap();
        assert_eq!(
            input_live_use_block_under(&read, &dev, &unreadable),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );

        for dir in [&dev, &missing, &unreadable] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn d1_input_device_absent_duplicated_or_without_handlers_line_denies_as_evidence_unavailable() {
        let read = RealKernel::new();
        let dev = tmp("input_handler_entry_bad");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");

        // Absent: only `input50` is listed, which must not match `input5`.
        let absent = tmp("input_handler_entry_absent_proc");
        write_proc_input_devices(&absent, &[("input50", "event50")]);
        assert_eq!(
            input_live_use_block_under(&read, &dev, &absent),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );

        // Listed twice: ambiguous.
        let duplicated = tmp("input_handler_entry_duplicated_proc");
        write_proc_input_devices(&duplicated, &[("input5", "event5"), ("input5", "event5")]);
        assert_eq!(
            input_live_use_block_under(&read, &dev, &duplicated),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );

        // Listed, but the block has no `H:` line.
        let no_handlers = tmp("input_handler_entry_no_h_line_proc");
        fs::create_dir_all(no_handlers.join("bus").join("input")).unwrap();
        fs::write(
            no_handlers.join("bus").join("input").join("devices"),
            format!(
                "I: Bus=0003 Vendor=046d Product=c52b Version=0111\n\
                 S: Sysfs=/devices/usb1/1-1/1-1:1.0/{HID_DEVICE}/input/input5\n\
                 B: EV=17\n\n"
            ),
        )
        .unwrap();
        assert_eq!(
            input_live_use_block_under(&read, &dev, &no_handlers),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );

        for dir in [&dev, &absent, &duplicated, &no_handlers] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn d1_input_whole_usb_device_node_is_refused_not_traversed() {
        // A whole USB device (`1-2`) classifies as input from its HID
        // interface child, but its input devices sit under that interface's
        // own HID device, one level further down than this predicate reads.
        // It is refused; the interface node is evaluated on its own.
        let read = RealKernel::new();
        let dev = tmp("input_whole_usb_device");
        fs::write(dev.join("bDeviceClass"), "00\n").unwrap();
        add_usb_interface(&dev, "1-2:1.0", "03");
        add_hid_input_node(&dev.join("1-2:1.0"), HID_DEVICE, "input5", "event5");
        assert_eq!(classify_device(&read, &dev), RuntimePmDeviceClass::Input);
        let proc_dir = tmp("input_whole_usb_device_proc");
        write_proc_input_devices(&proc_dir, &[("input5", "event5")]);

        assert_eq!(
            input_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );
        assert_eq!(
            input_live_use_block_under(&read, &dev.join("1-2:1.0"), &proc_dir),
            None
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_exited_process_between_listing_and_fd_read_is_skipped_not_denied() {
        let read = RealKernel::new();
        let dev = tmp("input_exited_pid");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");

        let proc_dir = tmp("input_exited_pid_proc");
        write_proc_input_devices(&proc_dir, &[("input5", "event5")]);
        fs::create_dir_all(proc_dir.join("999")).unwrap();
        let fd_dir = proc_with_pid(&proc_dir, "1000");
        symlink_fd(&fd_dir, "0", Path::new("/dev/null"));

        assert_eq!(input_live_use_block_under(&read, &dev, &proc_dir), None);
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_unreadable_proc_pid_fd_denies_the_whole_check() {
        let read = RealKernel::new();
        let dev = tmp("input_unreadable_fd_dir");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");

        let proc_dir = tmp("input_unreadable_fd_dir_proc");
        // A clean handler list, so the denial below is the fd scan's alone.
        write_proc_input_devices(&proc_dir, &[("input5", "event5")]);
        let pid_dir = proc_dir.join("5557");
        fs::create_dir_all(&pid_dir).unwrap();
        fs::write(pid_dir.join("fd"), b"not a directory").unwrap();
        assert_ne!(
            fs::read_dir(pid_dir.join("fd")).err().map(|e| e.kind()),
            Some(std::io::ErrorKind::NotFound)
        );

        assert_eq!(
            input_live_use_block_under(&read, &dev, &proc_dir),
            Some(RuntimePmActuationBlock::InputLiveUseEvidenceUnavailable)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_input_live_use_check_does_not_apply_to_other_classes() {
        let read = RealKernel::new();
        let network = tmp("input_check_on_network");
        fs::write(network.join("class"), "0x020000\n").unwrap();
        add_direct_input_node(&network, "input0", "event0");

        let proc_dir = tmp("input_check_on_network_proc");
        let fd_dir = proc_with_pid(&proc_dir, "6668");
        symlink_fd(&fd_dir, "1", Path::new("/dev/input/event0"));

        assert_eq!(input_live_use_block_under(&read, &network, &proc_dir), None);
        let _ = fs::remove_dir_all(&network);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn d1_actuation_precheck_permits_input_device_with_no_evidence_of_use() {
        // Integration-level proof that the precheck now reaches the
        // `runtime_status` check for input instead of denying it outright.
        // Uses `actuation_precheck_under` with a controlled proc fixture for
        // the reason given in the audio equivalent.
        let read = RealKernel::new();
        let dev = tmp("precheck_input_permits");
        mark_input_usb(&dev);
        add_hid_input_node(&dev, HID_DEVICE, "input5", "event5");
        set_runtime_status(&dev, "suspended");

        let proc_dir = tmp("precheck_input_permits_proc");
        write_proc_input_devices(&proc_dir, &[("input5", "event5")]);
        assert_eq!(
            actuation_precheck_under(&read, &dev, &proc_dir),
            Ok(RuntimePmActuationReady {
                class: RuntimePmDeviceClass::Input,
                runtime_status: RuntimePmStableStatus::Suspended,
            })
        );

        // A transitioning status still denies after a clean live-use scan.
        set_runtime_status(&dev, "resuming");
        assert_eq!(
            actuation_precheck_under(&read, &dev, &proc_dir),
            Err(RuntimePmActuationBlock::RuntimeStatusTransitioning)
        );
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&proc_dir);
    }
}
