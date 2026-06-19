//! WP-N6 — storage / link power-management helper logic (PCIe ASPM, SATA ALPM).
//!
//! Design: `docs/research/0008-nvme-apst-pcie-aspm-sata-alpm.md`. As with the
//! N5 runtime-PM helpers, the actuator (`actuator.rs`) owns the writes,
//! journaling, and the N4 allowlist gate; this module owns the small,
//! unit-testable predicates the brief calls out as safety constraints.
//!
//! Note: NVMe APST is intentionally NOT a separate knob here. Per 0008 §1.1 the
//! preferred path to drive an NVMe controller into its deep (non-operational)
//! power states is runtime-PM autosuspend on the controller device — which the
//! WP-N5 `runtime_pm` actuator already handles. N6 adds the two genuinely new
//! knobs: per-device PCIe ASPM and SATA ALPM.

use std::fs;
use std::path::Path;

/// The universally-safe SATA ALPM policy for flash storage (0008 §1.3 / §Decision C):
/// HIPM + DIPM, letting the device vote for link L1 when its queue is empty.
pub(crate) const DEFAULT_ALPM_POLICY: &str = "med_power_with_dipm";

/// True if this PCI device is an Intel CNVi radio (PCI class `0x0280`). CNVi is
/// not a standard PCIe endpoint — its link power is managed by the PCH/ME
/// firmware, and `link/l1_aspm` writes do not apply — so the actuator skips ASPM
/// for these devices (0008 §1.4).
pub(crate) fn is_cnvi(device_dir: &Path) -> bool {
    matches!(
        fs::read_to_string(device_dir.join("class")),
        Ok(c) if c.trim_start_matches("0x").starts_with("0280")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("optid_stor_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn cnvi_detected_by_class() {
        let dev = tmp("cnvi");
        fs::write(dev.join("class"), "0x028000\n").unwrap();
        assert!(is_cnvi(&dev));
        let _ = fs::remove_dir_all(&dev);
    }

    #[test]
    fn non_cnvi_not_flagged() {
        let dev = tmp("nvme");
        fs::write(dev.join("class"), "0x010802\n").unwrap(); // NVMe controller
        assert!(!is_cnvi(&dev));
        // Missing class attribute -> not CNVi.
        let dev2 = tmp("noclass");
        assert!(!is_cnvi(&dev2));
        let _ = fs::remove_dir_all(&dev);
        let _ = fs::remove_dir_all(&dev2);
    }

    #[test]
    fn default_alpm_policy_is_safe_dipm() {
        assert_eq!(DEFAULT_ALPM_POLICY, "med_power_with_dipm");
    }
}
