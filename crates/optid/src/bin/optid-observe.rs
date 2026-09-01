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
//!
//! Unlike the other executables it carries no `[[bin]]` section: Cargo
//! discovers `src/bin/*.rs` on its own, and `crates/optid/Cargo.toml` is a
//! declared I2 proof path, so listing it there would stale that package's cold
//! receipt for a read-only diagnostic. It is not installed by
//! `recipes/core/optid.toml` yet, for the same reason and on the same footing
//! as `optid-lever-contracts`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[path = "../runtime_observability.rs"]
mod runtime_observability;

use optid::RealKernel;
use runtime_observability::{RuntimeObservabilityMode, RuntimeObservabilitySnapshot};

/// Same policy file the daemon reads. Only the `[observability.runtime]`
/// fragment is parsed; every other section is ignored.
const DEFAULT_CONFIG_PATH: &str = "/usr/lib/optid/policy.toml";

/// The observer derives deltas from whole-second wall-clock timestamps and
/// refuses to report a delta when no time has passed. Consecutive samples must
/// therefore be at least one second apart, or every sample after the first is
/// correctly reported as stale.
const MINIMUM_INTERVAL_SECONDS: u64 = 1;

fn usage() {
    eprintln!("Usage: optid-observe [--config PATH] [--samples N] [--interval-seconds N]");
    eprintln!(
        "       Reads runtime state and prints a read-only summary. \
         It never writes to the kernel or to optid state."
    );
}

fn main() -> ExitCode {
    let mut config_path = PathBuf::from(DEFAULT_CONFIG_PATH);
    let mut samples: u32 = 1;
    let mut interval_seconds: u64 = MINIMUM_INTERVAL_SECONDS;

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
            "--interval-seconds" => match arguments.next().map(|value| value.parse::<u64>()) {
                Some(Ok(value)) if value >= MINIMUM_INTERVAL_SECONDS => interval_seconds = value,
                _ => {
                    eprintln!(
                        "--interval-seconds requires an integer of at least \
                         {MINIMUM_INTERVAL_SECONDS}"
                    );
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
    for sample in 0..samples {
        // Wait before every sample but the first, so a reported delta covers
        // real elapsed time instead of being suppressed as stale.
        if sample > 0 {
            std::thread::sleep(Duration::from_secs(interval_seconds));
        }
        let snapshot =
            RuntimeObservabilitySnapshot::collect(&kernel, &kernel, mode, previous.as_ref());
        print!("{}", snapshot.render_summary());
        previous = Some(snapshot);
    }

    ExitCode::SUCCESS
}
