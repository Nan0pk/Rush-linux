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

const DEFAULT_DWELL_WINDOW_SEC: u64 = 3;
const DEFAULT_MODE_DWELL_WINDOW_SEC: u64 = DEFAULT_INTERVAL_SEC * 3;

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

    fn pin_application(&self, app_id: &str, class: &str) -> zbus::fdo::Result<()> {
        let classes = [
            "idle",
            "light",
            "interactive",
            "latency-critical",
            "throughput",
        ];
        if !classes.contains(&class) {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "invalid workload class: {class}"
            )));
        }
        if app_id == "--global" {
            fs::write(self.state_dir.join("workload_class_pin"), class).map_err(|e| {
                zbus::fdo::Error::Failed(format!("failed to write global pin: {e}"))
            })?;
            println!("Pinned global workload class to {class}");
            return Ok(());
        }
        let pins_dir = self.state_dir.join("pins");
        fs::create_dir_all(&pins_dir)
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to create pins dir: {e}")))?;
        fs::write(pins_dir.join(app_id), class)
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to write pin: {e}")))?;
        println!("Pinned application {app_id} to class {class}");
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
    revert_pm_qos(&args.state_dir);

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

    let initial_class = fs::read_to_string(args.state_dir.join("workload_class"))
        .ok()
        .and_then(|s| WorkloadClass::parse(&s))
        .unwrap_or(WorkloadClass::Idle);
    let mut hysteresis = HysteresisState::new(initial_class);
    let mut mode_hysteresis = ModeHysteresisState::new(Mode::Balanced);

    let mut actuator = Actuator::new(args.state_dir.clone());

    loop {
        let override_mode = read_mode_override(&args.state_dir).unwrap_or(Mode::Auto);
        let mut snapshot = Snapshot::collect();
        snapshot.global_pinned_class = read_global_pinned_class(&args.state_dir);
        if let Some(ref app) = snapshot.foreground_app {
            snapshot.pinned_class = read_pinned_class(&args.state_dir, app);
        }

        let policy = Policy::load(&args.config_path);
        let (raw_class, class_reason) = policy.classify(&snapshot);
        let (committed_class, _) =
            hysteresis.update(raw_class, snapshot.timestamp, DEFAULT_DWELL_WINDOW_SEC);

        let resolved_mode = match override_mode {
            Mode::Auto => {
                let raw_mode = policy.auto_mode(&snapshot);
                let critical_thermal = policy.is_critical_thermal(&snapshot);
                let (mode, _, _) = mode_hysteresis.update(
                    raw_mode,
                    snapshot.timestamp,
                    DEFAULT_MODE_DWELL_WINDOW_SEC,
                    critical_thermal,
                );
                mode
            }
            explicit => {
                mode_hysteresis.force(explicit);
                explicit
            }
        };
        let mode_hysteresis_reason = mode_hysteresis.explain_pending(snapshot.timestamp);

        let _ = fs::write(
            args.state_dir.join("workload_class"),
            committed_class.to_string(),
        );

        let contracts_path = args
            .config_path
            .parent()
            .map(|p| p.join("contracts.toml"))
            .unwrap_or_else(|| PathBuf::from("contracts.toml"));
        let contracts = Contracts::load(&contracts_path);

        let decision = policy.decide_resolved(
            &snapshot,
            override_mode,
            committed_class,
            class_reason,
            &contracts,
            Some(resolved_mode),
            mode_hysteresis_reason,
        );
        let report = decision.render(&snapshot);

        fs::write(args.state_dir.join("status"), &report)?;
        append_log(&args.state_dir.join("decisions.log"), &report)?;

        if args.apply {
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
    revert_pm_qos(&args.state_dir);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WorkloadClass {
    Idle,
    Light,
    Interactive,
    LatencyCritical,
    Throughput,
}

impl fmt::Display for WorkloadClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Idle => "idle",
            Self::Light => "light",
            Self::Interactive => "interactive",
            Self::LatencyCritical => "latency-critical",
            Self::Throughput => "throughput",
        };
        f.write_str(value)
    }
}

impl WorkloadClass {
    fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "idle" => Some(Self::Idle),
            "light" => Some(Self::Light),
            "interactive" => Some(Self::Interactive),
            "latency-critical" => Some(Self::LatencyCritical),
            "throughput" => Some(Self::Throughput),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ContractFloors {
    cpu_wakeup_latency: i64,
    device_resume_latency: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Contracts {
    idle: ContractFloors,
    light: ContractFloors,
    interactive: ContractFloors,
    latency_critical: ContractFloors,
    throughput: ContractFloors,
}

impl Default for Contracts {
    fn default() -> Self {
        Self {
            idle: ContractFloors {
                cpu_wakeup_latency: 100000,
                device_resume_latency: 1000000,
            },
            light: ContractFloors {
                cpu_wakeup_latency: 50000,
                device_resume_latency: 500000,
            },
            interactive: ContractFloors {
                cpu_wakeup_latency: 1000,
                device_resume_latency: 10000,
            },
            latency_critical: ContractFloors {
                cpu_wakeup_latency: 10,
                device_resume_latency: 100,
            },
            throughput: ContractFloors {
                cpu_wakeup_latency: 10000,
                device_resume_latency: 100000,
            },
        }
    }
}

impl Contracts {
    fn load(path: &Path) -> Self {
        let mut contracts = Self::default();
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return contracts,
        };

        let mut current_class: Option<String> = None;
        for line in text.lines() {
            let line = line.trim();
            let line = if let Some(idx) = line.find('#') {
                line[..idx].trim()
            } else {
                line
            };
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                let section = line[1..line.len() - 1].trim();
                if let Some(stripped) = section.strip_prefix("contracts.") {
                    current_class = Some(stripped.trim().to_string());
                } else {
                    current_class = None;
                }
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

            if let Some(ref class) = current_class {
                let val_parsed: i64 = match val.parse() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match class.as_str() {
                    "idle" => match key {
                        "cpu_wakeup_latency" => contracts.idle.cpu_wakeup_latency = val_parsed,
                        "device_resume_latency" => {
                            contracts.idle.device_resume_latency = val_parsed
                        }
                        _ => {}
                    },
                    "light" => match key {
                        "cpu_wakeup_latency" => contracts.light.cpu_wakeup_latency = val_parsed,
                        "device_resume_latency" => {
                            contracts.light.device_resume_latency = val_parsed
                        }
                        _ => {}
                    },
                    "interactive" => match key {
                        "cpu_wakeup_latency" => {
                            contracts.interactive.cpu_wakeup_latency = val_parsed
                        }
                        "device_resume_latency" => {
                            contracts.interactive.device_resume_latency = val_parsed
                        }
                        _ => {}
                    },
                    "latency-critical" => match key {
                        "cpu_wakeup_latency" => {
                            contracts.latency_critical.cpu_wakeup_latency = val_parsed
                        }
                        "device_resume_latency" => {
                            contracts.latency_critical.device_resume_latency = val_parsed
                        }
                        _ => {}
                    },
                    "throughput" => match key {
                        "cpu_wakeup_latency" => {
                            contracts.throughput.cpu_wakeup_latency = val_parsed
                        }
                        "device_resume_latency" => {
                            contracts.throughput.device_resume_latency = val_parsed
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        contracts
    }

    fn resolve(&self, class: WorkloadClass) -> ContractFloors {
        match class {
            WorkloadClass::Idle => self.idle,
            WorkloadClass::Light => self.light,
            WorkloadClass::Interactive => self.interactive,
            WorkloadClass::LatencyCritical => self.latency_critical,
            WorkloadClass::Throughput => self.throughput,
        }
    }
}

#[allow(dead_code)]
fn fits_contract(exit_latency_us: u64, floor_us: u64) -> bool {
    exit_latency_us <= floor_us
}

trait PmqosSink {
    fn read_cpu_latency(&self) -> io::Result<String>;
    fn write_cpu_latency(&mut self, value: Option<i32>) -> io::Result<()>;
    fn read_device_latency(&self, device_path: &Path) -> io::Result<String>;
    fn write_device_latency(&mut self, device_path: &Path, value: &str) -> io::Result<()>;
}

struct RealPmqosSink {
    cpu_fd: Option<fs::File>,
}

impl RealPmqosSink {
    fn new() -> Self {
        Self { cpu_fd: None }
    }
}

impl PmqosSink for RealPmqosSink {
    fn read_cpu_latency(&self) -> io::Result<String> {
        let text = fs::read_to_string("/dev/cpu_dma_latency")?;
        Ok(text)
    }

    fn write_cpu_latency(&mut self, value: Option<i32>) -> io::Result<()> {
        use std::io::Write;
        match value {
            Some(val) => {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/cpu_dma_latency")?;
                file.write_all(&val.to_ne_bytes())?;
                file.flush()?;
                self.cpu_fd = Some(file);
            }
            None => {
                self.cpu_fd = None;
            }
        }
        Ok(())
    }

    fn read_device_latency(&self, device_path: &Path) -> io::Result<String> {
        fs::read_to_string(device_path)
    }

    fn write_device_latency(&mut self, device_path: &Path, value: &str) -> io::Result<()> {
        guarded_write(device_path, value)
    }
}

fn get_path_hash(path: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[derive(Debug, Clone)]
struct HysteresisState {
    committed_class: WorkloadClass,
    candidate_class: WorkloadClass,
    candidate_since: Option<u64>,
}

impl HysteresisState {
    fn new(initial_class: WorkloadClass) -> Self {
        Self {
            committed_class: initial_class,
            candidate_class: initial_class,
            candidate_since: None,
        }
    }

    fn update(
        &mut self,
        next_class: WorkloadClass,
        now: u64,
        dwell_window_sec: u64,
    ) -> (WorkloadClass, bool) {
        if next_class == self.committed_class {
            self.candidate_class = next_class;
            self.candidate_since = None;
            (self.committed_class, false)
        } else if next_class == self.candidate_class {
            match self.candidate_since {
                None => {
                    self.candidate_since = Some(now);
                    (self.committed_class, false)
                }
                Some(since) => {
                    if now >= since + dwell_window_sec {
                        self.committed_class = next_class;
                        self.candidate_since = None;
                        (self.committed_class, true)
                    } else {
                        (self.committed_class, false)
                    }
                }
            }
        } else {
            self.candidate_class = next_class;
            self.candidate_since = Some(now);
            (self.committed_class, false)
        }
    }
}

#[derive(Debug, Clone)]
struct ModeHysteresisState {
    committed_mode: Mode,
    candidate_mode: Mode,
    candidate_since: Option<u64>,
}

impl ModeHysteresisState {
    fn new(initial_mode: Mode) -> Self {
        Self {
            committed_mode: initial_mode,
            candidate_mode: initial_mode,
            candidate_since: None,
        }
    }

    fn force(&mut self, mode: Mode) {
        self.committed_mode = mode;
        self.candidate_mode = mode;
        self.candidate_since = None;
    }

    fn update(
        &mut self,
        next_mode: Mode,
        now: u64,
        dwell_window_sec: u64,
        bypass_hysteresis: bool,
    ) -> (Mode, bool, Option<String>) {
        if bypass_hysteresis {
            let changed = self.committed_mode != next_mode;
            self.force(next_mode);
            return (
                self.committed_mode,
                changed,
                Some(format!(
                    "mode hysteresis bypassed for safety: committed {} immediately",
                    self.committed_mode
                )),
            );
        }

        if next_mode == self.committed_mode {
            self.candidate_mode = next_mode;
            self.candidate_since = None;
            return (self.committed_mode, false, None);
        }

        if next_mode != self.candidate_mode {
            self.candidate_mode = next_mode;
            self.candidate_since = Some(now);
            return (
                self.committed_mode,
                false,
                Some(format!(
                    "mode hysteresis delaying transition: committed={}, candidate={}, elapsed=0s, required={}s",
                    self.committed_mode, self.candidate_mode, dwell_window_sec
                )),
            );
        }

        let since = self.candidate_since.unwrap_or(now);
        self.candidate_since = Some(since);
        if now >= since + dwell_window_sec {
            self.committed_mode = next_mode;
            self.candidate_since = None;
            return (
                self.committed_mode,
                true,
                Some(format!(
                    "mode hysteresis committed transition to {} after {}s dwell",
                    self.committed_mode,
                    now.saturating_sub(since)
                )),
            );
        }

        (
            self.committed_mode,
            false,
            Some(format!(
                "mode hysteresis delaying transition: committed={}, candidate={}, elapsed={}s, required={}s",
                self.committed_mode,
                self.candidate_mode,
                now.saturating_sub(since),
                dwell_window_sec
            )),
        )
    }

    fn explain_pending(&self, now: u64) -> Option<String> {
        self.candidate_since.map(|since| {
            format!(
                "mode hysteresis pending: committed={}, candidate={}, elapsed={}s, required={}s",
                self.committed_mode,
                self.candidate_mode,
                now.saturating_sub(since),
                DEFAULT_MODE_DWELL_WINDOW_SEC
            )
        })
    }
}

fn read_pinned_class(state_dir: &Path, app_id: &str) -> Option<WorkloadClass> {
    let pin_file = state_dir.join("pins").join(app_id);
    let text = fs::read_to_string(pin_file).ok()?;
    WorkloadClass::parse(&text)
}

fn read_global_pinned_class(state_dir: &Path) -> Option<WorkloadClass> {
    let pin_file = state_dir.join("workload_class_pin");
    let text = fs::read_to_string(pin_file).ok()?;
    let parsed = WorkloadClass::parse(&text);
    if parsed.is_none() {
        eprintln!("optid: ignored invalid global class pin: '{}'", text.trim());
    }
    parsed
}

fn discover_pm_qos_device_paths() -> Vec<PathBuf> {
    let base = Path::new("/sys/bus/pci/devices");
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir(base) else {
        return paths;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path().join("power").join("pm_qos_resume_latency_us");
        if path.exists() {
            paths.push(path);
        }
    }
    paths
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
    foreground_app: Option<String>,
    pinned_class: Option<WorkloadClass>,
    global_pinned_class: Option<WorkloadClass>,
    pm_qos_device_paths: Vec<PathBuf>,
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
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: discover_pm_qos_device_paths(),
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
#[allow(dead_code)]
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
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!(
                    "optid: failed to read policy TOML from {}: {}. Using defaults.",
                    path.display(),
                    e
                );
                return Self::default();
            }
        };

        match toml::from_str(&text) {
            Ok(policy) => policy,
            Err(e) => {
                eprintln!(
                    "optid: failed to parse policy TOML from {}: {}. Using defaults.",
                    path.display(),
                    e
                );
                Self::default()
            }
        }
    }

    fn classify(&self, snapshot: &Snapshot) -> (WorkloadClass, String) {
        if let Some(pinned) = snapshot.global_pinned_class {
            return (pinned, "pinned override (global)".to_string());
        }
        if let Some(pinned) = snapshot.pinned_class {
            return (pinned, "pinned override for foreground app".to_string());
        }

        let load = snapshot.loadavg_1.unwrap_or(0.0);
        let cpu_pressure = snapshot.cpu_pressure.map(|p| p.avg10).unwrap_or(0.0);
        let mem_pressure = snapshot.memory_pressure.map(|p| p.avg10).unwrap_or(0.0);
        let io_pressure = snapshot.io_pressure.map(|p| p.avg10).unwrap_or(0.0);

        if load >= 4.0
            && (cpu_pressure >= self.thresholds.cpu_pressure_perf_avg10
                || io_pressure >= self.thresholds.io_pressure_throttle_avg10)
        {
            return (
                WorkloadClass::Throughput,
                format!(
                    "high load ({:.2}) and high pressure (cpu: {:.2}, io: {:.2})",
                    load, cpu_pressure, io_pressure
                ),
            );
        }

        if (1.5..4.0).contains(&load)
            && cpu_pressure >= self.thresholds.cpu_pressure_perf_avg10
            && snapshot.on_ac == Some(true)
        {
            return (
                WorkloadClass::LatencyCritical,
                format!(
                    "moderate load ({:.2}) with cpu pressure ({:.2}) on AC",
                    load, cpu_pressure
                ),
            );
        }

        if load >= 0.5 || cpu_pressure > 2.0 || mem_pressure > 2.0 {
            return (
                WorkloadClass::Interactive,
                format!(
                    "active usage: load={:.2}, cpu_pressure={:.2}, mem_pressure={:.2}",
                    load, cpu_pressure, mem_pressure
                ),
            );
        }

        if load > 0.05 || cpu_pressure > 0.1 {
            return (
                WorkloadClass::Light,
                format!(
                    "low activity: load={:.2}, cpu_pressure={:.2}",
                    load, cpu_pressure
                ),
            );
        }

        (
            WorkloadClass::Idle,
            format!(
                "system idle: load={:.2}, cpu_pressure={:.2}",
                load, cpu_pressure
            ),
        )
    }

    #[allow(dead_code)]
    fn decide(
        &self,
        snapshot: &Snapshot,
        requested: Mode,
        workload_class: WorkloadClass,
        workload_reason: String,
        contracts: &Contracts,
    ) -> Decision {
        self.decide_resolved(
            snapshot,
            requested,
            workload_class,
            workload_reason,
            contracts,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn decide_resolved(
        &self,
        snapshot: &Snapshot,
        requested: Mode,
        workload_class: WorkloadClass,
        workload_reason: String,
        contracts: &Contracts,
        resolved_mode: Option<Mode>,
        mode_hysteresis_reason: Option<String>,
    ) -> Decision {
        let effective_mode = resolved_mode.unwrap_or_else(|| match requested {
            Mode::Auto => self.auto_mode(snapshot),
            explicit => explicit,
        });

        let mut reasons = Vec::new();
        let mut actions = Vec::new();

        if let Some(reason) = mode_hysteresis_reason {
            reasons.push(reason);
        }

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
                    "user.slice".to_string(),
                    vec!["MemoryLow=256M".to_string()],
                    "protect active user sessions from reclaim pressure".to_string(),
                ));
                actions.push(Action::systemd_set_property(
                    "background.slice".to_string(),
                    vec![
                        "CPUWeight=50".to_string(),
                        "IOWeight=50".to_string(),
                        "MemoryHigh=75%".to_string(),
                    ],
                    "throttle background work during memory pressure".to_string(),
                ));
            }
        }

        if let Some(io) = snapshot.io_pressure {
            if io.avg10 >= self.thresholds.io_pressure_throttle_avg10 {
                reasons.push(format!("I/O pressure avg10 is {:.2}", io.avg10));
                actions.push(Action::systemd_set_property(
                    "background.slice".to_string(),
                    vec!["IOWeight=25".to_string()],
                    "reduce background I/O interference".to_string(),
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
                Mode::Battery => "prefer battery life through CPU energy preference".to_string(),
                Mode::Balanced => {
                    "keep foreground responsiveness without full turbo bias".to_string()
                }
                Mode::Performance => {
                    "reduce CPU wakeup and ramp latency for sustained load".to_string()
                }
                Mode::Realtime => "minimize latency for realtime mode".to_string(),
                _ => "".to_string(),
            },
        ));

        actions.push(Action::platform_profile(
            mode_config.platform_profile.clone(),
            match effective_mode {
                Mode::Battery => "request low-power platform profile".to_string(),
                Mode::Balanced => "request balanced platform profile".to_string(),
                Mode::Performance => "request performance platform profile".to_string(),
                Mode::Realtime => "avoid firmware power-save latency in realtime mode".to_string(),
                _ => "".to_string(),
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
                "background.slice".to_string(),
                bg_properties,
                "deprioritize background services on battery".to_string(),
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
                "user.slice".to_string(),
                user_properties,
                match effective_mode {
                    Mode::Balanced => "favor interactive user sessions".to_string(),
                    Mode::Performance => "boost foreground user work".to_string(),
                    Mode::Realtime => "prioritize controlled realtime user workload".to_string(),
                    _ => "".to_string(),
                },
            ));
        }

        if self.memory.high_swappiness_requires_zram && !snapshot.zram_swap_active {
            reasons.push("vm.* actuation skipped: zram swap is not active".to_string());
        } else {
            // vm.swappiness
            if let Some(swappiness) = mode_config.vm_swappiness {
                actions.push(Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/swappiness"),
                    swappiness.to_string(),
                    "adjust swappiness for current mode".to_string(),
                ));
            }

            // vm.dirty_background_bytes
            if let Some(bytes) = mode_config.vm_dirty_background_bytes {
                actions.push(Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/dirty_background_bytes"),
                    bytes.to_string(),
                    "adjust dirty background bytes for current mode".to_string(),
                ));
            }

            // vm.dirty_bytes
            if let Some(bytes) = mode_config.vm_dirty_bytes {
                actions.push(Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/dirty_bytes"),
                    bytes.to_string(),
                    "adjust dirty bytes for current mode".to_string(),
                ));
            }
        }

        if snapshot
            .thermal_c()
            .is_some_and(|temp| temp >= self.thresholds.critical_temp_c)
        {
            actions.push(Action::cpu_epp(
                "balance_power".to_string(),
                "override performance bias because thermals are critical".to_string(),
            ));
            actions.push(Action::platform_profile(
                "balanced".to_string(),
                "back off platform profile under critical thermals".to_string(),
            ));
        }

        // PM QoS wakeup latency (CPU)
        let floors = contracts.resolve(workload_class);
        let cpu_wakeup_latency = Some(floors.cpu_wakeup_latency);
        let device_resume_latency = Some(floors.device_resume_latency);

        let reason_cpu = format!(
            "class={}, floor={}us, row=contracts.{}",
            workload_class, floors.cpu_wakeup_latency, workload_class
        );
        actions.push(Action::CpuDmaLatency {
            value: Some(floors.cpu_wakeup_latency as i32),
            reason: reason_cpu,
        });

        // PM QoS resume latency (Per-device)
        for path in &snapshot.pm_qos_device_paths {
            let reason_dev = format!(
                "class={}, floor={}us, row=contracts.{}",
                workload_class, floors.device_resume_latency, workload_class
            );
            actions.push(Action::DeviceResumeLatency {
                path: path.clone(),
                value: Some(floors.device_resume_latency as i32),
                reason: reason_dev,
            });
        }

        if reasons.is_empty() {
            reasons.push("default adaptive policy".to_string());
        }

        Decision {
            mode: effective_mode,
            reasons,
            actions,
            workload_class,
            workload_reason,
            cpu_wakeup_latency,
            device_resume_latency,
        }
    }

    fn is_critical_thermal(&self, snapshot: &Snapshot) -> bool {
        snapshot
            .thermal_c()
            .is_some_and(|temp| temp >= self.thresholds.critical_temp_c)
    }

    fn auto_mode(&self, snapshot: &Snapshot) -> Mode {
        if self.is_critical_thermal(snapshot) {
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
    workload_class: WorkloadClass,
    workload_reason: String,
    cpu_wakeup_latency: Option<i64>,
    device_resume_latency: Option<i64>,
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
        out.push_str(&format!("workload_class={}\n", self.workload_class));
        out.push_str(&format!("workload_reason={}\n", self.workload_reason));

        match self.cpu_wakeup_latency {
            Some(v) => out.push_str(&format!("cpu_wakeup_latency={}\n", v)),
            None => out.push_str("cpu_wakeup_latency=None\n"),
        }
        match self.device_resume_latency {
            Some(v) => out.push_str(&format!("device_resume_latency={}\n", v)),
            None => out.push_str("device_resume_latency=None\n"),
        }

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
        reason: String,
    },
    PlatformProfile {
        value: String,
        reason: String,
    },
    SystemdSetProperty {
        unit: String,
        properties: Vec<String>,
        reason: String,
    },
    VmSysctl {
        path: PathBuf,
        value: String,
        reason: String,
    },
    CpuDmaLatency {
        value: Option<i32>,
        reason: String,
    },
    DeviceResumeLatency {
        path: PathBuf,
        value: Option<i32>,
        reason: String,
    },
}

impl Action {
    fn cpu_epp(value: String, reason: String) -> Self {
        Self::CpuEpp { value, reason }
    }

    fn platform_profile(value: String, reason: String) -> Self {
        Self::PlatformProfile { value, reason }
    }

    fn systemd_set_property(unit: String, properties: Vec<String>, reason: String) -> Self {
        Self::SystemdSetProperty {
            unit,
            properties,
            reason,
        }
    }

    fn vm_sysctl(path: PathBuf, value: String, reason: String) -> Self {
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
            Self::CpuDmaLatency { value, reason } => {
                let val_str = value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "None".to_string());
                format!("cpu_dma_latency={val_str} ({reason})")
            }
            Self::DeviceResumeLatency {
                path,
                value,
                reason,
            } => {
                let val_str = value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "None".to_string());
                format!(
                    "device_resume_latency {}={val_str} ({reason})",
                    path.display()
                )
            }
        }
    }
}

struct Actuator {
    state_dir: PathBuf,
    log_path: PathBuf,
    pmqos_sink: Box<dyn PmqosSink>,
    last_cpu_latency: Option<Option<i32>>,
    last_device_latencies: std::collections::HashMap<PathBuf, Option<i32>>,
}

impl Actuator {
    fn new(state_dir: PathBuf) -> Self {
        let log_path = state_dir.join("actions.log");
        Self {
            state_dir,
            log_path,
            pmqos_sink: Box::new(RealPmqosSink::new()),
            last_cpu_latency: None,
            last_device_latencies: std::collections::HashMap::new(),
        }
    }

    #[allow(dead_code)]
    fn new_with_sink(state_dir: PathBuf, sink: Box<dyn PmqosSink>) -> Self {
        let log_path = state_dir.join("actions.log");
        Self {
            state_dir,
            log_path,
            pmqos_sink: sink,
            last_cpu_latency: None,
            last_device_latencies: std::collections::HashMap::new(),
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
                    // Soft-fail per CPU: a hotplug or transient EBUSY on one
                    // core should not terminate the daemon.
                    match guarded_write(&path, value) {
                        Ok(_) => {
                            self.log(&format!(
                                "write {} = {value} (was {old_value})",
                                path.display()
                            ))?;
                        }
                        Err(e) => {
                            self.log(&format!(
                                "skip cpu.epp {}: write failed: {e}",
                                path.display()
                            ))?;
                        }
                    }
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
                    // Soft-fail: a write rejection here should not crash the
                    // daemon. Log and move on; next cycle will retry.
                    match guarded_write(path, value) {
                        Ok(_) => {
                            self.log(&format!(
                                "write {} = {value} (was {old_value})",
                                path.display()
                            ))?;
                        }
                        Err(e) => {
                            self.log(&format!("skip platform.profile: write failed: {e}"))?;
                        }
                    }
                } else {
                    self.log("skip platform.profile: platform_profile is unavailable")?;
                }
            }
            Action::SystemdSetProperty {
                unit, properties, ..
            } => {
                // INVARIANT: `properties` must be produced by typed code paths
                // (Action::SystemdSetProperty constructors in Decision). It is
                // splatted directly into `systemctl set-property` argv with no
                // shell quoting. If a future code path ever lets policy.toml or
                // any other untrusted source feed strings into this Vec, this
                // becomes a systemd-syntax injection vector — guard at the
                // construction site, not here.
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
                        let _ = atomic_write_state_file(&orig_file, current_val.trim());
                    }
                }

                // Write intended value
                let intended_file = self.state_dir.join(format!("intended_{key}"));
                let _ = atomic_write_state_file(&intended_file, value);

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
            Action::CpuDmaLatency { value, reason } => {
                let should_apply = match self.last_cpu_latency {
                    Some(last_val) => last_val != *value,
                    None => true,
                };
                if should_apply {
                    let old_value = self
                        .pmqos_sink
                        .read_cpu_latency()
                        .unwrap_or_else(|_| "n/a".to_string());
                    // Soft-fail: missing /dev/cpu_dma_latency (e.g. running in
                    // a container or on a kernel without it) should not crash
                    // the daemon. Skip and log; `last_cpu_latency` is left
                    // untouched so a future success will still take effect.
                    let val_str = value
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "n/a".to_string());
                    match self.pmqos_sink.write_cpu_latency(*value) {
                        Ok(_) => {
                            self.last_cpu_latency = Some(*value);
                            self.log(&format!(
                                "write /dev/cpu_dma_latency = {val_str} (was {old_value}) reason: {reason}"
                            ))?;
                        }
                        Err(e) => {
                            self.log(&format!(
                                "skip /dev/cpu_dma_latency = {val_str}: write failed: {e} reason: {reason}"
                            ))?;
                        }
                    }
                }
            }
            Action::DeviceResumeLatency {
                path,
                value,
                reason,
            } => {
                let should_apply = match self.last_device_latencies.get(path) {
                    Some(last_val) => last_val != value,
                    None => true,
                };
                if should_apply {
                    let hash = get_path_hash(path);
                    let key = format!("dev_{hash}");

                    // Back up original value if not already backed up
                    let orig_file = self.state_dir.join(format!("original_{key}"));
                    if !orig_file.exists() {
                        if let Ok(current_val) = self.pmqos_sink.read_device_latency(path) {
                            let content = format!("{}\n{}", path.display(), current_val.trim());
                            let _ = atomic_write_state_file(&orig_file, &content);
                        }
                    }

                    // Write intended value
                    let val_str = value
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "0".to_string());
                    let intended_file = self.state_dir.join(format!("intended_{key}"));
                    let _ = atomic_write_state_file(&intended_file, &val_str);

                    let old_value = self
                        .pmqos_sink
                        .read_device_latency(path)
                        .ok()
                        .unwrap_or_default()
                        .trim()
                        .to_string();

                    match self.pmqos_sink.write_device_latency(path, &val_str) {
                        Ok(_) => {
                            self.last_device_latencies.insert(path.clone(), *value);
                            self.log(&format!(
                                "write {} = {val_str} (was {old_value}) reason: {reason}",
                                path.display()
                            ))?;
                        }
                        Err(e) => {
                            self.log(&format!(
                                "skip device latency {}: write failed: {e}",
                                path.display()
                            ))?;
                        }
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

fn revert_sysctls(state_dir: &Path) {
    let keys = [
        "vm_swappiness",
        "vm_dirty_background_bytes",
        "vm_dirty_bytes",
    ];
    for key in &keys {
        let orig_path = state_dir.join(format!("original_{key}"));
        if orig_path.exists() {
            if let Ok(orig_val) = fs::read_to_string(&orig_path) {
                let sysctl_name = key.replace('_', ".");
                let sysctl_path =
                    PathBuf::from(format!("/proc/sys/{}", sysctl_name.replace('.', "/")));
                if let Err(e) = guarded_write(&sysctl_path, orig_val.trim()) {
                    eprintln!("optid: failed to revert sysctl {sysctl_name}: {e}");
                } else {
                    println!("optid: reverted sysctl {sysctl_name} to {orig_val}");
                }
            }
            let _ = fs::remove_file(&orig_path);
            let _ = fs::remove_file(state_dir.join(format!("intended_{key}")));
        }
    }
}

fn revert_pm_qos(state_dir: &Path) {
    let Ok(entries) = fs::read_dir(state_dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("original_dev_") {
            let orig_path = entry.path();
            if let Ok(content) = fs::read_to_string(&orig_path) {
                let mut lines = content.lines();
                if let (Some(dev_path_str), Some(orig_val)) = (lines.next(), lines.next()) {
                    let dev_path = Path::new(dev_path_str);
                    if let Err(e) = guarded_write(dev_path, orig_val.trim()) {
                        eprintln!(
                            "optid: failed to revert PM QoS for {}: {e}",
                            dev_path.display()
                        );
                    } else {
                        println!(
                            "optid: reverted PM QoS for {} to {}",
                            dev_path.display(),
                            orig_val.trim()
                        );
                    }
                }
            }
            let _ = fs::remove_file(&orig_path);
            let hash = name_str.strip_prefix("original_dev_").unwrap_or("");
            if !hash.is_empty() {
                let intended_path = state_dir.join(format!("intended_dev_{hash}"));
                let _ = fs::remove_file(intended_path);
            }
        }
    }
}

fn guarded_write(path: &Path, value: &str) -> io::Result<()> {
    // Structural check for the per-PCI-device PM QoS resume-latency file.
    // Must be exactly `…/power/pm_qos_resume_latency_us` — not a substring of
    // some other file name. Compare via Path::file_name() rather than
    // stringifying the path.
    fn is_pm_qos_resume_latency(path: &Path) -> bool {
        path.file_name().and_then(|n| n.to_str()) == Some("pm_qos_resume_latency_us")
            && path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                == Some("power")
    }

    let allowed = path == Path::new("/sys/firmware/acpi/platform_profile")
        || path.starts_with("/sys/devices/system/cpu/")
        || path == Path::new("/proc/sys/vm/swappiness")
        || path == Path::new("/proc/sys/vm/dirty_background_bytes")
        || path == Path::new("/proc/sys/vm/dirty_bytes")
        || (path.starts_with("/sys/") && is_pm_qos_resume_latency(path))
        || (cfg!(test) && is_pm_qos_resume_latency(path));

    if !allowed {
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

/// Atomic write of a state file in `/run/optid`.
///
/// Writes to `<path>.tmp` first, then renames into place. The rename is
/// atomic on POSIX, so a SIGKILL between the write and the rename leaves
/// either the previous contents (if any) or no file at all — never a
/// truncated `original_*` or `intended_*` file that the next-boot revert
/// would interpret as a real backup.
fn atomic_write_state_file(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(tmp, path)
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
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };

        let decision = Policy::default().decide(
            &snapshot,
            Mode::Auto,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
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
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };

        let decision = Policy::default().decide(
            &snapshot,
            Mode::Auto,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
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
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };

        let decision = Policy::default().decide(
            &snapshot,
            Mode::Performance,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(decision.actions.iter().any(|action| {
            if let Action::CpuEpp { value, .. } = action {
                value == "balance_power"
            } else {
                false
            }
        }));
    }

    #[test]
    fn test_t1_dry_run_no_op() {
        let temp_dir = std::env::temp_dir().join(format!("optid_tests_t1_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("policy.toml");
        fs::write(&config_path, "").unwrap();

        let args = Args {
            apply: false, // DRY RUN
            once: true,
            help: false,
            interval_sec: 1,
            state_dir: temp_dir.clone(),
            config_path,
        };

        run(args).unwrap();

        assert!(!temp_dir.join("intended_vm_swappiness").exists());

        let decisions = fs::read_to_string(temp_dir.join("decisions.log")).unwrap();
        assert!(decisions.contains("vm.* actuation skipped: zram swap is not active"));
    }

    #[test]
    fn test_t2_apply_allowlisted_and_t4_revert() {
        let temp_dir = std::env::temp_dir().join(format!("optid_tests_t2_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("policy.toml");
        fs::write(&config_path, "").unwrap();

        let mut actuator = Actuator::new(temp_dir.clone());
        let action = Action::vm_sysctl(
            PathBuf::from("/proc/sys/vm/swappiness"),
            "60".to_string(),
            "test reason".to_string(),
        );
        let _ = actuator.apply(&action);

        assert!(temp_dir.join("intended_vm_swappiness").exists());

        let actions_log = fs::read_to_string(temp_dir.join("actions.log")).unwrap();
        assert!(actions_log.contains("vm.sysctl swappiness") || actions_log.contains("was"));

        revert_sysctls(&temp_dir);
        assert!(!temp_dir.join("intended_vm_swappiness").exists());
    }

    #[test]
    fn test_t2b_vm_sysctl_writes_revert_journal_entry() {
        let temp_dir = std::env::temp_dir().join(format!("optid_tests_t2b_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let sysctl_path = temp_dir.join("swappiness");
        fs::write(&sysctl_path, "100").unwrap();

        let mut actuator = Actuator::new(temp_dir.clone());
        let action = Action::vm_sysctl(
            sysctl_path.clone(),
            "60".to_string(),
            "test journal".to_string(),
        );
        actuator.apply(&action).unwrap();

        // The temp path is intentionally outside the guarded sysctl allowlist, so
        // the write may be skipped. The journal entries must still capture the
        // original and intended values for real allowlisted vm.* paths.
        assert_eq!(
            fs::read_to_string(temp_dir.join("original_vm_swappiness"))
                .unwrap()
                .trim(),
            "100"
        );
        assert_eq!(
            fs::read_to_string(temp_dir.join("intended_vm_swappiness"))
                .unwrap()
                .trim(),
            "60"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_t3_zram_gate() {
        let mut policy = Policy::default();
        policy.memory.high_swappiness_requires_zram = true;
        policy.modes.performance.vm_swappiness = Some(150);

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
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        let decision_with = policy.decide(
            &snapshot_with_zram,
            Mode::Performance,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        let has_150 = decision_with.actions.iter().any(|action| {
            if let Action::VmSysctl { path, value, .. } = action {
                path == Path::new("/proc/sys/vm/swappiness") && value == "150"
            } else {
                false
            }
        });
        assert!(has_150);

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
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        let decision_no = policy.decide(
            &snapshot_no_zram,
            Mode::Performance,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        let has_vm_action = decision_no
            .actions
            .iter()
            .any(|action| matches!(action, Action::VmSysctl { .. }));
        assert!(!has_vm_action);
        assert!(decision_no
            .reasons
            .iter()
            .any(|reason| reason.contains("vm.* actuation skipped")));
    }

    #[test]
    fn test_t5_explainability() {
        let action = Action::vm_sysctl(
            PathBuf::from("/proc/sys/vm/swappiness"),
            "100".to_string(),
            "adjust swappiness for current mode".to_string(),
        );
        let desc = action.describe();
        assert_eq!(
            desc,
            "vm.sysctl /proc/sys/vm/swappiness=100 (adjust swappiness for current mode)"
        );
    }

    #[test]
    fn test_n1_t1_class_mapping() {
        let policy = Policy::default();

        // 1. Idle snapshot
        let idle_snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        assert_eq!(policy.classify(&idle_snap).0, WorkloadClass::Idle);

        // 2. Light snapshot
        let light_snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.1),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        assert_eq!(policy.classify(&light_snap).0, WorkloadClass::Light);

        // 3. Interactive snapshot
        let int_snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.8),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        assert_eq!(policy.classify(&int_snap).0, WorkloadClass::Interactive);

        // 4. Latency-critical snapshot
        let lc_snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(2.0),
            cpu_pressure: Some(Pressure {
                avg10: 15.0,
                ..Pressure::default()
            }),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        assert_eq!(policy.classify(&lc_snap).0, WorkloadClass::LatencyCritical);

        // 5. Throughput snapshot
        let tp_snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(5.0),
            cpu_pressure: Some(Pressure {
                avg10: 15.0,
                ..Pressure::default()
            }),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        assert_eq!(policy.classify(&tp_snap).0, WorkloadClass::Throughput);
    }

    #[test]
    fn test_n1_t2_pin_precedence() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: Some("doom.exe".to_string()),
            pinned_class: Some(WorkloadClass::LatencyCritical),
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        let (class, reason) = policy.classify(&snap);
        assert_eq!(class, WorkloadClass::LatencyCritical);
        assert!(reason.contains("pinned override"));
    }

    #[test]
    fn test_n1_t3_hysteresis() {
        let mut hysteresis = HysteresisState::new(WorkloadClass::Idle);

        // Transition from Idle -> Interactive.
        // Sample at t=0, class remains Idle (candidate Interactive)
        let (class, _) = hysteresis.update(WorkloadClass::Interactive, 0, 3);
        assert_eq!(class, WorkloadClass::Idle);

        // Sample at t=2 (less than 3 seconds), class remains Idle
        let (class, _) = hysteresis.update(WorkloadClass::Interactive, 2, 3);
        assert_eq!(class, WorkloadClass::Idle);

        // Sample at t=3 (sustained 3 seconds), class transitions to Interactive
        let (class, changed) = hysteresis.update(WorkloadClass::Interactive, 3, 3);
        assert_eq!(class, WorkloadClass::Interactive);
        assert!(changed);

        // A single-sample blip at t=4 back to Idle does not immediately change committed class
        let (class, _) = hysteresis.update(WorkloadClass::Idle, 4, 3);
        assert_eq!(class, WorkloadClass::Interactive);
    }

    #[test]
    fn test_n1_t3b_mode_hysteresis_delays_auto_transition() {
        let mut hysteresis = ModeHysteresisState::new(Mode::Balanced);

        let (mode, changed, reason) = hysteresis.update(Mode::Performance, 0, 6, false);
        assert_eq!(mode, Mode::Balanced);
        assert!(!changed);
        assert!(reason.unwrap().contains("delaying transition"));

        let (mode, changed, _) = hysteresis.update(Mode::Performance, 5, 6, false);
        assert_eq!(mode, Mode::Balanced);
        assert!(!changed);

        let (mode, changed, reason) = hysteresis.update(Mode::Performance, 6, 6, false);
        assert_eq!(mode, Mode::Performance);
        assert!(changed);
        assert!(reason.unwrap().contains("committed transition"));
    }

    #[test]
    fn test_n1_t3c_mode_hysteresis_critical_thermal_bypasses_delay() {
        let mut hysteresis = ModeHysteresisState::new(Mode::Performance);

        let (mode, changed, reason) = hysteresis.update(Mode::Balanced, 10, 6, true);
        assert_eq!(mode, Mode::Balanced);
        assert!(changed);
        assert!(reason.unwrap().contains("bypassed for safety"));
    }

    #[test]
    fn test_n1_t3d_mode_hysteresis_reason_is_explainable() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 1,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(4.0),
            cpu_pressure: Some(Pressure {
                avg10: 20.0,
                ..Pressure::default()
            }),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        let decision = policy.decide_resolved(
            &snap,
            Mode::Auto,
            WorkloadClass::Throughput,
            "test".to_string(),
            &Contracts::default(),
            Some(Mode::Balanced),
            Some("mode hysteresis delaying transition: committed=balanced, candidate=performance, elapsed=1s, required=6s".to_string()),
        );
        let report = decision.render(&snap);
        assert!(report.contains("mode=balanced"));
        assert!(report.contains("mode hysteresis delaying transition"));
        assert!(report.contains("candidate=performance"));
    }

    #[test]
    fn test_n1_t4_determinism() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.5),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        let res1 = policy.classify(&snap);
        let res2 = policy.classify(&snap);
        assert_eq!(res1, res2);
    }

    #[test]
    fn test_n1_t5_explainability() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: Some(50_000),
            loadavg_1: Some(5.0),
            cpu_pressure: Some(Pressure {
                avg10: 20.0,
                ..Pressure::default()
            }),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        let (_, reason) = policy.classify(&snap);
        assert!(reason.contains("high load") && reason.contains("high pressure"));
    }

    #[test]
    fn test_n1_t6_absent_foreground() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(1.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        let (class, _) = policy.classify(&snap);
        assert_eq!(class, WorkloadClass::Interactive);
    }

    #[test]
    fn test_n1_t12_low_battery_on_ac_stays_balanced() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: Some(15),
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };

        assert_eq!(policy.auto_mode(&snap), Mode::Balanced);
    }

    #[test]
    fn test_n1_t13_critical_thermal_overrides_cpu_performance() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: Some(95_000),
            loadavg_1: Some(8.0),
            cpu_pressure: Some(Pressure {
                avg10: 50.0,
                ..Pressure::default()
            }),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };

        assert_eq!(policy.auto_mode(&snap), Mode::Balanced);
        let decision = policy.decide(
            &snap,
            Mode::Auto,
            WorkloadClass::Throughput,
            "test".to_string(),
            &Contracts::default(),
        );
        assert_eq!(decision.mode, Mode::Balanced);
        assert!(decision
            .reasons
            .iter()
            .any(|reason| reason.contains("critical thermal pressure")));
        assert!(decision.actions.iter().any(|action| matches!(
            action,
            Action::CpuEpp { value, reason }
                if value == "balance_power" && reason.contains("thermals are critical")
        )));
    }

    #[test]
    fn test_n1_t14_io_pressure_adds_background_io_throttle() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(1.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: Some(Pressure {
                avg10: 9.0,
                ..Pressure::default()
            }),
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };

        let decision = policy.decide(
            &snap,
            Mode::Auto,
            WorkloadClass::Interactive,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(decision.actions.iter().any(|action| matches!(
            action,
            Action::SystemdSetProperty { unit, properties, reason }
                if unit == "background.slice"
                    && properties.iter().any(|p| p == "IOWeight=25")
                    && reason.contains("background I/O")
        )));
    }

    #[test]
    fn test_n1_t15_memory_pressure_protects_user_and_throttles_background() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(1.0),
            cpu_pressure: None,
            memory_pressure: Some(Pressure {
                avg10: 6.0,
                ..Pressure::default()
            }),
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };

        let decision = policy.decide(
            &snap,
            Mode::Auto,
            WorkloadClass::Interactive,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(decision.actions.iter().any(|action| matches!(
            action,
            Action::SystemdSetProperty { unit, properties, .. }
                if unit == "user.slice" && properties.iter().any(|p| p == "MemoryLow=256M")
        )));
        assert!(decision.actions.iter().any(|action| matches!(
            action,
            Action::SystemdSetProperty { unit, properties, .. }
                if unit == "background.slice"
                    && properties.iter().any(|p| p == "MemoryHigh=75%")
                    && properties.iter().any(|p| p == "CPUWeight=50")
                    && properties.iter().any(|p| p == "IOWeight=50")
        )));
    }

    #[test]
    fn test_n1_t16_manual_performance_overrides_auto_battery() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(false),
            battery_pct: Some(10),
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: true,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };

        assert_eq!(policy.auto_mode(&snap), Mode::Battery);
        let decision = policy.decide(
            &snap,
            Mode::Performance,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        assert_eq!(decision.mode, Mode::Performance);
        assert!(decision
            .reasons
            .iter()
            .any(|reason| reason == "manual mode override: performance"));
    }

    #[test]
    fn test_n1_t17_missing_sensors_choose_safe_balanced() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: None,
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: None,
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };

        assert_eq!(policy.auto_mode(&snap), Mode::Balanced);
        assert_eq!(policy.classify(&snap).0, WorkloadClass::Idle);
    }

    struct MockPmqosSink {
        cpu_latency: Option<i32>,
        device_latencies: std::collections::HashMap<PathBuf, String>,
        cpu_fd_open: bool,
        write_count: usize,
    }

    impl MockPmqosSink {
        fn new() -> Self {
            Self {
                cpu_latency: None,
                device_latencies: std::collections::HashMap::new(),
                cpu_fd_open: false,
                write_count: 0,
            }
        }
    }

    impl PmqosSink for MockPmqosSink {
        fn read_cpu_latency(&self) -> io::Result<String> {
            Ok(self
                .cpu_latency
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".to_string()))
        }

        fn write_cpu_latency(&mut self, value: Option<i32>) -> io::Result<()> {
            self.cpu_latency = value;
            self.cpu_fd_open = value.is_some();
            self.write_count += 1;
            Ok(())
        }

        fn read_device_latency(&self, device_path: &Path) -> io::Result<String> {
            if let Some(val) = self.device_latencies.get(device_path) {
                Ok(val.clone())
            } else {
                Ok("0".to_string())
            }
        }

        fn write_device_latency(&mut self, device_path: &Path, value: &str) -> io::Result<()> {
            self.device_latencies
                .insert(device_path.to_path_buf(), value.to_string());
            Ok(())
        }
    }

    #[test]
    fn test_n2_t1_resolution() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t1_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let contracts_path = temp_dir.join("contracts.toml");
        fs::write(
            &contracts_path,
            r#"
[contracts.idle]
cpu_wakeup_latency = 100000
device_resume_latency = 1000000

[contracts.light]
cpu_wakeup_latency = 50000
device_resume_latency = 500000

[contracts.interactive]
cpu_wakeup_latency = 1000
device_resume_latency = 10000

[contracts.latency-critical]
cpu_wakeup_latency = 10
device_resume_latency = 100

[contracts.throughput]
cpu_wakeup_latency = 10000
device_resume_latency = 100000
"#,
        )
        .unwrap();

        let contracts = Contracts::load(&contracts_path);

        assert_eq!(
            contracts.resolve(WorkloadClass::Idle).cpu_wakeup_latency,
            100000
        );
        assert_eq!(
            contracts.resolve(WorkloadClass::Idle).device_resume_latency,
            1000000
        );

        assert_eq!(
            contracts.resolve(WorkloadClass::Light).cpu_wakeup_latency,
            50000
        );
        assert_eq!(
            contracts
                .resolve(WorkloadClass::Light)
                .device_resume_latency,
            500000
        );

        assert_eq!(
            contracts
                .resolve(WorkloadClass::Interactive)
                .cpu_wakeup_latency,
            1000
        );
        assert_eq!(
            contracts
                .resolve(WorkloadClass::Interactive)
                .device_resume_latency,
            10000
        );

        assert_eq!(
            contracts
                .resolve(WorkloadClass::LatencyCritical)
                .cpu_wakeup_latency,
            10
        );
        assert_eq!(
            contracts
                .resolve(WorkloadClass::LatencyCritical)
                .device_resume_latency,
            100
        );

        assert_eq!(
            contracts
                .resolve(WorkloadClass::Throughput)
                .cpu_wakeup_latency,
            10000
        );
        assert_eq!(
            contracts
                .resolve(WorkloadClass::Throughput)
                .device_resume_latency,
            100000
        );
    }

    #[test]
    fn test_n2_t2_dry_run_no_op() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t2_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("policy.toml");
        fs::write(&config_path, "").unwrap();
        fs::write(temp_dir.join("contracts.toml"), "").unwrap();

        let args = Args {
            apply: false, // DRY RUN
            once: true,
            help: false,
            interval_sec: 1,
            state_dir: temp_dir.clone(),
            config_path,
        };

        run(args).unwrap();

        let decisions = fs::read_to_string(temp_dir.join("decisions.log")).unwrap();
        assert!(decisions.contains("cpu_wakeup_latency="));
        assert!(decisions.contains("device_resume_latency="));
        assert!(
            !temp_dir.join("actions.log").exists()
                || fs::read_to_string(temp_dir.join("actions.log"))
                    .unwrap()
                    .is_empty()
        );
    }

    #[test]
    fn test_n2_t3_apply_cpu_floor() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t3_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let mock_sink = Box::new(MockPmqosSink::new());
        let mut actuator = Actuator::new_with_sink(temp_dir.clone(), mock_sink);

        let action = Action::CpuDmaLatency {
            value: Some(1000),
            reason: "test".to_string(),
        };

        actuator.apply(&action).unwrap();

        assert_eq!(actuator.pmqos_sink.read_cpu_latency().unwrap(), "1000");
    }

    #[test]
    fn test_n2_t4_per_device_revert() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t4_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        // Mirror real PCI structure so the structural allowlist check accepts
        // it: parent dir must be `power`, file must be `pm_qos_resume_latency_us`.
        let dev_dir = temp_dir.join("0000:00:1f.3").join("power");
        fs::create_dir_all(&dev_dir).unwrap();
        let dev_path = dev_dir.join("pm_qos_resume_latency_us");

        let mut mock_sink = MockPmqosSink::new();
        mock_sink.write_device_latency(&dev_path, "250").unwrap();
        fs::write(&dev_path, "250").unwrap();

        let mut actuator = Actuator::new_with_sink(temp_dir.clone(), Box::new(mock_sink));

        let action = Action::DeviceResumeLatency {
            path: dev_path.clone(),
            value: Some(100),
            reason: "test".to_string(),
        };

        actuator.apply(&action).unwrap();

        assert_eq!(
            actuator.pmqos_sink.read_device_latency(&dev_path).unwrap(),
            "100"
        );

        let hash = get_path_hash(&dev_path);
        let orig_file = temp_dir.join(format!("original_dev_{hash}"));
        assert!(orig_file.exists());
        let orig_content = fs::read_to_string(&orig_file).unwrap();
        let mut lines = orig_content.lines();
        assert_eq!(lines.next().unwrap(), dev_path.to_str().unwrap());
        assert_eq!(lines.next().unwrap(), "250");

        revert_pm_qos(&temp_dir);

        let current_disk_val = fs::read_to_string(&dev_path).unwrap();
        assert_eq!(current_disk_val.trim(), "250");

        assert!(!orig_file.exists());
        assert!(!temp_dir.join(format!("intended_dev_{hash}")).exists());
    }

    #[test]
    fn test_n2_t5_fd_release() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t5_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        struct DropCheckSink {
            dropped: std::sync::Arc<std::sync::atomic::AtomicBool>,
            cpu_latency: Option<i32>,
        }
        impl PmqosSink for DropCheckSink {
            fn read_cpu_latency(&self) -> io::Result<String> {
                Ok("n/a".to_string())
            }
            fn write_cpu_latency(&mut self, value: Option<i32>) -> io::Result<()> {
                self.cpu_latency = value;
                Ok(())
            }
            fn read_device_latency(&self, _device_path: &Path) -> io::Result<String> {
                Ok("0".to_string())
            }
            fn write_device_latency(
                &mut self,
                _device_path: &Path,
                _value: &str,
            ) -> io::Result<()> {
                Ok(())
            }
        }
        impl Drop for DropCheckSink {
            fn drop(&mut self) {
                self.dropped
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let sink = Box::new(DropCheckSink {
            dropped: dropped.clone(),
            cpu_latency: None,
        });

        let mut actuator = Actuator::new_with_sink(temp_dir.clone(), sink);

        actuator
            .apply(&Action::CpuDmaLatency {
                value: Some(1000),
                reason: "test".to_string(),
            })
            .unwrap();

        std::mem::drop(actuator);

        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_n2_t6_no_thrash() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t6_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let mock_sink = Box::new(MockPmqosSink::new());
        let mut actuator = Actuator::new_with_sink(temp_dir.clone(), mock_sink);

        let action1 = Action::CpuDmaLatency {
            value: Some(1000),
            reason: "test1".to_string(),
        };
        let action2 = Action::CpuDmaLatency {
            value: Some(1000),
            reason: "test2".to_string(),
        };

        actuator.apply(&action1).unwrap();
        actuator.apply(&action2).unwrap();

        let actions_log_path = temp_dir.join("actions.log");
        let logs = fs::read_to_string(&actions_log_path).unwrap_or_default();
        let occurrence_count = logs.matches("write /dev/cpu_dma_latency = 1000").count();
        assert_eq!(occurrence_count, 1);
    }

    #[test]
    fn test_n2_t7_fits_contract() {
        assert!(fits_contract(100, 200));
        assert!(fits_contract(100, 100));
        assert!(!fits_contract(200, 100));
    }

    #[test]
    fn test_n2_t8_explainability() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
        };
        let contracts = Contracts::default();
        let decision = policy.decide(
            &snap,
            Mode::Auto,
            WorkloadClass::Interactive,
            "test".to_string(),
            &contracts,
        );
        let action = decision
            .actions
            .iter()
            .find(|a| matches!(a, Action::CpuDmaLatency { .. }))
            .unwrap();
        if let Action::CpuDmaLatency { reason, .. } = action {
            assert!(reason.contains("class=interactive"));
            assert!(reason.contains("floor=1000us"));
            assert!(reason.contains("row=contracts.interactive"));
        } else {
            panic!("Expected CpuDmaLatency action");
        }
    }

    #[test]
    fn test_n1_t9_global_pin_loop_boundary_precedence() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n1_t9_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("policy.toml");
        fs::write(&config_path, "").unwrap(); // Empty policy to use defaults

        // G2: write a global pin file = "latency-critical" into temp state_dir
        let pin_file = temp_dir.join("workload_class_pin");
        fs::write(&pin_file, "latency-critical").unwrap();

        let args = Args {
            apply: false,
            once: false, // run in background loop
            help: false,
            interval_sec: 1,
            state_dir: temp_dir.clone(),
            config_path,
        };

        let _handle = std::thread::spawn(move || {
            let _ = run(args);
        });

        // Wait 4 seconds for hysteresis to transition (since interval is 1s and dwell is 3s)
        std::thread::sleep(std::time::Duration::from_secs(4));

        // READ BACK state_dir/workload_class and ASSERT == "latency-critical"
        let class_written = fs::read_to_string(temp_dir.join("workload_class")).unwrap();
        assert_eq!(class_written.trim(), "latency-critical");
    }

    #[test]
    fn test_n1_t10_negative_no_global_pin() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n1_t10_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        // G3: no global pin + idle signals => classify() returns signal-derived class.
        // We will construct a snapshot with no global pin and default/idle fields.
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None, // missing pin yields non-None pinned_class should be false
            pm_qos_device_paths: Vec::new(),
        };

        let policy = Policy::default();
        let (class, reason) = policy.classify(&snap);
        assert_eq!(class, WorkloadClass::Idle);
        assert!(reason.contains("system idle"));
    }

    #[test]
    fn test_n1_t11_bad_input_garbage() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n1_t11_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("policy.toml");
        fs::write(&config_path, "").unwrap();

        // G4: global pin file = "garbage" => pin ignored, no panic, fall back to signals.
        let pin_file = temp_dir.join("workload_class_pin");
        fs::write(&pin_file, "garbage").unwrap();

        let args = Args {
            apply: false,
            once: true,
            help: false,
            interval_sec: 1,
            state_dir: temp_dir.clone(),
            config_path,
        };

        // If it runs successfully without panic, that satisfies "no panic, fall back to signals"
        run(args).unwrap();

        // Since it fell back to signals, the workload class written should be "idle"
        let class_written = fs::read_to_string(temp_dir.join("workload_class")).unwrap();
        assert_eq!(class_written.trim(), "idle");
    }
}
