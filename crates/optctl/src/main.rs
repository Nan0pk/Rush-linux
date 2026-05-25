use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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

    let command = positional.first().map(String::as_str).unwrap_or("status");
    match command {
        "status" => print_file_or_hint(&state_dir.join("status"), "optid has not written status yet"),
        "explain" => print_file_or_hint(
            &state_dir.join("decisions.log"),
            "optid has not written decision history yet",
        ),
        "trace" => print_file_or_hint(
            &state_dir.join("actions.log"),
            "optid has not applied actions in this state directory yet",
        ),
        "mode" => mode_command(&state_dir, positional.get(1).map(String::as_str)),
        "pin" => {
            println!("pin support is reserved for the D-Bus/session helper implementation");
            Ok(())
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

