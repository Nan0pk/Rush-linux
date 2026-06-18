use serde::Serialize;

/// Schema version — bump when fields are removed or semantics change.
/// Additive fields do not require a bump.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Debug)]
pub struct CollectionRecord {
    pub schema_version: u32,
    pub collected_at: String,
    /// "linux" | "windows" — from std::env::consts::OS
    pub os: &'static str,
    pub window_sec: u64,
    pub hardware: HardwareProfile,
    pub energy: EnergyWindow,
    /// None when /proc/pressure is unavailable (older kernels, non-Linux)
    pub psi: Option<PsiWindow>,
    pub thermal: ThermalSnapshot,
    pub freq: FreqSnapshot,
    pub memory: MemorySnapshot,
    pub load: LoadSnapshot,
}

#[derive(Serialize, Debug)]
pub struct HardwareProfile {
    pub cpu_model: String,
    pub cpu_cores: usize,
    /// "GenuineIntel" | "AuthenticAMD" | "unknown"
    pub cpu_vendor: String,
    pub ram_total_kb: u64,
    /// "Notebook" | "Desktop" | "Server" | "unknown" — from DMI chassis type
    pub chassis: String,
    /// µWh design capacity; 0 if no battery
    pub battery_design_uwh: u64,
    pub kernel: String,
}

#[derive(Serialize, Debug)]
pub struct EnergyWindow {
    pub ac_online: Option<bool>,
    /// "rapl_sysfs" | "battery_sysfs" | "none"
    pub counter_used: String,
    /// Raw delta in µJ from intel-rapl:0/energy_uj
    pub rapl_delta_uj: Option<u64>,
    /// Raw delta in µWh from BAT*/energy_now (discharging = positive)
    pub battery_delta_uwh: Option<u64>,
    pub avg_watts_rapl: Option<f64>,
    pub avg_watts_battery: Option<f64>,
}

#[derive(Serialize, Debug)]
pub struct PsiWindow {
    /// CPU "some" stall percentage over the window
    pub cpu_stall_pct: f64,
    /// I/O "some" stall percentage over the window
    pub io_stall_pct: f64,
    /// Actual elapsed window in microseconds
    pub elapsed_us: u64,
}

#[derive(Serialize, Debug)]
pub struct ThermalSnapshot {
    pub max_celsius: Option<f64>,
    pub zones_read: usize,
}

#[derive(Serialize, Debug)]
pub struct FreqSnapshot {
    /// cpufreq governor name for cpu0, e.g. "powersave"
    pub governor: Option<String>,
    /// Configured max frequency in MHz
    pub max_mhz: Option<u64>,
    /// Average of scaling_cur_freq across online CPUs, in MHz
    pub current_mhz_avg: Option<u64>,
}

#[derive(Serialize, Debug)]
pub struct MemorySnapshot {
    pub total_kb: u64,
    pub available_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
}

#[derive(Serialize, Debug)]
pub struct LoadSnapshot {
    pub avg1: f64,
    pub avg5: f64,
    pub avg15: f64,
}
