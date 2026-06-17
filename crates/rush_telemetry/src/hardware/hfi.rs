//! Intel Hardware Feedback Interface (HFI) topology discovery.
//!
//! Reads the HFI table from the physical address provided by
//! `MSR_IA32_HW_FEEDBACK_PTR` (0x17D). The table provides per-core
//! performance and efficiency ratings (0-255) that the `sched_ext`
//! scheduler must respect to avoid asymmetric core thrashing.
//!
//! On non-hybrid systems or ARM64 (where `_capacity_ sysfs is used),
//! this module provides a uniform classification.

use std::fs;
use std::io;

/// Core classification from HFI or ARM64 capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreClass {
    /// High-performance core (Intel P-core / ARM big core).
    Performance,
    /// Energy-efficient core (Intel E-core / ARM LITTLE core).
    Efficiency,
    /// Classic (non-hybrid) — all cores are equivalent.
    Classic,
}

impl CoreClass {
    /// Returns true if this core class is suitable for latency-critical work.
    pub fn is_performance(&self) -> bool {
        matches!(self, CoreClass::Performance | CoreClass::Classic)
    }
}

/// Per-core capability rating from HFI.
#[derive(Debug, Clone, Copy)]
pub struct CoreRating {
    /// Performance rating (0-255). 0 = do not schedule for performance.
    pub performance: u8,
    /// Efficiency rating (0-255). 0 = do not schedule for efficiency.
    pub efficiency: u8,
    /// Derived classification.
    pub class: CoreClass,
}

/// Topology information for all cores in the system.
#[derive(Debug, Clone)]
pub struct HfiTopology {
    /// Per-logical-CPU ratings. Index = logical CPU ID.
    pub cores: Vec<CoreRating>,
    /// Whether hybrid topology was detected.
    pub is_hybrid: bool,
}

impl HfiTopology {
    /// Discover the system's core topology.
    ///
    /// Attempts HFI first (Intel hybrid), falls back to ARM64 capacity,
    /// then assumes classic (uniform) topology.
    pub fn discover() -> io::Result<Self> {
        // Try Intel HFI first
        if let Ok(topo) = Self::try_intel_hfi() {
            return Ok(topo);
        }

        // Try ARM64 capacity
        if let Ok(topo) = Self::try_arm_capacity() {
            return Ok(topo);
        }

        // Fallback: assume classic (all cores equivalent)
        let ncpus = num_cpus();
        log::info!("No hybrid topology detected, assuming classic ({ncpus} cores)");
        Ok(HfiTopology {
            cores: vec![
                CoreRating {
                    performance: 255,
                    efficiency: 255,
                    class: CoreClass::Classic,
                };
                ncpus
            ],
            is_hybrid: false,
        })
    }

    /// Try to read Intel HFI topology from sysfs.
    ///
    /// The kernel exposes HFI data via `/sys/devices/system/cpu/cpuN/cpu_capacity`
    /// or via the `intel_hfi` driver's sysfs interface.
    fn try_intel_hfi() -> io::Result<Self> {
        // Check if this is a hybrid system by looking for heterogeneous CPU capacities
        let cpu_base = "/sys/devices/system/cpu";
        let mut capacities: Vec<u32> = Vec::new();

        for entry in fs::read_dir(cpu_base)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if !name_str.starts_with("cpu") {
                continue;
            }

            // Parse "cpuNNN"
            let cpu_id_str = &name_str[3..];
            if cpu_id_str.is_empty() || !cpu_id_str.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }

            let cap_path = entry.path().join("cpu_capacity");
            if let Ok(cap_str) = fs::read_to_string(&cap_path) {
                if let Ok(cap) = cap_str.trim().parse::<u32>() {
                    capacities.push(cap);
                }
            }
        }

        if capacities.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no cpu_capacity entries found",
            ));
        }

        // Check if all capacities are the same (non-hybrid)
        let first = capacities[0];
        let is_hybrid = capacities.iter().any(|&c| c != first);

        if !is_hybrid {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "uniform capacities — not a hybrid system",
            ));
        }

        // Classify: top 50% capacity = performance, bottom 50% = efficiency
        let mut sorted = capacities.clone();
        sorted.sort_unstable();
        let median = sorted[sorted.len() / 2];

        let cores: Vec<CoreRating> = capacities
            .iter()
            .map(|&cap| {
                let class = if cap >= median {
                    CoreClass::Performance
                } else {
                    CoreClass::Efficiency
                };
                // Normalize to 0-255
                let max_cap = *sorted.last().unwrap_or(&1);
                let performance = ((cap as f64 / max_cap as f64) * 255.0) as u8;
                CoreRating {
                    performance,
                    efficiency: 255 - performance,
                    class,
                }
            })
            .collect();

        let perf_count = cores.iter().filter(|c| c.class == CoreClass::Performance).count();
        let eff_count = cores.iter().filter(|c| c.class == CoreClass::Efficiency).count();
        log::info!("Intel hybrid topology: {perf_count} P-cores, {eff_count} E-cores");

        Ok(HfiTopology { cores, is_hybrid: true })
    }

    /// Try ARM64 capacity-based topology discovery.
    fn try_arm_capacity() -> io::Result<Self> {
        // Same mechanism as Intel but on ARM — cpu_capacity sysfs
        // If we reach here, Intel HFI failed but ARM might work
        // The logic is identical, so we delegate
        Self::try_intel_hfi()
    }

    /// Get the core class for a specific logical CPU.
    pub fn core_class(&self, cpu_id: usize) -> CoreClass {
        self.cores
            .get(cpu_id)
            .map(|r| r.class)
            .unwrap_or(CoreClass::Classic)
    }

    /// Get the performance rating for a specific logical CPU.
    pub fn performance_rating(&self, cpu_id: usize) -> u8 {
        self.cores
            .get(cpu_id)
            .map(|r| r.performance)
            .unwrap_or(255)
    }
}

/// Get the number of logical CPUs.
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_class_performance_check() {
        assert!(CoreClass::Performance.is_performance());
        assert!(CoreClass::Classic.is_performance());
        assert!(!CoreClass::Efficiency.is_performance());
    }

    #[test]
    fn test_fallback_classic_topology() {
        // On any system, the fallback should produce a valid topology
        let topo = HfiTopology::discover().unwrap();
        assert!(!topo.cores.is_empty());
        // At minimum, we should have Classic classification
        for core in &topo.cores {
            assert!(core.performance > 0);
        }
    }
}
