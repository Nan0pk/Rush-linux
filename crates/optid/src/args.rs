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

/// v0.6 Phase C1: foreground-detection mode. `Off` (default in v0.6)
/// disables the foreground subscriber entirely. `Auto` spawns the
/// subscriber thread — in v0.6 this is a stub that never yields events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForegroundMode {
    Off,
    Auto,
}

#[derive(Debug, Clone)]
pub(crate) struct Args {
    pub(crate) apply: bool,
    pub(crate) once: bool,
    pub(crate) help: bool,
    pub(crate) interval_sec: u64,
    pub(crate) state_dir: PathBuf,
    pub(crate) config_path: PathBuf,
    /// WP-N4 hardware allowlist gate. Default `true` (enabled) as of v0.6
    /// Phase A3.
    pub(crate) allowlist: bool,
    /// v0.6 Phase C1: foreground-detection mode. Default `Off` (v0.6).
    pub(crate) foreground: ForegroundMode,
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
            // v0.6 Phase A3: default-on per docs/research/0006 §7.
            allowlist: true,
            // v0.6 Phase C1: foreground detection default-off.
            foreground: ForegroundMode::Off,
        };

        let mut it = iter.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--apply" => args.apply = true,
                "--once" => args.once = true,
                "-h" | "--help" => args.help = true,
                "--allowlist" => args.allowlist = true,
                "--allowlist=enabled" => args.allowlist = true,
                "--allowlist=disabled" => args.allowlist = false,
                "--no-allowlist" => args.allowlist = false,
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
                // v0.6 Phase C1: foreground-detection mode.
                "--foreground" => {
                    let value = it
                        .next()
                        .ok_or_else(|| "--foreground requires a value (off|auto)".to_string())?;
                    args.foreground = match value.as_str() {
                        "off" => ForegroundMode::Off,
                        "auto" => ForegroundMode::Auto,
                        _ => {
                            return Err(format!(
                                "invalid --foreground value: {value} (expected off|auto)"
                            ));
                        }
                    };
                }
                "--foreground=off" => args.foreground = ForegroundMode::Off,
                "--foreground=auto" => args.foreground = ForegroundMode::Auto,
                other if other.starts_with("--foreground=") => {
                    let value = other.strip_prefix("--foreground=").unwrap();
                    return Err(format!(
                        "invalid --foreground value: {value} (expected off|auto)"
                    ));
                }
                unknown => return Err(format!("unknown argument: {unknown}")),
            }
        }

        if !args.allowlist && !args.once {
            return Err(
                "disabling the hardware allowlist is limited to a single experimental run; \
                 combine --no-allowlist with --once"
                    .to_string(),
            );
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
         \x20            [--allowlist[=enabled|disabled]] [--no-allowlist]\n\
         \x20            [--foreground=off|auto]\n\
         \n\
         Default mode is dry-run. Use --apply only on Rush Linux or a test host.\n\
         The WP-N4 hardware allowlist gate is ENABLED by default (v0.6 Phase A3).\n\
         --no-allowlist is an experimental escape hatch accepted only with\n\
         --once, so the gate cannot remain disabled indefinitely.\n\
         --foreground=auto enables foreground-app detection (v0.6 stub — real\n\
         compositor integration lands in v0.7). Default is off."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // v0.6 Phase A3: the allowlist gate default flipped from disabled to
    // enabled. These tests pin the new default so a future refactor cannot
    // silently regress it. If the default ever flips back, these tests fail
    // and the contributor must update both the default in `Args::parse` AND
    // the research-0006 §7 marker that says "Done in v0.6 Phase A3".

    #[test]
    fn allowlist_default_is_enabled() {
        let args = Args::parse(std::iter::empty::<String>()).unwrap();
        assert!(
            args.allowlist,
            "v0.6 Phase A3: --allowlist must default to true (was {})",
            args.allowlist
        );
    }

    #[test]
    fn no_allowlist_requires_single_run() {
        let err = Args::parse(["--no-allowlist".to_string()]).unwrap_err();
        assert!(err.contains("--once"));
    }

    #[test]
    fn allowlist_enabled_form_is_idempotent() {
        // The bare `--allowlist` and `--allowlist=enabled` forms still work
        // (backward-compat with scripts that explicitly enable the gate).
        let args = Args::parse(["--allowlist".to_string()]).unwrap();
        assert!(args.allowlist);
        let args = Args::parse(["--allowlist=enabled".to_string()]).unwrap();
        assert!(args.allowlist);
    }

    #[test]
    fn allowlist_disabled_form_also_requires_single_run() {
        let err = Args::parse(["--allowlist=disabled".to_string()]).unwrap_err();
        assert!(err.contains("--once"));
    }

    #[test]
    fn no_allowlist_can_be_combined_with_other_flags() {
        // Smoke test: --no-allowlist composes with --apply and --once.
        let args = Args::parse([
            "--apply".to_string(),
            "--once".to_string(),
            "--no-allowlist".to_string(),
        ])
        .unwrap();
        assert!(args.apply);
        assert!(args.once);
        assert!(!args.allowlist);
    }

    // v0.6 Phase C1: foreground-detection flag tests.

    #[test]
    fn foreground_default_is_off() {
        let args = Args::parse(std::iter::empty::<String>()).unwrap();
        assert_eq!(args.foreground, ForegroundMode::Off);
    }

    #[test]
    fn foreground_auto_enables_detection() {
        let args = Args::parse(["--foreground=auto".to_string()]).unwrap();
        assert_eq!(args.foreground, ForegroundMode::Auto);
    }

    #[test]
    fn foreground_off_disables_detection() {
        let args = Args::parse(["--foreground=off".to_string()]).unwrap();
        assert_eq!(args.foreground, ForegroundMode::Off);
    }

    #[test]
    fn foreground_space_separated_value() {
        let args = Args::parse(["--foreground".to_string(), "auto".to_string()]).unwrap();
        assert_eq!(args.foreground, ForegroundMode::Auto);
    }

    #[test]
    fn foreground_invalid_value_rejected() {
        let err = Args::parse(["--foreground=on".to_string()]).unwrap_err();
        assert!(
            err.contains("on"),
            "error should mention the bad value: {err}"
        );
        assert!(
            err.contains("off|auto"),
            "error should list valid values: {err}"
        );
    }

    #[test]
    fn foreground_missing_value_rejected() {
        let err = Args::parse(["--foreground".to_string()]).unwrap_err();
        assert!(
            err.contains("requires a value"),
            "error should explain: {err}"
        );
    }

    #[test]
    fn foreground_auto_composes_with_other_flags() {
        let args = Args::parse([
            "--apply".to_string(),
            "--once".to_string(),
            "--foreground=auto".to_string(),
        ])
        .unwrap();
        assert!(args.apply);
        assert!(args.once);
        assert_eq!(args.foreground, ForegroundMode::Auto);
    }
}
