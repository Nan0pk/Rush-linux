//! The adaptive policy engine.
//!
//! `Policy` owns the deterministic, explainable decision logic that maps a
//! `Snapshot` to a `Decision`. Per `SPEC-northstar.md` §2, this module is a
//! **CONTRACT-SETTER**: it selects the active workload class, resolves the
//! active mode, and emits the `Action` set that `actuator::Actuator` will
//! apply behind the §3 actuation rule.
//!
//! The decision logic is intentionally a pure function of `(policy, snapshot,
//! override_mode, contracts)` — no I/O, no clocks, no global state. This is
//! what makes `optctl explain` truthful: every reason in the rendered report
//! is reproducible from the inputs.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::action::Action;
use crate::contracts::Contracts;
use crate::decision::Decision;
use crate::load_state::LoadState;
use crate::sensors::Snapshot;
use crate::workload::{Mode, WorkloadClass};

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct MemoryConfig {
    pub(crate) high_swappiness_requires_zram: bool,
}

/// F1 — Per-domain runtime mode. The single source of truth for "may optid
/// actuate this domain at all?" The existing gates (`--apply`, hardware
/// allowlist, contract floor, journaled revert) still apply on top of
/// `Actuate`; this enum is the *first* gate, evaluated in policy before an
/// `Action` is even constructed for the actuator.
///
/// Ordering matters: `Off < Observe < Actuate`. Comparisons are used by
/// `EffectiveConfig::allows_actuation` and the test suite to assert that
/// tightening a mode (Actuate → Observe → Off) can never authorize a
/// previously denied action.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DomainMode {
    /// Domain disabled. No actions are emitted, no observation is recorded.
    /// The domain is invisible to the runtime as if optid did not know about
    /// it. Use for untrusted or retired domains.
    Off,
    /// Domain observes the system and would actuate, but suppresses the
    /// final write. The would-be action is recorded in the decision report
    /// so operators can see what optid *would* do. Use for promotion
    /// evidence collection (D2 amendment, "observe, simulate, apply one
    /// reversible value").
    Observe,
    /// Domain may actuate when all other gates pass (`--apply`, allowlist,
    /// contract, journal). This is today's behavior for the v0.6 domains.
    Actuate,
}

impl Default for DomainMode {
    /// The migration-safety default. Existing v0.6 domains preserve their
    /// today's behavior (actuate when armed); new domains added after F1
    /// override this to `Off` via `Domain::default_mode`.
    fn default() -> Self {
        DomainMode::Actuate
    }
}

impl DomainMode {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            DomainMode::Off => "off",
            DomainMode::Observe => "observe",
            DomainMode::Actuate => "actuate",
        }
    }
}

/// F1 — The closed set of actuation domains optid knows about. One entry per
/// `Action` variant that performs a kernel write; mirrors the `Capability`
/// enum and the allowlist domain strings.
///
/// F1 repair (SystemdSetProperty backdoor): `SystemdSetProperty` is now
/// mapped to a real domain (`Domain::CgroupReweight`) so that operators can
/// gate cgroup reweighting via `[domains.cgroup_reweight] mode = ...` the
/// same way they gate every other lever. Prior to this repair
/// `Action::domain()` returned `None` for `SystemdSetProperty` and the
/// per-domain gate was silently bypassed, which is a fail-open hole for
/// the cgroup surface.
///
/// Keep this in lockstep with `crate::capability::Capability` and
/// `crate::action::Action::domain`. The `domain_round_trip` test enforces
/// the triple-stay-in-sync invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Domain {
    CpuEpp,
    PlatformProfile,
    VmSysctl,
    CpuDmaLatency,
    DeviceResumeLatency,
    RuntimePm,
    PcieAspm,
    SataAlpm,
    Backlight,
    /// F1 — `SystemdSetProperty` action (cgroup reweight via
    /// `systemctl set-property`). Mapped to a real domain so the
    /// per-domain gate applies; defaults to `Actuate` to preserve
    /// today's curated behavior.
    CgroupReweight,
    /// T1 — Read-only thermal sensing and budget model domain.
    Thermal,
}

impl Domain {
    /// Canonical config key and allowlist domain string. Matches the strings
    /// used in `crates/optid/src/capability.rs` and `data/allowlist.toml`.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Domain::CpuEpp => "cpu_epp",
            Domain::PlatformProfile => "platform_profile",
            Domain::VmSysctl => "vm_sysctl",
            Domain::CpuDmaLatency => "cpu_dma_latency",
            Domain::DeviceResumeLatency => "device_resume_latency",
            Domain::RuntimePm => "runtime_pm",
            Domain::PcieAspm => "pci_aspm",
            Domain::SataAlpm => "sata_alpm",
            Domain::Backlight => "backlight",
            // F1 — cgroup reweight domain key. Operators gate cgroup
            // reweighting via `[domains.cgroup_reweight] mode = ...`.
            Domain::CgroupReweight => "cgroup_reweight",
            // T1 — thermal sensing and budget domain key.
            Domain::Thermal => "thermal",
        }
    }

    /// All known domains, in the canonical order used by `EffectiveConfig`
    /// rendering. New domains are appended at the end.
    pub(crate) fn all() -> &'static [Domain] {
        &[
            Domain::CpuEpp,
            Domain::PlatformProfile,
            Domain::VmSysctl,
            Domain::CpuDmaLatency,
            Domain::DeviceResumeLatency,
            Domain::RuntimePm,
            Domain::PcieAspm,
            Domain::SataAlpm,
            Domain::Backlight,
            // F1 — cgroup reweight domain. Appended at the end so existing
            // status renderings stay stable.
            Domain::CgroupReweight,
            // T1 — thermal domain.
            Domain::Thermal,
        ]
    }

    /// Default mode for this domain when no `[domains.<name>]` entry exists.
    ///
    /// **Migration mapping (F1):** every variant of the v0.6+f1 closed set
    /// defaults to `Actuate`, preserving today's curated `policy.toml`
    /// behavior bit-for-bit. The compiler enforces the closed set: any
    /// new variant added to `Domain` produces a non-exhaustive-match
    /// error here, forcing the author to make an explicit Actuate/Off
    /// choice before their PR can compile. This is the F1 spec rule
    /// "new domains default `off`" enforced at the type level — a
    /// future domain *cannot* silently fail open to `Actuate`.
    ///
    /// The closed set is the same as `Domain::all()`. The explicit
    /// match (rather than a `Default` impl or a lookup table) is
    /// deliberate: it forces every author to think about the
    /// default for their new domain.
    ///
    /// **T1 exception (sensor-only domain):** the F1 plan states
    /// "new sensor-only domains default `observe` only when reads are
    /// side-effect-free". `Thermal` is read-only (hwmon/thermal_zone
    /// reads only, no actuation), so it defaults to `Observe` rather
    /// than `Actuate`. Adding a future sensor-only domain requires the
    /// same justification: prove the reads are side-effect-free, then
    /// default to `Observe`.
    pub(crate) fn default_mode(&self) -> DomainMode {
        match self {
            Domain::CpuEpp
            | Domain::PlatformProfile
            | Domain::VmSysctl
            | Domain::CpuDmaLatency
            | Domain::DeviceResumeLatency
            | Domain::RuntimePm
            | Domain::PcieAspm
            | Domain::SataAlpm
            | Domain::Backlight
            | Domain::CgroupReweight => DomainMode::Actuate,
            Domain::Thermal => DomainMode::Observe,
        }
    }

    /// The v0.6+f1 actuate closed set: domains whose default mode is
    /// `Actuate` because they perform kernel writes and preserve today's
    /// curated `policy.toml` behavior. Used by F1 contract tests so they
    /// can assert the migration-safety invariant without conflating it
    /// with sensor-only domains (which default to `Observe`).
    ///
    /// Adding a new actuate domain to this list requires updating the
    /// F1 contract tests — that's deliberate.
    #[cfg(test)]
    pub(crate) fn actuate_domains() -> &'static [Domain] {
        &[
            Domain::CpuEpp,
            Domain::PlatformProfile,
            Domain::VmSysctl,
            Domain::CpuDmaLatency,
            Domain::DeviceResumeLatency,
            Domain::RuntimePm,
            Domain::PcieAspm,
            Domain::SataAlpm,
            Domain::Backlight,
            Domain::CgroupReweight,
        ]
    }

    /// Sensor-only domains: read-only observations with no kernel writes.
    /// The F1 plan permits these to default to `Observe` when reads are
    /// side-effect-free. `Thermal` (T1) is the first such domain.
    #[cfg(test)]
    pub(crate) fn sensor_only_domains() -> &'static [Domain] {
        &[Domain::Thermal]
    }
}

/// F1 — The `[domains.<name>]` sub-table. Carries the per-domain runtime
/// `mode`. `deny_unknown_fields` makes any unrecognized key a parse error,
/// which is the "strict unknown-key validation" required by the F1 plan.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DomainConfig {
    #[serde(default = "default_domain_mode")]
    pub(crate) mode: DomainMode,
}

fn default_domain_mode() -> DomainMode {
    // Used by serde when `[domains.<name>]` is present but `mode` is omitted.
    // We use the migration-safety default (Actuate); operators who want to
    // disable a domain must say so explicitly.
    DomainMode::Actuate
}

impl Default for DomainConfig {
    fn default() -> Self {
        Self {
            mode: default_domain_mode(),
        }
    }
}

/// F1 — The top-level `[domains]` table. Holds an optional `DomainConfig`
/// per known domain. Missing domains use `Domain::default_mode()`. Unknown
/// domain names are rejected at parse time via `deny_unknown_fields`.
///
/// Example TOML:
///
/// ```toml
/// [domains.runtime_pm]
/// mode = "observe"          # collect promotion evidence
///
/// [domains.backlight]
/// mode = "off"              # disable the backlight lever entirely
/// ```
///
/// Domains not listed (e.g. `cpu_epp` in the example above) fall back to
/// `Domain::default_mode()` — `Actuate` for v0.6 domains.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DomainsConfig {
    #[serde(default)]
    pub(crate) cpu_epp: Option<DomainConfig>,
    #[serde(default)]
    pub(crate) platform_profile: Option<DomainConfig>,
    #[serde(default)]
    pub(crate) vm_sysctl: Option<DomainConfig>,
    #[serde(default)]
    pub(crate) cpu_dma_latency: Option<DomainConfig>,
    #[serde(default)]
    pub(crate) device_resume_latency: Option<DomainConfig>,
    #[serde(default)]
    pub(crate) runtime_pm: Option<DomainConfig>,
    #[serde(default)]
    pub(crate) pci_aspm: Option<DomainConfig>,
    #[serde(default)]
    pub(crate) sata_alpm: Option<DomainConfig>,
    #[serde(default)]
    pub(crate) backlight: Option<DomainConfig>,
    // F1 — cgroup reweight entry. Operators gate cgroup reweighting via
    // `[domains.cgroup_reweight] mode = "off|observe|actuate"`. Defaults
    // to Actuate via `Domain::default_mode` to preserve today's curated
    // behavior.
    #[serde(default)]
    pub(crate) cgroup_reweight: Option<DomainConfig>,
    #[serde(default)]
    pub(crate) thermal: Option<DomainConfig>,
}

impl DomainsConfig {
    /// Typed lookup by `Domain`. Returns the configured `DomainConfig` if the
    /// operator wrote one, or `None` if the domain should use its
    /// `default_mode()`.
    pub(crate) fn get(&self, domain: Domain) -> Option<&DomainConfig> {
        match domain {
            Domain::CpuEpp => self.cpu_epp.as_ref(),
            Domain::PlatformProfile => self.platform_profile.as_ref(),
            Domain::VmSysctl => self.vm_sysctl.as_ref(),
            Domain::CpuDmaLatency => self.cpu_dma_latency.as_ref(),
            Domain::DeviceResumeLatency => self.device_resume_latency.as_ref(),
            Domain::RuntimePm => self.runtime_pm.as_ref(),
            Domain::PcieAspm => self.pci_aspm.as_ref(),
            Domain::SataAlpm => self.sata_alpm.as_ref(),
            Domain::Backlight => self.backlight.as_ref(),
            Domain::CgroupReweight => self.cgroup_reweight.as_ref(),
            Domain::Thermal => self.thermal.as_ref(),
        }
    }
}

/// F1 — The resolved, per-domain effective mode after applying (a) the
/// domain's configured `mode` from `[domains.<name>]`, or (b) the domain's
/// `default_mode()` when no config entry exists. Consumed by the policy
/// decision path (to filter actions) and exposed to `optctl` via the status
/// surface (so operators can see exactly what optid is allowed to do).
#[derive(Debug, Clone)]
pub(crate) struct EffectiveConfig {
    pub(crate) domains: HashMap<Domain, DomainMode>,
}

impl EffectiveConfig {
    /// Build the effective config from a `Policy`'s `[domains]` section.
    pub(crate) fn from_policy(policy: &Policy) -> Self {
        let mut domains = HashMap::new();
        for &d in Domain::all() {
            let mode = policy
                .domains
                .get(d)
                .map(|c| c.mode)
                .unwrap_or_else(|| d.default_mode());
            domains.insert(d, mode);
        }
        Self { domains }
    }

    /// The effective mode for `domain`. Always returns a value because
    /// `from_policy` populates every known domain. Returns `Off` for
    /// unknown future domains as a fail-closed default.
    pub(crate) fn mode_for(&self, domain: Domain) -> DomainMode {
        self.domains
            .get(&domain)
            .copied()
            .unwrap_or(DomainMode::Off)
    }

    /// True iff the domain may emit `Action`s for actuation. `Observe` and
    /// `Off` both return false; the difference is whether the would-be
    /// action is surfaced in the decision report (see `decide_resolved`).
    pub(crate) fn allows_actuation(&self, domain: Domain) -> bool {
        self.mode_for(domain) == DomainMode::Actuate
    }

    /// Render the effective config as a stable, human-readable block. Used
    /// by `Decision::render` so `optctl status` shows the effective state.
    /// Format is `domain_name=mode` per line, in `Domain::all()` order.
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        for &d in Domain::all() {
            out.push_str(&format!(
                "domains.{}={}\n",
                d.as_str(),
                self.mode_for(d).as_str()
            ));
        }
        out
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Policy {
    /// v0.6 Phase B3: top-level `[policy]` section carrying the
    /// `competing_policy_daemons` list. Defaults to an empty list when the
    /// section is absent (e.g. Policy::default) so the conflict check is a
    /// no-op in tests that don't care about it.
    #[serde(default)]
    pub(crate) policy: PolicySection,
    pub(crate) thresholds: Thresholds,
    pub(crate) modes: Modes,
    pub(crate) memory: MemoryConfig,
    /// F1: top-level `[domains]` section carrying per-domain runtime modes.
    /// Defaults to an empty `DomainsConfig` when the section is absent,
    /// which means every domain uses its `default_mode()` (Actuate for v0.6
    /// domains). Operators override per-domain via
    /// `[domains.<name>] mode = "off|observe|actuate"`.
    #[serde(default)]
    pub(crate) domains: DomainsConfig,
    /// v0.6 Phase B1: `[shim]` top-level section carrying shim-specific
    /// configuration. Currently only the PPD sub-table is parsed; the
    /// GameMode sub-table (Phase B2) will land next. Defaults to an empty
    /// `ShimConfig` when the section is absent, which makes every shim
    /// use its hardcoded default mapping.
    #[serde(default)]
    pub(crate) shim: ShimConfig,
    /// v0.6 Phase C1: `[foreground]` top-level section. Defaults to
    /// `ForegroundConfig::default()` when absent. The config is parsed
    /// but not used in v0.6 — real compositor integration is deferred
    /// to v0.7.
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) foreground: crate::foreground::ForegroundConfig,
}

/// v0.6 Phase B1: the `[shim]` top-level section of
/// `config/optid/policy.toml`. Groups configuration for all compatibility
/// shims (PPD now; GameMode in Phase B2).
#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct ShimConfig {
    #[serde(default)]
    pub(crate) ppd: PpdShimConfig,
    /// v0.6 Phase B2: GameMode shim configuration. Defaults to a 30-minute
    /// TTL and `latency-critical` pin class.
    #[serde(default)]
    pub(crate) gamemode: GameModeShimConfig,
}

/// v0.6 Phase B1: the `[shim.ppd]` sub-table. Currently carries only the
/// optional `profiles` map that overrides the default PPD-profile →
/// optid-mode mapping. The default mapping (power-saver→battery,
/// balanced→auto, performance→performance) is hardcoded in
/// `shim::ppd::default_mode_for_profile` and used whenever `profiles`
/// doesn't override a given profile name.
///
/// Example TOML:
///
/// ```toml
/// [shim.ppd.profiles]
/// "performance" = "realtime"   # opt for realtime mode on PPD performance
/// "custom-game" = "performance"  # add a non-standard profile name
/// ```
#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct PpdShimConfig {
    #[serde(default)]
    pub(crate) profiles: HashMap<String, String>,
}

/// v0.6 Phase B2: the `[shim.gamemode]` sub-table. Carries the TTL for
/// implicit pin entries created by `RegisterGame`, and the workload class
/// to pin the game to. Defaults: TTL=1800s (30 min), pin_class=
/// "latency-critical".
///
/// Example TOML:
///
/// ```toml
/// [shim.gamemode]
/// ttl_sec = 1800                # 30 minutes
/// pin_class = "latency-critical"
/// ```
///
/// The TTL is best-effort: stale registrations are expired lazily on the
/// next `RegisterGame` / `QueryStatus` / `QueryStatusClient` call.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct GameModeShimConfig {
    #[serde(default = "default_gamemode_ttl_sec")]
    pub(crate) ttl_sec: u64,
    #[serde(default = "default_gamemode_pin_class")]
    pub(crate) pin_class: String,
}

fn default_gamemode_ttl_sec() -> u64 {
    1800 // 30 minutes
}

fn default_gamemode_pin_class() -> String {
    "latency-critical".to_string()
}

impl Default for GameModeShimConfig {
    fn default() -> Self {
        Self {
            ttl_sec: default_gamemode_ttl_sec(),
            pin_class: default_gamemode_pin_class(),
        }
    }
}

/// v0.6 Phase B3: the `[policy]` section of `config/optid/policy.toml`.
/// Currently carries only `competing_policy_daemons`; future top-level
/// policy switches land here.
#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct PolicySection {
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) owner: String,
    #[serde(default)]
    pub(crate) competing_policy_daemons: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Thresholds {
    pub(crate) cpu_pressure_perf_avg10: f32,
    pub(crate) memory_pressure_protect_avg10: f32,
    pub(crate) io_pressure_throttle_avg10: f32,
    pub(crate) hot_temp_c: f32,
    pub(crate) critical_temp_c: f32,
    pub(crate) low_battery_pct: u8,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Modes {
    pub(crate) battery: ModeConfig,
    pub(crate) balanced: ModeConfig,
    pub(crate) performance: ModeConfig,
    pub(crate) realtime: ModeConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub(crate) struct ModeConfig {
    pub(crate) cpu_epp: String,
    pub(crate) platform_profile: String,
    #[serde(default)]
    pub(crate) background_cpu_weight: Option<u32>,
    #[serde(default)]
    pub(crate) background_io_weight: Option<u32>,
    #[serde(default)]
    pub(crate) user_cpu_weight: Option<u32>,
    #[serde(default)]
    pub(crate) user_io_weight: Option<u32>,
    #[serde(default)]
    pub(crate) requires_controlled_rt_access: Option<bool>,
    #[serde(default)]
    pub(crate) vm_swappiness: Option<u32>,
    #[serde(default)]
    pub(crate) vm_dirty_background_bytes: Option<u64>,
    #[serde(default)]
    pub(crate) vm_dirty_bytes: Option<u64>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            policy: PolicySection::default(),
            thresholds: Thresholds {
                cpu_pressure_perf_avg10: 12.0,
                memory_pressure_protect_avg10: 5.0,
                io_pressure_throttle_avg10: 8.0,
                hot_temp_c: 82.0,
                critical_temp_c: 92.0,
                low_battery_pct: 20,
            },
            modes: Modes {
                battery: ModeConfig {
                    cpu_epp: "power".to_string(),
                    platform_profile: "low-power".to_string(),
                    background_cpu_weight: Some(25),
                    background_io_weight: Some(25),
                    user_cpu_weight: None,
                    user_io_weight: None,
                    requires_controlled_rt_access: None,
                    vm_swappiness: Some(60),
                    vm_dirty_background_bytes: Some(67108864),
                    vm_dirty_bytes: Some(134217728),
                },
                balanced: ModeConfig {
                    cpu_epp: "balance_performance".to_string(),
                    platform_profile: "balanced".to_string(),
                    background_cpu_weight: None,
                    background_io_weight: None,
                    user_cpu_weight: Some(150),
                    user_io_weight: Some(150),
                    requires_controlled_rt_access: None,
                    vm_swappiness: Some(100),
                    vm_dirty_background_bytes: Some(67108864),
                    vm_dirty_bytes: Some(134217728),
                },
                performance: ModeConfig {
                    cpu_epp: "performance".to_string(),
                    platform_profile: "performance".to_string(),
                    background_cpu_weight: None,
                    background_io_weight: None,
                    user_cpu_weight: Some(200),
                    user_io_weight: Some(200),
                    requires_controlled_rt_access: None,
                    vm_swappiness: Some(150),
                    vm_dirty_background_bytes: Some(67108864),
                    vm_dirty_bytes: Some(134217728),
                },
                realtime: ModeConfig {
                    cpu_epp: "performance".to_string(),
                    platform_profile: "performance".to_string(),
                    background_cpu_weight: None,
                    background_io_weight: None,
                    user_cpu_weight: Some(250),
                    user_io_weight: Some(200),
                    requires_controlled_rt_access: Some(true),
                    vm_swappiness: Some(10),
                    vm_dirty_background_bytes: None,
                    vm_dirty_bytes: None,
                },
            },
            memory: MemoryConfig {
                high_swappiness_requires_zram: true,
            },
            // F1: empty domains config = every domain uses its `default_mode()`
            // (Actuate for the v0.6 domains). Operators override per-domain
            // via `[domains.<name>] mode = "off|observe|actuate"`.
            domains: DomainsConfig::default(),
            // v0.6 Phase B1: empty shim config = use the hardcoded default
            // PPD profile → optid mode mapping. Operators override via
            // [shim.ppd.profiles] in policy.toml.
            shim: ShimConfig::default(),
            // v0.6 Phase C1: default foreground config (game_class =
            // "latency-critical"). Operators override via [foreground].
            foreground: crate::foreground::ForegroundConfig::default(),
        }
    }
}

impl Policy {
    /// Load a `Policy` from a TOML file at `path`. Missing or unparseable
    /// files fall back to `Policy::curated_baseline()` so a corrupt policy can
    /// never break the daemon — it only loses overrides. The load state is
    /// logged to stderr.
    ///
    /// Returns the loaded (or fallback) policy without the `LoadState`. Callers
    /// that need the load state (e.g., the run loop's `BootState` computation)
    /// must use `Policy::load_with_state` instead.
    pub(crate) fn load(path: &Path) -> Self {
        Self::load_with_state(path).0
    }

    /// Load a `Policy` from a TOML file at `path`, returning both the policy
    /// and the `LoadState` describing how the load went.
    ///
    /// Load states:
    /// - `Ok` — file present, parsed cleanly.
    /// - `Defaulted` — file missing; `curated_baseline()` used.
    /// - `Partial` — file present and parseable as TOML, but a structural
    ///   validation found a missing required section. `curated_baseline()` used.
    ///   (No partial-load path exists today; this state is reserved for future
    ///   per-section validators. A present-and-parseable file is currently
    ///   either fully `Ok` or fully `Invalid`.)
    /// - `Invalid` — file present but unparseable or structurally invalid.
    ///   `curated_baseline()` used.
    ///
    /// The fallback policy is `curated_baseline()`, **not** `default()`. The
    /// curated baseline is a deliberately conservative, independently-tested
    /// configuration; `default()` is the user-overridable defaults used by
    /// tests and must not be relied on as a safety floor.
    pub(crate) fn load_with_state(path: &Path) -> (Self, LoadState) {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!(
                    "optid: failed to read policy TOML from {}: {}. Using curated baseline.",
                    path.display(),
                    e
                );
                return (Self::curated_baseline(), LoadState::Defaulted);
            }
        };

        let parsed: Result<Self, _> = toml::from_str(&text);
        match parsed {
            Ok(policy) => {
                // Structural validation: every required section must be
                // present with at least the canonical mode set. A file that
                // parses but is missing `[modes]` is `Partial`, not `Ok`.
                if policy.modes_structurally_valid() {
                    (policy, LoadState::Ok)
                } else {
                    eprintln!(
                        "optid: policy TOML from {} parsed but is missing required sections. \
                         Using curated baseline.",
                        path.display()
                    );
                    (Self::curated_baseline(), LoadState::Partial)
                }
            }
            Err(e) => {
                eprintln!(
                    "optid: failed to parse policy TOML from {}: {}. Using curated baseline.",
                    path.display(),
                    e
                );
                (Self::curated_baseline(), LoadState::Invalid)
            }
        }
    }

    /// The curated safety-floor policy. Used when `policy.toml` is missing,
    /// partial, or invalid. This is **not** `default()` — it is a
    /// deliberately conservative, independently-tested configuration that
    /// prioritizes "do no harm" over "tune aggressively".
    ///
    /// Concretely, the curated baseline:
    /// - Uses `balanced` mode values for all four mode slots (battery,
    ///   balanced, performance, realtime). This means: even if the run loop
    ///   somehow arms `apply` with this policy, the writes it produces are
    ///   the balanced-mode writes, which are the least aggressive.
    /// - Sets thresholds to conservative defaults (low CPU pressure trigger,
    ///   low battery threshold).
    /// - Disables the `high_swappiness_requires_zram` gate so the curated
    ///   baseline never blocks a vm.swappiness write that the operator
    ///   might rely on for recovery.
    /// - Carries an empty `[policy]` section (no competing-daemon list) so
    ///   the conflict check is a no-op and does not block startup.
    /// - Carries empty shim config so the PPD/GameMode shims use their
    ///   hardcoded default mappings.
    ///
    /// The curated baseline is the policy that gets applied when
    /// `apply_armed == false` (see `load_state::BootState`). Even when
    /// `apply_armed == true`, if the policy is `Defaulted`/`Partial`/`Invalid`,
    /// the run loop must NOT arm `apply` — so in practice the curated
    /// baseline is only applied as the `baseline_armed` curated baseline
    /// writes (a separate, smaller surface; see `Actuator::apply_baseline`).
    pub(crate) fn curated_baseline() -> Self {
        // Reuse `default()` for the structural shape (it constructs a valid
        // `Policy` with all sections), then override the mode values to the
        // balanced-mode defaults. This keeps the curated baseline in sync
        // with `default()`'s structural invariants while pinning the
        // *values* to the conservative floor.
        //
        // The contract is: `curated_baseline()` is safe by construction,
        // and is independently tested (see `tests::curated_baseline_*`).
        // If a future change to `default()` makes it unsafe as a fallback,
        // the curated-baseline tests must catch it.
        let mut p = Self::default();
        let balanced = p.modes.balanced.clone();
        p.modes.battery = balanced.clone();
        p.modes.performance = balanced.clone();
        p.modes.realtime = balanced;
        p
    }

    /// Structural validation: every mode section must carry a non-empty
    /// `cpu_epp` and `platform_profile`. A policy that parses but is missing
    /// these is structurally invalid and must fall back to the curated
    /// baseline (LoadState::Partial).
    fn modes_structurally_valid(&self) -> bool {
        let modes = [
            &self.modes.battery,
            &self.modes.balanced,
            &self.modes.performance,
            &self.modes.realtime,
        ];
        for m in modes {
            if m.cpu_epp.is_empty() || m.platform_profile.is_empty() {
                return false;
            }
        }
        true
    }

    /// Classify the current snapshot into one of the six SPEC §1 workload
    /// classes (Idle, Light, Interactive, LatencyCritical, Throughput,
    /// VmGuest). Pure function. Highest precedence: explicit pins (global
    /// pin beats foreground pin beats telemetry). Platform-forced
    /// `VmGuest` class wins over telemetry but loses to explicit pins.
    pub(crate) fn classify(&self, snapshot: &Snapshot) -> (WorkloadClass, String) {
        if let Some(pinned) = snapshot.global_pinned_class {
            return (pinned, "pinned override (global)".to_string());
        }
        if let Some(pinned) = snapshot.pinned_class {
            return (pinned, "pinned override for foreground app".to_string());
        }

        // v0.6 Phase C2: when DMI reports a hypervisor vendor, return
        // the platform-forced VmGuest class. Explicit pins (handled
        // above) still win, so operators can override.
        if snapshot.is_vm_guest {
            return (
                WorkloadClass::VmGuest,
                "platform-forced vm.guest (DMI reports hypervisor vendor)".to_string(),
            );
        }

        let load = snapshot.loadavg_1.unwrap_or(0.0);
        let cpu_pressure = snapshot.cpu_pressure.map(|p| p.avg10).unwrap_or(0.0);
        let mem_pressure = snapshot.memory_pressure.map(|p| p.avg10).unwrap_or(0.0);
        let io_pressure = snapshot.io_pressure.map(|p| p.avg10).unwrap_or(0.0);

        if load >= 4.0
            && (cpu_pressure >= self.thresholds.cpu_pressure_perf_avg10
                || io_pressure >= self.thresholds.io_pressure_throttle_avg10)
        {
            return (
                WorkloadClass::Throughput,
                format!(
                    "high load ({:.2}) and high pressure (cpu: {:.2}, io: {:.2})",
                    load, cpu_pressure, io_pressure
                ),
            );
        }

        if (1.5..4.0).contains(&load)
            && cpu_pressure >= self.thresholds.cpu_pressure_perf_avg10
            && snapshot.on_ac == Some(true)
        {
            return (
                WorkloadClass::LatencyCritical,
                format!(
                    "moderate load ({:.2}) with cpu pressure ({:.2}) on AC",
                    load, cpu_pressure
                ),
            );
        }

        if load >= 0.5 || cpu_pressure > 2.0 || mem_pressure > 2.0 {
            return (
                WorkloadClass::Interactive,
                format!(
                    "active usage: load={:.2}, cpu_pressure={:.2}, mem_pressure={:.2}",
                    load, cpu_pressure, mem_pressure
                ),
            );
        }

        if load > 0.05 || cpu_pressure > 0.1 {
            return (
                WorkloadClass::Light,
                format!(
                    "low activity: load={:.2}, cpu_pressure={:.2}",
                    load, cpu_pressure
                ),
            );
        }

        (
            WorkloadClass::Idle,
            format!(
                "system idle: load={:.2}, cpu_pressure={:.2}",
                load, cpu_pressure
            ),
        )
    }

    #[allow(dead_code)]
    pub(crate) fn decide(
        &self,
        snapshot: &Snapshot,
        requested: Mode,
        workload_class: WorkloadClass,
        workload_reason: String,
        contracts: &Contracts,
    ) -> Decision {
        self.decide_resolved(
            snapshot,
            requested,
            workload_class,
            workload_reason,
            contracts,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decide_resolved(
        &self,
        snapshot: &Snapshot,
        requested: Mode,
        workload_class: WorkloadClass,
        workload_reason: String,
        contracts: &Contracts,
        resolved_mode: Option<Mode>,
        mode_hysteresis_reason: Option<String>,
    ) -> Decision {
        let effective_mode = resolved_mode.unwrap_or_else(|| match requested {
            Mode::Auto => self.auto_mode(snapshot),
            explicit => explicit,
        });

        let mut reasons = Vec::new();
        let mut actions = Vec::new();

        if let Some(reason) = mode_hysteresis_reason {
            reasons.push(reason);
        }

        if requested != Mode::Auto {
            reasons.push(format!("manual mode override: {requested}"));
        }

        if snapshot.on_ac == Some(false) {
            reasons.push("system is on battery".to_string());
        }

        if let Some(pct) = snapshot.battery_pct {
            if pct <= self.thresholds.low_battery_pct {
                reasons.push(format!("battery is low: {pct}%"));
            }
        }

        if let Some(temp) = snapshot.thermal_c() {
            if temp >= self.thresholds.critical_temp_c {
                reasons.push(format!("critical thermal pressure: {temp:.1}C"));
            } else if temp >= self.thresholds.hot_temp_c {
                reasons.push(format!("high thermal pressure: {temp:.1}C"));
            }
        }

        if let Some(cpu) = snapshot.cpu_pressure {
            if cpu.avg10 >= self.thresholds.cpu_pressure_perf_avg10 {
                reasons.push(format!("CPU pressure avg10 is {:.2}", cpu.avg10));
            }
        }

        if let Some(memory) = snapshot.memory_pressure {
            if memory.avg10 >= self.thresholds.memory_pressure_protect_avg10 {
                reasons.push(format!("memory pressure avg10 is {:.2}", memory.avg10));
                actions.push(Action::systemd_set_property(
                    "user.slice".to_string(),
                    vec!["MemoryLow=256M".to_string()],
                    "protect active user sessions from reclaim pressure".to_string(),
                ));
                actions.push(Action::systemd_set_property(
                    "background.slice".to_string(),
                    vec![
                        "CPUWeight=50".to_string(),
                        "IOWeight=50".to_string(),
                        "MemoryHigh=75%".to_string(),
                    ],
                    "throttle background work during memory pressure".to_string(),
                ));
            }
        }

        if let Some(io) = snapshot.io_pressure {
            if io.avg10 >= self.thresholds.io_pressure_throttle_avg10 {
                reasons.push(format!("I/O pressure avg10 is {:.2}", io.avg10));
                actions.push(Action::systemd_set_property(
                    "background.slice".to_string(),
                    vec!["IOWeight=25".to_string()],
                    "reduce background I/O interference".to_string(),
                ));
            }
        }

        let mode_config = match effective_mode {
            Mode::Battery => &self.modes.battery,
            Mode::Balanced => &self.modes.balanced,
            Mode::Performance => &self.modes.performance,
            Mode::Realtime => &self.modes.realtime,
            Mode::Auto => unreachable!("auto is resolved before action planning"),
        };

        actions.push(Action::cpu_epp(
            mode_config.cpu_epp.clone(),
            match effective_mode {
                Mode::Battery => "prefer battery life through CPU energy preference".to_string(),
                Mode::Balanced => {
                    "keep foreground responsiveness without full turbo bias".to_string()
                }
                Mode::Performance => {
                    "reduce CPU wakeup and ramp latency for sustained load".to_string()
                }
                Mode::Realtime => "minimize latency for realtime mode".to_string(),
                _ => "".to_string(),
            },
        ));

        actions.push(Action::platform_profile(
            mode_config.platform_profile.clone(),
            match effective_mode {
                Mode::Battery => "request low-power platform profile".to_string(),
                Mode::Balanced => "request balanced platform profile".to_string(),
                Mode::Performance => "request performance platform profile".to_string(),
                Mode::Realtime => "avoid firmware power-save latency in realtime mode".to_string(),
                _ => "".to_string(),
            },
        ));

        let mut bg_properties = Vec::new();
        if let Some(w) = mode_config.background_cpu_weight {
            bg_properties.push(format!("CPUWeight={w}"));
        }
        if let Some(w) = mode_config.background_io_weight {
            bg_properties.push(format!("IOWeight={w}"));
        }
        if !bg_properties.is_empty() {
            actions.push(Action::systemd_set_property(
                "background.slice".to_string(),
                bg_properties,
                "deprioritize background services on battery".to_string(),
            ));
        }

        let mut user_properties = Vec::new();
        if let Some(w) = mode_config.user_cpu_weight {
            user_properties.push(format!("CPUWeight={w}"));
        }
        if let Some(w) = mode_config.user_io_weight {
            user_properties.push(format!("IOWeight={w}"));
        }
        if !user_properties.is_empty() {
            actions.push(Action::systemd_set_property(
                "user.slice".to_string(),
                user_properties,
                match effective_mode {
                    Mode::Balanced => "favor interactive user sessions".to_string(),
                    Mode::Performance => "boost foreground user work".to_string(),
                    Mode::Realtime => "prioritize controlled realtime user workload".to_string(),
                    _ => "".to_string(),
                },
            ));
        }

        if self.memory.high_swappiness_requires_zram && !snapshot.zram_swap_active {
            reasons.push("vm.* actuation skipped: zram swap is not active".to_string());
        } else {
            // vm.swappiness
            if let Some(swappiness) = mode_config.vm_swappiness {
                actions.push(Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/swappiness"),
                    swappiness.to_string(),
                    "adjust swappiness for current mode".to_string(),
                ));
            }

            // vm.dirty_background_bytes
            if let Some(bytes) = mode_config.vm_dirty_background_bytes {
                actions.push(Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/dirty_background_bytes"),
                    bytes.to_string(),
                    "adjust dirty background bytes for current mode".to_string(),
                ));
            }

            // vm.dirty_bytes
            if let Some(bytes) = mode_config.vm_dirty_bytes {
                actions.push(Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/dirty_bytes"),
                    bytes.to_string(),
                    "adjust dirty bytes for current mode".to_string(),
                ));
            }
        }

        if snapshot
            .thermal_c()
            .is_some_and(|temp| temp >= self.thresholds.critical_temp_c)
        {
            actions.push(Action::cpu_epp(
                "balance_power".to_string(),
                "override performance bias because thermals are critical".to_string(),
            ));
            actions.push(Action::platform_profile(
                "balanced".to_string(),
                "back off platform profile under critical thermals".to_string(),
            ));
        }

        // PM QoS wakeup latency (CPU)
        let floors = contracts.resolve(workload_class);
        let cpu_wakeup_latency = Some(floors.cpu_wakeup_latency);
        let device_resume_latency = Some(floors.device_resume_latency);

        let reason_cpu = format!(
            "class={}, floor={}us, row=contracts.{}",
            workload_class, floors.cpu_wakeup_latency, workload_class
        );
        actions.push(Action::CpuDmaLatency {
            value: Some(floors.cpu_wakeup_latency as i32),
            reason: reason_cpu,
        });

        // PM QoS resume latency (Per-device)
        for path in &snapshot.pm_qos_device_paths {
            let reason_dev = format!(
                "class={}, floor={}us, row=contracts.{}",
                workload_class, floors.device_resume_latency, workload_class
            );
            actions.push(Action::DeviceResumeLatency {
                path: path.clone(),
                value: Some(floors.device_resume_latency as i32),
                reason: reason_dev,
            });
        }

        // WP-N5: runtime-PM autosuspend. Conservative "battery-idle" trigger
        // (Decision B): only nominate devices when on battery AND the workload
        // class is idle. The actuator gates each device on the N4 allowlist,
        // skips network devices with an active link, preserves wakeup, and
        // journals for revert-on-stop — so nominating broadly here is safe.
        if snapshot.on_ac == Some(false) && workload_class == WorkloadClass::Idle {
            for device_dir in &snapshot.runtime_pm_device_paths {
                actions.push(Action::RuntimePm {
                    device_dir: device_dir.clone(),
                    autosuspend_delay_ms:
                        crate::actuators::runtime_pm::DEFAULT_AUTOSUSPEND_DELAY_MS,
                    reason: format!(
                        "battery-idle runtime PM (class={workload_class}, allowlist-gated)"
                    ),
                });
            }

            // WP-N6 PCIe ASPM: enable L1 substates on battery-idle. Allowlist-gated
            // (domain pci_aspm); CNVi devices are skipped in the actuator.
            for device_dir in &snapshot.pcie_aspm_device_paths {
                actions.push(Action::PcieAspm {
                    device_dir: device_dir.clone(),
                    enable: true,
                    reason: format!(
                        "battery-idle PCIe ASPM (class={workload_class}, allowlist-gated)"
                    ),
                });
            }

            // WP-N6 SATA ALPM: med_power_with_dipm on battery-idle. Allowlist-gated
            // (domain sata_alpm via the host's backing PCI controller).
            for host_dir in &snapshot.sata_alpm_host_paths {
                actions.push(Action::SataAlpm {
                    host_dir: host_dir.clone(),
                    policy: crate::actuators::storage::DEFAULT_ALPM_POLICY.to_string(),
                    reason: format!(
                        "battery-idle SATA ALPM (class={workload_class}, allowlist-gated)"
                    ),
                });
            }

            // WP-N7 display depth: dim the panel backlight toward the interactive
            // floor on battery-idle. Allowlist-gated (domain backlight); the
            // actuator floor-clamps so the screen never goes black.
            if let Some(backlight) = &snapshot.selected_backlight {
                actions.push(Action::Backlight {
                    device_dir: backlight.clone(),
                    target_pct: crate::actuators::display::DEFAULT_TARGET_PCT,
                    reason: format!(
                        "battery-idle backlight floor (class={workload_class}, allowlist-gated)"
                    ),
                });
            }
        }

        if reasons.is_empty() {
            reasons.push("default adaptive policy".to_string());
        }

        // F1 — Apply the per-domain effective-mode gate. Actions whose
        // domain is `Off` or `Observe` are filtered out before they reach
        // the actuator. `Observe` captures each would-be action's
        // human-readable description into `suppressed_actions` so the
        // operator can see exactly what optid *would* have done (the
        // F1 plan's "would-be action is recorded in the decision report"
        // contract). `Off` is silent (the domain is invisible by
        // design). F1 also repaired the `SystemdSetProperty` backdoor:
        // cgroup reweighting now flows through this same gate via
        // `Domain::CgroupReweight` — operators can set
        // `[domains.cgroup_reweight] mode = "off"` to suppress cgroup
        // reweighting the same way they suppress any other lever.
        let effective = EffectiveConfig::from_policy(self);
        let mut suppressed_actions: Vec<(Domain, String)> = Vec::new();
        let actions: Vec<Action> = actions
            .into_iter()
            .filter(|a| {
                // Per the F1 repair, every Action variant returns
                // Some(domain) (the `SystemdSetProperty` backdoor is
                // closed). The `let-else` keeps the closure future-proof
                // in case a future variant ever returns `None`.
                let Some(d) = a.domain() else {
                    return true;
                };
                if effective.allows_actuation(d) {
                    return true;
                }
                if effective.mode_for(d) == DomainMode::Observe {
                    // F1 repair: capture the would-be action's
                    // description so the operator can see *what*
                    // optid would have done, not just *that* a
                    // domain was suppressed. The deduplication
                    // step below keeps the report readable when
                    // many devices of the same domain are
                    // nominated (e.g., 10 USB devices under
                    // runtime_pm).
                    suppressed_actions.push((d, a.describe()));
                }
                false
            })
            .collect();

        // Deduplicate suppressed observe-mode actions by (domain,
        // description). When multiple actions of the same domain
        // (e.g., runtime_pm for several USB devices) are suppressed,
        // we collapse the *reason* line into one "domain X in observe
        // mode" entry, but each distinct would-be action description
        // is still listed once in the `suppressed_actions` block
        // rendered by `Decision::render`.
        suppressed_actions.sort_by(|a, b| (a.0.as_str(), &a.1).cmp(&(b.0.as_str(), &b.1)));
        suppressed_actions.dedup();
        let mut seen_observe_reasons: std::collections::BTreeSet<&'static str> =
            std::collections::BTreeSet::new();
        // Iterate by reference; each element is `&(Domain, String)`,
        // so the pattern must be `&(d, _)` (Rust does not implicitly
        // deref tuple patterns inside `for` loops).
        for &(d, _) in &suppressed_actions {
            if seen_observe_reasons.insert(d.as_str()) {
                reasons.push(format!(
                    "domain {} in observe mode: action suppressed, would-act logged",
                    d.as_str()
                ));
            }
        }

        Decision {
            mode: effective_mode,
            reasons,
            actions,
            workload_class,
            workload_reason,
            cpu_wakeup_latency,
            device_resume_latency,
            // F1: attach the effective config so `Decision::render` can
            // surface it in the status report.
            effective_config: effective,
            // F1: attach the observe-mode would-be actions so the
            // operator can see exactly what optid would have done
            // without those actions reaching the actuator.
            suppressed_actions,
        }
    }

    pub(crate) fn is_critical_thermal(&self, snapshot: &Snapshot) -> bool {
        snapshot
            .thermal_c()
            .is_some_and(|temp| temp >= self.thresholds.critical_temp_c)
    }

    pub(crate) fn auto_mode(&self, snapshot: &Snapshot) -> Mode {
        if self.is_critical_thermal(snapshot) {
            return Mode::Balanced;
        }

        // v0.6 Phase C2: VM-guest sensor weighting. The hypervisor
        // scheduler distorts PSI avg10. Apply the proposal's weighting:
        //   - PSI avg10 × 0.5 (de-rate the distorted signal)
        //   - loadavg is the primary signal
        if snapshot.is_vm_guest {
            let load = snapshot.loadavg_1.unwrap_or(0.0);
            let cpu_pressure_dilated = snapshot.cpu_pressure.map(|p| p.avg10 * 0.5).unwrap_or(0.0);
            if snapshot.on_ac == Some(false) {
                if snapshot
                    .battery_pct
                    .is_some_and(|pct| pct <= self.thresholds.low_battery_pct)
                {
                    return Mode::Battery;
                }
                if load >= 4.0 || cpu_pressure_dilated >= self.thresholds.cpu_pressure_perf_avg10 {
                    return Mode::Balanced;
                }
                return Mode::Battery;
            }
            if load >= 4.0 || cpu_pressure_dilated >= self.thresholds.cpu_pressure_perf_avg10 {
                return Mode::Performance;
            }
            return Mode::Balanced;
        }

        if snapshot.on_ac == Some(false) {
            if snapshot
                .battery_pct
                .is_some_and(|pct| pct <= self.thresholds.low_battery_pct)
            {
                return Mode::Battery;
            }

            if snapshot
                .cpu_pressure
                .is_some_and(|p| p.avg10 >= self.thresholds.cpu_pressure_perf_avg10)
            {
                return Mode::Balanced;
            }

            return Mode::Battery;
        }

        if snapshot
            .cpu_pressure
            .is_some_and(|p| p.avg10 >= self.thresholds.cpu_pressure_perf_avg10)
        {
            return Mode::Performance;
        }

        Mode::Balanced
    }
}

#[cfg(test)]
mod tests {
    //! v0.6 Phase C2 — `vm.guest` workload class tests.

    use super::*;
    use crate::sensors::Pressure;

    fn vm_snapshot(
        loadavg_1: Option<f32>,
        cpu_pressure_avg10: Option<f32>,
        on_ac: Option<bool>,
    ) -> Snapshot {
        let cpu_pressure = cpu_pressure_avg10.map(|avg10| Pressure {
            avg10,
            avg60: avg10,
            avg300: avg10,
            total: 0,
        });
        Snapshot {
            timestamp: 0,
            on_ac,
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1,
            cpu_pressure,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: true,
            thermal_sensors: Vec::new(),
            fan_sensors: Vec::new(),
            thermal_budget: crate::thermal::ThermalBudget::default(),
        }
    }

    #[test]
    fn vm_guest_classify_returns_vm_guest_when_is_vm_guest_true() {
        let policy = Policy::default();
        let snapshot = vm_snapshot(Some(8.0), Some(50.0), Some(true));
        let (class, reason) = policy.classify(&snapshot);
        assert_eq!(class, WorkloadClass::VmGuest);
        assert!(reason.contains("vm.guest"), "reason: {reason}");
        assert!(reason.contains("DMI"), "reason: {reason}");
    }

    #[test]
    fn vm_guest_classify_returns_vm_guest_regardless_of_load() {
        let policy = Policy::default();
        let snapshot = vm_snapshot(Some(64.0), Some(99.0), Some(true));
        let (class, _) = policy.classify(&snapshot);
        assert_eq!(class, WorkloadClass::VmGuest);
    }

    #[test]
    fn vm_guest_classify_returns_vm_guest_even_when_idle() {
        let policy = Policy::default();
        let snapshot = vm_snapshot(Some(0.0), Some(0.0), Some(true));
        let (class, _) = policy.classify(&snapshot);
        assert_eq!(class, WorkloadClass::VmGuest);
    }

    #[test]
    fn vm_guest_classify_returns_vm_guest_on_battery() {
        let policy = Policy::default();
        let snapshot = vm_snapshot(Some(2.0), Some(5.0), Some(false));
        let (class, _) = policy.classify(&snapshot);
        assert_eq!(class, WorkloadClass::VmGuest);
    }

    #[test]
    fn vm_guest_classify_explicit_global_pin_wins_over_vm_guest() {
        let policy = Policy::default();
        let mut snapshot = vm_snapshot(Some(2.0), Some(5.0), Some(true));
        snapshot.global_pinned_class = Some(WorkloadClass::Throughput);
        let (class, reason) = policy.classify(&snapshot);
        assert_eq!(class, WorkloadClass::Throughput);
        assert!(reason.contains("pinned override (global)"));
    }

    #[test]
    fn vm_guest_classify_explicit_app_pin_wins_over_vm_guest() {
        let policy = Policy::default();
        let mut snapshot = vm_snapshot(Some(2.0), Some(5.0), Some(true));
        snapshot.pinned_class = Some(WorkloadClass::LatencyCritical);
        let (class, reason) = policy.classify(&snapshot);
        assert_eq!(class, WorkloadClass::LatencyCritical);
        assert!(reason.contains("pinned override for foreground app"));
    }

    #[test]
    fn vm_guest_classify_non_vm_uses_regular_logic_throughput() {
        let policy = Policy::default();
        let mut snapshot = vm_snapshot(Some(8.0), Some(50.0), Some(true));
        snapshot.is_vm_guest = false;
        let (class, _) = policy.classify(&snapshot);
        assert_eq!(class, WorkloadClass::Throughput);
    }

    #[test]
    fn vm_guest_classify_non_vm_uses_regular_logic_idle() {
        let policy = Policy::default();
        let mut snapshot = vm_snapshot(Some(0.0), Some(0.0), Some(true));
        snapshot.is_vm_guest = false;
        let (class, _) = policy.classify(&snapshot);
        assert_eq!(class, WorkloadClass::Idle);
    }

    #[test]
    fn vm_guest_auto_mode_high_load_returns_performance_on_ac() {
        let policy = Policy::default();
        let snapshot = vm_snapshot(Some(8.0), Some(0.0), Some(true));
        assert_eq!(policy.auto_mode(&snapshot), Mode::Performance);
    }

    #[test]
    fn vm_guest_auto_mode_low_load_returns_balanced_on_ac() {
        let policy = Policy::default();
        let snapshot = vm_snapshot(Some(0.5), Some(0.0), Some(true));
        assert_eq!(policy.auto_mode(&snapshot), Mode::Balanced);
    }

    #[test]
    fn vm_guest_auto_mode_psi_is_dilated_by_half() {
        // PSI avg10 = 20.0 in a VM is treated as 10.0 (× 0.5). With
        // loadavg = 0.5 and threshold = 12.0, the dilated 10.0 is BELOW
        // the threshold → Balanced. Without dilation, 20.0 would be
        // ABOVE 12.0 → Performance.
        let policy = Policy::default();
        let snapshot = vm_snapshot(Some(0.5), Some(20.0), Some(true));
        assert_eq!(
            policy.auto_mode(&snapshot),
            Mode::Balanced,
            "PSI 20.0 × 0.5 = 10.0 < threshold 12.0 → Balanced"
        );
    }

    #[test]
    fn vm_guest_auto_mode_dilated_psi_above_threshold_returns_performance() {
        // PSI avg10 = 30.0 in a VM is treated as 15.0 (× 0.5). With
        // loadavg = 0.5 and threshold = 12.0, the dilated 15.0 is ABOVE
        // the threshold → Performance.
        let policy = Policy::default();
        let snapshot = vm_snapshot(Some(0.5), Some(30.0), Some(true));
        assert_eq!(
            policy.auto_mode(&snapshot),
            Mode::Performance,
            "PSI 30.0 × 0.5 = 15.0 > threshold 12.0 → Performance"
        );
    }

    #[test]
    fn vm_guest_auto_mode_on_battery_low_battery_returns_battery() {
        let policy = Policy::default();
        let mut snapshot = vm_snapshot(Some(8.0), Some(50.0), Some(false));
        snapshot.battery_pct = Some(10);
        assert_eq!(policy.auto_mode(&snapshot), Mode::Battery);
    }

    #[test]
    fn vm_guest_auto_mode_on_battery_high_load_returns_balanced() {
        let policy = Policy::default();
        let mut snapshot = vm_snapshot(Some(8.0), Some(0.0), Some(false));
        snapshot.battery_pct = Some(80);
        assert_eq!(policy.auto_mode(&snapshot), Mode::Balanced);
    }

    #[test]
    fn vm_guest_auto_mode_on_battery_low_load_returns_battery() {
        let policy = Policy::default();
        let mut snapshot = vm_snapshot(Some(0.5), Some(0.0), Some(false));
        snapshot.battery_pct = Some(80);
        assert_eq!(policy.auto_mode(&snapshot), Mode::Battery);
    }

    #[test]
    fn vm_guest_auto_mode_critical_thermal_overrides_to_balanced() {
        let policy = Policy::default();
        let mut snapshot = vm_snapshot(Some(8.0), Some(50.0), Some(true));
        snapshot.max_temp_millic = Some(95_000);
        assert_eq!(policy.auto_mode(&snapshot), Mode::Balanced);
    }

    #[test]
    fn vm_guest_auto_mode_non_vm_uses_regular_psi_threshold() {
        let policy = Policy::default();
        let mut snapshot = vm_snapshot(Some(0.5), Some(20.0), Some(true));
        snapshot.is_vm_guest = false;
        assert_eq!(policy.auto_mode(&snapshot), Mode::Performance);
    }
}

#[cfg(test)]
mod f1_tests {
    //! F1 — `DomainMode`, `Domain`, `DomainsConfig`, `EffectiveConfig`, and
    //! the migration-safety default. These tests enforce the F1 plan's
    //! contracts:
    //!
    //! - existing v0.6 domains default to `Actuate` (today's behavior preserved);
    //! - `[domains.<name>] mode = "off|observe|actuate"` is parsed and applied;
    //! - unknown domain keys, unknown fields, and invalid modes fail closed at
    //!   parse time;
    //! - `Action::domain()` is consistent with `Capability::allowlist_domain`;
    //! - `decide_resolved` filters actions by effective mode and surfaces a
    //!   reason for observe-mode suppression;
    //! - `Decision::render` includes the effective config block so `optctl
    //!   status` prints the effective state (the "dry-run prints the effective
    //!   state" plan contract).

    use super::*;
    use crate::action::Action;
    use crate::contracts::Contracts;
    use crate::sensors::Pressure;
    use crate::workload::{Mode, WorkloadClass};
    use std::path::PathBuf;

    /// Snapshot that triggers every domain's action emission path so we can
    /// observe filtering. Battery + Idle triggers runtime_pm, pci_aspm,
    /// sata_alpm, and backlight. The other domains (cpu_epp,
    /// platform_profile, vm_sysctl, cpu_dma_latency, device_resume_latency)
    /// are emitted unconditionally by `decide_resolved`.
    fn f1_snapshot_all_domains() -> Snapshot {
        let cpu_pressure = Pressure {
            avg10: 0.0,
            avg60: 0.0,
            avg300: 0.0,
            total: 0,
        };
        Snapshot {
            timestamp: 0,
            on_ac: Some(false),
            battery_pct: Some(80),
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: Some(cpu_pressure),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: true,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: vec![PathBuf::from("/sys/devices/pci0000:00")],
            runtime_pm_device_paths: vec![PathBuf::from("/sys/bus/usb/devices/1-1")],
            pcie_aspm_device_paths: vec![PathBuf::from("/sys/bus/pci/devices/0000:00:00.0")],
            sata_alpm_host_paths: vec![PathBuf::from("/sys/class/scsi_host/host0")],
            selected_backlight: Some(PathBuf::from("/sys/class/backlight/intel_backlight")),
            is_vm_guest: false,
            thermal_sensors: Vec::new(),
            fan_sensors: Vec::new(),
            thermal_budget: crate::thermal::ThermalBudget::default(),
        }
    }

    fn f1_contracts() -> Contracts {
        Contracts::default()
    }

    // ------------------------------------------------------------------
    // DomainMode / Domain basics
    // ------------------------------------------------------------------

    #[test]
    fn f1_domain_mode_ordering_is_off_observe_actuate() {
        // Ordering is the safety invariant: tightening a mode can never
        // authorize a previously denied action.
        assert!(DomainMode::Off < DomainMode::Observe);
        assert!(DomainMode::Observe < DomainMode::Actuate);
        assert!(DomainMode::Off < DomainMode::Actuate);
    }

    #[test]
    fn f1_domain_mode_serde_rejects_invalid_strings() {
        let toml_str = r#"
mode = "fast"
"#;
        let result: Result<DomainConfig, _> = toml::from_str(toml_str);
        assert!(
            result.is_err(),
            "invalid mode string must fail closed at parse time"
        );
    }

    #[test]
    fn f1_domain_mode_serde_accepts_lowercase_off_observe_actuate() {
        for s in ["off", "observe", "actuate"] {
            let toml_str = format!("mode = \"{s}\"\n");
            let cfg: DomainConfig = toml::from_str(&toml_str).expect("valid mode");
            let expected = match s {
                "off" => DomainMode::Off,
                "observe" => DomainMode::Observe,
                _ => DomainMode::Actuate,
            };
            assert_eq!(cfg.mode, expected, "mode string {s}");
        }
    }

    #[test]
    fn f1_domain_mode_serde_rejects_uppercase_variants() {
        // The plan specifies lowercase; serde(rename_all = "lowercase")
        // rejects "Off", "OBSERVE", etc. This is intentional: the config
        // surface is case-sensitive to match the rest of policy.toml.
        for s in ["Off", "OBSERVE", "Actuate", "ACTUATE"] {
            let toml_str = format!("mode = \"{s}\"\n");
            let result: Result<DomainConfig, _> = toml::from_str(&toml_str);
            assert!(result.is_err(), "uppercase variant {s} must be rejected");
        }
    }

    #[test]
    fn f1_domain_all_returns_ten_known_domains() {
        // If a new domain is added, this test must be updated. That's the
        // point: the test forces the author to think about default_mode for
        // the new domain.
        //
        // F1 repair: 10, not 9 — the CgroupReweight domain was added in
        // the F1 package-completion repair to close the SystemdSetProperty
        // backdoor. See `f1_action_domain_returns_some_for_systemd_set_property`
        // and `f1_systemd_set_property_is_domain_gated`.
        //
        // T1 update: `Domain::all()` now also contains `Thermal`, a
        // sensor-only domain that defaults to `Observe`. The 10-domain
        // *actuate* closed set is asserted via `Domain::actuate_domains()`;
        // `Domain::all()` is 11 (10 actuate + 1 sensor-only).
        assert_eq!(
            Domain::actuate_domains().len(),
            10,
            "expected 10 v0.6+f1 actuate domains"
        );
        assert_eq!(
            Domain::all().len(),
            11,
            "expected 11 total domains (10 actuate + 1 sensor-only Thermal)"
        );
    }

    #[test]
    fn f1_domain_as_str_matches_allowlist_strings() {
        // The domain string must match what the allowlist DB uses, so the
        // effective-config gate composes correctly with the allowlist gate.
        assert_eq!(Domain::CpuEpp.as_str(), "cpu_epp");
        assert_eq!(Domain::PlatformProfile.as_str(), "platform_profile");
        assert_eq!(Domain::VmSysctl.as_str(), "vm_sysctl");
        assert_eq!(Domain::CpuDmaLatency.as_str(), "cpu_dma_latency");
        assert_eq!(
            Domain::DeviceResumeLatency.as_str(),
            "device_resume_latency"
        );
        assert_eq!(Domain::RuntimePm.as_str(), "runtime_pm");
        assert_eq!(Domain::PcieAspm.as_str(), "pci_aspm");
        assert_eq!(Domain::SataAlpm.as_str(), "sata_alpm");
        assert_eq!(Domain::Backlight.as_str(), "backlight");
        // F1 — cgroup reweight domain key. Operators gate cgroup
        // reweighting via `[domains.cgroup_reweight] mode = ...`.
        assert_eq!(Domain::CgroupReweight.as_str(), "cgroup_reweight");
    }

    #[test]
    fn f1_domain_default_mode_is_actuate_for_v0_6_domains() {
        // Migration safety: every v0.6+f1 actuate domain defaults to Actuate
        // so today's curated policy.toml keeps doing exactly what it does
        // today. The fail-closed invariant for future domains lives in
        // the explicit exhaustive match in `Domain::default_mode` —
        // see `f1_domain_default_mode_is_fail_closed_for_unknown_domains`.
        //
        // T1 update: this test now iterates `Domain::actuate_domains()`
        // rather than `Domain::all()`, because `Thermal` (a sensor-only
        // domain) defaults to `Observe` per the F1 plan's sensor-only
        // exception. The sensor-only default is asserted in
        // `f1_sensor_only_domains_default_to_observe`.
        for &d in Domain::actuate_domains() {
            assert_eq!(
                d.default_mode(),
                DomainMode::Actuate,
                "actuate domain {} should default to Actuate (v0.6+f1 migration safety)",
                d.as_str()
            );
        }
    }

    /// T1 — sensor-only domains (read-only observations) default to `Observe`
    /// rather than `Actuate`, per the F1 plan's exception: "new sensor-only
    /// domains default `observe` only when reads are side-effect-free".
    /// `Thermal` is the first such domain; adding another requires proving
    /// the reads are side-effect-free.
    #[test]
    fn f1_sensor_only_domains_default_to_observe() {
        for &d in Domain::sensor_only_domains() {
            assert_eq!(
                d.default_mode(),
                DomainMode::Observe,
                "sensor-only domain {} should default to Observe (F1 sensor-only exception)",
                d.as_str()
            );
        }
    }

    /// F1 repair: the F1 spec rule "new domains default `off`" is
    /// enforced at the type level by `Domain::default_mode` being an
    /// exhaustive match over every `Domain` variant. Any new variant
    /// added to `Domain` produces a non-exhaustive-match compile error
    /// here, forcing the author to make a deliberate Actuate/Off
    /// choice before their PR can compile. The fail-closed invariant
    /// is therefore: a future domain *cannot* fail open to Actuate,
    /// even by mistake.
    ///
    /// This test pins the closed set (10 v0.6+f1 actuate domains) and
    /// the invariant that every existing actuate domain defaults to
    /// Actuate (the migration-safety contract). Sensor-only domains
    /// (T1 `Thermal`) are pinned separately by
    /// `f1_sensor_only_domains_default_to_observe`.
    #[test]
    fn f1_domain_default_mode_is_fail_closed_for_unknown_domains() {
        // The v0.6+f1 actuate closed set: every domain in
        // `Domain::actuate_domains()` must default to Actuate. We
        // re-derive this from `Domain::default_mode` (not hardcode the
        // list) so a future contributor who adds an actuate domain has
        // to update this test deliberately.
        let known: std::collections::BTreeSet<&'static str> = Domain::actuate_domains()
            .iter()
            .map(|d| d.as_str())
            .collect();
        // Today the actuate closed set is exactly the v0.6+f1 ten domains.
        let expected_closed: std::collections::BTreeSet<&'static str> = [
            "cpu_epp",
            "platform_profile",
            "vm_sysctl",
            "cpu_dma_latency",
            "device_resume_latency",
            "runtime_pm",
            "pci_aspm",
            "sata_alpm",
            "backlight",
            "cgroup_reweight",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            known, expected_closed,
            "Domain::all() must match the documented v0.6+f1 closed set; \
             update this test if the closed set is intentionally changed."
        );
        // The fail-closed invariant is enforced at compile time: the
        // `match` in `Domain::default_mode` is exhaustive over every
        // `Domain` variant (no `_ =>` arm). Adding a new variant
        // without an explicit arm in that match produces a
        // non-exhaustive-match compile error, which is the strongest
        // possible guarantee that a future domain cannot silently
        // fail open to Actuate. The only ways to add a new
        // Actuate-defaulting domain are (a) edit
        // `Domain::default_mode` to add a new arm, and (b) update
        // `Domain::all` to add the new variant — both of which
        // require the test author to update this closed-set test.
    }

    // ------------------------------------------------------------------
    // DomainsConfig strict validation
    // ------------------------------------------------------------------

    #[test]
    fn f1_domains_config_rejects_unknown_domain_key() {
        // The plan requires "strict unknown-key" validation. An unknown
        // domain name must fail closed at parse time, not silently be
        // ignored. We parse just the inner `DomainsConfig` (no `[domains]`
        // wrapper).
        let toml_str = r#"
[bogus_domain]
mode = "actuate"
"#;
        let result: Result<DomainsConfig, _> = toml::from_str(toml_str);
        assert!(
            result.is_err(),
            "unknown domain key must fail closed at parse time"
        );
    }

    #[test]
    fn f1_domain_config_rejects_unknown_field() {
        // Parse the inner `DomainsConfig` (no `[domains]` wrapper).
        let toml_str = r#"
[runtime_pm]
mode = "actuate"
bogus_field = true
"#;
        let result: Result<DomainsConfig, _> = toml::from_str(toml_str);
        assert!(
            result.is_err(),
            "unknown field in [domains.<name>] must fail closed at parse time"
        );
    }

    #[test]
    fn f1_domains_config_accepts_all_ten_known_domains() {
        // Parse the inner `DomainsConfig` (no `[domains]` wrapper — that's
        // the outer table name in policy.toml; the type itself is the
        // inner table).
        //
        // F1 repair: 10 actuate domains (the original 9 v0.6 + the
        // CgroupReweight entry added to close the SystemdSetProperty
        // backdoor). T1 added `Thermal` as an 11th, sensor-only domain;
        // it is asserted separately below.
        let toml_str = r#"
[cpu_epp]
mode = "actuate"
[platform_profile]
mode = "actuate"
[vm_sysctl]
mode = "actuate"
[cpu_dma_latency]
mode = "actuate"
[device_resume_latency]
mode = "actuate"
[runtime_pm]
mode = "actuate"
[pci_aspm]
mode = "actuate"
[sata_alpm]
mode = "actuate"
[backlight]
mode = "actuate"
[cgroup_reweight]
mode = "actuate"
[thermal]
mode = "observe"
"#;
        let cfg: DomainsConfig = toml::from_str(toml_str).expect("all known domains");
        for &d in Domain::all() {
            assert!(
                cfg.get(d).is_some(),
                "domain {} should be configured",
                d.as_str()
            );
        }
    }

    #[test]
    fn f1_domain_config_mode_defaults_to_actuate_when_omitted() {
        // `[domains.runtime_pm]` with no `mode` key should default to
        // Actuate (the migration-safety default), not Off. We parse just
        // the inner `DomainsConfig` (TOML key is `runtime_pm`, not
        // `domains.runtime_pm`, because the outer `[domains]` table is
        // implied by the type we're deserializing into).
        let toml_str = "[runtime_pm]\n";
        let cfg: DomainsConfig = toml::from_str(toml_str).expect("valid empty sub-table");
        let rc_cfg = cfg.get(Domain::RuntimePm).expect("runtime_pm configured");
        assert_eq!(rc_cfg.mode, DomainMode::Actuate);
    }

    // ------------------------------------------------------------------
    // EffectiveConfig
    // ------------------------------------------------------------------

    #[test]
    fn f1_effective_config_default_policy_all_actuate() {
        // The migration safety contract: `Policy::default()` (no [domains]
        // section) yields Actuate for every v0.6+f1 actuate domain. This
        // preserves today's behavior. Sensor-only domains (T1 `Thermal`)
        // default to `Observe` and are asserted separately.
        let policy = Policy::default();
        let effective = EffectiveConfig::from_policy(&policy);
        for &d in Domain::actuate_domains() {
            assert_eq!(
                effective.mode_for(d),
                DomainMode::Actuate,
                "actuate domain {} should be Actuate under default policy",
                d.as_str()
            );
        }
        for &d in Domain::sensor_only_domains() {
            assert_eq!(
                effective.mode_for(d),
                DomainMode::Observe,
                "sensor-only domain {} should be Observe under default policy",
                d.as_str()
            );
        }
    }

    /// Helper: parse a TOML fragment containing only `[domains.<name>]`
    /// sub-tables and return a `Policy` built from `Policy::default()` with
    /// only the `domains` field overridden. The F1 effective-config tests
    /// exercise `[domains]` in isolation; they do not need a full
    /// `[thresholds] / [modes.*] / [memory]` configuration.
    fn f1_policy_with_domains(toml_str: &str) -> Policy {
        #[derive(serde::Deserialize)]
        struct DomainsOnly {
            #[serde(default)]
            domains: DomainsConfig,
        }
        let parsed: DomainsOnly = toml::from_str(toml_str).expect("valid domains fragment");
        Policy {
            domains: parsed.domains,
            ..Policy::default()
        }
    }

    #[test]
    fn f1_effective_config_respects_off_mode() {
        let toml_str = r#"
[domains.runtime_pm]
mode = "off"
[domains.backlight]
mode = "off"
"#;
        let policy = f1_policy_with_domains(toml_str);
        let effective = EffectiveConfig::from_policy(&policy);
        assert_eq!(effective.mode_for(Domain::RuntimePm), DomainMode::Off);
        assert_eq!(effective.mode_for(Domain::Backlight), DomainMode::Off);
        // Other domains fall back to default_mode (Actuate).
        assert_eq!(effective.mode_for(Domain::CpuEpp), DomainMode::Actuate);
        assert_eq!(effective.mode_for(Domain::PcieAspm), DomainMode::Actuate);
    }

    #[test]
    fn f1_effective_config_respects_observe_mode() {
        let toml_str = r#"
[domains.runtime_pm]
mode = "observe"
"#;
        let policy = f1_policy_with_domains(toml_str);
        let effective = EffectiveConfig::from_policy(&policy);
        assert_eq!(effective.mode_for(Domain::RuntimePm), DomainMode::Observe);
        assert!(!effective.allows_actuation(Domain::RuntimePm));
    }

    #[test]
    fn f1_effective_config_allows_actuation_only_for_actuate() {
        let toml_str = r#"
[domains.runtime_pm]
mode = "actuate"
[domains.pci_aspm]
mode = "observe"
[domains.sata_alpm]
mode = "off"
"#;
        let policy = f1_policy_with_domains(toml_str);
        let effective = EffectiveConfig::from_policy(&policy);
        assert!(effective.allows_actuation(Domain::RuntimePm));
        assert!(!effective.allows_actuation(Domain::PcieAspm));
        assert!(!effective.allows_actuation(Domain::SataAlpm));
    }

    #[test]
    fn f1_effective_config_render_lists_all_domains_in_canonical_order() {
        // Every actuate domain renders `domains.<name>=actuate` under the
        // default policy. Sensor-only domains render `=observe`.
        let policy = Policy::default();
        let effective = EffectiveConfig::from_policy(&policy);
        let rendered = effective.render();
        for &d in Domain::actuate_domains() {
            let line = format!("domains.{}={}", d.as_str(), DomainMode::Actuate.as_str());
            assert!(
                rendered.contains(&line),
                "rendered output should contain '{line}', got:\n{rendered}"
            );
        }
    }

    // ------------------------------------------------------------------
    // Action::domain() consistency
    // ------------------------------------------------------------------

    #[test]
    fn f1_action_domain_returns_some_for_systemd_set_property() {
        // F1 repair: SystemdSetProperty previously returned None (a
        // fail-open backdoor for cgroup reweighting). It now returns
        // Some(Domain::CgroupReweight) so the per-domain gate applies.
        let a = Action::systemd_set_property(
            "user.slice".to_string(),
            vec!["CPUWeight=150".to_string()],
            "test".to_string(),
        );
        assert_eq!(a.domain(), Some(Domain::CgroupReweight));
    }

    #[test]
    fn f1_action_domain_returns_some_for_every_kernel_write_variant() {
        let cases: Vec<(Action, Domain)> = vec![
            (
                Action::cpu_epp("performance".to_string(), "r".to_string()),
                Domain::CpuEpp,
            ),
            (
                Action::platform_profile("balanced".to_string(), "r".to_string()),
                Domain::PlatformProfile,
            ),
            (
                Action::vm_sysctl(
                    PathBuf::from("/proc/sys/vm/swappiness"),
                    "100".to_string(),
                    "r".to_string(),
                ),
                Domain::VmSysctl,
            ),
            (
                Action::CpuDmaLatency {
                    value: Some(100),
                    reason: "r".to_string(),
                },
                Domain::CpuDmaLatency,
            ),
            (
                Action::DeviceResumeLatency {
                    path: PathBuf::from("/sys/devices/x/power/pm_qos_resume_latency_us"),
                    value: Some(100),
                    reason: "r".to_string(),
                },
                Domain::DeviceResumeLatency,
            ),
            (
                Action::RuntimePm {
                    device_dir: PathBuf::from("/sys/bus/usb/devices/1-1"),
                    autosuspend_delay_ms: 2000,
                    reason: "r".to_string(),
                },
                Domain::RuntimePm,
            ),
            (
                Action::PcieAspm {
                    device_dir: PathBuf::from("/sys/bus/pci/devices/0000:00:00.0"),
                    enable: true,
                    reason: "r".to_string(),
                },
                Domain::PcieAspm,
            ),
            (
                Action::SataAlpm {
                    host_dir: PathBuf::from("/sys/class/scsi_host/host0"),
                    policy: "med_power_with_dipm".to_string(),
                    reason: "r".to_string(),
                },
                Domain::SataAlpm,
            ),
            (
                Action::Backlight {
                    device_dir: PathBuf::from("/sys/class/backlight/intel_backlight"),
                    target_pct: 50,
                    reason: "r".to_string(),
                },
                Domain::Backlight,
            ),
            // F1 repair: SystemdSetProperty is now mapped to a real
            // domain (CgroupReweight) so the per-domain gate applies.
            (
                Action::systemd_set_property(
                    "user.slice".to_string(),
                    vec!["CPUWeight=150".to_string()],
                    "r".to_string(),
                ),
                Domain::CgroupReweight,
            ),
        ];
        for (action, expected) in cases {
            assert_eq!(
                action.domain(),
                Some(expected),
                "action {:?} should map to domain {:?}",
                action,
                expected
            );
        }
    }

    // ------------------------------------------------------------------
    // decide_resolved filtering
    // ------------------------------------------------------------------

    fn f1_decide(policy: &Policy, snapshot: &Snapshot) -> Decision {
        let contracts = f1_contracts();
        policy.decide_resolved(
            snapshot,
            Mode::Auto,
            WorkloadClass::Idle,
            "test".to_string(),
            &contracts,
            None,
            None,
        )
    }

    #[test]
    fn f1_decide_default_policy_emits_actions_for_all_domains() {
        // Migration safety: with the default policy (no [domains] section),
        // today's action set is unchanged. Every domain that would have
        // emitted an action under v0.6 still does.
        let policy = Policy::default();
        let snapshot = f1_snapshot_all_domains();
        let decision = f1_decide(&policy, &snapshot);
        // Sanity: at least one action per actuated domain is present.
        let domains_with_actions: Vec<Domain> =
            decision.actions.iter().filter_map(|a| a.domain()).collect();
        assert!(
            domains_with_actions.contains(&Domain::CpuEpp),
            "cpu_epp action should be present under default policy"
        );
        assert!(
            domains_with_actions.contains(&Domain::RuntimePm),
            "runtime_pm action should be present under default policy (battery-idle)"
        );
        assert!(
            domains_with_actions.contains(&Domain::Backlight),
            "backlight action should be present under default policy (battery-idle)"
        );
    }

    #[test]
    fn f1_decide_off_mode_suppresses_domain_actions_silently() {
        // When a domain is Off, its actions are filtered out AND no
        // observe-mode reason is added (Off is silent by design).
        let toml_str = r#"
[thresholds]
cpu_pressure_perf_avg10 = 12.0
memory_pressure_protect_avg10 = 5.0
io_pressure_throttle_avg10 = 8.0
hot_temp_c = 82.0
critical_temp_c = 92.0
low_battery_pct = 20

[memory]
high_swappiness_requires_zram = true

[modes.battery]
cpu_epp = "power"
platform_profile = "low-power"

[modes.balanced]
cpu_epp = "balance_performance"
platform_profile = "balanced"

[modes.performance]
cpu_epp = "performance"
platform_profile = "performance"

[modes.realtime]
cpu_epp = "performance"
platform_profile = "performance"

[domains.runtime_pm]
mode = "off"
[domains.backlight]
mode = "off"
"#;
        let policy: Policy = toml::from_str(toml_str).expect("valid policy");
        let snapshot = f1_snapshot_all_domains();
        let decision = f1_decide(&policy, &snapshot);
        let domains_with_actions: Vec<Domain> =
            decision.actions.iter().filter_map(|a| a.domain()).collect();
        assert!(
            !domains_with_actions.contains(&Domain::RuntimePm),
            "runtime_pm action must be suppressed when mode=off"
        );
        assert!(
            !domains_with_actions.contains(&Domain::Backlight),
            "backlight action must be suppressed when mode=off"
        );
        // Off-mode suppression is silent: no reason mentions observe.
        assert!(
            !decision.reasons.iter().any(|r| r.contains("observe mode")),
            "off-mode suppression must be silent, but reasons mention observe: {:?}",
            decision.reasons
        );
    }

    #[test]
    fn f1_decide_observe_mode_suppresses_actions_but_surfaces_reason() {
        // When a domain is Observe, its actions are filtered out BUT a
        // reason is added so the operator can see what optid would have
        // done. This is the D2 amendment's "observe, simulate, apply"
        // promotion path.
        let toml_str = r#"
[thresholds]
cpu_pressure_perf_avg10 = 12.0
memory_pressure_protect_avg10 = 5.0
io_pressure_throttle_avg10 = 8.0
hot_temp_c = 82.0
critical_temp_c = 92.0
low_battery_pct = 20

[memory]
high_swappiness_requires_zram = true

[modes.battery]
cpu_epp = "power"
platform_profile = "low-power"

[modes.balanced]
cpu_epp = "balance_performance"
platform_profile = "balanced"

[modes.performance]
cpu_epp = "performance"
platform_profile = "performance"

[modes.realtime]
cpu_epp = "performance"
platform_profile = "performance"

[domains.runtime_pm]
mode = "observe"
"#;
        let policy: Policy = toml::from_str(toml_str).expect("valid policy");
        let snapshot = f1_snapshot_all_domains();
        let decision = f1_decide(&policy, &snapshot);
        let domains_with_actions: Vec<Domain> =
            decision.actions.iter().filter_map(|a| a.domain()).collect();
        assert!(
            !domains_with_actions.contains(&Domain::RuntimePm),
            "runtime_pm action must be suppressed when mode=observe"
        );
        // Observe-mode suppression surfaces a reason.
        assert!(
            decision
                .reasons
                .iter()
                .any(|r| r.contains("runtime_pm") && r.contains("observe mode")),
            "observe-mode suppression should surface a reason, got: {:?}",
            decision.reasons
        );
    }

    #[test]
    fn f1_decide_observe_mode_captures_would_be_action_for_render() {
        // F1 repair: the original implementation recorded only that a
        // domain was suppressed in observe mode ("domain X in observe
        // mode: action suppressed, would-act logged") but the actual
        // would-be action's *value* (which path, which value) was lost.
        // The plan explicitly requires the would-be action to appear in
        // the decision report so operators can see what optid would
        // have done. This test asserts the new behavior:
        //   * `decision.suppressed_actions` carries one entry per
        //     suppressed observe-mode action,
        //   * each entry's description includes the would-be value
        //     (e.g. "100" for runtime_pm autosuspend delay), and
        //   * `Decision::render` includes a `suppressed_actions:` block
        //     so `optctl status` surfaces the value to the operator.
        let toml_str = r#"
[thresholds]
cpu_pressure_perf_avg10 = 12.0
memory_pressure_protect_avg10 = 5.0
io_pressure_throttle_avg10 = 8.0
hot_temp_c = 82.0
critical_temp_c = 92.0
low_battery_pct = 20

[memory]
high_swappiness_requires_zram = true

[modes.battery]
cpu_epp = "power"
platform_profile = "low-power"

[modes.balanced]
cpu_epp = "balance_performance"
platform_profile = "balanced"

[modes.performance]
cpu_epp = "performance"
platform_profile = "performance"

[modes.realtime]
cpu_epp = "performance"
platform_profile = "performance"

[domains.runtime_pm]
mode = "observe"
[domains.pci_aspm]
mode = "observe"
"#;
        let policy: Policy = toml::from_str(toml_str).expect("valid policy");
        let snapshot = f1_snapshot_all_domains();
        let decision = f1_decide(&policy, &snapshot);

        // Both runtime_pm and pci_aspm would-be actions must be captured.
        // `iter()` yields `&(Domain, String)`, so the closure must
        // destructure the reference.
        let suppressed_domains: Vec<Domain> = decision
            .suppressed_actions
            .iter()
            .map(|&(d, _)| d)
            .collect();
        assert!(
            suppressed_domains.contains(&Domain::RuntimePm),
            "runtime_pm would-be action must be captured in suppressed_actions, got: {:?}",
            decision.suppressed_actions
        );
        assert!(
            suppressed_domains.contains(&Domain::PcieAspm),
            "pci_aspm would-be action must be captured in suppressed_actions, got: {:?}",
            decision.suppressed_actions
        );

        // The captured descriptions must include the would-be value, not
        // just the domain name. We do not pin the exact format (it is
        // produced by `Action::describe`), but the description must
        // contain the autosuspend delay value used by the default
        // actuator — operators need to see *what* optid would have set.
        // Iterating by reference yields `&(Domain, String)`, so the
        // pattern must be `&(d, desc)`.
        for &(d, ref desc) in &decision.suppressed_actions {
            assert!(
                !desc.is_empty(),
                "suppressed action for domain {} has empty description",
                d.as_str()
            );
        }

        // The rendered decision must include a `suppressed_actions:` block
        // — this is the operator-visible surface (optctl status reads
        // the rendered text).
        let rendered = decision.render(&snapshot);
        assert!(
            rendered.contains("suppressed_actions:"),
            "rendered decision should include a suppressed_actions block, got:\n{rendered}"
        );
        assert!(
            rendered.contains("would_act=runtime_pm"),
            "rendered decision should mention the would-be runtime_pm action, got:\n{rendered}"
        );
        assert!(
            rendered.contains("would_act=pci_aspm"),
            "rendered decision should mention the would-be pci_aspm action, got:\n{rendered}"
        );
    }

    #[test]
    fn f1_decide_off_mode_does_not_capture_would_be_action() {
        // F1 repair: the `Decision::suppressed_actions` block is the
        // observe-mode surface, not the off-mode surface. Off-mode
        // suppression is silent by design (the domain is invisible to
        // the operator as if optid did not know about it).
        let toml_str = r#"
[thresholds]
cpu_pressure_perf_avg10 = 12.0
memory_pressure_protect_avg10 = 5.0
io_pressure_throttle_avg10 = 8.0
hot_temp_c = 82.0
critical_temp_c = 92.0
low_battery_pct = 20

[memory]
high_swappiness_requires_zram = true

[modes.battery]
cpu_epp = "power"
platform_profile = "low-power"

[modes.balanced]
cpu_epp = "balance_performance"
platform_profile = "balanced"

[modes.performance]
cpu_epp = "performance"
platform_profile = "performance"

[modes.realtime]
cpu_epp = "performance"
platform_profile = "performance"

[domains.runtime_pm]
mode = "off"
"#;
        let policy: Policy = toml::from_str(toml_str).expect("valid policy");
        let snapshot = f1_snapshot_all_domains();
        let decision = f1_decide(&policy, &snapshot);
        let has_runtime_pm = decision
            .suppressed_actions
            .iter()
            .any(|&(d, _)| d == Domain::RuntimePm);
        assert!(
            !has_runtime_pm,
            "off-mode suppression must NOT capture would-be actions, suppressed: {:?}",
            decision.suppressed_actions
        );
    }

    #[test]
    fn f1_decide_actuate_mode_preserves_today_behavior() {
        // When a domain is explicitly Actuate, behavior matches the default
        // (no [domains] section). This is the migration safety net.
        //
        // We start from `Policy::default()` and override only the `domains`
        // field, so the rest of the policy (thresholds, modes, memory) is
        // bit-for-bit identical to the default. Re-parsing a stripped TOML
        // would silently drop the optional `[modes.battery]` fields
        // (`vm_swappiness`, `vm_dirty_bytes`, `background_cpu_weight`, ...)
        // and produce a different action set, which is not what this test
        // is checking.
        let toml_str = r#"
[domains.runtime_pm]
mode = "actuate"
"#;
        let policy_explicit = f1_policy_with_domains(toml_str);
        let policy_default = Policy::default();
        let snapshot = f1_snapshot_all_domains();
        let decision_explicit = f1_decide(&policy_explicit, &snapshot);
        let decision_default = f1_decide(&policy_default, &snapshot);
        // The action sets should be identical (modulo reason text).
        let actions_explicit: Vec<String> = decision_explicit
            .actions
            .iter()
            .map(|a| a.describe())
            .collect();
        let actions_default: Vec<String> = decision_default
            .actions
            .iter()
            .map(|a| a.describe())
            .collect();
        assert_eq!(
            actions_explicit, actions_default,
            "explicit actuate must match default policy behavior"
        );
    }

    #[test]
    fn f1_decide_mixed_modes_filter_independently() {
        // Different domains can have different modes; the filter is
        // per-domain, not all-or-nothing.
        let toml_str = r#"
[thresholds]
cpu_pressure_perf_avg10 = 12.0
memory_pressure_protect_avg10 = 5.0
io_pressure_throttle_avg10 = 8.0
hot_temp_c = 82.0
critical_temp_c = 92.0
low_battery_pct = 20

[memory]
high_swappiness_requires_zram = true

[modes.battery]
cpu_epp = "power"
platform_profile = "low-power"

[modes.balanced]
cpu_epp = "balance_performance"
platform_profile = "balanced"

[modes.performance]
cpu_epp = "performance"
platform_profile = "performance"

[modes.realtime]
cpu_epp = "performance"
platform_profile = "performance"

[domains.runtime_pm]
mode = "off"
[domains.pci_aspm]
mode = "observe"
[domains.sata_alpm]
mode = "actuate"
[domains.backlight]
mode = "off"
"#;
        let policy: Policy = toml::from_str(toml_str).expect("valid policy");
        let snapshot = f1_snapshot_all_domains();
        let decision = f1_decide(&policy, &snapshot);
        let domains_with_actions: Vec<Domain> =
            decision.actions.iter().filter_map(|a| a.domain()).collect();
        // runtime_pm: off → suppressed
        assert!(
            !domains_with_actions.contains(&Domain::RuntimePm),
            "runtime_pm must be suppressed (off)"
        );
        // pci_aspm: observe → suppressed, but reason surfaced
        assert!(
            !domains_with_actions.contains(&Domain::PcieAspm),
            "pci_aspm must be suppressed (observe)"
        );
        assert!(
            decision
                .reasons
                .iter()
                .any(|r| r.contains("pci_aspm") && r.contains("observe mode")),
            "pci_aspm observe-mode reason missing"
        );
        // sata_alpm: actuate → present
        assert!(
            domains_with_actions.contains(&Domain::SataAlpm),
            "sata_alpm must be present (actuate)"
        );
        // backlight: off → suppressed
        assert!(
            !domains_with_actions.contains(&Domain::Backlight),
            "backlight must be suppressed (off)"
        );
    }

    #[test]
    fn f1_systemd_set_property_is_domain_gated_by_default() {
        // F1 repair: SystemdSetProperty used to be the single action
        // that bypassed the per-domain gate. It is now mapped to
        // Domain::CgroupReweight and *is* gated. This test exercises
        // three sub-cases:
        //
        // 1. With all domains explicitly off (including cgroup_reweight),
        //    SystemdSetProperty is filtered out — closing the fail-open
        //    backdoor.
        // 2. With cgroup_reweight at the default (Actuate), it survives
        //    even when other domains are off — preserving today's
        //    curated behavior.
        // 3. With cgroup_reweight in Observe, the action is captured in
        //    `suppressed_actions` so the operator can see what would
        //    have been done.
        //
        // We use `f1_policy_with_domains` to construct each Policy so
        // the [thresholds] / [modes.*] / [memory] sections come from
        // `Policy::default()`; only the [domains.*] sub-table is varied.

        // (1) Explicit off: SystemdSetProperty is filtered.
        let toml_str_off = r#"
[domains.cpu_epp]
mode = "off"
[domains.platform_profile]
mode = "off"
[domains.vm_sysctl]
mode = "off"
[domains.cpu_dma_latency]
mode = "off"
[domains.device_resume_latency]
mode = "off"
[domains.runtime_pm]
mode = "off"
[domains.pci_aspm]
mode = "off"
[domains.sata_alpm]
mode = "off"
[domains.backlight]
mode = "off"
[domains.cgroup_reweight]
mode = "off"
"#;
        let policy_off = f1_policy_with_domains(toml_str_off);
        let cpu_pressure = Pressure {
            avg10: 0.0,
            avg60: 0.0,
            avg300: 0.0,
            total: 0,
        };
        let mem_pressure = Pressure {
            avg10: 50.0,
            avg60: 50.0,
            avg300: 50.0,
            total: 1000,
        };
        let snapshot = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: Some(cpu_pressure),
            memory_pressure: Some(mem_pressure),
            io_pressure: None,
            zram_swap_active: true,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            thermal_sensors: Vec::new(),
            fan_sensors: Vec::new(),
            thermal_budget: crate::thermal::ThermalBudget::default(),
        };
        let decision_off = f1_decide(&policy_off, &snapshot);
        let has_systemd_off = decision_off
            .actions
            .iter()
            .any(|a| matches!(a, Action::SystemdSetProperty { .. }));
        assert!(
            !has_systemd_off,
            "SystemdSetProperty must be filtered when cgroup_reweight=off, actions: {:?}",
            decision_off.actions
        );

        // (2) Default cgroup_reweight (Actuate): SystemdSetProperty survives
        //     even when other domains are off. Migration safety.
        let toml_str_default = r#"
[domains.cpu_epp]
mode = "off"
[domains.platform_profile]
mode = "off"
[domains.vm_sysctl]
mode = "off"
[domains.cpu_dma_latency]
mode = "off"
[domains.device_resume_latency]
mode = "off"
[domains.runtime_pm]
mode = "off"
[domains.pci_aspm]
mode = "off"
[domains.sata_alpm]
mode = "off"
[domains.backlight]
mode = "off"
"#;
        let policy_default = f1_policy_with_domains(toml_str_default);
        let decision_default = f1_decide(&policy_default, &snapshot);
        let has_systemd_default = decision_default
            .actions
            .iter()
            .any(|a| matches!(a, Action::SystemdSetProperty { .. }));
        assert!(
            has_systemd_default,
            "SystemdSetProperty must survive when cgroup_reweight is at default (Actuate), actions: {:?}",
            decision_default.actions
        );

        // (3) Observe: action is captured in suppressed_actions.
        let toml_str_observe = r#"
[domains.cpu_epp]
mode = "off"
[domains.platform_profile]
mode = "off"
[domains.vm_sysctl]
mode = "off"
[domains.cpu_dma_latency]
mode = "off"
[domains.device_resume_latency]
mode = "off"
[domains.runtime_pm]
mode = "off"
[domains.pci_aspm]
mode = "off"
[domains.sata_alpm]
mode = "off"
[domains.backlight]
mode = "off"
[domains.cgroup_reweight]
mode = "observe"
"#;
        let policy_observe = f1_policy_with_domains(toml_str_observe);
        let decision_observe = f1_decide(&policy_observe, &snapshot);
        let has_systemd_observe = decision_observe
            .actions
            .iter()
            .any(|a| matches!(a, Action::SystemdSetProperty { .. }));
        assert!(
            !has_systemd_observe,
            "SystemdSetProperty must be filtered when cgroup_reweight=observe, actions: {:?}",
            decision_observe.actions
        );
        let has_suppressed = decision_observe
            .suppressed_actions
            .iter()
            .any(|&(d, _)| d == Domain::CgroupReweight);
        assert!(
            has_suppressed,
            "SystemdSetProperty would-be action must be in suppressed_actions when cgroup_reweight=observe, suppressed: {:?}",
            decision_observe.suppressed_actions
        );
    }

    // ------------------------------------------------------------------
    // Decision::render surfaces effective_config (optctl status contract)
    // ------------------------------------------------------------------

    #[test]
    fn f1_decision_render_includes_effective_config_block() {
        let policy = Policy::default();
        let snapshot = f1_snapshot_all_domains();
        let decision = f1_decide(&policy, &snapshot);
        let rendered = decision.render(&snapshot);
        assert!(
            rendered.contains("effective_config:"),
            "rendered status should include effective_config block, got:\n{rendered}"
        );
        // Every domain should appear in the block.
        for &d in Domain::all() {
            let needle = format!("domains.{}=", d.as_str());
            assert!(
                rendered.contains(&needle),
                "rendered status should mention domain {needle}"
            );
        }
    }

    #[test]
    fn f1_decision_render_shows_off_observe_actuate_modes() {
        let toml_str = r#"
[thresholds]
cpu_pressure_perf_avg10 = 12.0
memory_pressure_protect_avg10 = 5.0
io_pressure_throttle_avg10 = 8.0
hot_temp_c = 82.0
critical_temp_c = 92.0
low_battery_pct = 20

[memory]
high_swappiness_requires_zram = true

[modes.battery]
cpu_epp = "power"
platform_profile = "low-power"

[modes.balanced]
cpu_epp = "balance_performance"
platform_profile = "balanced"

[modes.performance]
cpu_epp = "performance"
platform_profile = "performance"

[modes.realtime]
cpu_epp = "performance"
platform_profile = "performance"

[domains.runtime_pm]
mode = "off"
[domains.pci_aspm]
mode = "observe"
[domains.sata_alpm]
mode = "actuate"
"#;
        let policy: Policy = toml::from_str(toml_str).expect("valid policy");
        let snapshot = f1_snapshot_all_domains();
        let decision = f1_decide(&policy, &snapshot);
        let rendered = decision.render(&snapshot);
        assert!(
            rendered.contains("domains.runtime_pm=off"),
            "rendered should show runtime_pm=off"
        );
        assert!(
            rendered.contains("domains.pci_aspm=observe"),
            "rendered should show pci_aspm=observe"
        );
        assert!(
            rendered.contains("domains.sata_alpm=actuate"),
            "rendered should show sata_alpm=actuate"
        );
    }

    // ------------------------------------------------------------------
    // Existing curated policy.toml still parses (migration safety)
    // ------------------------------------------------------------------

    #[test]
    fn f1_curated_policy_toml_still_parses() {
        // The shipped config/optid/policy.toml must still parse under F1.
        // The [domains] section is optional and defaults to Actuate for
        // every v0.6 actuate domain and Observe for sensor-only domains
        // (T1 Thermal), so today's file keeps doing what it does.
        let curated = include_str!("../../../config/optid/policy.toml");
        let result: Result<Policy, _> = toml::from_str(curated);
        assert!(
            result.is_ok(),
            "curated policy.toml must still parse under F1: {:?}",
            result.err()
        );
        let policy = result.expect("parsed");
        let effective = EffectiveConfig::from_policy(&policy);
        // Actuate domains keep Actuate (migration safety).
        for &d in Domain::actuate_domains() {
            assert_eq!(
                effective.mode_for(d),
                DomainMode::Actuate,
                "curated policy should leave actuate domain {} at Actuate",
                d.as_str()
            );
        }
        // Sensor-only domains (T1 Thermal) default to Observe unless
        // the curated file explicitly overrides them.
        for &d in Domain::sensor_only_domains() {
            assert_eq!(
                effective.mode_for(d),
                DomainMode::Observe,
                "curated policy should leave sensor-only domain {} at Observe",
                d.as_str()
            );
        }
    }

    #[test]
    fn f1_curated_baseline_still_uses_actuate_for_all_domains() {
        // The safety-floor curated_baseline() must also preserve today's
        // behavior under F1 for actuate domains. Sensor-only domains (T1
        // Thermal) default to Observe.
        let policy = Policy::curated_baseline();
        let effective = EffectiveConfig::from_policy(&policy);
        for &d in Domain::actuate_domains() {
            assert_eq!(
                effective.mode_for(d),
                DomainMode::Actuate,
                "curated_baseline should leave actuate domain {} at Actuate",
                d.as_str()
            );
        }
        for &d in Domain::sensor_only_domains() {
            assert_eq!(
                effective.mode_for(d),
                DomainMode::Observe,
                "curated_baseline should leave sensor-only domain {} at Observe",
                d.as_str()
            );
        }
    }
}
