use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zbus::blocking::ConnectionBuilder;
use zbus::dbus_interface;

const DEFAULT_STATE_DIR: &str = "/run/optid";
const DEFAULT_CONFIG_PATH: &str = "/usr/lib/optid/policy.toml";
const DEFAULT_INTERVAL_SEC: u64 = 2;

struct OptidServer {
    state_dir: PathBuf,
}

#[dbus_interface(name = "io.rushlinux.Optid1")]
impl OptidServer {
    fn status(&self) -> zbus::fdo::Result<String> {
        fs::read_to_string(self.state_dir.join("status"))
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to read status: {e}")))
    }

    fn explain(&self) -> zbus::fdo::Result<String> {
        fs::read_to_string(self.state_dir.join("decisions.log"))
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to read decisions.log: {e}")))
    }

    fn set_mode(&self, mode: &str) -> zbus::fdo::Result<()> {
        let mode_parsed = Mode::parse(mode)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("invalid mode: {mode}")))?;
        fs::write(self.state_dir.join("mode"), mode_parsed.to_string())
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to write mode: {e}")))
    }

    fn pin_application(&self, app_id: &str, mode: &str) -> zbus::fdo::Result<()> {
        let _mode_parsed = Mode::parse(mode)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("invalid mode: {mode}")))?;
        println!("Pinning application {app_id} to mode {mode}");
        Ok(())
    }

    #[dbus_interface(property)]
    fn mode(&self) -> String {
        let text = fs::read_to_string(self.state_dir.join("mode")).unwrap_or_default();
        Mode::parse(&text).unwrap_or(Mode::Auto).to_string()
    }

    #[dbus_interface(property)]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

fn main() {
    let args = match Args::parse(env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("optid: {err}");
            print_usage();
            std::process::exit(2);
        }
    };

    if args.help {
        print_usage();
        return;
    }

    if let Err(err) = run(args) {
        eprintln!("optid: {err}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> io::Result<()> {
    fs::create_dir_all(&args.state_dir)?;

    // Revert sysctls on startup to clean up any left-over state
    revert_sysctls(&args.state_dir);

    let state_dir_clone = args.state_dir.clone();
    thread::spawn(move || {
        let server = OptidServer {
            state_dir: state_dir_clone,
        };
        let run_server = || -> zbus::Result<()> {
            let _conn = ConnectionBuilder::system()?
                .name("io.rushlinux.Optid")?
                .serve_at("/io/rushlinux/Optid", server)?
                .build()?;
            println!("D-Bus server running on system bus at /io/rushlinux/Optid");
            loop {
                thread::park();
            }
        };
        if let Err(e) = run_server() {
            eprintln!("D-Bus server error: {e}. Running without D-Bus.");
        }
    });

    loop {
        let override_mode = read_mode_override(&args.state_dir).unwrap_or(Mode::Auto);
        let snapshot = Snapshot::collect();
        let decision = Policy::load(&args.config_path).decide(&snapshot, override_mode);
        let report = decision.render(&snapshot);

        fs::write(args.state_dir.join("status"), &report)?;
        append_log(&args.state_dir.join("decisions.log"), &report)?;

        if args.apply {
            let mut actuator = Actuator::new(args.state_dir.clone());
            for action in &decision.actions {
                actuator.apply(action)?;
            }
        }

        if args.once {
            break;
        }

        thread::sleep(Duration::from_secs(args.interval_sec));
    }

    // Also revert sysctls on clean exit
    revert_sysctls(&args.state_dir);

    Ok(())
}

#[derive(Debug, Clone)]
struct Args {
    apply: bool,
    once: bool,
    help: bool,
    interval_sec: u64,
    state_dir: PathBuf,
    config_path: PathBuf,
}

impl Args {
    fn parse<I>(iter: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut args = Self {
            apply: false,
            once: false,
            help: false,
            interval_sec: DEFAULT_INTERVAL_SEC,
            state_dir: PathBuf::from(DEFAULT_STATE_DIR),
            config_path: PathBuf::from(DEFAULT_CONFIG_PATH),
        };

        let mut it = iter.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--apply" => args.apply = true,
                "--once" => args.once = true,
                "-h" | "--help" => args.help = true,
                "--interval-sec" => {
                    let value = it
                        .next()
                        .ok_or_else(|| "--interval-sec requires a value".to_string())?;
                    args.interval_sec = value
                        .parse::<u64>()
                        .map_err(|_| "--interval-sec must be an integer".to_string())?;
                }
                "--state-dir" => {
                    let value = it
                        .next()
                        .ok_or_else(|| "--state-dir requires a value".to_string())?;
                    args.state_dir = PathBuf::from(value);
                }
                "--config" => {
                    let value = it
                        .next()
                        .ok_or_else(|| "--config requires a value".to_string())?;
                    args.config_path = PathBuf::from(value);
                }
                unknown => return Err(format!("unknown argument: {unknown}")),
            }
        }

        Ok(args)
    }
}

fn print_usage() {
    println!(
        "Usage: optid [--apply] [--once] [--interval-sec N] [--state-dir PATH] [--config PATH]\n\
         \n\
         Default mode is dry-run. Use --apply only on Rush Linux or a test host."
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Auto,
    Battery,
    Balanced,
    Performance,
    Realtime,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "battery" => Some(Self::Battery),
            "balanced" => Some(Self::Balanced),
            "performance" => Some(Self::Performance),
            "realtime" => Some(Self::Realtime),
            _ => None,
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Auto => "auto",
            Self::Battery => "battery",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
            Self::Realtime => "realtime",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Pressure {
    avg10: f32,
    avg60: f32,
    avg300: f32,
    total: u64,
}

impl Pressure {
    fn read(path: &str) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        parse_pressure(&text)
    }
}

#[derive(Debug, Clone)]
struct Snapshot {
    timestamp: u64,
    on_ac: Option<bool>,
    battery_pct: Option<u8>,
    max_temp_millic: Option<i64>,
    loadavg_1: Option<f32>,
    cpu_pressure: Option<Pressure>,
    memory_pressure: Option<Pressure>,
    io_pressure: Option<Pressure>,
    zram_swap_active: bool,
}

impl Snapshot {
    fn collect() -> Self {
        Self {
            timestamp: now_unix(),
            on_ac: read_on_ac(),
            battery_pct: read_battery_pct(),
            max_temp_millic: read_max_thermal_millic(),
            loadavg_1: read_loadavg_1(),
            cpu_pressure: Pressure::read("/proc/pressure/cpu"),
            memory_pressure: Pressure::read("/proc/pressure/memory"),
            io_pressure: Pressure::read("/proc/pressure/io"),
            zram_swap_active: read_zram_swap_active(),
        }
    }

    fn thermal_c(&self) -> Option<f32> {
        self.max_temp_millic.map(|temp| temp as f32 / 1000.0)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct MemoryConfig {
    high_swappiness_requires_zram: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct Policy {
    thresholds: Thresholds,
    modes: Modes,
    memory: MemoryConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct Thresholds {
    cpu_pressure_perf_avg10: f32,
    memory_pressure_protect_avg10: f32,
    io_pressure_throttle_avg10: f32,
    hot_temp_c: f32,
    critical_temp_c: f32,
    low_battery_pct: u8,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct Modes {
    battery: ModeConfig,
    balanced: ModeConfig,
    performance: ModeConfig,
    realtime: ModeConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ModeConfig {
    cpu_epp: String,
    platform_profile: String,
    #[serde(default)]
    background_cpu_weight: Option<u32>,
    #[serde(default)]
    background_io_weight: Option<u32>,
    #[serde(default)]
    user_cpu_weight: Option<u32>,
    #[serde(default)]
    user_io_weight: Option<u32>,
    #[serde(default)]
    requires_controlled_rt_access: Option<bool>,
    #[serde(default)]
    vm_swappiness: Option<u32>,
    #[serde(default)]
    vm_dirty_background_bytes: Option<u64>,
    #[serde(default)]
    vm_dirty_bytes: Option<u64>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            thresholds: Thresholds {
                cpu_pressure_perf_avg10: 12.0,
                memory_pressure_protect_avg10: 5.0,
                io_pressure_throttle_avg10: 8.0,
                hot_temp_c: 82.0,
                critical_temp_c: 92.0,
                low_battery_pct: 20,
            },
            modes: Modes {
                battery: ModeConfig {
                    cpu_epp: "power".to_string(),
                    platform_profile: "low-power".to_string(),
                    background_cpu_weight: Some(25),
                    background_io_weight: Some(25),
                    user_cpu_weight: None,
                    user_io_weight: None,
                    requires_controlled_rt_access: None,
                    vm_swappiness: Some(60),
                    vm_dirty_background_bytes: Some(67108864),
                    vm_dirty_bytes: Some(134217728),
                },
                balanced: ModeConfig {
                    cpu_epp: "balance_performance".to_string(),
                    platform_profile: "balanced".to_string(),
                    background_cpu_weight: None,
                    background_io_weight: None,
                    user_cpu_weight: Some(150),
                    user_io_weight: Some(150),
                    requires_controlled_rt_access: None,
                    vm_swappiness: Some(100),
                    vm_dirty_background_bytes: Some(67108864),
                    vm_dirty_bytes: Some(134217728),
                },
                performance: ModeConfig {
                    cpu_epp: "performance".to_string(),
                    platform_profile: "performance".to_string(),
                    background_cpu_weight: None,
                    background_io_weight: None,
                    user_cpu_weight: Some(200),
                    user_io_weight: Some(200),
                    requires_controlled_rt_access: None,
                    vm_swappiness: Some(150),
                    vm_dirty_background_bytes: Some(67108864),
                    vm_dirty_bytes: Some(134217728),
                },
                realtime: ModeConfig {
                    cpu_epp: "performance".to_string(),
                    platform_profile: "performance".to_string(),
                    background_cpu_weight: None,
                    background_io_weight: None,
                    user_cpu_weight: Some(250),
                    user_io_weight: Some(200),
                    requires_controlled_rt_access: Some(true),
                    vm_swappiness: Some(10),
                    vm_dirty_background_bytes: None,
                    vm_dirty_bytes: None,
                },
            },
            memory: MemoryConfig {
                high_swappiness_requires_zram: true,
            },
        }
    }
}

impl Policy {
    fn load(path: &Path) -> Self {
        let mut policy = Self::default();
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!(
                    "optid: failed to read policy TOML from {}: {}. Using defaults.",
                    path.display(),
                    e
                );
                return policy;
            }
        };

        let mut current_section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].trim().to_string();
                continue;
            }

            let mut parts = line.splitn(2, '=');
            let key = match parts.next() {
                Some(k) => k.trim(),
                None => continue,
            };
            let val = match parts.next() {
                Some(v) => v.trim(),
                None => continue,
            };

            let clean_str = |s: &str| -> String {
                let s = s.trim();
                if (s.starts_with('"') && s.ends_with('"'))
                    || (s.starts_with('\'') && s.ends_with('\''))
                {
                    s[1..s.len() - 1].to_string()
                } else {
                    s.to_string()
                }
            };

            match current_section.as_str() {
                "thresholds" => match key {
                    "cpu_pressure_perf_avg10" => {
                        if let Ok(n) = val.parse() {
                            policy.thresholds.cpu_pressure_perf_avg10 = n;
                        }
                    }
                    "memory_pressure_protect_avg10" => {
                        if let Ok(n) = val.parse() {
                            policy.thresholds.memory_pressure_protect_avg10 = n;
                        }
                    }
                    "io_pressure_throttle_avg10" => {
                        if let Ok(n) = val.parse() {
                            policy.thresholds.io_pressure_throttle_avg10 = n;
                        }
                    }
                    "hot_temp_c" => {
                        if let Ok(n) = val.parse() {
                            policy.thresholds.hot_temp_c = n;
                        }
                    }
                    "critical_temp_c" => {
                        if let Ok(n) = val.parse() {
                            policy.thresholds.critical_temp_c = n;
                        }
                    }
                    "low_battery_pct" => {
                        if let Ok(n) = val.parse() {
                            policy.thresholds.low_battery_pct = n;
                        }
                    }
                    _ => {}
                },
                "memory" => {
                    if key == "high_swappiness_requires_zram" {
                        if let Ok(b) = val.parse::<bool>() {
                            policy.memory.high_swappiness_requires_zram = b;
                        }
                    }
                }
                "modes.battery" => match key {
                    "cpu_epp" => policy.modes.battery.cpu_epp = clean_str(val),
                    "platform_profile" => policy.modes.battery.platform_profile = clean_str(val),
                    "background_cpu_weight" => {
                        policy.modes.battery.background_cpu_weight = val.parse().ok()
                    }
                    "background_io_weight" => {
                        policy.modes.battery.background_io_weight = val.parse().ok()
                    }
                    "user_cpu_weight" => policy.modes.battery.user_cpu_weight = val.parse().ok(),
                    "user_io_weight" => policy.modes.battery.user_io_weight = val.parse().ok(),
                    "vm_swappiness" => policy.modes.battery.vm_swappiness = val.parse().ok(),
                    "vm_dirty_background_bytes" => {
                        policy.modes.battery.vm_dirty_background_bytes = val.parse().ok()
                    }
                    "vm_dirty_bytes" => policy.modes.battery.vm_dirty_bytes = val.parse().ok(),
                    _ => {}
                },
                "modes.balanced" => match key {
                    "cpu_epp" => policy.modes.balanced.cpu_epp = clean_str(val),
                    "platform_profile" => policy.modes.balanced.platform_profile = clean_str(val),
                    "background_cpu_weight" => {
                        policy.modes.balanced.background_cpu_weight = val.parse().ok()
                    }
                    "background_io_weight" => {
                        policy.modes.balanced.background_io_weight = val.parse().ok()
                    }
                    "user_cpu_weight" => policy.modes.balanced.user_cpu_weight = val.parse().ok(),
                    "user_io_weight" => policy.modes.balanced.user_io_weight = val.parse().ok(),
                    "vm_swappiness" => policy.modes.balanced.vm_swappiness = val.parse().ok(),
                    "vm_dirty_background_bytes" => {
                        policy.modes.balanced.vm_dirty_background_bytes = val.parse().ok()
                    }
                    "vm_dirty_bytes" => policy.modes.balanced.vm_dirty_bytes = val.parse().ok(),
                    _ => {}
                },
                "modes.performance" => match key {
                    "cpu_epp" => policy.modes.performance.cpu_epp = clean_str(val),
                    "platform_profile" => {
                        policy.modes.performance.platform_profile = clean_str(val)
                    }
                    "background_cpu_weight" => {
                        policy.modes.performance.background_cpu_weight = val.parse().ok()
                    }
                    "background_io_weight" => {
                        policy.modes.performance.background_io_weight = val.parse().ok()
                    }
                    "user_cpu_weight" => {
                        policy.modes.performance.user_cpu_weight = val.parse().ok()
                    }
                    "user_io_weight" => policy.modes.performance.user_io_weight = val.parse().ok(),
                    "vm_swappiness" => policy.modes.performance.vm_swappiness = val.parse().ok(),
                    "vm_dirty_background_bytes" => {
                        policy.modes.performance.vm_dirty_background_bytes = val.parse().ok()
                    }
                    "vm_dirty_bytes" => policy.modes.performance.vm_dirty_bytes = val.parse().ok(),
                    _ => {}
                },
                "modes.realtime" => match key {
                    "cpu_epp" => policy.modes.realtime.cpu_epp = clean_str(val),
                    "platform_profile" => policy.modes.realtime.platform_profile = clean_str(val),
                    "background_cpu_weight" => {
                        policy.modes.realtime.background_cpu_weight = val.parse().ok()
                    }
                    "background_io_weight" => {
                        policy.modes.realtime.background_io_weight = val.parse().ok()
                    }
                    "user_cpu_weight" => policy.modes.realtime.user_cpu_weight = val.parse().ok(),
                    "user_io_weight" => policy.modes.realtime.user_io_weight = val.parse().ok(),
                    "requires_controlled_rt_access" => {
                        policy.modes.realtime.requires_controlled_rt_access = val.parse().ok()
                    }
                    "vm_swappiness" => policy.modes.realtime.vm_swappiness = val.parse().ok(),
                    "vm_dirty_background_bytes" => {
                        policy.modes.realtime.vm_dirty_background_bytes = val.parse().ok()
                    }
                    "vm_dirty_bytes" => policy.modes.realtime.vm_dirty_bytes = val.parse().ok(),
                    _ => {}
                },
                _ => {}
            }
        }
        policy
    }

    fn decide(&self, snapshot: &Snapshot, requested: Mode) -> Decision {
        let effective_mode = match requested {
            Mode::Auto => self.auto_mode(snapshot),
            explicit => explicit,
        };

        let mut reasons = Vec::new();
        let mut actions = Vec::new();

        if requested != Mode::Auto {
            reasons.push(format!("manual mode override: {requested}"));
        }

        if snapshot.on_ac == Some(false) {
            reasons.push("system is on battery".to_string());
        }

        if let Some(pct) = snapshot.battery_pct {
            if pct <= self.thresholds.low_battery_pct {
                reasons.push(format!("battery is low: {pct}%"));
            }
        }

        if let Some(temp) = snapshot.thermal_c() {
            if temp >= self.thresholds.critical_temp_c {
                reasons.push(format!("critical thermal pressure: {temp:.1}C"));
            } else if temp >= self.thresholds.hot_temp_c {
                reasons.push(format!("high thermal pressure: {temp:.1}C"));
            }
        }

        if let Some(cpu) = snapshot.cpu_pressure {
            if cpu.avg10 >= self.thresholds.cpu_pressure_perf_avg10 {
                reasons.push(format!("CPU pressure avg10 is {:.2}", cpu.avg10));
            }
        }

        if let Some(memory) = snapshot.memory_pressure {
            if memory.avg10 >= self.thresholds.memory_pressure_protect_avg10 {
                reasons.push(format!("memory pressure avg10 is {:.2}", memory.avg10));
                actions.push(Action::systemd_set_property(
                    "user.slice",
                    vec!["MemoryLow=256M".to_string()],
                    "protect active user sessions from reclaim pressure",
                ));
                actions.push(Action::systemd_set_property(
                    "background.slice",
                    vec![
                        "CPUWeight=50".to_string(),
                        "IOWeight=50".to_string(),
                        "MemoryHigh=75%".to_string(),
                    ],
                    "throttle background work during memory pressure",
                ));
            }
        }

        if let Some(io) = snapshot.io_pressure {
            if io.avg10 >= self.thresholds.io_pressure_throttle_avg10 {
                reasons.push(format!("I/O pressure avg10 is {:.2}", io.avg10));
                actions.push(Action::systemd_set_property(
                    "background.slice",
                    vec!["IOWeight=25".to_string()],
                    "reduce background I/O interference",
                ));
            }
        }

        let mode_config = match effective_mode {
            Mode::Battery => &self.modes.battery,
            Mode::Balanced => &self.modes.balanced,
            Mode::Performance => &self.modes.performance,
            Mode::Realtime => &self.modes.realtime,
            Mode::Auto => unreachable!("auto is resolved before action planning"),
        };

        actions.push(Action::cpu_epp(
            mode_config.cpu_epp.clone(),
            match effective_mode {
                Mode::Battery => "prefer battery life through CPU energy preference",
                Mode::Balanced => "keep foreground responsiveness without full turbo bias",
                Mode::Performance => "reduce CPU wakeup and ramp latency for sustained load",
                Mode::Realtime => "minimize latency for realtime mode",
                _ => "",
            },
        ));

        actions.push(Action::platform_profile(
            mode_config.platform_profile.clone(),
            match effective_mode {
                Mode::Battery => "request low-power platform profile",
                Mode::Balanced => "request balanced platform profile",
                Mode::Performance => "request performance platform profile",
                Mode::Realtime => "avoid firmware power-save latency in realtime mode",
                _ => "",
            },
        ));

        let mut bg_properties = Vec::new();
        if let Some(w) = mode_config.background_cpu_weight {
            bg_properties.push(format!("CPUWeight={w}"));
        }
        if let Some(w) = mode_config.background_io_weight {
            bg_properties.push(format!("IOWeight={w}"));
        }
        if !bg_properties.is_empty() {
            actions.push(Action::systemd_set_property(
                "background.slice",
                bg_properties,
                "deprioritize background services on battery",
            ));
        }

        let mut user_properties = Vec::new();
        if let Some(w) = mode_config.user_cpu_weight {
            user_properties.push(format!("CPUWeight={w}"));
        }
        if let Some(w) = mode_config.user_io_weight {
            user_properties.push(format!("IOWeight={w}"));
        }
        if !user_properties.is_empty() {
            actions.push(Action::systemd_set_property(
                "user.slice",
                user_properties,
                match effective_mode {
                    Mode::Balanced => "favor interactive user sessions",
                    Mode::Performance => "boost foreground user work",
                    Mode::Realtime => "prioritize controlled realtime user workload",
                    _ => "",
                },
            ));
        }

        // vm.swappiness
        if let Some(mut swappiness) = mode_config.vm_swappiness {
            if self.memory.high_swappiness_requires_zram
                && !snapshot.zram_swap_active
                && swappiness > 60
            {
                swappiness = 60;
            }
            actions.push(Action::vm_sysctl(
                PathBuf::from("/proc/sys/vm/swappiness"),
                swappiness.to_string(),
                "adjust swappiness for current mode",
            ));
        }

        // vm.dirty_background_bytes
        if let Some(bytes) = mode_config.vm_dirty_background_bytes {
            actions.push(Action::vm_sysctl(
                PathBuf::from("/proc/sys/vm/dirty_background_bytes"),
                bytes.to_string(),
                "adjust dirty background bytes for current mode",
            ));
        }

        // vm.dirty_bytes
        if let Some(bytes) = mode_config.vm_dirty_bytes {
            actions.push(Action::vm_sysctl(
                PathBuf::from("/proc/sys/vm/dirty_bytes"),
                bytes.to_string(),
                "adjust dirty bytes for current mode",
            ));
        }

        if snapshot
            .thermal_c()
            .is_some_and(|temp| temp >= self.thresholds.critical_temp_c)
        {
            actions.push(Action::cpu_epp(
                "balance_power".to_string(),
                "override performance bias because thermals are critical",
            ));
            actions.push(Action::platform_profile(
                "balanced".to_string(),
                "back off platform profile under critical thermals",
            ));
        }

        if reasons.is_empty() {
            reasons.push("default adaptive policy".to_string());
        }

        Decision {
            mode: effective_mode,
            reasons,
            actions,
        }
    }

    fn auto_mode(&self, snapshot: &Snapshot) -> Mode {
        if snapshot
            .thermal_c()
            .is_some_and(|temp| temp >= self.thresholds.critical_temp_c)
        {
            return Mode::Balanced;
        }

        if snapshot.on_ac == Some(false) {
            if snapshot
                .battery_pct
                .is_some_and(|pct| pct <= self.thresholds.low_battery_pct)
            {
                return Mode::Battery;
            }

            if snapshot
                .cpu_pressure
                .is_some_and(|p| p.avg10 >= self.thresholds.cpu_pressure_perf_avg10)
            {
                return Mode::Balanced;
            }

            return Mode::Battery;
        }

        if snapshot
            .cpu_pressure
            .is_some_and(|p| p.avg10 >= self.thresholds.cpu_pressure_perf_avg10)
        {
            return Mode::Performance;
        }

        Mode::Balanced
    }
}

#[derive(Debug, Clone)]
struct Decision {
    mode: Mode,
    reasons: Vec<String>,
    actions: Vec<Action>,
}

impl Decision {
    fn render(&self, snapshot: &Snapshot) -> String {
        let mut out = String::new();
        out.push_str(&format!("timestamp={}\n", snapshot.timestamp));
        out.push_str(&format!("mode={}\n", self.mode));
        out.push_str(&format!("on_ac={:?}\n", snapshot.on_ac));
        out.push_str(&format!("battery_pct={:?}\n", snapshot.battery_pct));
        out.push_str(&format!("thermal_c={:?}\n", snapshot.thermal_c()));
        out.push_str(&format!("loadavg_1={:?}\n", snapshot.loadavg_1));
        out.push_str(&format!(
            "cpu_pressure={}\n",
            fmt_pressure(snapshot.cpu_pressure)
        ));
        out.push_str(&format!(
            "memory_pressure={}\n",
            fmt_pressure(snapshot.memory_pressure)
        ));
        out.push_str(&format!(
            "io_pressure={}\n",
            fmt_pressure(snapshot.io_pressure)
        ));
        out.push_str("reasons:\n");
        for reason in &self.reasons {
            out.push_str(&format!("- {reason}\n"));
        }
        out.push_str("actions:\n");
        for action in &self.actions {
            out.push_str(&format!("- {}\n", action.describe()));
        }
        out
    }
}

#[derive(Debug, Clone)]
enum Action {
    CpuEpp {
        value: String,
        reason: &'static str,
    },
    PlatformProfile {
        value: String,
        reason: &'static str,
    },
    SystemdSetProperty {
        unit: &'static str,
        properties: Vec<String>,
        reason: &'static str,
    },
    VmSysctl {
        path: PathBuf,
        value: String,
        reason: &'static str,
    },
}

impl Action {
    fn cpu_epp(value: String, reason: &'static str) -> Self {
        Self::CpuEpp { value, reason }
    }

    fn platform_profile(value: String, reason: &'static str) -> Self {
        Self::PlatformProfile { value, reason }
    }

    fn systemd_set_property(
        unit: &'static str,
        properties: Vec<String>,
        reason: &'static str,
    ) -> Self {
        Self::SystemdSetProperty {
            unit,
            properties,
            reason,
        }
    }

    fn vm_sysctl(path: PathBuf, value: String, reason: &'static str) -> Self {
        Self::VmSysctl {
            path,
            value,
            reason,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::CpuEpp { value, reason } => format!("cpu.epp={value} ({reason})"),
            Self::PlatformProfile { value, reason } => {
                format!("platform.profile={value} ({reason})")
            }
            Self::SystemdSetProperty {
                unit,
                properties,
                reason,
            } => format!(
                "systemd.set-property {unit} {} ({reason})",
                properties.join(" ")
            ),
            Self::VmSysctl {
                path,
                value,
                reason,
            } => {
                format!("vm.sysctl {}={value} ({reason})", path.display())
            }
        }
    }
}

struct Actuator {
    state_dir: PathBuf,
    log_path: PathBuf,
}

impl Actuator {
    fn new(state_dir: PathBuf) -> Self {
        let log_path = state_dir.join("actions.log");
        Self {
            state_dir,
            log_path,
        }
    }

    fn apply(&mut self, action: &Action) -> io::Result<()> {
        match action {
            Action::CpuEpp { value, .. } => {
                let paths = discover_cpu_epp_paths();
                if paths.is_empty() {
                    self.log("skip cpu.epp: no energy_performance_preference paths")?;
                    return Ok(());
                }
                for path in paths {
                    let old_value = fs::read_to_string(&path)
                        .ok()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    guarded_write(&path, value)?;
                    self.log(&format!(
                        "write {} = {value} (was {old_value})",
                        path.display()
                    ))?;
                }
            }
            Action::PlatformProfile { value, .. } => {
                let path = Path::new("/sys/firmware/acpi/platform_profile");
                if path.exists() {
                    let old_value = fs::read_to_string(path)
                        .ok()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    guarded_write(path, value)?;
                    self.log(&format!(
                        "write {} = {value} (was {old_value})",
                        path.display()
                    ))?;
                } else {
                    self.log("skip platform.profile: platform_profile is unavailable")?;
                }
            }
            Action::SystemdSetProperty {
                unit, properties, ..
            } => {
                let status = Command::new("systemctl")
                    .arg("set-property")
                    .arg("--runtime")
                    .arg(unit)
                    .args(properties)
                    .status();
                match status {
                    Ok(status) if status.success() => {
                        self.log(&format!(
                            "systemctl set-property --runtime {unit} {}",
                            properties.join(" ")
                        ))?;
                    }
                    Ok(status) => {
                        self.log(&format!(
                            "skip systemd.set-property {unit}: systemctl exited with {status}"
                        ))?;
                    }
                    Err(err) => {
                        self.log(&format!(
                            "skip systemd.set-property {unit}: systemctl unavailable: {err}"
                        ))?;
                    }
                }
            }
            Action::VmSysctl { path, value, .. } => {
                let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                let key = format!("vm_{filename}");

                // Back up original value if not already backed up
                let orig_file = self.state_dir.join(format!("original_{key}"));
                if !orig_file.exists() {
                    if let Ok(current_val) = fs::read_to_string(path) {
                        let _ = fs::write(&orig_file, current_val.trim());
                    }
                }

                // Write intended value
                let intended_file = self.state_dir.join(format!("intended_{key}"));
                let _ = fs::write(&intended_file, value);

                // Write new value to sysctl path
                let old_value = fs::read_to_string(path)
                    .ok()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                match guarded_write(path, value) {
                    Ok(_) => {
                        self.log(&format!(
                            "write {} = {value} (was {old_value})",
                            path.display()
                        ))?;
                    }
                    Err(e) => {
                        self.log(&format!("skip vm.sysctl {filename}: write failed: {e}"))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn log(&mut self, message: &str) -> io::Result<()> {
        append_log(&self.log_path, &format!("{} {message}\n", now_unix()))
    }
}

/// Maps a state-dir backup key (`vm_<sysctl file name>`) back to the sysctl
/// path it was captured from. The sysctl file name may itself contain
/// underscores (e.g. `dirty_background_bytes`), so only the `vm_` prefix is
/// translated.
fn vm_sysctl_path(key: &str) -> Option<PathBuf> {
    let name = key.strip_prefix("vm_")?;
    Some(PathBuf::from(format!("/proc/sys/vm/{name}")))
}

fn revert_sysctls(state_dir: &Path) {
    let keys = [
        "vm_swappiness",
        "vm_dirty_background_bytes",
        "vm_dirty_bytes",
    ];
    let log_path = state_dir.join("actions.log");
    for key in &keys {
        let orig_path = state_dir.join(format!("original_{key}"));
        if !orig_path.exists() {
            continue;
        }
        let Some(sysctl_path) = vm_sysctl_path(key) else {
            continue;
        };
        let Ok(orig_val) = fs::read_to_string(&orig_path) else {
            continue;
        };
        let orig_val = orig_val.trim();
        match guarded_write(&sysctl_path, orig_val) {
            Ok(()) => {
                let _ = append_log(
                    &log_path,
                    &format!(
                        "{} revert {} = {orig_val}\n",
                        now_unix(),
                        sysctl_path.display()
                    ),
                );
                let _ = fs::remove_file(&orig_path);
                let _ = fs::remove_file(state_dir.join(format!("intended_{key}")));
            }
            Err(e) => {
                // Keep the backup so the next startup can retry the revert.
                let _ = append_log(
                    &log_path,
                    &format!(
                        "{} revert {} failed: {e}\n",
                        now_unix(),
                        sysctl_path.display()
                    ),
                );
                eprintln!(
                    "optid: failed to revert sysctl {}: {e}",
                    sysctl_path.display()
                );
            }
        }
    }
}

fn write_allowed(path: &Path) -> bool {
    path == Path::new("/sys/firmware/acpi/platform_profile")
        || path.starts_with("/sys/devices/system/cpu/")
        || path == Path::new("/proc/sys/vm/swappiness")
        || path == Path::new("/proc/sys/vm/dirty_background_bytes")
        || path == Path::new("/proc/sys/vm/dirty_bytes")
}

fn guarded_write(path: &Path, value: &str) -> io::Result<()> {
    if !write_allowed(path) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to write unallowlisted path {}", path.display()),
        ));
    }

    fs::write(path, value)
}

fn discover_cpu_epp_paths() -> Vec<PathBuf> {
    let base = Path::new("/sys/devices/system/cpu");
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("cpu") && name[3..].chars().all(|ch| ch.is_ascii_digit())
                })
        })
        .map(|path| path.join("cpufreq/energy_performance_preference"))
        .filter(|path| path.exists())
        .collect()
}

fn read_mode_override(state_dir: &Path) -> Option<Mode> {
    let text = fs::read_to_string(state_dir.join("mode")).ok()?;
    Mode::parse(&text)
}

fn read_on_ac() -> Option<bool> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    let mut saw_battery = false;

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let kind = fs::read_to_string(path.join("type")).unwrap_or_default();
        let kind = kind.trim();
        if kind.eq_ignore_ascii_case("Battery") {
            saw_battery = true;
            continue;
        }

        if matches!(kind, "Mains" | "USB" | "USB_C" | "USB_PD") {
            if let Ok(online) = fs::read_to_string(path.join("online")) {
                return Some(online.trim() == "1");
            }
        }
    }

    if saw_battery {
        Some(false)
    } else {
        None
    }
}

fn read_battery_pct() -> Option<u8> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let kind = fs::read_to_string(path.join("type")).unwrap_or_default();
        if kind.trim().eq_ignore_ascii_case("Battery") {
            let capacity = fs::read_to_string(path.join("capacity")).ok()?;
            return capacity.trim().parse::<u8>().ok();
        }
    }
    None
}

fn read_zram_swap_active() -> bool {
    let Ok(text) = fs::read_to_string("/proc/swaps") else {
        return false;
    };
    for line in text.lines().skip(1) {
        if line.contains("/dev/zram") {
            return true;
        }
    }
    false
}

fn read_max_thermal_millic() -> Option<i64> {
    let entries = fs::read_dir("/sys/class/thermal").ok()?;
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("thermal_zone"))
        })
        .filter_map(|entry| fs::read_to_string(entry.path().join("temp")).ok())
        .filter_map(|value| value.trim().parse::<i64>().ok())
        .max()
}

fn read_loadavg_1() -> Option<f32> {
    let text = fs::read_to_string("/proc/loadavg").ok()?;
    text.split_whitespace().next()?.parse::<f32>().ok()
}

fn parse_pressure(text: &str) -> Option<Pressure> {
    let line = text
        .lines()
        .find(|line| line.starts_with("some "))
        .or_else(|| text.lines().next())?;

    let mut pressure = Pressure::default();
    for token in line.split_whitespace().skip(1) {
        let (key, value) = token.split_once('=')?;
        match key {
            "avg10" => pressure.avg10 = value.parse().ok()?,
            "avg60" => pressure.avg60 = value.parse().ok()?,
            "avg300" => pressure.avg300 = value.parse().ok()?,
            "total" => pressure.total = value.parse().ok()?,
            _ => {}
        }
    }
    Some(pressure)
}

fn fmt_pressure(value: Option<Pressure>) -> String {
    match value {
        Some(p) => format!(
            "avg10={:.2} avg60={:.2} avg300={:.2} total={}",
            p.avg10, p.avg60, p.avg300, p.total
        ),
        None => "unavailable".to_string(),
    }
}

fn append_log(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(text.as_bytes())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_psi_some_line() {
        let pressure = parse_pressure(
            "some avg10=1.25 avg60=2.50 avg300=3.75 total=42\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
        )
        .unwrap();
        assert_eq!(pressure.avg10, 1.25);
        assert_eq!(pressure.avg60, 2.50);
        assert_eq!(pressure.avg300, 3.75);
        assert_eq!(pressure.total, 42);
    }

    #[test]
    fn battery_auto_mode_prefers_battery() {
        let snapshot = Snapshot {
            timestamp: 0,
            on_ac: Some(false),
            battery_pct: Some(80),
            max_temp_millic: Some(40_000),
            loadavg_1: Some(0.2),
            cpu_pressure: Some(Pressure::default()),
            memory_pressure: Some(Pressure::default()),
            io_pressure: Some(Pressure::default()),
            zram_swap_active: false,
        };

        let decision = Policy::default().decide(&snapshot, Mode::Auto);
        assert_eq!(decision.mode, Mode::Battery);
    }

    #[test]
    fn ac_cpu_pressure_prefers_performance() {
        let snapshot = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: Some(50_000),
            loadavg_1: Some(6.0),
            cpu_pressure: Some(Pressure {
                avg10: 20.0,
                ..Pressure::default()
            }),
            memory_pressure: Some(Pressure::default()),
            io_pressure: Some(Pressure::default()),
            zram_swap_active: false,
        };

        let decision = Policy::default().decide(&snapshot, Mode::Auto);
        assert_eq!(decision.mode, Mode::Performance);
    }

    #[test]
    fn critical_thermal_overrides_performance_bias() {
        let snapshot = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: Some(95_000),
            loadavg_1: Some(12.0),
            cpu_pressure: Some(Pressure {
                avg10: 30.0,
                ..Pressure::default()
            }),
            memory_pressure: Some(Pressure::default()),
            io_pressure: Some(Pressure::default()),
            zram_swap_active: false,
        };

        let decision = Policy::default().decide(&snapshot, Mode::Performance);
        assert!(decision.actions.iter().any(|action| {
            if let Action::CpuEpp { value, .. } = action {
                value == "balance_power"
            } else {
                false
            }
        }));
    }

    #[test]
    fn revert_keys_map_to_allowlisted_sysctl_paths() {
        // Regression: keys like vm_dirty_background_bytes contain underscores
        // inside the sysctl name; a naive '_'→'/' translation produced
        // /proc/sys/vm/dirty/background/bytes, which the allowlist rejected,
        // so dirty_* values were never restored on revert.
        for key in [
            "vm_swappiness",
            "vm_dirty_background_bytes",
            "vm_dirty_bytes",
        ] {
            let path = vm_sysctl_path(key).expect("backup key must map to a sysctl path");
            assert!(
                write_allowed(&path),
                "revert path {} for key {key} is not allowlisted",
                path.display()
            );
        }
        assert_eq!(
            vm_sysctl_path("vm_dirty_background_bytes"),
            Some(PathBuf::from("/proc/sys/vm/dirty_background_bytes"))
        );
        assert_eq!(vm_sysctl_path("swappiness"), None);
    }

    #[test]
    fn test_high_swappiness_gating() {
        let mut policy = Policy::default();
        policy.memory.high_swappiness_requires_zram = true;
        policy.modes.performance.vm_swappiness = Some(150);

        // Scenario A: zram is active -> swappiness should be 150
        let snapshot_with_zram = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: None,
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: true,
        };
        let decision_with = policy.decide(&snapshot_with_zram, Mode::Performance);
        let has_150 = decision_with.actions.iter().any(|action| {
            if let Action::VmSysctl { path, value, .. } = action {
                path == Path::new("/proc/sys/vm/swappiness") && value == "150"
            } else {
                false
            }
        });
        assert!(has_150, "should apply swappiness 150 with ZRAM");

        // Scenario B: zram is inactive -> swappiness should be clamped to 60
        let snapshot_no_zram = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: None,
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
        };
        let decision_no = policy.decide(&snapshot_no_zram, Mode::Performance);
        let has_60 = decision_no.actions.iter().any(|action| {
            if let Action::VmSysctl { path, value, .. } = action {
                path == Path::new("/proc/sys/vm/swappiness") && value == "60"
            } else {
                false
            }
        });
        assert!(has_60, "should clamp swappiness to 60 without ZRAM");
    }
}
