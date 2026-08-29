//! WP-N7 — display depth helper logic (backlight selection + floor clamp).
//!
//! Design: `docs/research/0007-display-panel-backlight-psr-vrr-dpms.md`. Most
//! N7 levers (PSR observe, VRR/DPMS/ABM hints) are compositor-owned and require
//! the `optid-display-bridge` user service (deferred). The one lever optid owns
//! directly is **backlight brightness** (Decision 2): the system service writes
//! `/sys/class/backlight/<bl>/brightness` itself.
//!
//! This module holds the two unit-testable pieces: picking the right backlight
//! device among several (§1.1) and clamping the target so the panel never goes
//! black — the §1.3 / Decision 6 user-safety floor.

use std::path::{Path, PathBuf};

use crate::kernel_io::KernelRead;

/// Battery-idle backlight target as a percentage of `max_brightness`.
pub(crate) const DEFAULT_TARGET_PCT: u8 = 40;

/// Hard minimum brightness floor (percent of max) optid will never dim below,
/// regardless of target. Protects against a black screen and serves as the
/// conservative default PWM-flicker floor until per-HWID `pwm_floor_pct` data
/// exists (§1.3). The "fixed interactive floor" of the WP-N7 PASS criterion.
pub(crate) const MIN_FLOOR_PCT: u8 = 10;

/// Read a backlight device's `max_brightness`, if present and parseable.
///
/// F2: reads go through the injected kernel seam, not `std::fs`, so the same
/// code path serves production and a simulated machine. A direct `std::fs`
/// read here was the one remaining backlight hole in that seam.
pub(crate) fn read_max_brightness(read: &dyn KernelRead, device_dir: &Path) -> Option<u64> {
    read.read_to_string(&device_dir.join("max_brightness"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Select the active backlight device among candidates (§1.1 heuristic): prefer
/// vendor-GPU backlights (`intel_backlight`, `amdgpu_bl*`, `nvidia_*`) over the
/// often-inert `acpi_video*`, and among those pick the one with the highest
/// `max_brightness` (the most granular control). Returns `None` if the only
/// candidates are `acpi_video*` with no vendor device — still selecting the best
/// available rather than nothing.
pub(crate) fn select_backlight(read: &dyn KernelRead, candidates: &[PathBuf]) -> Option<PathBuf> {
    fn is_acpi_video(dir: &Path) -> bool {
        dir.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("acpi_video"))
            .unwrap_or(false)
    }

    // Prefer non-acpi_video devices; fall back to acpi_video only if nothing else.
    let mut best: Option<(PathBuf, u64)> = None;
    let mut best_acpi: Option<(PathBuf, u64)> = None;
    for dir in candidates {
        let max = read_max_brightness(read, dir).unwrap_or(0);
        if is_acpi_video(dir) {
            if best_acpi.as_ref().map(|(_, m)| max > *m).unwrap_or(true) {
                best_acpi = Some((dir.clone(), max));
            }
        } else if best.as_ref().map(|(_, m)| max > *m).unwrap_or(true) {
            best = Some((dir.clone(), max));
        }
    }
    best.or(best_acpi).map(|(dir, _)| dir)
}

/// Compute the target raw brightness for `target_pct` of `max_brightness`,
/// clamped to never fall below `MIN_FLOOR_PCT` of max (and never reach 0). The
/// result is always in `1..=max_brightness`.
pub(crate) fn compute_target_brightness(max_brightness: u64, target_pct: u8) -> u64 {
    if max_brightness == 0 {
        return 0;
    }
    let pct = target_pct.max(MIN_FLOOR_PCT) as u64;
    let raw = max_brightness.saturating_mul(pct) / 100;
    raw.clamp(1, max_brightness)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel_io::RealKernel;
    use std::fs;

    fn mkdev(base: &Path, name: &str, max: &str) -> PathBuf {
        let d = base.join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("max_brightness"), max).unwrap();
        d
    }

    #[test]
    fn prefers_vendor_over_acpi_video() {
        let base = std::env::temp_dir().join(format!("optid_bl_sel_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let acpi = mkdev(&base, "acpi_video0", "100");
        let intel = mkdev(&base, "intel_backlight", "96000");
        let kernel = RealKernel::new();
        let sel = select_backlight(&kernel, &[acpi.clone(), intel.clone()]).unwrap();
        assert_eq!(sel, intel);
        // Only acpi_video present -> still selected (best available).
        let sel2 = select_backlight(&kernel, std::slice::from_ref(&acpi)).unwrap();
        assert_eq!(sel2, acpi);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn target_clamps_to_floor_and_never_zero() {
        // 40% of 1000 = 400.
        assert_eq!(compute_target_brightness(1000, 40), 400);
        // Below floor (5% < 10%) clamps up to 10% = 100.
        assert_eq!(compute_target_brightness(1000, 5), 100);
        // 0% requested still floored to 10%.
        assert_eq!(compute_target_brightness(1000, 0), 100);
        // Tiny max never yields 0.
        assert_eq!(compute_target_brightness(5, 10), 1);
        // Zero max -> zero (no device).
        assert_eq!(compute_target_brightness(0, 40), 0);
    }
}
