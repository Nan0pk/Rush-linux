//! Host fingerprinting — captures the identity of the machine so results can
//! be grouped by hardware in `benchmarks/results/<date>/<host>/`.
//!
//! Field choices match `rushbench`'s `HostInfo` struct so the two rigs can
//! be joined later without translation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostFingerprint {
    /// `uname -r`.
    pub kernel: String,
    /// First line of `/proc/cpuinfo` Model name, stripped.
    pub cpu_model: String,
    /// "board_vendor board_name" from DMI sysfs, or "unknown".
    pub dmi_board: String,
    /// Battery design capacity in µWh, 0 if no battery.
    pub battery_design_uwh: u64,
    /// A short stable hash of the above, used as the directory name in results/.
    pub fingerprint: String,
}

impl HostFingerprint {
    /// Capture from the running system. Best-effort — missing files yield "unknown" / 0.
    pub fn capture() -> Self {
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_default()
            .trim()
            .to_string();

        let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|text| {
                text.lines()
                    .find(|l| l.starts_with("model name"))
                    .and_then(|l| l.split(':').nth(1))
                    .map(|s| s.trim().to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        let board_vendor = read_trim("/sys/class/dmi/id/board_vendor").unwrap_or_default();
        let board_name = read_trim("/sys/class/dmi/id/board_name").unwrap_or_default();
        let dmi_board = if board_vendor.is_empty() || board_name.is_empty() {
            "unknown".to_string()
        } else {
            format!("{} {}", board_vendor, board_name)
        };

        let battery_design_uwh = battery_design_uwh();

        let fingerprint = {
            // Simple FNV-1a 64-bit, hex-encoded. Good enough for a directory name;
            // not a security primitive.
            let mut hash: u64 = 0xcbf29ce484222325;
            for b in kernel.as_bytes().iter().chain(b"|".iter())
                .chain(cpu_model.as_bytes().iter()).chain(b"|".iter())
                .chain(dmi_board.as_bytes().iter()).chain(b"|".iter())
                .chain(battery_design_uwh.to_string().as_bytes().iter())
            {
                hash ^= *b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            format!("{:016x}", hash)
        };

        HostFingerprint {
            kernel,
            cpu_model,
            dmi_board,
            battery_design_uwh,
            fingerprint,
        }
    }
}

fn read_trim(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn battery_design_uwh() -> u64 {
    // Take the first battery found. BAT0 on most laptops, BAT1 on some.
    for bat in ["BAT0", "BAT1", "BATT"] {
        let path = format!("/sys/class/power_supply/{}/energy_full_design", bat);
        if let Some(s) = read_trim(&path) {
            if let Ok(v) = s.parse::<u64>() {
                return v;
            }
        }
    }
    0
}
