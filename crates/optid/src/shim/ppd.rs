//! v0.6 Phase B1 — `net.hadess.PowerProfiles` D-Bus shim.
//!
//! `power-profiles-daemon` (PPD) is the de-facto standard power-policy D-Bus
//! interface spoken by GNOME Settings, KDE `powerdevil`, and most desktop
//! environments. Rather than fork PPD (which would maintain a parallel
//! policy stack and contradict ADR 0004 — optid is the single owner of
//! hardware policy), optid implements the `net.hadess.PowerProfiles`
//! interface on its own connection. Existing desktop software drives optid
//! mode changes without code changes on their side.
//!
//! ## Profile → optid mode mapping
//!
//! Configurable via `config/optid/policy.toml` `[shim.ppd]` table. Defaults:
//!
//! | PPD profile    | optid mode (written to `state_dir/mode`) |
//! |----------------|-----------------------------------------|
//! | `power-saver`  | `battery`                               |
//! | `balanced`     | `auto` (clears the override)            |
//! | `performance`  | `performance`                           |
//!
//! Writing `"auto"` to `state_dir/mode` causes the run loop to revert to
//! classifier-driven mode selection (see `workload::read_mode_override`
//! and the `override_mode` handling in `main.rs`).
//!
//! ## HoldProfile / ReleaseProfile
//!
//! PPD's transient-claim API. Each `HoldProfile(profile, reason, app_id)`
//! call returns a cookie (monotonic `u32`). The held profile becomes the
//! effective active profile until `ReleaseProfile(cookie)` is called.
//! Multiple concurrent holds are tracked in an in-memory `Mutex<HashMap>`;
//! the most-recently-registered hold wins. Holds do NOT persist across
//! optid restarts — this matches PPD's documented behavior.
//!
//! ## Signals
//!
//! `ActiveProfileChanged` and `ProfileReleased` are defined on the
//! interface (so they appear in introspection XML and clients can
//! subscribe), but **not emitted** in v0.6. Clients see profile changes
//! via the standard `org.freedesktop.DBus.Properties.PropertiesChanged`
//! signal, which zbus auto-emits because `ActiveProfile` carries
//! `emits_changed_signal = "true"`. Custom-signal emission is deferred
//! to v0.7 when we have a desktop edition to test against.
//!
//! ## Conflict handling
//!
//! If `power-profiles-daemon.service` is already running (detected by
//! `shim::detect_conflicts`), `main.rs` skips registering this shim —
//! attempting to claim the `net.hadess.PowerProfiles` bus name would
//! fail and the conflict report already advises the operator to mask PPD.
//!
//! See `docs/plans/v0.6-hardware-aware-optid-proposal.md` §3 Phase B (B1).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use zbus::interface;
use zbus::object_server::SignalEmitter;
use zvariant::{OwnedValue, Value};

use crate::workload::Mode;

/// A registered `HoldProfile` claim. The cookie (key in `HoldRegistry`) is
/// returned to the caller and used as the `ReleaseProfile` argument.
///
/// `reason` and `app_id` are stored for diagnostic logging (future v0.7
/// work will surface them in `optctl explain`); they are not currently
/// read by any code path, hence the `#[allow(dead_code)]` suppression.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Hold {
    profile: String,
    reason: String,
    app_id: String,
}

/// In-memory cookie registry for `HoldProfile` / `ReleaseProfile`. The
/// counter is monotonic across the daemon's lifetime; cookies are never
/// reused within a single optid process (matches PPD's behavior).
#[derive(Debug, Default)]
struct HoldRegistry {
    next_cookie: u32,
    holds: HashMap<u32, Hold>,
}

impl HoldRegistry {
    fn new() -> Self {
        Self {
            // Start at 1 — PPD clients treat 0 as "no cookie" in some
            // implementations; we avoid the ambiguity.
            next_cookie: 1,
            holds: HashMap::new(),
        }
    }

    fn register(&mut self, profile: &str, reason: &str, app_id: &str) -> u32 {
        let cookie = self.next_cookie;
        // wrapping_add so a runaway caller (4 billion holds in one process
        // lifetime) doesn't panic; in practice this is unreachable.
        self.next_cookie = self.next_cookie.wrapping_add(1);
        self.holds.insert(
            cookie,
            Hold {
                profile: profile.to_string(),
                reason: reason.to_string(),
                app_id: app_id.to_string(),
            },
        );
        cookie
    }

    fn release(&mut self, cookie: u32) -> bool {
        self.holds.remove(&cookie).is_some()
    }

    fn is_empty(&self) -> bool {
        self.holds.is_empty()
    }

    /// PPD semantics: the most-recently-registered hold wins. We
    /// approximate by selecting the hold with the highest cookie number
    /// (since cookies are monotonic). Ties cannot occur.
    fn effective_profile(&self) -> Option<&str> {
        self.holds
            .iter()
            .max_by_key(|(c, _)| *c)
            .map(|(_, h)| h.profile.as_str())
    }
}

/// The PPD D-Bus server. Serves the `net.hadess.PowerProfiles` interface
/// from `/net/hadess/PowerProfiles` on optid's connection.
///
/// `state_dir` is shared with `OptidServer` (and the run loop). Profile
/// changes write to `state_dir/mode`, which the run loop reads each tick
/// via `workload::read_mode_override`.
///
/// `profile_map` carries the PPD-profile → optid-mode mapping loaded from
/// `policy.toml`'s `[shim.ppd]` table. Empty map = use the standard
/// hardcoded defaults (see `default_mode_for_profile`).
pub(crate) struct PpdServer {
    state_dir: PathBuf,
    profile_map: HashMap<String, String>,
    holds: Mutex<HoldRegistry>,
}

impl PpdServer {
    pub(crate) fn new(state_dir: PathBuf, profile_map: HashMap<String, String>) -> Self {
        Self {
            state_dir,
            profile_map,
            holds: Mutex::new(HoldRegistry::new()),
        }
    }

    /// Look up the optid mode string for a PPD profile name. Consults the
    /// configurable `profile_map` first, then falls back to the standard
    /// hardcoded mapping. Returns `None` for unknown profiles.
    fn mode_for_profile(&self, profile: &str) -> Option<String> {
        if let Some(mode) = self.profile_map.get(profile) {
            return Some(mode.clone());
        }
        default_mode_for_profile(profile).map(|s| s.to_string())
    }
}

/// Standard PPD profile → optid mode mapping, used when `policy.toml`
/// doesn't override. The three PPD profile names are fixed by the upstream
/// spec; no other values are valid `ActiveProfile` strings.
fn default_mode_for_profile(profile: &str) -> Option<&'static str> {
    match profile {
        "power-saver" => Some("battery"),
        "balanced" => Some("auto"),
        "performance" => Some("performance"),
        _ => None,
    }
}

/// Inverse mapping: read the current optid mode override and report it
/// as the active PPD profile. `"auto"` and `"balanced"` both collapse to
/// `"balanced"` (PPD has no "auto" profile); `"realtime"` collapses to
/// `"performance"` (PPD has no realtime profile). This is the same
/// mapping PPD uses when it can't represent an internal state exactly.
fn profile_for_mode(mode: Mode) -> &'static str {
    match mode {
        Mode::Battery => "power-saver",
        Mode::Auto => "balanced",
        Mode::Balanced => "balanced",
        Mode::Performance => "performance",
        Mode::Realtime => "performance",
    }
}

#[interface(name = "net.hadess.PowerProfiles")]
impl PpdServer {
    /// Read the active PPD profile. If any `HoldProfile` holds are active,
    /// the most-recently-registered held profile wins (PPD semantics).
    /// Otherwise we translate the persisted optid mode override (or
    /// `"auto"` if no override is set) into a PPD profile name.
    ///
    /// `emits_changed_signal = "true"` makes zbus auto-emit
    /// `PropertiesChanged` when the setter writes a new value — clients
    /// see profile changes without optid having to emit a custom signal.
    #[zbus(property(emits_changed_signal = "true"))]
    fn active_profile(&self) -> zbus::fdo::Result<String> {
        // Check held profiles first — they take precedence over the
        // persisted mode override.
        if let Ok(holds) = self.holds.lock() {
            if let Some(profile) = holds.effective_profile() {
                return Ok(profile.to_string());
            }
        }
        // Otherwise read the persisted mode override. Missing file or
        // unparseable contents → Mode::Auto, which maps to "balanced".
        let mode_text = fs::read_to_string(self.state_dir.join("mode")).unwrap_or_default();
        let mode = Mode::parse(&mode_text).unwrap_or(Mode::Auto);
        Ok(profile_for_mode(mode).to_string())
    }

    /// Set the active PPD profile. Writes the mapped optid Mode string to
    /// `state_dir/mode`, which the run loop reads next tick.
    ///
    /// Note: setting the active profile does NOT clear existing
    /// `HoldProfile` holds — per PPD's spec, a `Set` call is a persistent
    /// change that holds temporarily override. If the caller wants to
    /// clear holds as well, they must call `ReleaseProfile` for each
    /// outstanding cookie.
    ///
    /// `emits_changed_signal` lives on the getter only — zbus auto-emits
    /// `PropertiesChanged` when the setter returns Ok, so we don't repeat
    /// the attribute here.
    #[zbus(property)]
    fn set_active_profile(&self, profile: String) -> zbus::fdo::Result<()> {
        let mode_str = self.mode_for_profile(&profile).ok_or_else(|| {
            zbus::fdo::Error::InvalidArgs(format!(
                "invalid PPD profile: {profile} \
                 (expected one of: power-saver, balanced, performance)"
            ))
        })?;
        // Validate that the mapped mode string is a real Mode variant.
        // This catches a misconfigured `policy.toml` early — better to
        // refuse the call than to write a garbage string to state_dir/mode
        // that the run loop will silently ignore.
        if Mode::parse(&mode_str).is_none() {
            return Err(zbus::fdo::Error::Failed(format!(
                "PPD profile {profile} maps to invalid optid mode '{mode_str}' \
                 (check [shim.ppd] in policy.toml)"
            )));
        }
        fs::write(self.state_dir.join("mode"), mode_str).map_err(|e| {
            zbus::fdo::Error::Failed(format!("failed to write state_dir/mode: {e}"))
        })?;
        // The ActiveProfileChanged custom signal is NOT emitted in v0.6.
        // PropertiesChanged fires automatically via emits_changed_signal
        // above. Custom-signal emission is deferred to v0.7 (see module
        // docstring).
        Ok(())
    }

    /// Read-only property: the three standard PPD profiles. Returned as
    /// `aa{sv}` (array of dicts with a `Profile` key) to match PPD's
    /// on-wire type exactly — GNOME Settings expects this layout.
    #[zbus(property)]
    fn profiles(&self) -> Vec<HashMap<String, OwnedValue>> {
        // OwnedValue does not implement From<String>; go through Value<'_>
        // (which does have From<&str>) and then convert with try_into. The
        // conversion is infallible for Value::Str — the only error path is
        // a non-owned borrow, which Value::from(&str) doesn't produce.
        fn profile_entry(name: &str) -> HashMap<String, OwnedValue> {
            HashMap::from([(
                "Profile".to_string(),
                Value::from(name)
                    .try_into()
                    .expect("Value::Str -> OwnedValue"),
            )])
        }
        vec![
            profile_entry("power-saver"),
            profile_entry("balanced"),
            profile_entry("performance"),
        ]
    }

    /// Read-only property: PPD "actions" (e.g. `inhibit-suspend`). optid
    /// does not implement any PPD actions — the empty array tells clients
    /// there is nothing to enumerate.
    #[zbus(property)]
    fn actions(&self) -> Vec<String> {
        Vec::new()
    }

    /// Read-only property: returns `""` (no degradation). optid does not
    /// degrade `performance` mode via the PPD interface — thermal guard
    /// rails in `policy.rs::auto_mode` handle critical-thermal backoff
    /// directly, transparently to PPD clients.
    #[zbus(property)]
    fn performance_degraded(&self) -> String {
        String::new()
    }

    /// Register a transient hold on a PPD profile. Returns a cookie that
    /// the caller passes to `ReleaseProfile` to release the hold.
    ///
    /// The held profile becomes the effective active profile immediately
    /// (it overrides the persisted `ActiveProfile` until released).
    fn hold_profile(
        &self,
        profile: String,
        reason: String,
        app_id: String,
    ) -> zbus::fdo::Result<u32> {
        // Validate profile name and the mapped mode string before
        // mutating any state. This keeps the registry consistent if
        // policy.toml is misconfigured.
        let mode_str = self.mode_for_profile(&profile).ok_or_else(|| {
            zbus::fdo::Error::InvalidArgs(format!(
                "invalid PPD profile: {profile} \
                 (expected one of: power-saver, balanced, performance)"
            ))
        })?;
        if Mode::parse(&mode_str).is_none() {
            return Err(zbus::fdo::Error::Failed(format!(
                "PPD profile {profile} maps to invalid optid mode '{mode_str}' \
                 (check [shim.ppd] in policy.toml)"
            )));
        }
        let mut holds = self
            .holds
            .lock()
            .map_err(|e| zbus::fdo::Error::Failed(format!("holds mutex poisoned: {e}")))?;
        let cookie = holds.register(&profile, &reason, &app_id);
        // Apply the held profile as the active profile.
        fs::write(self.state_dir.join("mode"), mode_str).map_err(|e| {
            zbus::fdo::Error::Failed(format!("failed to write state_dir/mode: {e}"))
        })?;
        Ok(cookie)
    }

    /// Release a previously-registered hold. If the registry is empty
    /// after the release, the active profile reverts to the persisted
    /// mode override (or `"auto"` → `"balanced"` if no override is set).
    /// If other holds remain, the most-recent still-active hold wins.
    fn release_profile(&self, cookie: u32) -> zbus::fdo::Result<()> {
        let mut holds = self
            .holds
            .lock()
            .map_err(|e| zbus::fdo::Error::Failed(format!("holds mutex poisoned: {e}")))?;
        if !holds.release(cookie) {
            return Err(zbus::fdo::Error::Failed(format!(
                "no hold registered for cookie {cookie}"
            )));
        }
        // If no more holds, revert to "auto" (which the run loop maps to
        // classifier-driven mode). If a hold remains, the most-recent one
        // becomes the effective active profile again.
        if holds.is_empty() {
            fs::write(self.state_dir.join("mode"), "auto").map_err(|e| {
                zbus::fdo::Error::Failed(format!("failed to revert state_dir/mode: {e}"))
            })?;
        } else if let Some(profile) = holds.effective_profile() {
            // mode_for_profile on the still-held profile. Safe to unwrap
            // because the held profile was validated at HoldProfile time.
            if let Some(mode_str) = self.mode_for_profile(profile) {
                fs::write(self.state_dir.join("mode"), mode_str).map_err(|e| {
                    zbus::fdo::Error::Failed(format!(
                        "failed to write state_dir/mode on hold restore: {e}"
                    ))
                })?;
            }
        }
        // The ProfileReleased custom signal is NOT emitted in v0.6 —
        // see module docstring.
        Ok(())
    }

    /// Signal: emitted when `ActiveProfile` changes, whether by `Set` or
    /// by hold/release. **Not emitted in v0.6** — clients see changes via
    /// `PropertiesChanged` instead. Defined here so introspection reports
    /// the signal and clients can subscribe (subscription will simply
    /// never fire until v0.7 enables emission).
    ///
    /// The Rust method is named `emit_active_profile_changed` (not
    /// `active_profile_changed`) to avoid colliding with the
    /// auto-generated property-changed emitter that zbus creates for the
    /// `active_profile` property. The D-Bus member name is preserved as
    /// `ActiveProfileChanged` via the `name` attribute.
    ///
    /// Declared `async fn` because zbus 5.x requires it even in blocking
    /// mode — the blocking API wraps the async signal emitter via
    /// `zbus::block_on`. To emit from a blocking method:
    ///
    /// ```ignore
    /// let _ = zbus::block_on(Self::emit_active_profile_changed(
    ///     iface_ref.signal_emitter(),
    ///     profile,
    ///     reason,
    /// ));
    /// ```
    #[zbus(signal, name = "ActiveProfileChanged")]
    async fn emit_active_profile_changed(
        emitter: &SignalEmitter<'_>,
        profile: &str,
        reason: &str,
    ) -> zbus::Result<()>;

    /// Signal: emitted when a hold is released. **Not emitted in v0.6**.
    /// Renamed to `emit_profile_released` to follow the same collision-
    /// avoidance convention as `emit_active_profile_changed` (no
    /// `profile_released` property exists today, but the prefix makes the
    /// intent clear and prevents future collisions).
    #[zbus(signal, name = "ProfileReleased")]
    async fn emit_profile_released(emitter: &SignalEmitter<'_>, cookie: u32) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    //! Unit tests for the PPD shim's pure logic. End-to-end D-Bus
    //! behavior (property getters, method dispatch, state file writes) is
    //! tested in `crates/optid/tests/shim_ppd.rs`.

    use super::*;

    fn empty_server(tmp: &std::path::Path) -> PpdServer {
        PpdServer::new(tmp.to_path_buf(), HashMap::new())
    }

    // ── default_mode_for_profile ─────────────────────────────────────────

    #[test]
    fn default_mode_for_profile_known_profiles() {
        assert_eq!(default_mode_for_profile("power-saver"), Some("battery"));
        assert_eq!(default_mode_for_profile("balanced"), Some("auto"));
        assert_eq!(default_mode_for_profile("performance"), Some("performance"));
    }

    #[test]
    fn default_mode_for_profile_unknown_returns_none() {
        assert_eq!(default_mode_for_profile("realtime"), None);
        assert_eq!(default_mode_for_profile(""), None);
        // Case-sensitive: PPD's spec uses lowercase, we match exactly.
        assert_eq!(default_mode_for_profile("POWER-SAVER"), None);
        assert_eq!(default_mode_for_profile("Power-Saver"), None);
    }

    // ── profile_for_mode ─────────────────────────────────────────────────

    #[test]
    fn profile_for_mode_round_trip() {
        assert_eq!(profile_for_mode(Mode::Battery), "power-saver");
        assert_eq!(profile_for_mode(Mode::Auto), "balanced");
        assert_eq!(profile_for_mode(Mode::Balanced), "balanced");
        assert_eq!(profile_for_mode(Mode::Performance), "performance");
        // Realtime has no PPD equivalent; collapse to performance.
        assert_eq!(profile_for_mode(Mode::Realtime), "performance");
    }

    // ── PpdServer::mode_for_profile (configurable path) ─────────────────

    #[test]
    fn mode_for_profile_uses_defaults_when_map_empty() {
        let tmp = std::env::temp_dir().join("optid_ppd_test_empty_map");
        let server = empty_server(&tmp);
        assert_eq!(
            server.mode_for_profile("power-saver"),
            Some("battery".to_string())
        );
        assert_eq!(
            server.mode_for_profile("balanced"),
            Some("auto".to_string())
        );
        assert_eq!(
            server.mode_for_profile("performance"),
            Some("performance".to_string())
        );
        assert_eq!(server.mode_for_profile("unknown"), None);
    }

    #[test]
    fn mode_for_profile_uses_overrides_when_map_has_them() {
        let tmp = std::env::temp_dir().join("optid_ppd_test_overrides");
        let mut map = HashMap::new();
        map.insert("performance".to_string(), "realtime".to_string());
        map.insert("custom-profile".to_string(), "performance".to_string());
        let server = PpdServer::new(tmp.to_path_buf(), map);
        // Overridden
        assert_eq!(
            server.mode_for_profile("performance"),
            Some("realtime".to_string())
        );
        // Custom profile name only known via the map
        assert_eq!(
            server.mode_for_profile("custom-profile"),
            Some("performance".to_string())
        );
        // Not in map, falls back to default
        assert_eq!(
            server.mode_for_profile("power-saver"),
            Some("battery".to_string())
        );
        // Unknown to both map and defaults
        assert_eq!(server.mode_for_profile("nope"), None);
    }

    // ── HoldRegistry ─────────────────────────────────────────────────────

    #[test]
    fn hold_registry_register_returns_monotonic_cookies() {
        let mut reg = HoldRegistry::new();
        let c1 = reg.register("performance", "test", "app1");
        let c2 = reg.register("power-saver", "test", "app2");
        let c3 = reg.register("balanced", "test", "app3");
        assert!(c2 > c1, "cookies must be monotonic: {c1} -> {c2}");
        assert!(c3 > c2, "cookies must be monotonic: {c2} -> {c3}");
    }

    #[test]
    fn hold_registry_first_cookie_is_one_not_zero() {
        // 0 is reserved as "no cookie" in some PPD client implementations;
        // we avoid the ambiguity by starting at 1.
        let mut reg = HoldRegistry::new();
        let c = reg.register("performance", "test", "app1");
        assert_eq!(c, 1);
    }

    #[test]
    fn hold_registry_release_returns_true_for_known_cookie() {
        let mut reg = HoldRegistry::new();
        let c = reg.register("performance", "test", "app1");
        assert!(reg.release(c));
    }

    #[test]
    fn hold_registry_release_returns_false_for_unknown_cookie() {
        let mut reg = HoldRegistry::new();
        assert!(!reg.release(999));
        assert!(!reg.release(0));
    }

    #[test]
    fn hold_registry_effective_profile_is_most_recent() {
        let mut reg = HoldRegistry::new();
        let _c1 = reg.register("power-saver", "test1", "app1");
        let _c2 = reg.register("performance", "test2", "app2");
        assert_eq!(reg.effective_profile(), Some("performance"));
        // After releasing the most-recent, the previous one wins.
        let last = *reg.holds.keys().max().unwrap();
        assert!(reg.release(last));
        assert_eq!(reg.effective_profile(), Some("power-saver"));
    }

    #[test]
    fn hold_registry_is_empty_after_all_released() {
        let mut reg = HoldRegistry::new();
        assert!(reg.is_empty());
        let c1 = reg.register("performance", "test", "app1");
        let c2 = reg.register("power-saver", "test", "app2");
        assert!(!reg.is_empty());
        reg.release(c1);
        assert!(!reg.is_empty());
        reg.release(c2);
        assert!(reg.is_empty());
    }

    #[test]
    fn hold_registry_effective_profile_none_when_empty() {
        let reg = HoldRegistry::new();
        assert_eq!(reg.effective_profile(), None);
    }

    // ── Functional tests: PpdServer method dispatch ─────────────────────
    //
    // These tests construct a `PpdServer` and call its `#[interface]`
    // methods directly as Rust methods. The `#[interface]` macro leaves
    // the methods callable from pure Rust — they only become D-Bus
    // methods when dispatched via the ObjectServer. This lets us test
    // the full state-machine (mode file writes, hold registry, mapping)
    // without a session bus.

    use std::fs;

    /// Each test gets a fresh tempdir so state files don't leak. We don't
    /// pull in the `tempfile` crate (not already a dep — see ponytail
    /// ladder); `std::env::temp_dir()` + unique suffix is enough.
    fn fresh_state_dir(test_name: &str) -> PathBuf {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("optid_shim_ppd_test_{test_name}_{pid}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn read_mode(state_dir: &std::path::Path) -> Option<String> {
        fs::read_to_string(state_dir.join("mode"))
            .ok()
            .map(|s| s.trim().to_string())
    }

    // ── active_profile getter ───────────────────────────────────────────

    #[test]
    fn active_profile_defaults_to_balanced_when_no_mode_file() {
        let dir = fresh_state_dir("active_default");
        let server = empty_server(&dir);
        assert_eq!(server.active_profile().unwrap(), "balanced");
    }

    #[test]
    fn active_profile_reads_persisted_mode_battery() {
        let dir = fresh_state_dir("active_battery");
        fs::write(dir.join("mode"), "battery").unwrap();
        let server = empty_server(&dir);
        assert_eq!(server.active_profile().unwrap(), "power-saver");
    }

    #[test]
    fn active_profile_reads_persisted_mode_performance() {
        let dir = fresh_state_dir("active_perf");
        fs::write(dir.join("mode"), "performance").unwrap();
        let server = empty_server(&dir);
        assert_eq!(server.active_profile().unwrap(), "performance");
    }

    #[test]
    fn active_profile_reads_persisted_mode_auto() {
        let dir = fresh_state_dir("active_auto");
        fs::write(dir.join("mode"), "auto").unwrap();
        let server = empty_server(&dir);
        assert_eq!(server.active_profile().unwrap(), "balanced");
    }

    #[test]
    fn active_profile_realtime_collapses_to_performance() {
        // PPD has no realtime profile; we collapse to performance.
        let dir = fresh_state_dir("active_rt");
        fs::write(dir.join("mode"), "realtime").unwrap();
        let server = empty_server(&dir);
        assert_eq!(server.active_profile().unwrap(), "performance");
    }

    #[test]
    fn active_profile_unparseable_mode_file_falls_back_to_balanced() {
        // Garbage mode file should not crash the getter.
        let dir = fresh_state_dir("active_garbage");
        fs::write(dir.join("mode"), "this-is-not-a-mode").unwrap();
        let server = empty_server(&dir);
        assert_eq!(server.active_profile().unwrap(), "balanced");
    }

    // ── set_active_profile setter ───────────────────────────────────────

    #[test]
    fn set_active_profile_power_saver_writes_battery() {
        let dir = fresh_state_dir("set_power_saver");
        let server = empty_server(&dir);
        server
            .set_active_profile("power-saver".to_string())
            .unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("battery"));
    }

    #[test]
    fn set_active_profile_balanced_writes_auto() {
        let dir = fresh_state_dir("set_balanced");
        let server = empty_server(&dir);
        server.set_active_profile("balanced".to_string()).unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("auto"));
    }

    #[test]
    fn set_active_profile_performance_writes_performance() {
        let dir = fresh_state_dir("set_performance");
        let server = empty_server(&dir);
        server
            .set_active_profile("performance".to_string())
            .unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("performance"));
    }

    #[test]
    fn set_active_profile_invalid_name_returns_invalid_args() {
        let dir = fresh_state_dir("set_invalid");
        let server = empty_server(&dir);
        let err = server
            .set_active_profile("turbo-mode".to_string())
            .unwrap_err();
        match err {
            zbus::fdo::Error::InvalidArgs(msg) => {
                assert!(
                    msg.contains("turbo-mode"),
                    "error should mention the bad profile: {msg}"
                );
                assert!(
                    msg.contains("power-saver"),
                    "error should list valid options: {msg}"
                );
            }
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
        // State file should NOT have been written.
        assert!(read_mode(&dir).is_none());
    }

    #[test]
    fn set_active_profile_case_sensitive() {
        // PPD profile names are lowercase by spec; we match exactly.
        let dir = fresh_state_dir("set_case");
        let server = empty_server(&dir);
        let err = server
            .set_active_profile("Power-Saver".to_string())
            .unwrap_err();
        assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
    }

    // ── configurable profile map ────────────────────────────────────────

    #[test]
    fn set_active_profile_uses_custom_mapping_when_provided() {
        let dir = fresh_state_dir("set_custom_map");
        let mut map = HashMap::new();
        map.insert("performance".to_string(), "realtime".to_string());
        let server = PpdServer::new(dir.clone(), map);
        server
            .set_active_profile("performance".to_string())
            .unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("realtime"));
    }

    #[test]
    fn set_active_profile_falls_back_to_default_when_map_partial() {
        let dir = fresh_state_dir("set_partial_map");
        let mut map = HashMap::new();
        map.insert("performance".to_string(), "realtime".to_string());
        let server = PpdServer::new(dir.clone(), map);
        server
            .set_active_profile("power-saver".to_string())
            .unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("battery"));
    }

    #[test]
    fn set_active_profile_rejects_invalid_mapped_mode() {
        // policy.toml maps a profile to a non-existent Mode string — the
        // setter should fail with a clear error rather than write garbage.
        let dir = fresh_state_dir("set_invalid_mapped");
        let mut map = HashMap::new();
        map.insert("performance".to_string(), "not-a-real-mode".to_string());
        let server = PpdServer::new(dir.clone(), map);
        let err = server
            .set_active_profile("performance".to_string())
            .unwrap_err();
        match err {
            zbus::fdo::Error::Failed(msg) => {
                assert!(
                    msg.contains("not-a-real-mode"),
                    "error should mention the bad mapping: {msg}"
                );
                assert!(
                    msg.contains("policy.toml"),
                    "error should hint at policy.toml: {msg}"
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(read_mode(&dir).is_none());
    }

    #[test]
    fn set_active_profile_accepts_custom_profile_name_via_map() {
        let dir = fresh_state_dir("set_custom_profile_name");
        let mut map = HashMap::new();
        map.insert("game-mode".to_string(), "performance".to_string());
        let server = PpdServer::new(dir.clone(), map);
        server.set_active_profile("game-mode".to_string()).unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("performance"));
    }

    // ── profiles / actions / performance_degraded getters ───────────────

    #[test]
    fn profiles_returns_three_standard_entries() {
        let dir = fresh_state_dir("profiles_basic");
        let server = empty_server(&dir);
        let profiles = server.profiles();
        assert_eq!(profiles.len(), 3, "PPD has exactly three profiles");
        // Each entry must have a "Profile" key whose value is a string.
        // OwnedValue downcasts return Result<String, Error>; we map to
        // Option<String> via ok() and map(|r| r.to_string()).
        let names: Vec<String> = profiles
            .iter()
            .map(|d| {
                d.get("Profile")
                    .and_then(|v| v.downcast_ref::<String>().ok())
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            })
            .collect();
        assert!(names.contains(&"power-saver".to_string()));
        assert!(names.contains(&"balanced".to_string()));
        assert!(names.contains(&"performance".to_string()));
    }

    #[test]
    fn actions_returns_empty_vec() {
        let dir = fresh_state_dir("actions_empty");
        let server = empty_server(&dir);
        assert!(server.actions().is_empty());
    }

    #[test]
    fn performance_degraded_returns_empty_string() {
        let dir = fresh_state_dir("perf_degraded_empty");
        let server = empty_server(&dir);
        assert_eq!(server.performance_degraded(), "");
    }

    // ── HoldProfile / ReleaseProfile ────────────────────────────────────

    #[test]
    fn hold_profile_returns_monotonic_cookies() {
        let dir = fresh_state_dir("hold_monotonic");
        let server = empty_server(&dir);
        let c1 = server
            .hold_profile(
                "performance".to_string(),
                "game".to_string(),
                "steam".to_string(),
            )
            .unwrap();
        let c2 = server
            .hold_profile(
                "power-saver".to_string(),
                "low battery".to_string(),
                "gnome".to_string(),
            )
            .unwrap();
        assert!(c2 > c1, "cookies must be monotonic: {c1} -> {c2}");
        assert!(c1 > 0, "first cookie should not be 0 (reserved)");
    }

    #[test]
    fn hold_profile_writes_held_mode_to_state_dir() {
        let dir = fresh_state_dir("hold_writes_mode");
        let server = empty_server(&dir);
        let _ = server
            .hold_profile(
                "performance".to_string(),
                "game".to_string(),
                "steam".to_string(),
            )
            .unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("performance"));
    }

    #[test]
    fn hold_profile_invalid_name_returns_invalid_args() {
        let dir = fresh_state_dir("hold_invalid");
        let server = empty_server(&dir);
        let err = server
            .hold_profile("turbo".to_string(), "r".to_string(), "a".to_string())
            .unwrap_err();
        assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
        assert!(
            read_mode(&dir).is_none(),
            "no mode file should be written on error"
        );
    }

    #[test]
    fn active_profile_returns_held_profile_when_hold_active() {
        let dir = fresh_state_dir("hold_active_returns_held");
        let server = empty_server(&dir);
        // Persisted mode is "battery" (power-saver), but a hold on
        // "performance" should win.
        fs::write(dir.join("mode"), "battery").unwrap();
        let _ = server
            .hold_profile(
                "performance".to_string(),
                "game".to_string(),
                "steam".to_string(),
            )
            .unwrap();
        assert_eq!(server.active_profile().unwrap(), "performance");
    }

    #[test]
    fn release_profile_reverts_to_auto_when_no_holds_remain() {
        let dir = fresh_state_dir("release_reverts");
        let server = empty_server(&dir);
        let cookie = server
            .hold_profile(
                "performance".to_string(),
                "game".to_string(),
                "steam".to_string(),
            )
            .unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("performance"));
        server.release_profile(cookie).unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("auto"));
        // Active profile should now be "balanced" (Auto → balanced).
        assert_eq!(server.active_profile().unwrap(), "balanced");
    }

    #[test]
    fn release_profile_unknown_cookie_returns_failed() {
        let dir = fresh_state_dir("release_unknown");
        let server = empty_server(&dir);
        let err = server.release_profile(999).unwrap_err();
        match err {
            zbus::fdo::Error::Failed(msg) => {
                assert!(
                    msg.contains("999"),
                    "error should mention the cookie: {msg}"
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn release_profile_with_multiple_holds_restores_previous() {
        // Hold #1: power-saver. Hold #2: performance. Release #2 → power-saver wins.
        let dir = fresh_state_dir("release_multi");
        let server = empty_server(&dir);
        let _c1 = server
            .hold_profile(
                "power-saver".to_string(),
                "low".to_string(),
                "a".to_string(),
            )
            .unwrap();
        let c2 = server
            .hold_profile(
                "performance".to_string(),
                "game".to_string(),
                "b".to_string(),
            )
            .unwrap();
        assert_eq!(server.active_profile().unwrap(), "performance");
        server.release_profile(c2).unwrap();
        assert_eq!(server.active_profile().unwrap(), "power-saver");
        assert_eq!(read_mode(&dir).as_deref(), Some("battery"));
    }

    #[test]
    fn hold_release_reverts_to_auto_even_when_persisted_override_existed() {
        // PPD semantics: after the last hold is released, the active
        // profile reverts to "balanced" (Auto), NOT to the previously-
        // persisted override. Test pins this behavior so a future change
        // is intentional.
        let dir = fresh_state_dir("hold_release_persisted");
        let server = empty_server(&dir);
        server
            .set_active_profile("performance".to_string())
            .unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("performance"));
        let c = server
            .hold_profile(
                "power-saver".to_string(),
                "low battery".to_string(),
                "app".to_string(),
            )
            .unwrap();
        assert_eq!(server.active_profile().unwrap(), "power-saver");
        server.release_profile(c).unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("auto"));
        assert_eq!(server.active_profile().unwrap(), "balanced");
    }

    // ── concurrency sanity (Mutex correctness) ──────────────────────────

    #[test]
    fn holds_can_be_released_in_reverse_order() {
        // Register 3 holds, release them in reverse order. Each release
        // should leave the registry in a consistent state.
        let dir = fresh_state_dir("hold_release_reverse");
        let server = empty_server(&dir);
        let c1 = server
            .hold_profile("power-saver".to_string(), "1".to_string(), "a".to_string())
            .unwrap();
        let c2 = server
            .hold_profile("balanced".to_string(), "2".to_string(), "b".to_string())
            .unwrap();
        let c3 = server
            .hold_profile("performance".to_string(), "3".to_string(), "c".to_string())
            .unwrap();
        // Release in reverse: c3, c2, c1.
        server.release_profile(c3).unwrap();
        assert_eq!(server.active_profile().unwrap(), "balanced");
        server.release_profile(c2).unwrap();
        assert_eq!(server.active_profile().unwrap(), "power-saver");
        server.release_profile(c1).unwrap();
        assert_eq!(server.active_profile().unwrap(), "balanced"); // reverted to auto
    }

    #[test]
    fn holds_mutex_survives_sequential_burst() {
        // Verify the Mutex doesn't deadlock under a sequential burst
        // that mimics concurrent access patterns.
        let dir = fresh_state_dir("mutex_sequential");
        let server = empty_server(&dir);
        let mut cookies = Vec::new();
        for _ in 0..10 {
            let c = server
                .hold_profile("performance".to_string(), "x".to_string(), "y".to_string())
                .unwrap();
            cookies.push(c);
        }
        for c in cookies {
            server.release_profile(c).unwrap();
        }
        assert_eq!(read_mode(&dir).as_deref(), Some("auto"));
    }

    // ── state_dir sharing with OptidServer ──────────────────────────────

    #[test]
    fn ppd_server_writes_same_mode_file_format_as_optid_server() {
        // The OptidServer.set_mode method writes to state_dir/mode with
        // the exact mode string. PpdServer.set_active_profile writes to
        // the same file with the mapped mode string. They must agree.
        let dir = fresh_state_dir("shared_mode_file");
        fs::write(dir.join("mode"), "battery").unwrap();
        let server = empty_server(&dir);
        assert_eq!(server.active_profile().unwrap(), "power-saver");
        server
            .set_active_profile("performance".to_string())
            .unwrap();
        let mode_text = fs::read_to_string(dir.join("mode")).unwrap();
        assert_eq!(mode_text, "performance");
    }

    // ── full custom map overrides all three profiles ────────────────────

    #[test]
    fn ppd_server_with_full_custom_map_overrides_all() {
        let dir = fresh_state_dir("full_custom_map");
        let mut map = HashMap::new();
        map.insert("power-saver".to_string(), "balanced".to_string());
        map.insert("balanced".to_string(), "performance".to_string());
        map.insert("performance".to_string(), "realtime".to_string());
        let server = PpdServer::new(dir.clone(), map);
        server
            .set_active_profile("power-saver".to_string())
            .unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("balanced"));
        server.set_active_profile("balanced".to_string()).unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("performance"));
        server
            .set_active_profile("performance".to_string())
            .unwrap();
        assert_eq!(read_mode(&dir).as_deref(), Some("realtime"));
    }
}
