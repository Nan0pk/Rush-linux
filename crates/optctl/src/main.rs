use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use zbus::blocking::Connection;
use zbus::dbus_proxy;

#[dbus_proxy(
    interface = "io.adaptive.Optid1",
    default_service = "io.adaptive.Optid",
    default_path = "/io/adaptive/Optid"
)]
trait Optid {
    fn status(&self) -> zbus::Result<String>;
    fn explain(&self) -> zbus::Result<String>;
    fn set_mode(&self, mode: &str) -> zbus::Result<()>;
    fn pin_application(&self, app_id: &str, mode: &str) -> zbus::Result<()>;
    #[dbus_proxy(property)]
    fn mode(&self) -> zbus::Result<String>;
    #[dbus_proxy(property)]
    fn version(&self) -> zbus::Result<String>;
}

const DEFAULT_STATE_DIR: &str = "/run/optid";
const MODES: &[&str] = &["auto", "battery", "balanced", "performance", "realtime"];

fn main() {
    if let Err(err) = run(env::args().skip(1).collect()) {
        eprintln!("optctl: {err}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> io::Result<()> {
    let mut state_dir = PathBuf::from(DEFAULT_STATE_DIR);
    let mut positional = Vec::new();
    let mut it = args.into_iter();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--state-dir" => {
                let value = it.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--state-dir requires a value")
                })?;
                state_dir = PathBuf::from(value);
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
            if let Some(ref p) = proxy {
                if let Ok(status) = p.status() {
                    print!("{status}");
                    return Ok(());
                }
            }
            print_file_or_hint(
                &state_dir.join("status"),
                "optid has not written status yet",
            )
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
            let mode = positional.get(2).map(String::as_str);
            match (app_id, mode) {
                (Some(app_id), Some(mode)) => {
                    if !MODES.contains(&mode) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("invalid mode {mode}; expected one of {}", MODES.join(", ")),
                        ));
                    }
                    if let Some(ref p) = proxy {
                        if p.pin_application(app_id, mode).is_ok() {
                            println!("Pinned application {app_id} to mode {mode}");
                            return Ok(());
                        }
                    }
                    println!("pin support failed or D-Bus offline");
                    Ok(())
                }
                _ => {
                    println!("Usage: optctl pin <app_id> <mode>");
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "pin requires <app_id> and <mode>",
                    ))
                }
            }
        }
        "benchmark" => {
            println!("benchmark suite placeholder:");
            println!("- mixed-load responsiveness: browser + build + fio");
            println!("- battery: idle, video playback, video call, suspend/resume");
            println!("- realtime: cyclictest/oslat + PipeWire underruns");
            println!("- server: PostgreSQL, nginx, containers, fio, iperf3");
            Ok(())
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
        "Usage: optctl [--state-dir PATH] <status|explain|mode|pin|trace|benchmark>\n\
         \n\
         Examples:\n\
           optctl status\n\
           optctl mode performance\n\
           optctl explain"
    );
}
