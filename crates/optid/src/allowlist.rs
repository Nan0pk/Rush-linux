//! WP-N4 — the hardware allowlist DB: the `hwid ∈ allowlist(D, S)` safety gate
//! from `docs/SPEC-northstar.md` §3, clause 2.
//!
//! This is the *hardware* allowlist (threat: buggy hardware/firmware), and is
//! deliberately distinct from `io_util::guarded_write`'s *write* allowlist
//! (threat: malicious admin, ADR-0009). The two are orthogonal gates; a write
//! must pass BOTH. This module never weakens `guarded_write`.
//!
//! Design: `docs/research/0006-hw-allowlist-db-design.md` "Hybrid E". The
//! seeded safe baseline lives in `data/allowlist.toml`, is compiled into a
//! `static` table by `build.rs`, and is the lowest-precedence layer. Runtime
//! overrides are layered on top at load time with this precedence (§1.9):
//!
//! ```text
//! compiled-in seeded  <  distro (/usr/share/optid/allowlist.d)
//!                     <  admin + optctl runtime (/etc/optid/allowlist.d)
//! ```
//!
//! Within a directory, files are applied in lexicographic order and the last
//! definition of a `(domain, hwid)` key wins entirely (last-write-wins per
//! §1.9). `--unsafe-once` is not represented here — it is a single transient
//! allow handled at the call site, never persisted.
//!
//! Default-deny: an `(domain, hwid)` pair with no matching entry is DENIED with
//! reason `hwid_not_in_allowlist`. This is the safe failure mode (§1.2).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::load_state::LoadState;

/// Action stored in a compiled-in seeded entry. Mirrors `EntryAction` but is a
/// `'static`-friendly type the generated table can reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeededAction {
    Allow,
    Deny,
}

/// A single compiled-in baseline entry. `build.rs` emits a `static` slice of
/// these from `data/allowlist.toml`.
pub(crate) struct SeededEntry {
    pub(crate) domain: &'static str,
    pub(crate) hwid: &'static str,
    pub(crate) action: SeededAction,
    pub(crate) max_state: Option<u32>,
    pub(crate) reason: &'static str,
    pub(crate) tested_on: &'static str,
    pub(crate) verified: bool,
}

// Pulls in `SEEDED_VERSION` and `SEEDED_ENTRIES` (see build.rs). Included inside
// this module so the generated code resolves `SeededEntry` / `SeededAction`.
include!(concat!(env!("OUT_DIR"), "/allowlist_generated.rs"));

const DISTRO_DIR: &str = "/usr/share/optid/allowlist.d";
const ADMIN_DIR: &str = "/etc/optid/allowlist.d";

/// Default-deny base directories, lowest-to-highest precedence after the
/// compiled-in baseline.
pub(crate) const DEFAULT_OVERRIDE_DIRS: &[&str] = &[DISTRO_DIR, ADMIN_DIR];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryAction {
    Allow,
    Deny,
}

/// An effective allowlist entry after precedence resolution.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) domain: String,
    pub(crate) hwid: String,
    pub(crate) action: EntryAction,
    pub(crate) max_state: Option<u32>,
    pub(crate) reason: String,
    /// Hardware/evidence the entry is attributable to (0006 §5).
    pub(crate) tested_on: String,
    /// `true` once validated on real hardware per 0006 §4.
    pub(crate) verified: bool,
    /// Where the winning definition came from (for `optctl list-allow`).
    pub(crate) source: String,
}

impl Entry {
    /// Human-readable one-line summary for `optctl explain` / `list-allow`.
    pub(crate) fn describe(&self) -> String {
        let action = match self.action {
            EntryAction::Allow => "allow",
            EntryAction::Deny => "deny",
        };
        let max = match self.max_state {
            Some(n) => n.to_string(),
            None => "-".to_string(),
        };
        let verified = if self.verified {
            "verified"
        } else {
            "unverified"
        };
        format!(
            "{domain} {hwid} {action} max_state={max} [{verified}] source={source} \
tested_on={tested_on:?} reason={reason:?}",
            domain = self.domain,
            hwid = self.hwid,
            source = self.source,
            tested_on = self.tested_on,
            reason = self.reason,
        )
    }
}

/// Result of a gate check. `Deny` always carries a machine-readable reason so
/// every denial can be logged per the WP-N4 verifier criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    Allow,
    Deny { reason: String },
}

impl Verdict {
    pub(crate) fn is_allow(&self) -> bool {
        matches!(self, Verdict::Allow)
    }

    /// The reason string for a denial, or `None` for an allow.
    pub(crate) fn deny_reason(&self) -> Option<&str> {
        match self {
            Verdict::Allow => None,
            Verdict::Deny { reason } => Some(reason),
        }
    }
}

/// TOML shape of a runtime override file (`*.toml` in an override dir).
#[derive(serde::Deserialize)]
struct OverrideFile {
    #[serde(default)]
    entry: Vec<OverrideEntry>,
}

#[derive(serde::Deserialize)]
struct OverrideEntry {
    domain: String,
    hwid: String,
    #[serde(default = "default_action")]
    action: String,
    max_state: Option<u32>,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    tested_on: String,
    #[serde(default)]
    verified: bool,
}

fn default_action() -> String {
    "allow".to_string()
}

pub(crate) struct Allowlist {
    /// Effective entries keyed by `(domain, hwid)`, after precedence resolution.
    entries: HashMap<(String, String), Entry>,
    version: String,
}

impl Allowlist {
    /// The compiled-in seeded baseline only — no runtime overrides. Used as the
    /// base layer and directly in tests.
    pub(crate) fn seeded() -> Self {
        let mut entries = HashMap::new();
        for s in SEEDED_ENTRIES {
            let action = match s.action {
                SeededAction::Allow => EntryAction::Allow,
                SeededAction::Deny => EntryAction::Deny,
            };
            entries.insert(
                (s.domain.to_string(), s.hwid.to_string()),
                Entry {
                    domain: s.domain.to_string(),
                    hwid: s.hwid.to_string(),
                    action,
                    max_state: s.max_state,
                    reason: s.reason.to_string(),
                    tested_on: s.tested_on.to_string(),
                    verified: s.verified,
                    source: "seeded-baseline".to_string(),
                },
            );
        }
        Self {
            entries,
            version: SEEDED_VERSION.to_string(),
        }
    }

    /// Production load: seeded baseline + the default override directories
    /// (distro then admin), applied in precedence order.
    #[allow(dead_code)]
    pub(crate) fn load() -> Self {
        Self::load_with_state(DEFAULT_OVERRIDE_DIRS).0
    }

    /// Production load with explicit `LoadState`. Returns the loaded allowlist
    /// and a state describing how the load went:
    ///
    /// - `Ok` — every override directory either did not exist or parsed
    ///   cleanly. The seeded baseline + all overrides are in effect.
    /// - `Defaulted` — never returned for the allowlist (the seeded baseline
    ///   is always present, compiled into the binary). Reserved for future
    ///   use.
    /// - `Partial` — at least one override file was present but unparseable
    ///   or structurally invalid. The seeded baseline + the parseable
    ///   overrides are in effect; the malformed override was skipped with a
    ///   stderr warning. Dynamic writes are disabled because the operator's
    ///   intent is ambiguous.
    /// - `Invalid` — never returned today (a fully-invalid allowlist would
    ///   require the seeded baseline itself to be unparseable, which is a
    ///   compile-time error). Reserved for future use.
    ///
    /// The allowlist gate is consulted by the run loop's `BootState`
    /// computation: if `allowlist_load_state != Ok`, `apply_armed` is
    /// disarmed even if the policy loaded cleanly.
    pub(crate) fn load_with_state<P: AsRef<Path>>(dirs: &[P]) -> (Self, LoadState) {
        let mut al = Self::seeded();
        let mut saw_partial = false;
        for dir in dirs {
            let p = dir.as_ref();
            let partial = al.apply_dir_tracking(p);
            if partial {
                saw_partial = true;
            }
        }
        let state = if saw_partial {
            LoadState::Partial
        } else {
            LoadState::Ok
        };
        (al, state)
    }

    /// Load the seeded baseline then apply each override directory in order.
    /// Later directories (and lexicographically later files within a directory)
    /// win. Exposed for tests so they can point at temp dirs.
    #[allow(dead_code)]
    pub(crate) fn load_from<P: AsRef<Path>>(dirs: &[P]) -> Self {
        Self::load_with_state(dirs).0
    }

    /// Apply every `*.toml` file in `dir` in lexicographic order. Missing dirs
    /// are silently skipped (the common case: a host with no overrides).
    /// Unparseable files are skipped with a stderr warning rather than aborting
    /// — a corrupt drop-in must never break the daemon, only lose its overrides
    /// (mirrors `Contracts::load`).
    ///
    /// Returns `true` if any file in the directory was present but unparseable
    /// or contained invalid entries (the `Partial` load state). The seeded
    /// baseline and any parseable overrides are still applied; only the
    /// malformed file is skipped.
    fn apply_dir_tracking(&mut self, dir: &Path) -> bool {
        let Ok(read) = fs::read_dir(dir) else {
            return false;
        };
        let mut files: Vec<_> = read
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
            .collect();
        files.sort();
        let mut saw_partial = false;
        for path in files {
            let source = path.display().to_string();
            let text = match fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("optid: skipping allowlist override {source}: {e}");
                    saw_partial = true;
                    continue;
                }
            };
            let parsed: OverrideFile = match toml::from_str(&text) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("optid: skipping malformed allowlist override {source}: {e}");
                    saw_partial = true;
                    continue;
                }
            };
            for oe in parsed.entry {
                let action = match oe.action.as_str() {
                    "allow" => EntryAction::Allow,
                    "deny" => EntryAction::Deny,
                    other => {
                        eprintln!(
                            "optid: skipping allowlist entry in {source}: invalid action {other:?}"
                        );
                        saw_partial = true;
                        continue;
                    }
                };
                self.entries.insert(
                    (oe.domain.clone(), oe.hwid.clone()),
                    Entry {
                        domain: oe.domain,
                        hwid: oe.hwid,
                        action,
                        max_state: oe.max_state,
                        reason: oe.reason,
                        tested_on: oe.tested_on,
                        verified: oe.verified,
                        source: source.clone(),
                    },
                );
            }
        }
        saw_partial
    }

    /// Back-compat wrapper for callers that don't need the partial flag.
    /// Kept so the existing `apply_dir` name still resolves if anything
    /// outside this module referenced it (nothing does today, but the
    /// function is `pub(crate)`-adjacent and the rename is mechanical).
    #[allow(dead_code)]
    fn apply_dir(&mut self, dir: &Path) {
        let _ = self.apply_dir_tracking(dir);
    }

    /// The effective allowlist version (the seeded baseline's version string).
    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    /// Look up the effective entry for a `(domain, hwid)` pair, if any.
    pub(crate) fn lookup(&self, domain: &str, hwid: &str) -> Option<&Entry> {
        self.entries.get(&(domain.to_string(), hwid.to_string()))
    }

    /// The core gate: may domain `D` be actuated on `hwid` at `requested_state`?
    ///
    /// Default-deny — an unknown `(domain, hwid)` is denied with
    /// `hwid_not_in_allowlist`. An explicit `deny` entry wins. An `allow` entry
    /// with `max_state = N` denies `requested_state > N` with `state_exceeds_max`
    /// (the §1.3 two-gate interaction; the contract floor check is a separate,
    /// independent gate enforced by the caller).
    pub(crate) fn check(&self, domain: &str, hwid: &str, requested_state: u32) -> Verdict {
        match self.lookup(domain, hwid) {
            None => Verdict::Deny {
                reason: "hwid_not_in_allowlist".to_string(),
            },
            Some(entry) => match entry.action {
                EntryAction::Deny => Verdict::Deny {
                    reason: if entry.reason.is_empty() {
                        "denied_by_allowlist".to_string()
                    } else {
                        format!("denied_by_allowlist: {}", entry.reason)
                    },
                },
                EntryAction::Allow => match entry.max_state {
                    Some(max) if requested_state > max => Verdict::Deny {
                        reason: format!(
                            "state_exceeds_max (requested {requested_state} > max {max})"
                        ),
                    },
                    _ => Verdict::Allow,
                },
            },
        }
    }

    /// Iterate the effective entries (used by the startup summary and, in the
    /// future, `optctl list-allow` over D-Bus).
    pub(crate) fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    /// Number of effective entries after precedence resolution.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Resolve a device's canonical HWID (kernel MODALIAS) from a sysfs attribute
/// path that optid is about to write.
///
/// For a per-device PM QoS path `…/<device>/power/pm_qos_resume_latency_us`,
/// the device directory is the grandparent of the attribute file, and its
/// `modalias` attribute holds the canonical form (`pci:v…`, `usb:…`, `acpi:…`)
/// per 0006 §1.1. Returns `None` if the modalias cannot be read (unbound driver,
/// slow-bus race, or a non-device path) — the caller treats that as default-deny.
pub(crate) fn hwid_from_attr_path(attr_path: &Path) -> Option<String> {
    // attr file -> `power` dir -> device dir
    let device_dir = attr_path.parent()?.parent()?;
    read_modalias(device_dir)
}

/// Resolve a device's canonical HWID directly from its sysfs device directory
/// (e.g. `/sys/bus/usb/devices/1-1`). Used by the WP-N5 runtime-PM actuator,
/// which works at device-directory granularity rather than per-attribute.
pub(crate) fn hwid_from_device_dir(device_dir: &Path) -> Option<String> {
    read_modalias(device_dir)
}

/// Resolve an HWID by walking up from `start` toward the filesystem root,
/// returning the first ancestor (including `start`) that has a readable
/// `modalias`. Used by the WP-N6 SATA ALPM path: a `scsi_host` directory has no
/// modalias of its own, but its backing AHCI/PCI controller (an ancestor) does.
/// The walk is bounded by the path depth, so it always terminates.
pub(crate) fn hwid_from_ancestors(start: &Path) -> Option<String> {
    // Resolve symlinks first: `/sys/class/scsi_host/hostN` is a symlink farm;
    // the real controller (with the modalias) is the canonical path's ancestor.
    // canonicalize is a no-op on the plain directory trees tests use.
    let real = fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let mut cur = Some(real.as_path());
    while let Some(dir) = cur {
        if let Some(hwid) = read_modalias(dir) {
            return Some(hwid);
        }
        cur = dir.parent();
    }
    None
}

fn read_modalias(device_dir: &Path) -> Option<String> {
    let raw = fs::read_to_string(device_dir.join("modalias")).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_deny_on_unknown_hwid() {
        let al = Allowlist::seeded();
        let v = al.check("nvme_apst", "pci:vDEADpBEEF", 0);
        assert_eq!(
            v,
            Verdict::Deny {
                reason: "hwid_not_in_allowlist".to_string()
            }
        );
        assert_eq!(v.deny_reason(), Some("hwid_not_in_allowlist"));
    }

    #[test]
    fn allow_on_seeded_baseline() {
        let al = Allowlist::seeded();
        // Samsung PM9A1 is seeded allow with max_state=3.
        let hwid = "pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02";
        assert!(al.check("nvme_apst", hwid, 0).is_allow());
        assert!(al.check("nvme_apst", hwid, 3).is_allow());
    }

    #[test]
    fn max_state_denies_with_reason() {
        let al = Allowlist::seeded();
        let hwid = "pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02";
        let v = al.check("nvme_apst", hwid, 4);
        match v {
            Verdict::Deny { reason } => assert!(
                reason.starts_with("state_exceeds_max"),
                "unexpected reason: {reason}"
            ),
            Verdict::Allow => panic!("state 4 should exceed max_state=3"),
        }
    }

    #[test]
    fn seeded_deny_entry_is_denied() {
        let al = Allowlist::seeded();
        // Intel Wireless-AC 9260 is a seeded pci_aspm DENY.
        let hwid = "pci:v00008086p00002526sv00008086sd00000010bc02sc80i00";
        let v = al.check("pci_aspm", hwid, 0);
        assert!(matches!(v, Verdict::Deny { .. }));
        assert!(v.deny_reason().unwrap().contains("denied_by_allowlist"));
    }

    #[test]
    fn seeded_version_is_non_empty() {
        assert!(!Allowlist::seeded().version().is_empty());
    }

    #[test]
    fn override_precedence_admin_beats_distro_beats_seeded() {
        let base = std::env::temp_dir().join(format!("optid_al_prec_{}", std::process::id()));
        let distro = base.join("distro");
        let admin = base.join("admin");
        let _ = fs::create_dir_all(&distro);
        let _ = fs::create_dir_all(&admin);

        // A novel HWID not in the seeded baseline: default-deny.
        let hwid = "pci:v00001234p00005678sv00001234sd00005678bc01sc08i02";
        assert!(matches!(
            Allowlist::seeded().check("nvme_apst", hwid, 0),
            Verdict::Deny { .. }
        ));

        // Distro allows it (max_state 1); admin then denies it. Admin must win.
        fs::write(
            distro.join("80-community.toml"),
            format!(
                "[[entry]]\ndomain=\"nvme_apst\"\nhwid=\"{hwid}\"\naction=\"allow\"\nmax_state=1\nreason=\"distro test\"\n"
            ),
        )
        .unwrap();

        // With only distro applied, state 0 is allowed, state 2 exceeds max.
        let distro_only = Allowlist::load_from(std::slice::from_ref(&distro));
        assert!(distro_only.check("nvme_apst", hwid, 0).is_allow());
        assert!(matches!(
            distro_only.check("nvme_apst", hwid, 2),
            Verdict::Deny { .. }
        ));

        fs::write(
            admin.join("90-admin.toml"),
            format!(
                "[[entry]]\ndomain=\"nvme_apst\"\nhwid=\"{hwid}\"\naction=\"deny\"\nreason=\"admin override\"\n"
            ),
        )
        .unwrap();

        let effective = Allowlist::load_from(&[distro.clone(), admin.clone()]);
        let v = effective.check("nvme_apst", hwid, 0);
        assert!(matches!(v, Verdict::Deny { .. }), "admin deny must win");
        let entry = effective.lookup("nvme_apst", hwid).unwrap();
        assert_eq!(entry.action, EntryAction::Deny);
        assert!(entry.source.contains("90-admin.toml"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn last_file_wins_within_dir() {
        let base = std::env::temp_dir().join(format!("optid_al_lww_{}", std::process::id()));
        let _ = fs::create_dir_all(&base);
        let hwid = "pci:vAAAApBBBBsvAAAAsdBBBBbc01sc08i02";

        fs::write(
            base.join("10-first.toml"),
            format!("[[entry]]\ndomain=\"nvme_apst\"\nhwid=\"{hwid}\"\naction=\"deny\"\nreason=\"first\"\n"),
        )
        .unwrap();
        fs::write(
            base.join("20-second.toml"),
            format!("[[entry]]\ndomain=\"nvme_apst\"\nhwid=\"{hwid}\"\naction=\"allow\"\nreason=\"second\"\n"),
        )
        .unwrap();

        let al = Allowlist::load_from(std::slice::from_ref(&base));
        assert!(al.check("nvme_apst", hwid, 0).is_allow(), "later file wins");
        assert_eq!(al.entries().filter(|e| e.hwid == hwid).count(), 1);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn hwid_from_attr_path_reads_modalias() {
        let base = std::env::temp_dir().join(format!("optid_al_modalias_{}", std::process::id()));
        let dev = base.join("0000:00:1f.3");
        let power = dev.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(dev.join("modalias"), "pci:v0000144Dp00009A36\n").unwrap();
        let attr = power.join("pm_qos_resume_latency_us");
        assert_eq!(
            hwid_from_attr_path(&attr).as_deref(),
            Some("pci:v0000144Dp00009A36")
        );
        // Missing modalias -> None (caller default-denies).
        let other = base.join("nodev").join("power").join("attr");
        assert_eq!(hwid_from_attr_path(&other), None);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn malformed_override_is_skipped_not_fatal() {
        let base = std::env::temp_dir().join(format!("optid_al_bad_{}", std::process::id()));
        let _ = fs::create_dir_all(&base);
        fs::write(base.join("00-broken.toml"), "this is not valid toml = = =").unwrap();
        // Should not panic; seeded baseline still intact.
        let al = Allowlist::load_from(std::slice::from_ref(&base));
        let hwid = "pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02";
        assert!(al.check("nvme_apst", hwid, 0).is_allow());
        let _ = fs::remove_dir_all(&base);
    }
}
