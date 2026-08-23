use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use zbus::blocking::Connection;
use zbus::proxy;

mod allow;

#[proxy(
    interface = "io.rushlinux.Optid1",
    default_service = "io.rushlinux.Optid",
    default_path = "/io/rushlinux/Optid"
)]
trait Optid {
    fn status(&self) -> zbus::Result<String>;
    fn status_json(&self) -> zbus::Result<String>;
    fn explain(&self) -> zbus::Result<String>;
    fn circuits(&self) -> zbus::Result<String>;
    fn set_mode(&self, mode: &str) -> zbus::Result<()>;
    fn pin_application(&self, app_id: &str, class: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn mode(&self) -> zbus::Result<String>;
    #[zbus(property)]
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
            if json {
                let status_json = if let Some(ref proxy) = proxy {
                    proxy.status_json().ok()
                } else {
                    None
                }
                .map(Ok)
                .unwrap_or_else(|| read_status_json_file(&state_dir))?;
                let validated = validate_daemon_status_json(&status_json)?;
                println!("{}", validated.trim_end());
                return Ok(());
            }

            let status = if let Some(ref proxy) = proxy {
                proxy.status().ok()
            } else {
                None
            };
            match status {
                Some(status) => print!("{status}"),
                None => match fs::read_to_string(state_dir.join("status")) {
                    Ok(status) => print!("{status}"),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        println!("optid has not written status yet");
                    }
                    Err(error) => return Err(error),
                },
            }
            Ok(())
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
        "circuits" => {
            if let Some(ref p) = proxy {
                if let Ok(circuits) = p.circuits() {
                    print!("{circuits}");
                    return Ok(());
                }
            }
            // No D-Bus connection: this crate delegates rendering to optid
            // (CLAUDE.md "no business logic here"), so without the daemon to
            // ask, the raw persisted record is the best available answer.
            println!("no D-Bus connection to optid; showing the raw persisted record instead:");
            print_file_or_hint(
                &circuit_state_path(&state_dir),
                "no circuit records; nothing has ever tripped",
            )
        }
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
        "allow" | "deny" | "list-allow" => allow::run(&positional),
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

/// Where optid persists circuit-breaker state for a given `--state-dir`.
/// Mirrors `optid::circuit_breaker::CircuitBreaker::state_path_for` -- kept in
/// sync by hand, since optctl does not depend on the `optid` binary crate.
/// Only reached when no D-Bus connection is available.
fn circuit_state_path(state_dir: &Path) -> PathBuf {
    if state_dir == Path::new(DEFAULT_STATE_DIR) {
        PathBuf::from("/var/lib/optid/circuits-v1.json")
    } else {
        state_dir.join("persistent-circuits-v1.json")
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
        "Usage: optctl [--state-dir PATH] [--json] <status|explain|circuits|mode|pin|trace|allow|deny|list-allow>\n\
         \n\
         Examples:\n\
           optctl status\n\
           optctl status --json\n\
           optctl mode performance\n\
           optctl explain\n\
           optctl circuits\n\
           optctl allow nvme_apst pci:v0000144Dp00009A36 --max-state 3 --reason \"tested on T14\"\n\
           optctl deny pci_aspm /sys/bus/pci/devices/0000:04:00.0 --reason \"L1.2 link drop\"\n\
           optctl list-allow"
    );
}

fn read_status_json_file(state_dir: &Path) -> io::Result<String> {
    fs::read_to_string(state_dir.join("status.json")).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::new(
                io::ErrorKind::NotFound,
                "optid has not written status.json; refusing to reconstruct machine status from text",
            )
        } else {
            error
        }
    })
}

fn validate_daemon_status_json(status_json: &str) -> io::Result<String> {
    let value: serde_json::Value = serde_json::from_str(status_json).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("malformed daemon status.json: {error}"),
        )
    })?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "daemon status.json is missing numeric schema_version",
            )
        })?;
    if schema_version == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon status.json has invalid schema_version 0",
        ));
    }
    let correlation_id = value
        .get("correlation_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "daemon status.json is missing correlation_id",
            )
        })?;
    let _ = correlation_id;
    Ok(status_json.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_state_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("optctl-{name}-{nonce}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn sample_json() -> String {
        r#"{"schema_version":2,"correlation_id":"cycle-1","future_field":true}"#.to_string()
    }

    #[test]
    fn circuit_state_path_uses_the_production_path_for_the_default_state_dir() {
        assert_eq!(
            circuit_state_path(Path::new(DEFAULT_STATE_DIR)),
            PathBuf::from("/var/lib/optid/circuits-v1.json")
        );
    }

    #[test]
    fn circuit_state_path_keeps_a_non_production_state_dir_self_contained() {
        assert_eq!(
            circuit_state_path(Path::new("/tmp/optid-test-state")),
            PathBuf::from("/tmp/optid-test-state/persistent-circuits-v1.json")
        );
    }

    #[test]
    fn f3_status_json_passes_through_daemon_schema() {
        let input = sample_json();
        assert_eq!(validate_daemon_status_json(&input).unwrap(), input);
    }

    #[test]
    fn f3_status_json_tolerates_unknown_fields() {
        assert!(validate_daemon_status_json(&sample_json()).is_ok());
    }

    #[test]
    fn f3_malformed_status_json_fails_clearly() {
        let error = validate_daemon_status_json("{not-json").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("malformed daemon status.json"));
    }

    #[test]
    fn f3_missing_json_never_reconstructs_from_text() {
        let state_dir = temp_state_dir("missing-json");
        fs::write(state_dir.join("status"), "mode=balanced\n").unwrap();
        let error = read_status_json_file(&state_dir).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("refusing to reconstruct"));
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn f3_offline_status_json_reads_daemon_file_without_mutation() {
        let state_dir = temp_state_dir("offline-json");
        let input = sample_json();
        fs::write(state_dir.join("status.json"), &input).unwrap();
        let before: Vec<_> = fs::read_dir(&state_dir).unwrap().collect();
        let read = read_status_json_file(&state_dir).unwrap();
        let after: Vec<_> = fs::read_dir(&state_dir).unwrap().collect();
        assert_eq!(validate_daemon_status_json(&read).unwrap(), input);
        assert_eq!(before.len(), after.len());
        fs::remove_dir_all(state_dir).unwrap();
    }
}
