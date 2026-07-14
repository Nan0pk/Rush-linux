//! D-Bus server surface exposed by `optid`.
//!
//! The interface `io.rushlinux.Optid1` is the public control API documented
//! in `packaging/dbus/io.rushlinux.Optid.xml`. `optctl` calls into this
//! interface; if the bus is offline (e.g. running outside systemd), `optctl`
//! falls back to reading the same state files directly, so the methods here
//! only need to mirror the file writes — they do not need to broadcast.

use std::fs;
use std::path::PathBuf;

use zbus::interface;

use crate::workload::Mode;

pub(crate) struct OptidServer {
    pub(crate) state_dir: PathBuf,
}

/// Maximum length of an `app_id` passed to `PinApplication`.
///
/// Filesystems impose their own per-component limits (typically 255 bytes for
/// ext4/tmpfs NAME_MAX), but we enforce a shorter cap so the pin filename
/// stays human-readable in `optctl explain` output and in the `pins/`
/// directory listing.
const APP_ID_MAX_LEN: usize = 255;

/// Validate an `app_id` before it is used as a path component under the
/// daemon's `pins/` directory.
///
/// `app_id` arrives from an untrusted D-Bus caller and is joined directly to
/// the pins directory in `PinApplication`. Without validation, a caller could
/// supply `../mode` to overwrite the mode file, an absolute path like
/// `/etc/passwd` to write outside the pins directory (because
/// `PathBuf::join("/abs")` discards the base), or a NUL-containing string to
/// confuse downstream tooling.
///
/// This function returns `Ok(())` only when `app_id` is a single path
/// component consisting of ASCII alphanumeric, `_`, `-`, or `.` — and is not
/// `.` or `..` (both of which would resolve to a parent/self reference).
///
/// The special token `--global` is handled separately in `pin_application`
/// before this function is called, so it is not allowed here.
fn validate_app_id(app_id: &str) -> Result<(), zbus::fdo::Error> {
    if app_id.is_empty() {
        return Err(zbus::fdo::Error::InvalidArgs(
            "app_id must not be empty".to_string(),
        ));
    }
    if app_id.len() > APP_ID_MAX_LEN {
        return Err(zbus::fdo::Error::InvalidArgs(format!(
            "app_id exceeds {APP_ID_MAX_LEN} bytes"
        )));
    }
    // Reject anything that is not a single safe path component.
    // Allowed: ASCII letters, digits, underscore, hyphen, dot.
    // Disallowed: path separators (/ \), NUL, leading dot (covers "." and
    // ".." and hidden-file-style names), and any byte outside the safe set.
    if app_id.starts_with('.') {
        return Err(zbus::fdo::Error::InvalidArgs(
            "app_id must not start with a dot".to_string(),
        ));
    }
    for b in app_id.bytes() {
        let safe = b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.';
        if !safe {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "app_id contains disallowed byte 0x{b:02x}; only ASCII alphanumeric, '_', '-', '.' are permitted"
            )));
        }
    }
    // Redundant belt-and-braces: explicitly reject the traversal names even
    // though the leading-dot check above already catches them.
    if app_id == ".." || app_id == "." {
        return Err(zbus::fdo::Error::InvalidArgs(
            "app_id must not be '.' or '..'".to_string(),
        ));
    }
    // The "--global" token is intercepted earlier in pin_application to
    // write the global pin file; it must never be used as a per-app
    // filename (it would collide with the global-pin semantics).
    if app_id == "--global" {
        return Err(zbus::fdo::Error::InvalidArgs(
            "app_id must not be the reserved token '--global'".to_string(),
        ));
    }
    Ok(())
}

/// Returns `true` only if a caller has explicitly opted in to the
/// not-yet-polkit-authorized `PinApplication` method.
///
/// ADR 0009 (proposed) specifies that state-changing D-Bus methods
/// (`SetMode`, `PinApplication`) require a polkit action. Polkit integration
/// is not yet implemented; until it lands, `PinApplication` is disabled by
/// default to prevent any local user from writing root-owned pin files.
///
/// Operators who need to test `PinApplication` locally (e.g. on a dev box
/// with no other local users) can set `OPTID_ALLOW_PIN_APPLICATION=1` in the
/// daemon's environment. This escape hatch is documented as
/// **testing-only**; production deployments must not set it, and it will be
/// removed once polkit checks land.
fn pin_application_allowed() -> bool {
    matches!(
        std::env::var("OPTID_ALLOW_PIN_APPLICATION").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

#[interface(name = "io.rushlinux.Optid1")]
impl OptidServer {
    fn status(&self) -> zbus::fdo::Result<String> {
        fs::read_to_string(self.state_dir.join("status"))
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to read status: {e}")))
    }

    fn explain(&self) -> zbus::fdo::Result<String> {
        fs::read_to_string(self.state_dir.join("decisions.log"))
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to read decisions.log: {e}")))
    }

    fn set_mode(&self, mode: &str) -> zbus::fdo::Result<()> {
        let mode_parsed = Mode::parse(mode)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("invalid mode: {mode}")))?;
        fs::write(self.state_dir.join("mode"), mode_parsed.to_string())
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to write mode: {e}")))
    }

    fn pin_application(&self, app_id: &str, class: &str) -> zbus::fdo::Result<()> {
        // v0.6 Phase C2: this hardcoded list intentionally does NOT
        // include "vm.guest". The VmGuest class is platform-forced and
        // cannot be set manually via optctl pin.
        let classes = [
            "idle",
            "light",
            "interactive",
            "latency-critical",
            "throughput",
        ];
        if !classes.contains(&class) {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "invalid workload class: {class}"
            )));
        }

        // Security gate (audit finding #1): PinApplication writes as root into
        // the pins/ directory using an untrusted app_id. Until polkit
        // authorization lands (ADR 0009), the method is disabled by default.
        // The validation below still runs even when the gate is open, so that
        // a misconfigured env var cannot re-introduce the path-traversal
        // vulnerability.
        if !pin_application_allowed() {
            return Err(zbus::fdo::Error::Failed(
                "PinApplication is disabled pending polkit authorization (ADR 0009); \
                 set OPTID_ALLOW_PIN_APPLICATION=1 for local testing only"
                    .to_string(),
            ));
        }

        if app_id == "--global" {
            fs::write(self.state_dir.join("workload_class_pin"), class).map_err(|e| {
                zbus::fdo::Error::Failed(format!("failed to write global pin: {e}"))
            })?;
            println!("Pinned global workload class to {class}");
            return Ok(());
        }

        // Defense-in-depth: validate app_id BEFORE joining it to the pins
        // directory. This catches path traversal (../), absolute paths
        // (which PathBuf::join would silently treat as a new root), NUL
        // bytes, and overly-long names.
        validate_app_id(app_id)?;

        let pins_dir = self.state_dir.join("pins");
        fs::create_dir_all(&pins_dir)
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to create pins dir: {e}")))?;

        // Canonicalize the pins directory and the target file path, then
        // verify the target is strictly inside the pins directory. This is
        // a redundant post-check: validate_app_id already rejects every
        // input that could escape, but the cost is negligible and the check
        // turns a future regression in validate_app_id into a hard failure
        // instead of a silent escape.
        let pins_dir_canon = pins_dir.canonicalize().map_err(|e| {
            zbus::fdo::Error::Failed(format!("failed to canonicalize pins dir: {e}"))
        })?;
        let target = pins_dir_canon.join(app_id);
        let target_parent_canon = target
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("could not resolve target parent dir".to_string())
            })?;
        if target_parent_canon != pins_dir_canon {
            return Err(zbus::fdo::Error::Failed(format!(
                "refusing to write pin outside pins directory: {} is not under {}",
                target_parent_canon.display(),
                pins_dir_canon.display()
            )));
        }

        fs::write(&target, class)
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to write pin: {e}")))?;
        println!("Pinned application {app_id} to class {class}");
        Ok(())
    }

    #[zbus(property)]
    fn mode(&self) -> String {
        let text = fs::read_to_string(self.state_dir.join("mode")).unwrap_or_default();
        Mode::parse(&text).unwrap_or(Mode::Auto).to_string()
    }

    #[zbus(property)]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_app_id_rejects_empty() {
        assert!(validate_app_id("").is_err());
    }

    #[test]
    fn validate_app_id_rejects_traversal_dot_dot() {
        assert!(validate_app_id("..").is_err());
    }

    #[test]
    fn validate_app_id_rejects_single_dot() {
        assert!(validate_app_id(".").is_err());
    }

    #[test]
    fn validate_app_id_rejects_leading_dot() {
        // Hidden-file-style names are rejected to prevent confusion with
        // config files and to keep the pins/ listing clean.
        assert!(validate_app_id(".hidden").is_err());
        assert!(validate_app_id(".config").is_err());
    }

    #[test]
    fn validate_app_id_rejects_forward_slash() {
        // Path traversal via / — including the case where PathBuf::join
        // would treat an absolute component as a new root.
        assert!(validate_app_id("../mode").is_err());
        assert!(validate_app_id("a/b").is_err());
        assert!(validate_app_id("/etc/passwd").is_err());
        assert!(validate_app_id("/").is_err());
    }

    #[test]
    fn validate_app_id_rejects_backslash() {
        assert!(validate_app_id("..\\..\\windows").is_err());
        assert!(validate_app_id("a\\b").is_err());
    }

    #[test]
    fn validate_app_id_rejects_nul() {
        assert!(validate_app_id("a\0b").is_err());
        assert!(validate_app_id("\0").is_err());
    }

    #[test]
    fn validate_app_id_rejects_global_token() {
        // --global is handled separately in pin_application and must never
        // be used as a filename.
        assert!(validate_app_id("--global").is_err());
    }

    #[test]
    fn validate_app_id_rejects_spaces_and_special_chars() {
        assert!(validate_app_id("my app").is_err());
        assert!(validate_app_id("app;rm -rf /").is_err());
        assert!(validate_app_id("app|cat").is_err());
        assert!(validate_app_id("app$HOME").is_err());
        assert!(validate_app_id("app`whoami`").is_err());
    }

    #[test]
    fn validate_app_id_accepts_valid_ids() {
        assert!(validate_app_id("firefox").is_ok());
        assert!(validate_app_id("org.gnome.Calculator").is_ok());
        assert!(validate_app_id("my-app_2").is_ok());
        assert!(validate_app_id("123").is_ok());
        assert!(validate_app_id("a").is_ok());
        assert!(validate_app_id("steam_64-bit").is_ok());
    }

    #[test]
    fn validate_app_id_rejects_overlong() {
        let long = "a".repeat(APP_ID_MAX_LEN + 1);
        assert!(validate_app_id(&long).is_err());
    }

    #[test]
    fn validate_app_id_accepts_max_length() {
        let max = "a".repeat(APP_ID_MAX_LEN);
        assert!(validate_app_id(&max).is_ok());
    }

    /// Regression test for audit finding #1: the exact attack vector
    /// described in the audit (joining `../mode` to the pins directory to
    /// overwrite the mode file) must now be rejected at the validation
    /// layer, not just at the filesystem layer.
    #[test]
    fn validate_app_id_blocks_audit_attack_vector() {
        // These are the specific inputs an attacker would try based on the
        // audit description.
        assert!(validate_app_id("../mode").is_err());
        assert!(validate_app_id("../workload_class_pin").is_err());
        assert!(validate_app_id("../../etc/passwd").is_err());
        assert!(validate_app_id("/etc/shadow").is_err());
        assert!(validate_app_id("/run/optid/mode").is_err());
    }

    /// Combined into a single test to avoid parallel env-var races between
    /// independent test functions. The semantics tested here are:
    ///
    /// - unset / empty / "0" / "false" / "no" / garbage → disabled
    /// - "1" / "true" / "yes" → enabled
    ///
    /// In production the env var is set once at daemon startup, so the
    /// read is single-threaded; the race only exists in the test harness.
    #[test]
    fn pin_application_allowed_env_var_semantics() {
        let saved = std::env::var("OPTID_ALLOW_PIN_APPLICATION").ok();
        std::env::remove_var("OPTID_ALLOW_PIN_APPLICATION");
        assert!(
            !pin_application_allowed(),
            "should be disabled when env var is unset"
        );

        for val in &["1", "true", "yes"] {
            std::env::set_var("OPTID_ALLOW_PIN_APPLICATION", val);
            assert!(
                pin_application_allowed(),
                "should be enabled for OPTID_ALLOW_PIN_APPLICATION={val}"
            );
        }

        for val in &["0", "", "false", "no", "random", "2"] {
            std::env::set_var("OPTID_ALLOW_PIN_APPLICATION", val);
            assert!(
                !pin_application_allowed(),
                "should be disabled for OPTID_ALLOW_PIN_APPLICATION={val}"
            );
        }

        // Restore so we don't pollute other tests.
        match saved {
            Some(v) => std::env::set_var("OPTID_ALLOW_PIN_APPLICATION", v),
            None => std::env::remove_var("OPTID_ALLOW_PIN_APPLICATION"),
        }
    }
}
