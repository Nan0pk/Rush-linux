//! `optid-observe` — O1 read-only runtime-state reporter.
//!
//! This binary is the production surface for O1. It is deliberately separate
//! from the `optid` daemon: observability reads stable kernel interfaces and
//! reports, while the control loop decides and actuates. Keeping the two apart
//! means a new diagnostic never edits a control-loop or safety file, so it
//! cannot invalidate the cold-verification receipts those packages depend on.
//!
//! The reporter owns no actuator, opens no write path, and never touches the
//! daemon's state directory. It reads the O1 policy fragment for its mode,
//! samples the runtime state, and prints the summary to standard output.

use std::path::PathBuf;
use std::process::ExitCode;

#[path = "../runtime_observability.rs"]
mod runtime_observability;

use optid::RealKernel;
use runtime_observability::{RuntimeObservabilityMode, RuntimeObservabilitySnapshot};

/// Same policy file the daemon reads. Only the `[observability.runtime]`
/// fragment is parsed; every other section is ignored.
const DEFAULT_CONFIG_PATH: &str = "/usr/lib/optid/policy.toml";

fn usage() {
    eprintln!("Usage: optid-observe [--config PATH] [--samples N]");
    eprintln!(
        "       Reads runtime state and prints a read-only summary. \
         It never writes to the kernel or to optid state."
    );
}

fn main() -> ExitCode {
    let mut config_path = PathBuf::from(DEFAULT_CONFIG_PATH);
    let mut samples: u32 = 1;

    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--config" => match arguments.next() {
                Some(value) => config_path = PathBuf::from(value),
                None => {
                    eprintln!("--config requires a value");
                    usage();
                    return ExitCode::FAILURE;
                }
            },
            // Two or more samples let an operator see real counter deltas
            // without the reporter holding any long-lived state. Sampling is
            // still read-only; only the previous snapshot is retained.
            "--samples" => match arguments.next().map(|value| value.parse::<u32>()) {
                Some(Ok(value)) if value >= 1 => samples = value,
                _ => {
                    eprintln!("--samples requires a positive integer");
                    usage();
                    return ExitCode::FAILURE;
                }
            },
            "--help" | "-h" => {
                usage();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                usage();
                return ExitCode::FAILURE;
            }
        }
    }

    let kernel = RealKernel::new();
    let mode = RuntimeObservabilityMode::from_policy_file(&kernel, &config_path);

    let mut previous: Option<RuntimeObservabilitySnapshot> = None;
    for _ in 0..samples {
        let snapshot =
            RuntimeObservabilitySnapshot::collect(&kernel, &kernel, mode, previous.as_ref());
        print!("{}", snapshot.render_summary());
        previous = Some(snapshot);
    }

    ExitCode::SUCCESS
}
