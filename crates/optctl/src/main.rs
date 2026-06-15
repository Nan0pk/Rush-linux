use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use zbus::blocking::Connection;
use zbus::dbus_proxy;

#[dbus_proxy(
    interface = "io.rushlinux.Optid1",
    default_service = "io.rushlinux.Optid",
    default_path = "/io/rushlinux/Optid"
)]
trait Optid {
    fn status(&self) -> zbus::Result<String>;
    fn explain(&self) -> zbus::Result<String>;
    fn set_mode(&self, mode: &str) -> zbus::Result<()>;
    fn pin_application(&self, app_id: &str, class: &str) -> zbus::Result<()>;
    #[dbus_proxy(property)]
    fn mode(&self) -> zbus::Result<String>;
    #[dbus_proxy(property)]
    fn version(&self) -> zbus::Result<String>;
}

const DEFAULT_STATE_DIR: &str = "/run/optid";
const MODES: &[&str] = &["auto", "battery", "balanced", "performance", "realtime"];
const CLASSES: &[&str] = &[
    "idle",
    "light",
    "interactive",
    "latency-critical",
    "throughput",
];

fn main() {
    if let Err(err) = run(env::args().skip(1).collect()) {
        eprintln!("optctl: {err}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> io::Result<()> {
    let mut state_dir = PathBuf::from(DEFAULT_STATE_DIR);
    let mut positional = Vec::new();
    let mut json = false;
    let mut it = args.into_iter();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--state-dir" => {
                let value = it.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--state-dir requires a value")
                })?;
                state_dir = PathBuf::from(value);
            }
            "--json" => {
                json = true;
            }
            "-h" | "--help" => {
                print_usage();
                return Ok(());
            }
            _ => positional.push(arg),
        }
    }

    // Try to connect to D-Bus
    let proxy = Connection::system()
        .ok()
        .and_then(|conn| OptidProxyBlocking::new(&conn).ok());

    let command = positional.first().map(String::as_str).unwrap_or("status");
    match command {
        "status" => {
            let status_str = if let Some(ref p) = proxy {
                p.status().ok()
            } else {
                None
            };
            let status_str = match status_str {
                Some(s) => s,
                None => match fs::read_to_string(state_dir.join("status")) {
                    Ok(s) => s,
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {
                        if json {
                            println!("{{\"error\": \"optid has not written status yet\"}}");
                            return Ok(());
                        } else {
                            println!("optid has not written status yet");
                            return Ok(());
                        }
                    }
                    Err(err) => return Err(err),
                },
            };

            if json {
                match format_status_as_json(&status_str) {
                    Ok(json_str) => {
                        println!("{json_str}");
                        Ok(())
                    }
                    Err(e) => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("failed to format status as JSON: {e}"),
                    )),
                }
            } else {
                print!("{status_str}");
                Ok(())
            }
        }
        "explain" => {
            if let Some(ref p) = proxy {
                if let Ok(explanation) = p.explain() {
                    print!("{explanation}");
                    return Ok(());
                }
            }
            print_file_or_hint(
                &state_dir.join("decisions.log"),
                "optid has not written decision history yet",
            )
        }
        "trace" => print_file_or_hint(
            &state_dir.join("actions.log"),
            "optid has not applied actions in this state directory yet",
        ),
        "mode" => {
            let requested = positional.get(1).map(String::as_str);
            if let Some(ref p) = proxy {
                match requested {
                    Some(mode) => {
                        if !MODES.contains(&mode) {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                format!(
                                    "invalid mode {mode}; expected one of {}",
                                    MODES.join(", ")
                                ),
                            ));
                        }
                        if p.set_mode(mode).is_ok() {
                            println!("mode={mode}");
                            return Ok(());
                        }
                    }
                    None => {
                        if let Ok(mode) = p.mode() {
                            println!("mode={mode}");
                            return Ok(());
                        }
                    }
                }
            }
            mode_command(&state_dir, requested)
        }
        "pin" => {
            let app_id = positional.get(1).map(String::as_str);
            let class = positional.get(2).map(String::as_str);
            match (app_id, class) {
                (Some(app_id), Some(class)) => {
                    if !CLASSES.contains(&class) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!(
                                "invalid class {class}; expected one of {}",
                                CLASSES.join(", ")
                            ),
                        ));
                    }
                    if app_id == "--global" {
                        if let Some(ref p) = proxy {
                            if p.pin_application(app_id, class).is_ok() {
                                println!("Pinned global workload class to {class}");
                                return Ok(());
                            }
                        }
                        fs::write(state_dir.join("workload_class_pin"), class)?;
                        println!("Pinned global workload class to {class} (offline)");
                        return Ok(());
                    }
                    if let Some(ref p) = proxy {
                        if p.pin_application(app_id, class).is_ok() {
                            println!("Pinned application {app_id} to class {class}");
                            return Ok(());
                        }
                    }
                    // Offline fallback (writing file directly to state_dir/pins/app_id)
                    let pins_dir = state_dir.join("pins");
                    fs::create_dir_all(&pins_dir)?;
                    fs::write(pins_dir.join(app_id), class)?;
                    println!("Pinned application {app_id} to class {class} (offline)");
                    Ok(())
                }
                _ => {
                    println!("Usage: optctl pin <app_id> <class> or optctl pin --global <class>");
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "pin requires <app_id> and <class> (or --global and <class>)",
                    ))
                }
            }
        }
        "benchmark" => {
            eprintln!("optctl benchmark is removed: use rushbench binary instead.");
            Err(io::Error::other(
                "benchmark command removed (handled by rushbench)",
            ))
        }
        unknown => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown command: {unknown}"),
        )),
    }
}

fn mode_command(state_dir: &Path, requested: Option<&str>) -> io::Result<()> {
    fs::create_dir_all(state_dir)?;

    match requested {
        Some(mode) if MODES.contains(&mode) => {
            fs::write(state_dir.join("mode"), mode)?;
            println!("mode={mode}");
            Ok(())
        }
        Some(mode) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid mode {mode}; expected one of {}", MODES.join(", ")),
        )),
        None => print_file_or_hint(&state_dir.join("mode"), "mode=auto"),
    }
}

fn print_file_or_hint(path: &Path, hint: &str) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(text) => {
            print!("{text}");
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            println!("{hint}");
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn print_usage() {
    println!(
        "Usage: optctl [--state-dir PATH] [--json] <status|explain|mode|pin|trace>\n\
         \n\
         Examples:\n\
           optctl status\n\
           optctl status --json\n\
           optctl mode performance\n\
           optctl explain"
    );
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PressureJson {
    avg10: f32,
    avg60: f32,
    avg300: f32,
    total: u64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct StatusReport {
    timestamp: u64,
    mode: String,
    workload_class: String,
    workload_reason: String,
    cpu_wakeup_latency: Option<i64>,
    device_resume_latency: Option<i64>,
    on_ac: Option<bool>,
    battery_pct: Option<u8>,
    thermal_c: Option<f32>,
    loadavg_1: Option<f32>,
    cpu_pressure: Option<PressureJson>,
    memory_pressure: Option<PressureJson>,
    io_pressure: Option<PressureJson>,
    reasons: Vec<String>,
    actions: Vec<String>,
}

fn format_status_as_json(status_str: &str) -> Result<String, String> {
    let mut report = StatusReport {
        timestamp: 0,
        mode: String::new(),
        workload_class: String::new(),
        workload_reason: String::new(),
        cpu_wakeup_latency: None,
        device_resume_latency: None,
        on_ac: None,
        battery_pct: None,
        thermal_c: None,
        loadavg_1: None,
        cpu_pressure: None,
        memory_pressure: None,
        io_pressure: None,
        reasons: Vec::new(),
        actions: Vec::new(),
    };

    let mut current_section = "";

    for line in status_str.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "reasons:" {
            current_section = "reasons";
            continue;
        }
        if line == "actions:" {
            current_section = "actions";
            continue;
        }

        if current_section == "reasons" {
            if let Some(stripped) = line.strip_prefix("- ") {
                report.reasons.push(stripped.to_string());
            }
            continue;
        }
        if current_section == "actions" {
            if let Some(stripped) = line.strip_prefix("- ") {
                report.actions.push(stripped.to_string());
            }
            continue;
        }

        let (key, val) = match line.split_once('=') {
            Some(pair) => pair,
            None => continue,
        };

        let parse_option_bool = |s: &str| -> Option<bool> {
            if s == "None" {
                None
            } else if s.starts_with("Some(") && s.ends_with(')') {
                s[5..s.len() - 1].parse().ok()
            } else {
                s.parse().ok()
            }
        };

        let parse_option_u8 = |s: &str| -> Option<u8> {
            if s == "None" {
                None
            } else if s.starts_with("Some(") && s.ends_with(')') {
                s[5..s.len() - 1].parse().ok()
            } else {
                s.parse().ok()
            }
        };

        let parse_option_f32 = |s: &str| -> Option<f32> {
            if s == "None" {
                None
            } else if s.starts_with("Some(") && s.ends_with(')') {
                s[5..s.len() - 1].parse().ok()
            } else {
                s.parse().ok()
            }
        };

        let parse_pressure = |s: &str| -> Option<PressureJson> {
            if s == "unavailable" {
                return None;
            }
            let mut avg10 = 0.0;
            let mut avg60 = 0.0;
            let mut avg300 = 0.0;
            let mut total = 0;
            for token in s.split_whitespace() {
                if let Some((k, v)) = token.split_once('=') {
                    match k {
                        "avg10" => avg10 = v.parse().unwrap_or(0.0),
                        "avg60" => avg60 = v.parse().unwrap_or(0.0),
                        "avg300" => avg300 = v.parse().unwrap_or(0.0),
                        "total" => total = v.parse().unwrap_or(0),
                        _ => {}
                    }
                }
            }
            Some(PressureJson {
                avg10,
                avg60,
                avg300,
                total,
            })
        };

        match key {
            "timestamp" => report.timestamp = val.parse().unwrap_or(0),
            "mode" => report.mode = val.to_string(),
            "workload_class" => report.workload_class = val.to_string(),
            "workload_reason" => report.workload_reason = val.to_string(),
            "cpu_wakeup_latency" => report.cpu_wakeup_latency = val.parse().ok(),
            "device_resume_latency" => report.device_resume_latency = val.parse().ok(),
            "on_ac" => report.on_ac = parse_option_bool(val),
            "battery_pct" => report.battery_pct = parse_option_u8(val),
            "thermal_c" => report.thermal_c = parse_option_f32(val),
            "loadavg_1" => report.loadavg_1 = parse_option_f32(val),
            "cpu_pressure" => report.cpu_pressure = parse_pressure(val),
            "memory_pressure" => report.memory_pressure = parse_pressure(val),
            "io_pressure" => report.io_pressure = parse_pressure(val),
            _ => {}
        }
    }

    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_status_as_json_all_values() {
        let input = "\
timestamp=1717500000
mode=balanced
workload_class=interactive
workload_reason=pinned override for foreground app
cpu_wakeup_latency=1000
device_resume_latency=10000
on_ac=Some(true)
battery_pct=Some(95)
thermal_c=Some(45.5)
loadavg_1=Some(0.45)
cpu_pressure=avg10=0.01 avg60=0.02 avg300=0.03 total=42
memory_pressure=avg10=0.04 avg60=0.05 avg300=0.06 total=84
io_pressure=avg10=0.07 avg60=0.08 avg300=0.09 total=126
reasons:
- reason 1
- reason 2
actions:
- action 1
- action 2
";
        let result = format_status_as_json(input).unwrap();
        assert!(result.contains("\"timestamp\": 1717500000"));
        assert!(result.contains("\"mode\": \"balanced\""));
        assert!(result.contains("\"workload_class\": \"interactive\""));
        assert!(result.contains("\"workload_reason\": \"pinned override for foreground app\""));
        assert!(result.contains("\"cpu_wakeup_latency\": 1000"));
        assert!(result.contains("\"device_resume_latency\": 10000"));
        assert!(result.contains("\"on_ac\": true"));
        assert!(result.contains("\"battery_pct\": 95"));
        assert!(result.contains("\"thermal_c\": 45.5"));
        assert!(result.contains("\"loadavg_1\": 0.45"));
        assert!(result.contains(
            "\"cpu_pressure\": {\n    \"avg10\": 0.01,\n    \"avg60\": 0.02,\n    \"avg300\": 0.03,\n    \"total\": 42\n  }"
        ));
        assert!(result.contains(
            "\"memory_pressure\": {\n    \"avg10\": 0.04,\n    \"avg60\": 0.05,\n    \"avg300\": 0.06,\n    \"total\": 84\n  }"
        ));
        assert!(result.contains(
            "\"io_pressure\": {\n    \"avg10\": 0.07,\n    \"avg60\": 0.08,\n    \"avg300\": 0.09,\n    \"total\": 126\n  }"
        ));
        assert!(result.contains("\"reasons\": [\n    \"reason 1\",\n    \"reason 2\"\n  ]"));
        assert!(result.contains("\"actions\": [\n    \"action 1\",\n    \"action 2\"\n  ]"));
    }

    #[test]
    fn test_format_status_as_json_none_values() {
        let input = "\
timestamp=1717500000
mode=battery
on_ac=None
battery_pct=None
thermal_c=None
loadavg_1=None
cpu_pressure=unavailable
memory_pressure=unavailable
io_pressure=unavailable
reasons:
actions:
";
        let result = format_status_as_json(input).unwrap();
        assert!(result.contains("\"on_ac\": null"));
        assert!(result.contains("\"battery_pct\": null"));
        assert!(result.contains("\"thermal_c\": null"));
        assert!(result.contains("\"loadavg_1\": null"));
        assert!(result.contains("\"cpu_pressure\": null"));
        assert!(result.contains("\"cpu_wakeup_latency\": null"));
        assert!(result.contains("\"device_resume_latency\": null"));
        assert!(result.contains("\"reasons\": []"));
        assert!(result.contains("\"actions\": []"));
    }
}
