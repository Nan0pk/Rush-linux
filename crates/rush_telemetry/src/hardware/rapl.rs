//! Intel RAPL energy telemetry via direct MSR access.
//!
//! This module reads `MSR_PKG_ENERGY_STATUS` (0x611) directly via
//! `/dev/cpu/CPUID/msr`, bypassing the powercap sysfs layer entirely.
//! Falls back to perf_event, then sysfs, if direct MSR access is blocked.
//!
//! ## MSR Register Layout
//!
//! - `MSR_RAPL_POWER_UNIT` (0x606): bits [12:8] = energy unit exponent ESU.
//!   Default ESU=16 → each LSB = 2^-16 J ≈ 15.3 μJ.
//! - `MSR_PKG_ENERGY_STATUS` (0x611): bits [31:0] = rolling energy counter.
//!   Wraps at 2^32 × 15.3μJ ≈ 65.5s at high power.

use std::fs::File;
use std::io;
use std::os::unix::io::RawFd;
use std::path::Path;
use std::time::Instant;

/// Intel MSR addresses for RAPL.
const MSR_RAPL_POWER_UNIT: u32 = 0x606;
const MSR_PKG_ENERGY_STATUS: u32 = 0x611;

/// Fallback tier for energy telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnergyTier {
    /// Direct MSR read via /dev/cpu/N/msr — ~200ns, requires cap_sys_rawio.
    MsrDirect,
    /// perf_event_open with PERF_TYPE_POWER — ~1μs, works without cap_sys_rawio.
    PerfEvent,
    /// Powercap sysfs /sys/class/powercap/intel-rapl:0/energy_uj — ~5μs, no special caps.
    SysfsRapl,
    /// Battery sysfs /sys/class/power_supply/BAT0/energy_now — ~10μs.
    SysfsBattery,
    /// No energy counter available.
    Unavailable,
}

/// A single raw energy reading from the hardware.
///
/// This is the in-memory representation. No scaling has been applied.
#[derive(Debug, Clone, Copy)]
pub struct RawEnergySample {
    /// Timestamp from `CLOCK_MONOTONIC` (not TSC — we convert later).
    pub instant: Instant,
    /// Raw counter value. For MSR: 32-bit RAPL ticks. For sysfs: microjoules.
    pub raw_value: u64,
    /// Which tier produced this reading.
    pub tier: EnergyTier,
}

/// Energy source handle — opened once, reused for all samples.
pub enum EnergySource {
    Msr {
        file: File,
        fd: RawFd,
        energy_unit_joules: f64,
        last_raw: u32,
        rollover_accumulator: u64,
    },
    Sysfs {
        path: std::path::PathBuf,
        is_rapl: bool,
    },
    Unavailable,
}

impl EnergySource {
    /// Attempt to open the best available energy source.
    ///
    /// Walks the fallback chain: MSR → perf_event → sysfs RAPL → sysfs battery.
    pub fn open() -> io::Result<(Self, EnergyTier)> {
        // Tier 1: Direct MSR access
        match Self::try_open_msr(0) {
            Ok((source, tier)) => return Ok((source, tier)),
            Err(e) => {
                // EPERM = Lockdown mode or missing cap_sys_rawio
                // EACCES = permission denied
                // ENOENT = msr module not loaded
                log::debug!("MSR direct access failed: {e}, trying sysfs fallback");
            }
        }

        // Tier 2: Sysfs RAPL (powercap)
        let sysfs_root = detect_sysfs_root();
        let rapl_path = sysfs_root.join("sys/class/powercap/intel-rapl:0/energy_uj");
        if rapl_path.exists() && std::fs::read_to_string(&rapl_path).is_ok() {
            log::info!("Using RAPL sysfs fallback at {}", rapl_path.display());
            return Ok((
                EnergySource::Sysfs {
                    path: rapl_path,
                    is_rapl: true,
                },
                EnergyTier::SysfsRapl,
            ));
        }

        // Tier 3: Battery sysfs
        let power_supply = sysfs_root.join("sys/class/power_supply");
        if let Ok(entries) = std::fs::read_dir(&power_supply) {
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("BAT") {
                    let path = entry.path().join("energy_now");
                    if path.exists() {
                        log::info!("Using battery sysfs fallback at {}", path.display());
                        return Ok((
                            EnergySource::Sysfs {
                                path,
                                is_rapl: false,
                            },
                            EnergyTier::SysfsBattery,
                        ));
                    }
                }
            }
        }

        Ok((EnergySource::Unavailable, EnergyTier::Unavailable))
    }

    /// Try to open MSR direct access for the given CPU.
    fn try_open_msr(cpu_id: u32) -> io::Result<(Self, EnergyTier)> {
        let msr_path = format!("/dev/cpu/{cpu_id}/msr");
        let file = File::open(&msr_path)?;
        let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);

        // Read MSR_RAPL_POWER_UNIT to calibrate energy unit
        let mut buf = [0u8; 8];
        pread_msr(fd, MSR_RAPL_POWER_UNIT, &mut buf)?;
        let power_unit_reg = u64::from_le_bytes(buf);

        // bits [12:8] = energy status unit exponent (ESU)
        let esu = ((power_unit_reg >> 8) & 0x1F) as u32;
        let energy_unit_joules = 1.0 / (1u64 << esu) as f64;

        // Verify MSR_PKG_ENERGY_STATUS is readable
        pread_msr(fd, MSR_PKG_ENERGY_STATUS, &mut buf)?;
        let initial_raw = u32::from_le_bytes(buf[..4].try_into().unwrap());

        log::info!(
            "MSR RAPL direct: energy_unit={energy_unit_joules:.9} J/tick, initial_raw={initial_raw}"
        );

        Ok((
            EnergySource::Msr {
                file,
                fd,
                energy_unit_joules,
                last_raw: initial_raw,
                rollover_accumulator: 0,
            },
            EnergyTier::MsrDirect,
        ))
    }

    /// Take a raw energy sample. This is the hot path — no string formatting,
    /// no float math (the raw_value stays as raw ticks/μJ).
    #[inline]
    pub fn sample_raw(&mut self) -> io::Result<RawEnergySample> {
        let instant = Instant::now();

        match self {
            EnergySource::Msr {
                fd,
                last_raw,
                rollover_accumulator,
                ..
            } => {
                let mut buf = [0u8; 8];
                pread_msr(*fd, MSR_PKG_ENERGY_STATUS, &mut buf)?;
                let current_raw = u32::from_le_bytes(buf[..4].try_into().unwrap());

                // Handle 32-bit counter wraparound
                if current_raw < *last_raw {
                    *rollover_accumulator += u32::MAX as u64;
                }
                *last_raw = current_raw;

                Ok(RawEnergySample {
                    instant,
                    raw_value: *rollover_accumulator + current_raw as u64,
                    tier: EnergyTier::MsrDirect,
                })
            }
            EnergySource::Sysfs { path, is_rapl } => {
                let text = std::fs::read_to_string(path)?;
                let raw: u64 = text
                    .trim()
                    .parse()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

                Ok(RawEnergySample {
                    instant,
                    raw_value: if *is_rapl { raw } else { raw * 3600 }, // μWh → μJ
                    tier: if *is_rapl {
                        EnergyTier::SysfsRapl
                    } else {
                        EnergyTier::SysfsBattery
                    },
                })
            }
            EnergySource::Unavailable => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no energy counter available",
            )),
        }
    }

    /// Get the energy unit in Joules per tick (only meaningful for MSR tier).
    pub fn energy_unit_joules(&self) -> f64 {
        match self {
            EnergySource::Msr {
                energy_unit_joules, ..
            } => *energy_unit_joules,
            EnergySource::Sysfs { is_rapl, .. } => {
                if *is_rapl {
                    1e-6 // sysfs RAPL is already in microjoules
                } else {
                    0.0036 // battery μWh to joules
                }
            }
            EnergySource::Unavailable => 0.0,
        }
    }

    /// Compute energy delta between two samples in Joules.
    pub fn delta_joules(&self, start: &RawEnergySample, end: &RawEnergySample) -> f64 {
        match self {
            EnergySource::Msr {
                energy_unit_joules,
                ..
            } => {
                (end.raw_value.wrapping_sub(start.raw_value)) as f64 * energy_unit_joules
            }
            EnergySource::Sysfs { is_rapl, .. } => {
                if *is_rapl {
                    (end.raw_value - start.raw_value) as f64 * 1e-6
                } else {
                    // Battery counts down
                    (start.raw_value - end.raw_value) as f64 * 1e-6
                }
            }
            EnergySource::Unavailable => 0.0,
        }
    }
}

/// Read a 64-bit MSR value via pread on /dev/cpu/N/msr.
fn pread_msr(fd: RawFd, msr: u32, buf: &mut [u8; 8]) -> io::Result<()> {
    let n = unsafe {
        libc::pread(
            fd,
            buf.as_mut_ptr() as *mut libc::c_void,
            8,
            msr as i64,
        )
    };
    if n != 8 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Detect the sysfs root (handles chroot/container environments).
fn detect_sysfs_root() -> std::path::PathBuf {
    // Check RUSHBENCH_SYSFS_ROOT env for test/mock support
    if let Ok(root) = std::env::var("RUSHBENCH_SYSFS_ROOT") {
        return std::path::PathBuf::from(root);
    }
    std::path::PathBuf::from("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_energy_tier_ordering() {
        assert!(EnergyTier::MsrDirect < EnergyTier::SysfsRapl);
        assert!(EnergyTier::SysfsRapl < EnergyTier::SysfsBattery);
        assert!(EnergyTier::SysfsBattery < EnergyTier::Unavailable);
    }

    #[test]
    fn test_energy_unit_calculation() {
        // Default ESU = 0x10 = 16 → 2^-16 ≈ 1.526e-5
        let esu = 16u32;
        let unit = 1.0 / (1u64 << esu) as f64;
        assert!((unit - 1.52587890625e-5).abs() < 1e-15);
    }
}
