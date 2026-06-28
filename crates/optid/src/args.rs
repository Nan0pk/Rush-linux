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
    /// WP-N4 hardware allowlist gate. Default `true` (enabled) as of v0.6
    /// Phase A3 — see docs/research/0006-hw-allowlist-db-design.md §7.
    /// When `true`, depth-enabler writes are default-denied unless the
    /// device's HWID is allowlisted, and every denial is audited. The
    /// `--no-allowlist` flag is an emergency escape hatch for bring-up on
    /// hardware the seeded baseline does not yet cover; it must not be the
    /// default in any released build.
    pub(crate) allowlist: bool,
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
        };

        let mut it = iter.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--apply" => args.apply = true,
                "--once" => args.once = true,
                "-h" | "--help" => args.help = true,
                // v0.6 Phase A3: default is now enabled. The bare `--allowlist`
                // form remains accepted (idempotent set to true) for
                // backward-compat with scripts that explicitly enable the gate.
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
         \x20            [--allowlist[=enabled|disabled]] [--no-allowlist]\n\
         \n\
         Default mode is dry-run. Use --apply only on Rush Linux or a test host.\n\
         The WP-N4 hardware allowlist gate is ENABLED by default (v0.6 Phase A3).\n\
         --no-allowlist disables it (emergency escape hatch for bring-up on\n\
         hardware the seeded baseline does not yet cover)."
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
    fn no_allowlist_flag_disables_the_gate() {
        let args = Args::parse(["--no-allowlist".to_string()]).unwrap();
        assert!(
            !args.allowlist,
            "--no-allowlist must disable the gate (was {})",
            args.allowlist
        );
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
    fn allowlist_disabled_form_disables_the_gate() {
        // `--allowlist=disabled` is the explicit opt-out (same as --no-allowlist).
        let args = Args::parse(["--allowlist=disabled".to_string()]).unwrap();
        assert!(!args.allowlist);
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
}
