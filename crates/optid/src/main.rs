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
const DEFAULT_INTERVAL_SEC: u64 = 2;

struct OptidServer {
    state_dir: PathBuf,
}

#[dbus_interface(name = "io.adaptive.Optid1")]
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

    let state_dir_clone = args.state_dir.clone();
    thread::spawn(move || {
        let server = OptidServer {
            state_dir: state_dir_clone,
        };
        let run_server = || -> zbus::Result<()> {
            let _conn = ConnectionBuilder::system()?
                .name("io.adaptive.Optid")?
                .serve_at("/io/adaptive/Optid", server)?
                .build()?;
            println!("D-Bus server running on system bus at /io/adaptive/Optid");
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
        let decision = Policy::default().decide(&snapshot, override_mode);
        let report = decision.render(&snapshot);

        fs::write(args.state_dir.join("status"), &report)?;
        append_log(&args.state_dir.join("decisions.log"), &report)?;

        if args.apply {
            let mut actuator = Actuator::new(args.state_dir.join("actions.log"));
            for action in &decision.actions {
                actuator.apply(action)?;
            }
        }

        if args.once {
            break;
        }

        thread::sleep(Duration::from_secs(args.interval_sec));
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct Args {
    apply: bool,
    once: bool,
    help: bool,
    interval_sec: u64,
    state_dir: PathBuf,
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
                unknown => return Err(format!("unknown argument: {unknown}")),
            }
        }

        Ok(args)
    }
}

fn print_usage() {
    println!(
        "Usage: optid [--apply] [--once] [--interval-sec N] [--state-dir PATH]\n\
         \n\
         Default mode is dry-run. Use --apply only on Adaptive Linux or a test host."
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
        }
    }

    fn thermal_c(&self) -> Option<f32> {
        self.max_temp_millic.map(|temp| temp as f32 / 1000.0)
    }
}

#[derive(Debug, Clone)]
struct Policy {
    cpu_pressure_perf_threshold: f32,
    memory_pressure_protect_threshold: f32,
    io_pressure_throttle_threshold: f32,
    hot_temp_c: f32,
    critical_temp_c: f32,
    low_battery_pct: u8,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            cpu_pressure_perf_threshold: 12.0,
            memory_pressure_protect_threshold: 5.0,
            io_pressure_throttle_threshold: 8.0,
            hot_temp_c: 82.0,
            critical_temp_c: 92.0,
            low_battery_pct: 20,
        }
    }
}

impl Policy {
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
            if pct <= self.low_battery_pct {
                reasons.push(format!("battery is low: {pct}%"));
            }
        }

        if let Some(temp) = snapshot.thermal_c() {
            if temp >= self.critical_temp_c {
                reasons.push(format!("critical thermal pressure: {temp:.1}C"));
            } else if temp >= self.hot_temp_c {
                reasons.push(format!("high thermal pressure: {temp:.1}C"));
            }
        }

        if let Some(cpu) = snapshot.cpu_pressure {
            if cpu.avg10 >= self.cpu_pressure_perf_threshold {
                reasons.push(format!("CPU pressure avg10 is {:.2}", cpu.avg10));
            }
        }

        if let Some(memory) = snapshot.memory_pressure {
            if memory.avg10 >= self.memory_pressure_protect_threshold {
                reasons.push(format!("memory pressure avg10 is {:.2}", memory.avg10));
                actions.push(Action::systemd_set_property(
                    "user.slice",
                    &["MemoryLow=256M"],
                    "protect active user sessions from reclaim pressure",
                ));
                actions.push(Action::systemd_set_property(
                    "background.slice",
                    &["CPUWeight=50", "IOWeight=50", "MemoryHigh=75%"],
                    "throttle background work during memory pressure",
                ));
            }
        }

        if let Some(io) = snapshot.io_pressure {
            if io.avg10 >= self.io_pressure_throttle_threshold {
                reasons.push(format!("I/O pressure avg10 is {:.2}", io.avg10));
                actions.push(Action::systemd_set_property(
                    "background.slice",
                    &["IOWeight=25"],
                    "reduce background I/O interference",
                ));
            }
        }

        match effective_mode {
            Mode::Battery => {
                actions.push(Action::cpu_epp(
                    "power",
                    "prefer battery life through CPU energy preference",
                ));
                actions.push(Action::platform_profile(
                    "low-power",
                    "request low-power platform profile",
                ));
                actions.push(Action::systemd_set_property(
                    "background.slice",
                    &["CPUWeight=25", "IOWeight=25"],
                    "deprioritize background services on battery",
                ));
            }
            Mode::Balanced => {
                actions.push(Action::cpu_epp(
                    "balance_performance",
                    "keep foreground responsiveness without full turbo bias",
                ));
                actions.push(Action::platform_profile(
                    "balanced",
                    "request balanced platform profile",
                ));
                actions.push(Action::systemd_set_property(
                    "user.slice",
                    &["CPUWeight=150", "IOWeight=150"],
                    "favor interactive user sessions",
                ));
            }
            Mode::Performance => {
                actions.push(Action::cpu_epp(
                    "performance",
                    "reduce CPU wakeup and ramp latency for sustained load",
                ));
                actions.push(Action::platform_profile(
                    "performance",
                    "request performance platform profile",
                ));
                actions.push(Action::systemd_set_property(
                    "user.slice",
                    &["CPUWeight=200", "IOWeight=200"],
                    "boost foreground user work",
                ));
            }
            Mode::Realtime => {
                actions.push(Action::cpu_epp(
                    "performance",
                    "minimize latency for realtime mode",
                ));
                actions.push(Action::platform_profile(
                    "performance",
                    "avoid firmware power-save latency in realtime mode",
                ));
                actions.push(Action::systemd_set_property(
                    "user.slice",
                    &["CPUWeight=250", "IOWeight=200"],
                    "prioritize controlled realtime user workload",
                ));
            }
            Mode::Auto => unreachable!("auto is resolved before action planning"),
        }

        if snapshot
            .thermal_c()
            .is_some_and(|temp| temp >= self.critical_temp_c)
        {
            actions.push(Action::cpu_epp(
                "balance_power",
                "override performance bias because thermals are critical",
            ));
            actions.push(Action::platform_profile(
                "balanced",
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
            .is_some_and(|temp| temp >= self.critical_temp_c)
        {
            return Mode::Balanced;
        }

        if snapshot.on_ac == Some(false) {
            if snapshot
                .battery_pct
                .is_some_and(|pct| pct <= self.low_battery_pct)
            {
                return Mode::Battery;
            }

            if snapshot
                .cpu_pressure
                .is_some_and(|p| p.avg10 >= self.cpu_pressure_perf_threshold)
            {
                return Mode::Balanced;
            }

            return Mode::Battery;
        }

        if snapshot
            .cpu_pressure
            .is_some_and(|p| p.avg10 >= self.cpu_pressure_perf_threshold)
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
        value: &'static str,
        reason: &'static str,
    },
    PlatformProfile {
        value: &'static str,
        reason: &'static str,
    },
    SystemdSetProperty {
        unit: &'static str,
        properties: Vec<String>,
        reason: &'static str,
    },
}

impl Action {
    fn cpu_epp(value: &'static str, reason: &'static str) -> Self {
        Self::CpuEpp { value, reason }
    }

    fn platform_profile(value: &'static str, reason: &'static str) -> Self {
        Self::PlatformProfile { value, reason }
    }

    fn systemd_set_property(
        unit: &'static str,
        properties: &[&'static str],
        reason: &'static str,
    ) -> Self {
        Self::SystemdSetProperty {
            unit,
            properties: properties.iter().map(|p| p.to_string()).collect(),
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
        }
    }
}

struct Actuator {
    log_path: PathBuf,
}

impl Actuator {
    fn new(log_path: PathBuf) -> Self {
        Self { log_path }
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
                    guarded_write(&path, value)?;
                    self.log(&format!("write {} = {value}", path.display()))?;
                }
            }
            Action::PlatformProfile { value, .. } => {
                let path = Path::new("/sys/firmware/acpi/platform_profile");
                if path.exists() {
                    guarded_write(path, value)?;
                    self.log(&format!("write {} = {value}", path.display()))?;
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
        }
        Ok(())
    }

    fn log(&mut self, message: &str) -> io::Result<()> {
        append_log(&self.log_path, &format!("{} {message}\n", now_unix()))
    }
}

fn guarded_write(path: &Path, value: &str) -> io::Result<()> {
    let allowed = path == Path::new("/sys/firmware/acpi/platform_profile")
        || path.starts_with("/sys/devices/system/cpu/");

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
        };

        let decision = Policy::default().decide(&snapshot, Mode::Performance);
        assert!(decision.actions.iter().any(|action| matches!(
            action,
            Action::CpuEpp {
                value: "balance_power",
                ..
            }
        )));
    }
}
