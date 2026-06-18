//! CLI argument parsing for `optid`.
//!
//! Separated from `main.rs` so the parsing logic and the run loop can evolve
//! independently. The `Args` struct is the single source of truth for runtime
//! configuration that comes from the command line.

use std::env;
use std::path::PathBuf;

pub(crate) const DEFAULT_STATE_DIR: &str = "/run/optid";
pub(crate) const DEFAULT_CONFIG_PATH: &str = "/usr/lib/optid/policy.toml";
pub(crate) const DEFAULT_INTERVAL_SEC: u64 = 2;

pub(crate) const DEFAULT_DWELL_WINDOW_SEC: u64 = 3;
pub(crate) const DEFAULT_MODE_DWELL_WINDOW_SEC: u64 = DEFAULT_INTERVAL_SEC * 3;

#[derive(Debug, Clone)]
pub(crate) struct Args {
    pub(crate) apply: bool,
    pub(crate) once: bool,
    pub(crate) help: bool,
    pub(crate) interval_sec: u64,
    pub(crate) state_dir: PathBuf,
    pub(crate) config_path: PathBuf,
}

impl Args {
    pub(crate) fn parse<I>(iter: I) -> Result<Self, String>
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

pub(crate) fn parse_from_env() -> Result<Args, String> {
    Args::parse(env::args().skip(1))
}

pub(crate) fn print_usage() {
    println!(
        "Usage: optid [--apply] [--once] [--interval-sec N] [--state-dir PATH] [--config PATH]\n\
         \n\
         Default mode is dry-run. Use --apply only on Rush Linux or a test host."
    );
}
