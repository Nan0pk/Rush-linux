//! WP-N5 — runtime-PM autosuspend helper logic.
//!
//! Design: `docs/research/0009-runtime-pm-autosuspend-policy.md`. The actuator
//! (`actuator.rs`) owns the writes, journaling, and the N4 allowlist gate; this
//! module owns the *should-we-touch-this-device?* predicates that the design
//! brief calls out as hard safety constraints (§1.3, §1.6) and that are worth
//! unit-testing against a synthetic sysfs tree.
//!
//! Conservative-by-construction: the actuator only ever writes `power/control`
//! and `power/autosuspend_delay_ms`. It never writes `power/wakeup`, so a
//! device's wake capability is preserved no matter what (§1.3). These helpers
//! add the remaining guards: skip network devices whose link is up (§1.6), and
//! surface a warning when autosuspending an input device whose wakeup is
//! already disabled.

use std::fs;
use std::path::Path;

/// Default battery-idle autosuspend delay (ms). The 0009 §1.2 table has tighter
/// per-class targets (e.g. 500 ms for PCIe Wi-Fi), but those are `[HYPOTHESIS]`
/// values pending the §4 hardware experiments. The focused-core increment uses
/// one conservative delay across all allowlisted devices; per-class tuning is a
/// tracked follow-up. 2000 ms matches the common kernel/USB default.
pub(crate) const DEFAULT_AUTOSUSPEND_DELAY_MS: i32 = 2000;

/// True if any network interface backed by this device has its link up
/// (`carrier == 1`). Autosuspending a device with an active link would silently
/// drop packets (§1.6), so the actuator skips these.
pub(crate) fn network_carrier_up(device_dir: &Path) -> bool {
    let net_dir = device_dir.join("net");
    let Ok(entries) = fs::read_dir(&net_dir) else {
        return false; // not a network device
    };
    for entry in entries.filter_map(Result::ok) {
        let carrier = entry.path().join("carrier");
        if let Ok(val) = fs::read_to_string(&carrier) {
            if val.trim() == "1" {
                return true;
            }
        }
    }
    false
}

/// Heuristic: does this device expose a USB HID (interface class `03`) child?
/// Used only to decide whether to emit the §1.3 wakeup warning — never to gate
/// the write itself.
pub(crate) fn is_hid_input(device_dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(device_dir) else {
        return false;
    };
    for entry in entries.filter_map(Result::ok) {
        let class = entry.path().join("bInterfaceClass");
        if let Ok(val) = fs::read_to_string(&class) {
            if val.trim() == "03" {
                return true;
            }
        }
    }
    false
}

/// True if the device's `power/wakeup` attribute reads `disabled`. Absent
/// attribute ⇒ false (nothing to warn about).
pub(crate) fn wakeup_disabled(device_dir: &Path) -> bool {
    matches!(
        fs::read_to_string(device_dir.join("power").join("wakeup")),
        Ok(v) if v.trim() == "disabled"
    )
}

/// If autosuspending this device would be questionable for wakeup reasons
/// (it is an input device but wakeup is disabled), return a human-readable
/// warning. The actuator logs it but proceeds — it does not modify wakeup.
pub(crate) fn wakeup_warning(device_dir: &Path) -> Option<String> {
    if is_hid_input(device_dir) && wakeup_disabled(device_dir) {
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

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("optid_rpm_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn carrier_up_detected() {
        let dev = tmp("carrier_up");
        let iface = dev.join("net").join("enp0s31f6");
        fs::create_dir_all(&iface).unwrap();
        fs::write(iface.join("carrier"), "1\n").unwrap();
        assert!(network_carrier_up(&dev));
        let _ = fs::remove_dir_all(&dev);
    }

    #[test]
    fn carrier_down_or_absent_not_up() {
        let dev = tmp("carrier_down");
        let iface = dev.join("net").join("enp0s31f6");
        fs::create_dir_all(&iface).unwrap();
        fs::write(iface.join("carrier"), "0\n").unwrap();
        assert!(!network_carrier_up(&dev));
        // A non-network device (no net/ dir) is also "not up".
        let dev2 = tmp("carrier_none");
        assert!(!network_carrier_up(&dev2));
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&dev2);
    }

    #[test]
    fn hid_input_and_wakeup_warning() {
        let dev = tmp("hid");
        // USB HID interface child.
        let iface = dev.join("1-1:1.0");
        fs::create_dir_all(&iface).unwrap();
        fs::write(iface.join("bInterfaceClass"), "03\n").unwrap();
        assert!(is_hid_input(&dev));

        // wakeup disabled -> warning present.
        let power = dev.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(power.join("wakeup"), "disabled\n").unwrap();
        assert!(wakeup_disabled(&dev));
        assert!(wakeup_warning(&dev).is_some());

        // wakeup enabled -> no warning.
        fs::write(power.join("wakeup"), "enabled\n").unwrap();
        assert!(!wakeup_disabled(&dev));
        assert!(wakeup_warning(&dev).is_none());
        let _ = fs::remove_dir_all(&dev);
    }
}
