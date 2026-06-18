/// Windows platform backend — stubs for now.
///
/// To implement: replace each stub with the appropriate Win32 or PDH call.
/// Required crate additions: `windows = { version = "0.58", features = [...] }`
///
/// CPU model:    HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0 -> ProcessorNameString
/// CPU cores:    GetSystemInfo() -> dwNumberOfProcessors
/// RAM total:    GetPhysicallyInstalledSystemMemory()
/// Chassis:      WMI Win32_SystemEnclosure.ChassisTypes
/// Battery:      WMI Win32_Battery.DesignCapacity (mWh) or PDH \\Battery Status\*
/// AC online:    WMI Win32_Battery.BatteryStatus == 2 (AC)
/// RAPL:         Windows Energy Estimation Engine (E3) via PDH
/// Thermal:      WMI MSAcpi_ThermalZoneTemperature
/// CPU freq:     WMI Win32_Processor.CurrentClockSpeed / MaxClockSpeed
/// CPU governor: Windows Power Plan GUID (powercfg /getactivescheme)
/// Memory:       GlobalMemoryStatusEx()
/// Load:         PDH Processor Information\% Processor Time
use crate::types::*;

pub fn read_cpu_model() -> String {
    eprintln!("rush-collect: Windows CPU model read not yet implemented");
    "windows-stub".to_string()
}
pub fn read_cpu_vendor() -> String {
    "unknown".to_string()
}
pub fn read_cpu_cores() -> usize {
    0
}
pub fn read_ram_total_kb() -> u64 {
    0
}
pub fn read_chassis() -> String {
    "unknown".to_string()
}
pub fn read_battery_design_uwh() -> u64 {
    0
}
pub fn read_kernel() -> String {
    // std::env::var("OS").unwrap_or_else(|_| "Windows".to_string())
    "Windows".to_string()
}

pub fn read_rapl_uj() -> Option<u64> {
    None
}
pub fn read_battery_uwh() -> Option<u64> {
    None
}
pub fn read_ac_online() -> Option<bool> {
    None
}
pub fn read_psi_total_us(_resource: &str) -> Option<u64> {
    None
}

pub fn read_thermal() -> ThermalSnapshot {
    ThermalSnapshot {
        max_celsius: None,
        zones_read: 0,
    }
}
pub fn read_freq() -> FreqSnapshot {
    FreqSnapshot {
        governor: None,
        max_mhz: None,
        current_mhz_avg: None,
    }
}
pub fn read_memory() -> MemorySnapshot {
    MemorySnapshot {
        total_kb: 0,
        available_kb: 0,
        swap_total_kb: 0,
        swap_free_kb: 0,
    }
}
pub fn read_load() -> LoadSnapshot {
    LoadSnapshot {
        avg1: 0.0,
        avg5: 0.0,
        avg15: 0.0,
    }
}
