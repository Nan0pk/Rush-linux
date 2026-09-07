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

use std::path::Path;

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
            0x00 => {},
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
pub(crate) fn classify_device(
    read: &dyn KernelRead,
    device_dir: &Path,
) -> RuntimePmDeviceClass {
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

    // PCI exposes a 24-bit class code as 0xBBSSPP (base, subclass,
    // programming interface). This is available without driver-specific I/O.
    if let Ok(value) = read.read_to_string(&device_dir.join("class")) {
        if let Some(value) = parse_pci_class(&value) {
            flags.mark_pci_class(value);
        }
    }

    flags.finish()
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
        assert_eq!(
            classify_device(&read, &audio),
            RuntimePmDeviceClass::Audio
        );

        let camera = tmp("class_camera");
        add_usb_interface(&camera, "1-2:1.0", "0e");
        assert_eq!(
            classify_device(&read, &camera),
            RuntimePmDeviceClass::Camera
        );

        let input = tmp("class_input");
        add_usb_interface(&input, "1-3:1.0", "03");
        assert_eq!(
            classify_device(&read, &input),
            RuntimePmDeviceClass::Input
        );

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
        assert_eq!(
            classify_device(&read, &other),
            RuntimePmDeviceClass::Other
        );
        let _ = fs::remove_dir_all(unknown);
        let _ = fs::remove_dir_all(per_interface_without_interfaces);
        let _ = fs::remove_dir_all(other);
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
}
