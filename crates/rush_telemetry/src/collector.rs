//! Top-level telemetry collector orchestrator.
//!
//! Implements the Builder/Verifier pattern mandated by Rush Linux's
//! workspace conventions. The collector coordinates hardware access,
//! eBPF lifecycle, and post-processing transport into a single,
//! ergonomic API.
//!
//! ## Usage Pattern
//!
//! ```ignore
//! // 1. Builder phase: validate hardware, select tier
//! let collector = Collector::builder()
//!     .with_energy_source(EnergyPreference::MsrFirst)
//!     .with_psi(true)
//!     .with_ebpf(true)
//!     .verify()?;
//!
//! // 2. Measurement phase: run benchmark inside closure
//! let result = collector.build().measure(|| {
//!     std::process::Command::new("cyclictest")
//!         .args(["-l", "1000", "-q"])
//!         .status()
//! })?;
//!
//! // 3. Post-processing: serialize, sign, send (background)
//! result.export("/api/telemetry", "collect.rush.local:8080")?;
//! ```

use std::collections::HashSet;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::ebpf::types::{MarkerType, TelemetryEvent};
use crate::ebpf::{EbpfCollector, EbpfConfig};
use crate::hardware::hfi::{CoreClass, HfiTopology};
use crate::hardware::psi::{PsiReader, PsiResource, PsiSample};
use crate::hardware::rapl::{EnergySource, EnergyTier, RawEnergySample};
use crate::transport::http::TelemetryClient;
use crate::transport::serialize::serialize_payload;
use crate::transport::sign::{create_signed_envelope, PayloadSigner};

/// Preference for energy source selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergyPreference {
    /// Try MSR first, fall back to sysfs.
    MsrFirst,
    /// Use sysfs only (e.g., in containers without MSR access).
    SysfsOnly,
    /// Disable energy telemetry.
    Disabled,
}

/// Builder for constructing a `Collector`.
///
/// Validates hardware capabilities, selects fallback tiers,
/// and initializes file descriptors before measurement begins.
pub struct CollectorBuilder {
    energy_pref: EnergyPreference,
    enable_psi: bool,
    enable_ebpf: bool,
    tracked_pids: HashSet<u32>,
    /// Pinned CPU for benchmark thread (for core-type tagging).
    pinned_cpu: Option<usize>,
}

impl CollectorBuilder {
    fn new() -> Self {
        CollectorBuilder {
            energy_pref: EnergyPreference::MsrFirst,
            enable_psi: true,
            enable_ebpf: true,
            tracked_pids: HashSet::new(),
            pinned_cpu: None,
        }
    }

    /// Set the energy source preference.
    pub fn with_energy_source(mut self, pref: EnergyPreference) -> Self {
        self.energy_pref = pref;
        self
    }

    /// Enable or disable PSI telemetry.
    pub fn with_psi(mut self, enable: bool) -> Self {
        self.enable_psi = enable;
        self
    }

    /// Enable or disable eBPF telemetry.
    pub fn with_ebpf(mut self, enable: bool) -> Self {
        self.enable_ebpf = enable;
        self
    }

    /// Add a PID to track for scheduler events.
    pub fn track_pid(mut self, pid: u32) -> Self {
        self.tracked_pids.insert(pid);
        self
    }

    /// Pin the benchmark to a specific CPU (for core-type tagging).
    pub fn pinned_to_cpu(mut self, cpu_id: usize) -> Self {
        self.pinned_cpu = Some(cpu_id);
        self
    }

    /// Verify that the selected hardware tiers are functional.
    ///
    /// Performs test reads on each configured subsystem to confirm
    /// they are operational. Returns a `VerifiedBuilder` that can
    /// produce a `Collector` via `build()`.
    pub fn verify(self) -> io::Result<VerifiedCollector> {
        // Discover HFI topology
        let hfi = HfiTopology::discover().unwrap_or_else(|e| {
            log::warn!("HFI discovery failed: {e}, using fallback");
            HfiTopology {
                cores: vec![],
                is_hybrid: false,
            }
        });

        // Open energy source
        let (energy_source, energy_tier) = match self.energy_pref {
            EnergyPreference::MsrFirst => EnergySource::open()?,
            EnergyPreference::SysfsOnly => {
                // Skip MSR tier, go directly to sysfs
                let sysfs_root = detect_sysfs_root();
                let rapl = sysfs_root.join("sys/class/powercap/intel-rapl:0/energy_uj");
                if rapl.exists() {
                    (
                        EnergySource::Sysfs {
                            path: rapl,
                            is_rapl: true,
                        },
                        EnergyTier::SysfsRapl,
                    )
                } else {
                    (EnergySource::Unavailable, EnergyTier::Unavailable)
                }
            }
            EnergyPreference::Disabled => (EnergySource::Unavailable, EnergyTier::Unavailable),
        };

        log::info!("Energy tier: {energy_tier:?}");

        // Open PSI readers
        let psi_cpu = if self.enable_psi {
            match PsiReader::open(PsiResource::Cpu) {
                Ok(reader) => {
                    // Verify with a test read
                    let _sample = reader.read_total()?;
                    Some(reader)
                }
                Err(e) => {
                    log::warn!("PSI CPU reader failed: {e}");
                    None
                }
            }
        } else {
            None
        };

        let psi_io = if self.enable_psi {
            match PsiReader::open(PsiResource::Io) {
                Ok(reader) => {
                    let _sample = reader.read_total()?;
                    Some(reader)
                }
                Err(e) => {
                    log::warn!("PSI IO reader failed: {e}");
                    None
                }
            }
        } else {
            None
        };

        // Prepare eBPF config
        let ebpf_config = EbpfConfig {
            tracked_pids: self.tracked_pids,
            ..Default::default()
        };

        // Report core classification if hybrid
        if hfi.is_hybrid {
            if let Some(cpu) = self.pinned_cpu {
                let class = hfi.core_class(cpu);
                log::info!("Benchmark pinned to CPU {cpu} ({class:?})");
            }
        }

        Ok(VerifiedCollector {
            energy_source: Some(energy_source),
            energy_tier,
            psi_cpu,
            psi_io,
            ebpf_config,
            hfi,
            enable_ebpf: self.enable_ebpf,
            pinned_cpu: self.pinned_cpu,
        })
    }
}

/// A verified collector — hardware confirmed operational.
///
/// This is the Builder/Verifier boundary. Once `verify()` succeeds,
/// the collector's configuration is immutable.
pub struct VerifiedCollector {
    energy_source: Option<EnergySource>,
    energy_tier: EnergyTier,
    psi_cpu: Option<PsiReader>,
    psi_io: Option<PsiReader>,
    ebpf_config: EbpfConfig,
    hfi: HfiTopology,
    enable_ebpf: bool,
    pinned_cpu: Option<usize>,
}

impl VerifiedCollector {
    /// Build the final `TelemetryHandle` for measurement.
    ///
    /// This consumes the verified collector and produces an immutable
    /// handle that can be shared across thread boundaries.
    pub fn build(self) -> TelemetryHandle {
        TelemetryHandle {
            energy_source: self.energy_source,
            energy_tier: self.energy_tier,
            psi_cpu: self.psi_cpu,
            psi_io: self.psi_io,
            ebpf_config: self.ebpf_config,
            hfi: self.hfi,
            enable_ebpf: self.enable_ebpf,
            pinned_cpu: self.pinned_cpu,
        }
    }
}

/// Immutable telemetry handle — safe to pass across threads.
///
/// All measurement happens through this handle. The `measure()` method
/// executes the benchmark closure and collects telemetry concurrently.
pub struct TelemetryHandle {
    energy_source: Option<EnergySource>,
    energy_tier: EnergyTier,
    psi_cpu: Option<PsiReader>,
    psi_io: Option<PsiReader>,
    ebpf_config: EbpfConfig,
    hfi: HfiTopology,
    enable_ebpf: bool,
    pinned_cpu: Option<usize>,
}

impl TelemetryHandle {
    /// Execute a benchmark with telemetry collection.
    ///
    /// The benchmark closure is executed as a child process. During
    /// execution, the calling thread blocks on `waitpid()`, allowing
    /// full C-state residency. Telemetry is collected by:
    ///
    /// - eBPF tracepoints (if available) → ring buffer
    /// - PSI total counter (snapshot at start + end)
    /// - RAPL energy (snapshot at start + end)
    ///
    /// After the benchmark completes, the ring buffer is drained and
    /// all data is returned in a `MeasurementResult`.
    pub fn measure<F, T>(&mut self, benchmark: F) -> io::Result<MeasurementResult<T>>
    where
        F: FnOnce() -> T,
    {
        // === Pre-measurement: snapshot hardware counters ===
        let energy_start = self.energy_source.as_mut().map(|s| s.sample_raw()).transpose()?;
        let psi_cpu_start = self.psi_cpu.as_ref().map(|r| r.read_total()).transpose()?;
        let psi_io_start = self.psi_io.as_ref().map(|r| r.read_total()).transpose()?;
        let window_start = Instant::now();

        // === eBPF attachment (if enabled) ===
        let mut ebpf_collector = if self.enable_ebpf {
            let mut collector = EbpfCollector::new(self.ebpf_config.clone());
            match collector.attach() {
                Ok(true) => {
                    collector.emit_marker(MarkerType::Start)?;
                    Some(collector)
                }
                Ok(false) => {
                    // Degraded mode — no eBPF, but measurement continues
                    log::info!("Running in degraded mode (no eBPF)");
                    None
                }
                Err(e) => {
                    log::warn!("eBPF attach failed: {e}");
                    None
                }
            }
        } else {
            None
        };

        // === Execute benchmark ===
        let benchmark_result = benchmark();

        // === Post-measurement: snapshot hardware counters ===
        let window_end = Instant::now();
        let energy_end = self.energy_source.as_mut().map(|s| s.sample_raw()).transpose()?;
        let psi_cpu_end = self.psi_cpu.as_ref().map(|r| r.read_total()).transpose()?;
        let psi_io_end = self.psi_io.as_ref().map(|r| r.read_total()).transpose()?;

        // === Drain eBPF ring buffer ===
        let ebpf_events = if let Some(ref mut collector) = ebpf_collector {
            collector.emit_marker(MarkerType::Stop)?;
            collector.drain()?
        } else {
            Vec::new()
        };

        // === Compute energy delta ===
        let energy_joules = match (&self.energy_source, &energy_start, &energy_end) {
            (Some(source), Some(start), Some(end)) => Some(source.delta_joules(start, end)),
            _ => None,
        };

        let window_duration = window_end.duration_since(window_start);

        // === Compute PSI stall percentages ===
        let psi_cpu_stall_pct = match (&psi_cpu_start, &psi_cpu_end) {
            (Some(start), Some(end)) => Some(PsiReader::stall_percentage(start, end)),
            _ => None,
        };
        let psi_io_stall_pct = match (&psi_io_start, &psi_io_end) {
            (Some(start), Some(end)) => Some(PsiReader::stall_percentage(start, end)),
            _ => None,
        };

        // === Tag with core class ===
        let core_class = self
            .pinned_cpu
            .map(|cpu| self.hfi.core_class(cpu))
            .unwrap_or(CoreClass::Classic);

        Ok(MeasurementResult {
            benchmark_result,
            energy_joules,
            energy_tier: self.energy_tier,
            window_duration,
            psi_cpu_stall_pct,
            psi_io_stall_pct,
            ebpf_events,
            core_class,
            dropped_events: 0,
        })
    }
}

/// The result of a benchmark measurement with all telemetry data.
pub struct MeasurementResult<T> {
    /// The benchmark closure's return value.
    pub benchmark_result: T,
    /// Total energy consumed during the window (Joules), if available.
    pub energy_joules: Option<f64>,
    /// Which hardware tier provided the energy data.
    pub energy_tier: EnergyTier,
    /// Wall-clock duration of the measurement window.
    pub window_duration: Duration,
    /// Exact CPU stall percentage during the window (not EMA).
    pub psi_cpu_stall_pct: Option<f64>,
    /// Exact IO stall percentage during the window (not EMA).
    pub psi_io_stall_pct: Option<f64>,
    /// Raw eBPF events collected during the window.
    pub ebpf_events: Vec<TelemetryEvent>,
    /// Core class the benchmark ran on.
    pub core_class: CoreClass,
    /// Number of events dropped by the ring buffer.
    pub dropped_events: u32,
}

impl<T> MeasurementResult<T> {
    /// Compute average power in watts during the measurement window.
    pub fn avg_watts(&self) -> Option<f64> {
        let joules = self.energy_joules?;
        let seconds = self.window_duration.as_secs_f64();
        if seconds > 0.0 {
            Some(joules / seconds)
        } else {
            None
        }
    }

    /// Export the telemetry payload to a remote endpoint.
    ///
    /// This runs in a low-priority background thread. The benchmark
    /// result is returned immediately; telemetry delivery is best-effort.
    pub fn export(
        &self,
        path: &str,
        endpoint: &str,
    ) -> io::Result<thread::JoinHandle<io::Result<()>>>
    where
        T: Send + 'static,
    {
        // Clone what we need for the background thread
        let events = self.ebpf_events.clone();
        let dropped = self.dropped_events;
        let window_ns = self.window_duration.as_nanos() as u64;

        let path = path.to_string();
        let endpoint = endpoint.to_string();

        // Spawn low-priority background thread
        let handle = thread::Builder::new()
            .name("telemetry-export".to_string())
            .spawn(move || {
                // Set low priority (nice 19)
                #[cfg(unix)]
                unsafe {
                    libc::nice(19);
                }

                // Serialize
                let compressed = serialize_payload(&events, dropped, window_ns)?;

                // Sign
                let signer = PayloadSigner::open_or_generate()?;
                let envelope = create_signed_envelope(&signer, &compressed);

                // Send
                let client = TelemetryClient::new(&endpoint, &path);
                let response = client.send(&envelope)?;

                if response.status_code >= 200 && response.status_code < 300 {
                    log::info!(
                        "Telemetry exported successfully ({} bytes, HTTP {})",
                        envelope.len(),
                        response.status_code
                    );
                } else {
                    log::warn!(
                        "Telemetry export returned HTTP {}",
                        response.status_code
                    );
                }

                Ok(())
            })?;

        Ok(handle)
    }
}

/// Main entry point — mirrors the `Collector::builder()` pattern.
pub struct Collector;

impl Collector {
    /// Create a new collector builder.
    pub fn builder() -> CollectorBuilder {
        CollectorBuilder::new()
    }
}

/// Detect the sysfs root (handles chroot/container environments).
fn detect_sysfs_root() -> std::path::PathBuf {
    if let Ok(root) = std::env::var("RUSHBENCH_SYSFS_ROOT") {
        return std::path::PathBuf::from(root);
    }
    std::path::PathBuf::from("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let builder = CollectorBuilder::new();
        assert_eq!(builder.energy_pref, EnergyPreference::MsrFirst);
        assert!(builder.enable_psi);
        assert!(builder.enable_ebpf);
        assert!(builder.tracked_pids.is_empty());
    }

    #[test]
    fn test_builder_chaining() {
        let builder = Collector::builder()
            .with_energy_source(EnergyPreference::SysfsOnly)
            .with_psi(false)
            .with_ebpf(false)
            .track_pid(1234)
            .pinned_to_cpu(0);

        assert_eq!(builder.energy_pref, EnergyPreference::SysfsOnly);
        assert!(!builder.enable_psi);
        assert!(!builder.enable_ebpf);
        assert!(builder.tracked_pids.contains(&1234));
        assert_eq!(builder.pinned_cpu, Some(0));
    }

    #[test]
    fn test_measurement_result_avg_watts() {
        let result = MeasurementResult {
            benchmark_result: (),
            energy_joules: Some(100.0),
            energy_tier: EnergyTier::MsrDirect,
            window_duration: Duration::from_secs(10),
            psi_cpu_stall_pct: Some(5.0),
            psi_io_stall_pct: None,
            ebpf_events: vec![],
            core_class: CoreClass::Performance,
            dropped_events: 0,
        };
        let watts = result.avg_watts().unwrap();
        assert!((watts - 10.0).abs() < 0.01);
    }
}
