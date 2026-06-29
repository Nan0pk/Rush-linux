//! v0.6 Phase B2 — `com.feralinteractive.GameMode` D-Bus shim.
//!
//! GameMode is Feral Interactive's D-Bus interface that games (Steam,
//! Lutris, Heroic) call to ask the system for "performance mode" while
//! the game is running. Rather than fork GameMode — which would maintain
//! a parallel policy stack and contradict ADR 0004 — optid implements
//! the `com.feralinteractive.GameMode` interface on its own connection.
//! Existing game launchers drive optid without code changes on their
//! side.
//!
//! ## API surface
//!
//! | Method                   | Returns | Notes                                  |
//! |--------------------------|---------|----------------------------------------|
//! | `RegisterGame(pid)`      | `i32`   | 1=registered, 2=already registered, 0=error |
//! | `UnregisterGame(pid)`    | `i32`   | 1=unregistered, 2=was not registered, 0=error |
//! | `QueryStatus()`          | `i32`   | Count of active (non-expired) games    |
//! | `QueryStatusClient(pid)` | `i32`   | 1=registered, 0=not registered         |
//!
//! No properties, no signals. (GameMode's official spec is method-only.)
//!
//! ## Pin semantics
//!
//! On `RegisterGame(pid)`, optid writes `state_dir/pins/<pid>` with the
//! configured workload class (default: `latency-critical`). The run loop
//! reads this file on the next tick via `workload::read_pinned_class`
//! and applies the latency contract. On `UnregisterGame(pid)`, the pin
//! file is removed and the run loop reverts to classifier-driven mode.
//!
//! ## TTL
//!
//! Each registration carries a TTL (default: 30 minutes, configurable
//! via `policy.toml`'s `[shim.gamemode]` table). If a game crashes
//! without calling `UnregisterGame`, the registration expires lazily on
//! the next `RegisterGame` / `QueryStatus` / `QueryStatusClient` call.
//! Lazy expiration is sufficient because GameMode clients poll
//! `QueryStatus` regularly; a stale pin is a minor inefficiency, not a
//! correctness issue.
//!
//! The TTL does NOT persist across optid restarts — the in-memory
//! registry is empty on startup.
//!
//! ## Conflict handling
//!
//! If `gamemoded.service` is added to `competing_policy_daemons` and
//! detected as active at startup, `main.rs` skips registering this shim.
//!
//! See `docs/plans/v0.6-hardware-aware-optid-proposal.md` §3 Phase B (B2).

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use zbus::interface;

use crate::workload::WorkloadClass;

/// A registered game. PIDs are `i32` (signed; -1 is "no process" on
/// Unix). The TTL is stored as a Duration so tests can construct
/// fixed-TTL registrations without depending on `Instant::now()`.
#[derive(Debug, Clone)]
struct GameRegistration {
    pid: i32,
    registered_at: Instant,
    ttl: Duration,
}

impl GameRegistration {
    /// `true` if the registration's TTL has elapsed at `now`.
    fn is_expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.registered_at) >= self.ttl
    }
}

/// In-memory registry of active GameMode registrations. The PID is the
/// key (one registration per PID — matches GameMode's spec, which
/// deduplicates RegisterGame calls for the same PID).
#[derive(Debug, Default)]
struct GameRegistry {
    games: Vec<GameRegistration>,
}

impl GameRegistry {
    fn new() -> Self {
        Self { games: Vec::new() }
    }

    /// Register a PID. Returns `true` if newly registered, `false` if
    /// the PID was already in the registry (and the existing entry's
    /// TTL is refreshed — matches GameMode's behavior of "re-register
    /// extends the TTL").
    fn register(&mut self, pid: i32, ttl: Duration, now: Instant) -> bool {
        if let Some(existing) = self.games.iter_mut().find(|g| g.pid == pid) {
            existing.registered_at = now;
            existing.ttl = ttl;
            return false;
        }
        self.games.push(GameRegistration {
            pid,
            registered_at: now,
            ttl,
        });
        true
    }

    /// Unregister a PID. Returns `true` if it was present and removed,
    /// `false` if it wasn't registered.
    fn unregister(&mut self, pid: i32) -> bool {
        let before = self.games.len();
        self.games.retain(|g| g.pid != pid);
        self.games.len() < before
    }

    /// Remove all registrations whose TTL has elapsed at `now`. Returns
    /// the PIDs that were expired.
    fn expire_stale(&mut self, now: Instant) -> Vec<i32> {
        let mut expired = Vec::new();
        self.games.retain(|g| {
            if g.is_expired(now) {
                expired.push(g.pid);
                false
            } else {
                true
            }
        });
        expired
    }

    /// Count of active (non-expired) registrations.
    fn active_count(&self, now: Instant) -> usize {
        self.games.iter().filter(|g| !g.is_expired(now)).count()
    }

    /// `true` if the PID is currently registered and not expired.
    fn is_registered(&self, pid: i32, now: Instant) -> bool {
        self.games
            .iter()
            .any(|g| g.pid == pid && !g.is_expired(now))
    }
}

/// The GameMode D-Bus server. Serves the `com.feralinteractive.GameMode`
/// interface from `/com/feralinteractive/GameMode` on optid's connection.
pub(crate) struct GameModeServer {
    state_dir: PathBuf,
    pin_class: String,
    ttl: Duration,
    registry: Mutex<GameRegistry>,
}

impl GameModeServer {
    pub(crate) fn new(state_dir: PathBuf, pin_class: String, ttl_sec: u64) -> Self {
        Self {
            state_dir,
            pin_class,
            ttl: Duration::from_secs(ttl_sec),
            registry: Mutex::new(GameRegistry::new()),
        }
    }

    /// Validate the configured pin class. Returns `true` if it's a valid
    /// `WorkloadClass` string. Called once at construction by `main.rs`
    /// to fail fast on a misconfigured `policy.toml`.
    pub(crate) fn pin_class_is_valid(&self) -> bool {
        WorkloadClass::parse(&self.pin_class).is_some()
    }

    /// Lazy-expire stale registrations AND remove their pin files.
    fn expire_stale_and_cleanup(&self, now: Instant) -> usize {
        let Ok(mut registry) = self.registry.lock() else {
            return 0;
        };
        let expired_pids = registry.expire_stale(now);
        let pins_dir = self.state_dir.join("pins");
        for pid in &expired_pids {
            let pin_file = pins_dir.join(pid.to_string());
            let _ = fs::remove_file(pin_file);
        }
        expired_pids.len()
    }

    /// Write the pin file for `pid`. Idempotent.
    fn write_pin_file(&self, pid: i32) -> std::io::Result<()> {
        let pins_dir = self.state_dir.join("pins");
        fs::create_dir_all(&pins_dir)?;
        fs::write(pins_dir.join(pid.to_string()), &self.pin_class)
    }

    /// Remove the pin file for `pid`. Idempotent.
    fn remove_pin_file(&self, pid: i32) {
        let pin_file = self.state_dir.join("pins").join(pid.to_string());
        let _ = fs::remove_file(&pin_file);
    }
}

#[interface(name = "com.feralinteractive.GameMode")]
impl GameModeServer {
    /// Register a game by PID. Writes the pin file with the configured
    /// workload class (default: `latency-critical`).
    ///
    /// Returns:
    /// - `1` if newly registered.
    /// - `2` if the PID was already registered (TTL is refreshed).
    /// - `0` on error.
    fn register_game(&self, pid: i32) -> i32 {
        if pid < 0 {
            return 0;
        }
        let _ = self.expire_stale_and_cleanup(Instant::now());
        let newly_registered = {
            let Ok(mut registry) = self.registry.lock() else {
                return 0;
            };
            registry.register(pid, self.ttl, Instant::now())
        };
        if let Err(e) = self.write_pin_file(pid) {
            eprintln!("optid: GameMode RegisterGame({pid}) — failed to write pin file: {e}");
            return 0;
        }
        if newly_registered {
            1
        } else {
            2
        }
    }

    /// Unregister a game by PID. Removes the pin file.
    ///
    /// Returns:
    /// - `1` if the PID was registered and is now unregistered.
    /// - `2` if the PID was not registered (no-op).
    /// - `0` on error.
    fn unregister_game(&self, pid: i32) -> i32 {
        if pid < 0 {
            return 2;
        }
        let _ = self.expire_stale_and_cleanup(Instant::now());
        let was_registered = {
            let Ok(mut registry) = self.registry.lock() else {
                return 0;
            };
            registry.unregister(pid)
        };
        if was_registered {
            self.remove_pin_file(pid);
            1
        } else {
            self.remove_pin_file(pid);
            2
        }
    }

    /// Query the count of active (non-expired) game registrations.
    fn query_status(&self) -> i32 {
        let _ = self.expire_stale_and_cleanup(Instant::now());
        let Ok(registry) = self.registry.lock() else {
            return 0;
        };
        registry.active_count(Instant::now()) as i32
    }

    /// Query whether a specific PID is currently registered.
    /// Returns `1` if registered, `0` if not.
    fn query_status_client(&self, pid: i32) -> i32 {
        let _ = self.expire_stale_and_cleanup(Instant::now());
        let Ok(registry) = self.registry.lock() else {
            return 0;
        };
        if registry.is_registered(pid, Instant::now()) {
            1
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_state_dir(test_name: &str) -> PathBuf {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("optid_shim_gamemode_test_{test_name}_{pid}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn default_server(dir: PathBuf) -> GameModeServer {
        GameModeServer::new(dir, "latency-critical".to_string(), 1800)
    }

    fn read_pin(state_dir: &std::path::Path, pid: i32) -> Option<String> {
        fs::read_to_string(state_dir.join("pins").join(pid.to_string()))
            .ok()
            .map(|s| s.trim().to_string())
    }

    // ── GameRegistry ─────────────────────────────────────────────────────

    #[test]
    fn registry_register_returns_true_for_new_pid() {
        let mut reg = GameRegistry::new();
        let now = Instant::now();
        assert!(reg.register(1234, Duration::from_secs(1800), now));
    }

    #[test]
    fn registry_register_returns_false_for_existing_pid() {
        let mut reg = GameRegistry::new();
        let now = Instant::now();
        assert!(reg.register(1234, Duration::from_secs(1800), now));
        assert!(!reg.register(1234, Duration::from_secs(1800), now));
    }

    #[test]
    fn registry_register_refreshes_ttl_on_re_register() {
        let mut reg = GameRegistry::new();
        let now = Instant::now();
        reg.register(1234, Duration::from_secs(60), now);
        reg.register(1234, Duration::from_secs(3600), now);
        let later = now + Duration::from_secs(120);
        assert!(reg.is_registered(1234, later));
    }

    #[test]
    fn registry_unregister_returns_true_for_known_pid() {
        let mut reg = GameRegistry::new();
        let now = Instant::now();
        reg.register(1234, Duration::from_secs(1800), now);
        assert!(reg.unregister(1234));
    }

    #[test]
    fn registry_unregister_returns_false_for_unknown_pid() {
        let mut reg = GameRegistry::new();
        assert!(!reg.unregister(9999));
    }

    #[test]
    fn registry_expire_stale_removes_expired_entries() {
        let mut reg = GameRegistry::new();
        let now = Instant::now();
        reg.register(111, Duration::from_secs(10), now);
        reg.register(222, Duration::from_secs(60), now);
        let later = now + Duration::from_secs(30);
        let expired = reg.expire_stale(later);
        assert_eq!(expired, vec![111]);
        assert_eq!(reg.games.len(), 1);
        assert_eq!(reg.games[0].pid, 222);
    }

    #[test]
    fn registry_expire_stale_returns_empty_when_nothing_expired() {
        let mut reg = GameRegistry::new();
        let now = Instant::now();
        reg.register(111, Duration::from_secs(3600), now);
        let expired = reg.expire_stale(now);
        assert!(expired.is_empty());
    }

    #[test]
    fn registry_active_count_excludes_expired() {
        let mut reg = GameRegistry::new();
        let now = Instant::now();
        reg.register(111, Duration::from_secs(10), now);
        reg.register(222, Duration::from_secs(60), now);
        reg.register(333, Duration::from_secs(3600), now);
        let later = now + Duration::from_secs(30);
        assert_eq!(reg.active_count(later), 2);
    }

    #[test]
    fn registry_is_registered_excludes_expired() {
        let mut reg = GameRegistry::new();
        let now = Instant::now();
        reg.register(111, Duration::from_secs(10), now);
        let later = now + Duration::from_secs(30);
        assert!(!reg.is_registered(111, later));
    }

    #[test]
    fn registry_is_registered_true_for_active() {
        let mut reg = GameRegistry::new();
        let now = Instant::now();
        reg.register(111, Duration::from_secs(3600), now);
        assert!(reg.is_registered(111, now));
    }

    // ── GameRegistration::is_expired ─────────────────────────────────────

    #[test]
    fn registration_is_expired_false_when_within_ttl() {
        let now = Instant::now();
        let reg = GameRegistration {
            pid: 1234,
            registered_at: now,
            ttl: Duration::from_secs(60),
        };
        assert!(!reg.is_expired(now));
        assert!(!reg.is_expired(now + Duration::from_secs(59)));
    }

    #[test]
    fn registration_is_expired_true_when_ttl_elapsed() {
        let now = Instant::now();
        let reg = GameRegistration {
            pid: 1234,
            registered_at: now,
            ttl: Duration::from_secs(60),
        };
        assert!(reg.is_expired(now + Duration::from_secs(60)));
        assert!(reg.is_expired(now + Duration::from_secs(120)));
    }

    // ── GameModeServer: register_game ────────────────────────────────────

    #[test]
    fn register_game_returns_1_for_new_pid() {
        let dir = fresh_state_dir("register_new");
        let server = default_server(dir.clone());
        assert_eq!(server.register_game(1234), 1);
    }

    #[test]
    fn register_game_returns_2_for_existing_pid() {
        let dir = fresh_state_dir("register_existing");
        let server = default_server(dir.clone());
        assert_eq!(server.register_game(1234), 1);
        assert_eq!(server.register_game(1234), 2);
    }

    #[test]
    fn register_game_writes_pin_file_with_configured_class() {
        let dir = fresh_state_dir("register_writes_pin");
        let server = default_server(dir.clone());
        server.register_game(1234);
        assert_eq!(read_pin(&dir, 1234).as_deref(), Some("latency-critical"));
    }

    #[test]
    fn register_game_uses_custom_pin_class_when_configured() {
        let dir = fresh_state_dir("register_custom_class");
        let server = GameModeServer::new(dir.clone(), "throughput".to_string(), 1800);
        server.register_game(1234);
        assert_eq!(read_pin(&dir, 1234).as_deref(), Some("throughput"));
    }

    #[test]
    fn register_game_rejects_negative_pid() {
        let dir = fresh_state_dir("register_negative");
        let server = default_server(dir.clone());
        assert_eq!(server.register_game(-1), 0);
        assert_eq!(server.register_game(-100), 0);
        assert!(read_pin(&dir, -1).is_none());
    }

    #[test]
    fn register_game_idempotent_pin_file() {
        let dir = fresh_state_dir("register_idempotent");
        let server = default_server(dir.clone());
        server.register_game(1234);
        server.register_game(1234);
        assert_eq!(read_pin(&dir, 1234).as_deref(), Some("latency-critical"));
    }

    // ── GameModeServer: unregister_game ──────────────────────────────────

    #[test]
    fn unregister_game_returns_1_for_registered_pid() {
        let dir = fresh_state_dir("unregister_registered");
        let server = default_server(dir.clone());
        server.register_game(1234);
        assert_eq!(server.unregister_game(1234), 1);
    }

    #[test]
    fn unregister_game_returns_2_for_unknown_pid() {
        let dir = fresh_state_dir("unregister_unknown");
        let server = default_server(dir.clone());
        assert_eq!(server.unregister_game(9999), 2);
    }

    #[test]
    fn unregister_game_removes_pin_file() {
        let dir = fresh_state_dir("unregister_removes_pin");
        let server = default_server(dir.clone());
        server.register_game(1234);
        assert!(read_pin(&dir, 1234).is_some());
        server.unregister_game(1234);
        assert!(read_pin(&dir, 1234).is_none());
    }

    #[test]
    fn unregister_game_treats_negative_pid_as_not_registered() {
        let dir = fresh_state_dir("unregister_negative");
        let server = default_server(dir.clone());
        assert_eq!(server.unregister_game(-1), 2);
    }

    #[test]
    fn unregister_game_after_re_register_removes_pin() {
        let dir = fresh_state_dir("unregister_after_reregister");
        let server = default_server(dir.clone());
        server.register_game(1234);
        server.register_game(1234);
        server.unregister_game(1234);
        assert!(read_pin(&dir, 1234).is_none());
    }

    // ── GameModeServer: query_status ─────────────────────────────────────

    #[test]
    fn query_status_returns_0_when_no_games_registered() {
        let dir = fresh_state_dir("query_empty");
        let server = default_server(dir);
        assert_eq!(server.query_status(), 0);
    }

    #[test]
    fn query_status_counts_active_games() {
        let dir = fresh_state_dir("query_counts");
        let server = default_server(dir);
        server.register_game(111);
        server.register_game(222);
        server.register_game(333);
        assert_eq!(server.query_status(), 3);
    }

    #[test]
    fn query_status_excludes_unregistered_games() {
        let dir = fresh_state_dir("query_excludes_unregistered");
        let server = default_server(dir);
        server.register_game(111);
        server.register_game(222);
        server.unregister_game(111);
        assert_eq!(server.query_status(), 1);
    }

    // ── GameModeServer: query_status_client ──────────────────────────────

    #[test]
    fn query_status_client_returns_1_for_registered_pid() {
        let dir = fresh_state_dir("query_client_registered");
        let server = default_server(dir);
        server.register_game(1234);
        assert_eq!(server.query_status_client(1234), 1);
    }

    #[test]
    fn query_status_client_returns_0_for_unregistered_pid() {
        let dir = fresh_state_dir("query_client_unregistered");
        let server = default_server(dir);
        assert_eq!(server.query_status_client(9999), 0);
    }

    #[test]
    fn query_status_client_returns_0_after_unregister() {
        let dir = fresh_state_dir("query_client_after_unregister");
        let server = default_server(dir);
        server.register_game(1234);
        assert_eq!(server.query_status_client(1234), 1);
        server.unregister_game(1234);
        assert_eq!(server.query_status_client(1234), 0);
    }

    // ── pin_class_is_valid ───────────────────────────────────────────────

    #[test]
    fn pin_class_is_valid_for_standard_classes() {
        for class in [
            "idle",
            "light",
            "interactive",
            "latency-critical",
            "throughput",
        ] {
            let dir = fresh_state_dir(&format!("valid_class_{class}"));
            let server = GameModeServer::new(dir, class.to_string(), 1800);
            assert!(
                server.pin_class_is_valid(),
                "class '{class}' should be valid"
            );
        }
    }

    #[test]
    fn pin_class_is_invalid_for_garbage() {
        let dir = fresh_state_dir("invalid_class");
        let server = GameModeServer::new(dir, "not-a-real-class".to_string(), 1800);
        assert!(!server.pin_class_is_valid());
    }

    // ── state_dir sharing with OptidServer ───────────────────────────────

    #[test]
    fn gamemode_writes_to_same_pins_dir_as_optidserver() {
        let dir = fresh_state_dir("shared_pins_dir");
        let pins_dir = dir.join("pins");
        fs::create_dir_all(&pins_dir).unwrap();
        fs::write(pins_dir.join("firefox"), "interactive").unwrap();
        let server = default_server(dir.clone());
        server.register_game(1234);
        assert_eq!(read_pin(&dir, 1234).as_deref(), Some("latency-critical"));
        assert_eq!(
            fs::read_to_string(pins_dir.join("firefox")).unwrap(),
            "interactive"
        );
    }

    // ── lazy expiration integration ──────────────────────────────────────

    #[test]
    fn register_game_lazy_expires_before_registering() {
        let dir = fresh_state_dir("lazy_expire");
        let server = default_server(dir);
        server.register_game(111);
        server.register_game(222);
        server.register_game(111);
        assert_eq!(server.query_status(), 2);
    }

    #[test]
    fn registry_can_be_manipulated_to_simulate_expiration() {
        let dir = fresh_state_dir("manual_expiration");
        let server = default_server(dir.clone());
        server.register_game(111);
        server.register_game(222);
        assert_eq!(server.query_status(), 2);
        {
            let Ok(mut registry) = server.registry.lock() else {
                panic!("registry mutex poisoned");
            };
            let ancient = Instant::now() - Duration::from_secs(86400);
            for game in registry.games.iter_mut() {
                game.registered_at = ancient;
                game.ttl = Duration::from_secs(1);
            }
        }
        assert_eq!(server.query_status(), 0);
        assert!(read_pin(&dir, 111).is_none());
        assert!(read_pin(&dir, 222).is_none());
    }
}
