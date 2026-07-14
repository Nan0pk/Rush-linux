//! v0.6 Phase C1 — foreground-application detection.
//!
//! When a desktop session is active, optid can promote the focused
//! application to a specific workload class — e.g., games get
//! `latency-critical`, IDEs get `throughput`, browsers get `interactive`.
//! This replaces the manual `optctl pin` workflow for desktop users.
//!
//! ## v0.6 scope (minimal)
//!
//! The v0.6 implementation is a **stub**:
//! - The `subscribe()` function exists and returns a receiver, but the
//!   receiver never yields events. The spawned thread sleeps forever.
//! - The `[foreground]` policy table is parsed (so operators can
//!   configure `game_class`), but the configuration is not used yet.
//! - The `--foreground=auto|off` flag is parsed; `auto` enables the
//!   stub subscriber, `off` (the default) disables it entirely.
//!
//! ## v0.7 plan
//!
//! Real compositor integration is deferred to v0.7 when we have a
//! desktop edition to test against:
//!
//! 1. Subscribe to `org.freedesktop.login1` `SessionNew` / `SessionRemoved`.
//! 2. For each new session, check `Active` and `Class` (= "user").
//! 3. If the session is `Type=wayland` or `Type=x11`, subscribe to the
//!    compositor's focus signal:
//!    - Wayland: `org.gnome.Mutter.IdleMonitor` (GNOME) / `org.kde.KWin`
//!      (KDE) — or fall back to `wlr-foreign-toplevel-management`
//!      (wlroots/sway/Hyprland).
//!    - X11: `_NET_ACTIVE_WINDOW` root window property via `xcb`.
//! 4. On focus change, parse the window class against `[foreground.*]`
//!    rules and emit `(pid, class)` on the receiver.
//!
//! See `docs/plans/v0.6-hardware-aware-optid-proposal.md` §3 Phase C1.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Configuration for foreground-app detection. Parsed from
/// `config/optid/policy.toml`'s `[foreground]` table.
///
/// v0.6 carries only `game_class`. v0.7 will add per-app rules.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ForegroundConfig {
    /// The workload class to pin game windows to. Default:
    /// `latency-critical`.
    #[serde(default = "default_game_class")]
    #[allow(dead_code)]
    pub(crate) game_class: String,
}

fn default_game_class() -> String {
    "latency-critical".to_string()
}

// Manual `Default` impl because `#[serde(default = "...")]` only fires
// during TOML deserialization — `ForegroundConfig::default()` would
// otherwise produce `game_class: String::default()` (empty string).
impl Default for ForegroundConfig {
    fn default() -> Self {
        Self {
            game_class: default_game_class(),
        }
    }
}

/// Subscribe to foreground-window focus changes. Returns a receiver
/// that yields `(pid, class_string)` tuples on each focus change.
///
/// **v0.6 stub:** the returned receiver never yields. The function
/// spawns a thread that sleeps forever (holding the sender open so
/// `recv()` blocks instead of returning `Disconnected`).
pub(crate) fn subscribe(
    _state_dir: PathBuf,
    _config: ForegroundConfig,
) -> mpsc::Receiver<(i32, String)> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _tx = tx;
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_config_defaults_game_class_to_latency_critical() {
        let config = ForegroundConfig::default();
        assert_eq!(config.game_class, "latency-critical");
    }

    #[test]
    fn foreground_config_parses_custom_game_class() {
        let toml = "game_class = \"throughput\"\n";
        let config: ForegroundConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.game_class, "throughput");
    }

    #[test]
    fn foreground_config_parses_empty_toml_uses_default() {
        let config: ForegroundConfig = toml::from_str("").unwrap();
        assert_eq!(config.game_class, "latency-critical");
    }

    #[test]
    fn foreground_config_parses_missing_game_class_uses_default() {
        let toml = "# no game_class field\nother_field = \"ignored\"\n";
        let config: ForegroundConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.game_class, "latency-critical");
    }

    #[test]
    fn subscribe_returns_receiver_that_does_not_yield_in_v0_6() {
        let rx = subscribe(PathBuf::from("/tmp"), ForegroundConfig::default());
        match rx.recv_timeout(Duration::from_millis(100)) {
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // expected
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("v0.6 stub should keep the channel open");
            }
            Ok((pid, _class)) => {
                panic!("v0.6 stub should not yield events, got pid={pid}");
            }
        }
    }

    #[test]
    fn subscribe_does_not_panic_with_empty_config() {
        let rx = subscribe(PathBuf::from("/tmp"), ForegroundConfig::default());
        drop(rx);
        thread::sleep(Duration::from_millis(10));
    }
}
