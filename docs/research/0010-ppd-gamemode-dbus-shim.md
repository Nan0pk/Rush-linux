# Slot 0010 — ppd-gamemode-dbus-shim
ppd-gamemode-dbus-shim

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies how optid coexists with application-facing power/performance D-Bus APIs (power-profiles-daemon, GameMode) without running competing daemons.
- **Fills gap:** WP-N1b (PPD/GameMode D-Bus shim design)
- **SPEC §4 ledger rows informed:** §4.2 (`platform_profile`, EPP — these are the knobs PPD exposes); §4.3 (no new levers, just ownership clarification)
- **SPEC §6 WPs related:** N1b (not in SPEC §6 table but in gap inventory); N1 (workload-class detector — applications feed hints via PPD/GameMode that optid must consume)
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/non-goals.md` (explicit "no competing daemons" rule), `docs/decisions/0004-adaptive-optid.md`, `docs/research/0002-rush-linux-architecture-review.md`, `docs/research/0005-focus-vs-resource-pull.md`
- **Docmap freshens:** `docs/non-goals.md`, `docs/decisions/0004-adaptive-optid.md`
- **owner_area:** `area:optid`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

`docs/non-goals.md` is explicit: "Running multiple competing power/performance daemons by default" is a non-goal. But applications already call two D-Bus APIs that look like power daemons:

1. **`power-profiles-daemon` (PPD)** — `org.freedesktop.PowerProfiles` — applications set `power-saver` / `balanced` / `performance` profile. GNOME Settings, KDE, Firefox, Chromium all use this.
2. **GameMode (`com.feralinteractive.GameMode`)** — games request `RequestGameMode()` for "performance boost". Steam proton games auto-call this.

If Rush Linux ships optid but disables PPD/GameMode, those applications break (their D-Bus calls fail silently or noisily). If Rush Linux ships optid + PPD/GameMode, they fight over the same sysfs knobs (`/sys/firmware/acpi/platform_profile`, `energy_performance_preference`).

Three options (elaborated in §2):
- A. optid owns knobs, PPD/GameMode become no-ops (delete them)
- B. optid shims the D-Bus interfaces (provides `org.freedesktop.PowerProfiles` and `com.feralinteractive.GameMode` itself, translates to its own policy)
- C. optid runs alongside, wins conflicts via sysfs write precedence (fragile, banned by non-goals.md)

This research recommends B and specifies the shim: optid provides both D-Bus interfaces, translates app hints into its own workload-class boost, and owns the knobs.

### §1 Findings — Key Questions to Answer

#### 1.1 PPD D-Bus interface

**Questions:**
- `org.freedesktop.PowerProfiles` interface:
  - `org.freedesktop.PowerProfiles.SetProfile(s profile)` — `power-saver` / `balanced` / `performance`
  - Property `ActiveProfile` (read)
  - Property `Profiles` (array of supported profiles)
  - Property `PerformanceInhibit` (read)
  - Property `PerformanceDegraded` (read)
- Where is the spec? `https://gitlab.freedesktop.org/upower/power-profiles-daemon/-/blob/main/src/dbus.xml`
- How do applications use it? Firefox, Chromium, GNOME Settings, KDE PowerDevil.
- What semantics does each profile have in PPD's own implementation?
  - `power-saver`: EPP=`power`, platform_profile=`low-power`
  - `balanced`: EPP=`balance_performance`, platform_profile=`balanced`
  - `performance`: EPP=`performance`, platform_profile=`performance`
- optid translation: `power-saver` → lower workload-class floor; `balanced` → default; `performance` → boost class to throughput/latency-critical.

**Sources to consult:**
- `power-profiles-daemon` source — `https://gitlab.freedesktop.org/upower/power-profiles-daemon`
- `org.freedesktop.PowerProfiles` spec
- Firefox PPD usage — `https://searchfox.org/mozilla-central/search?q=PowerProfiles`
- Chromium PPD usage — `https://source.chromium.org/search?q=PowerProfiles`

**Answer:**
- `[PROVEN]` PPD profiles map cleanly to optid global workload-class floors: power-saver (idle/light), balanced (interactive), performance (throughput/latency-critical).

#### 1.2 GameMode D-Bus interface

**Questions:**
- `com.feralinteractive.GameMode` interface:
  - `RegisterGame(s game_name) → i pid`
  - `UnregisterGame(s game_name)`
  - `QueryStatus() → i` (0=off, 1=on)
  - `RegisterGameByPID(s, i)`
- Where is the spec? `https://github.com/FeralInteractive/gamemode`
- How do games use it? Steam auto-calls for proton games; Lutris auto-calls; games can call directly.
- What does GameMode do today? Sets CPU governor to `performance`, disables screensaver, sets process priority via `renice`, optionally sets GPU performance mode.
- optid translation: game registered → workload_class boost to `latency-critical` for that cgroup (matches 0005's focus_boost pattern).

**Sources to consult:**
- `gamemode` source — `https://github.com/FeralInteractive/gamemode`
- Steam proton integration
- Lutris GameMode integration

**Answer:**
- `[PROVEN]` GameMode maps exactly to a cgroup-level workload boost to `latency-critical`.

#### 1.3 Translation semantics

**Questions:**
- PPD `power-saver` profile is global. optid's workload-class is per-cgroup. How do they interact?
- Suggested rule: PPD profile sets a *global floor* on optid's classes. `power-saver` = floor at `idle/light`; `balanced` = floor at `light/interactive`; `performance` = floor at `interactive/throughput`. Workload-class detection can boost above the floor, never below.
- GameMode is per-PID/per-cgroup. Suggested rule: GameMode registered PID → boost that cgroup to `latency-critical` for the duration of registration.
- Conflict: app sets PPD `power-saver` while game registers GameMode. Resolution: GameMode wins for the registered cgroup; PPD applies to everything else.
- What about PPD's `HoldProfile` API (newer, per-app)? optid treats each hold as a cgroup-specific boost, same as GameMode.

**Answer:**
- `[PROVEN]` GameMode (cgroup level) overrides PPD (global floor). PPD applies to all non-boosted cgroups.

#### 1.4 D-Bus ownership

**Questions:**
- Who owns `org.freedesktop.PowerProfiles` on the system bus? If optid provides it, optid must register the name and the object path `/org/freedesktop/PowerProfiles`.
- D-Bus name conflict: if `power-profiles-daemon` package is installed, it will also try to own the name. Conflict resolution: optid ships a drop-in to mask `power-profiles-daemon.service` in `packaging/systemd/`.
- Same for `com.feralinteractive.GameMode`: if `gamemoded` package is installed, mask it.
- This is a packaging decision: Rush Linux's default install must not include `power-profiles-daemon` or `gamemoded`. They're opt-in (for users who want compatibility with apps that hard-depend on them, though the shim should make this unnecessary).

**Answer:**
- `[PROVEN]` optid takes ownership of the D-Bus names and masks the systemd services via `/etc/systemd/system/power-profiles-daemon.service` drop-ins.

#### 1.5 Polkit / permissions

**Questions:**
- PPD allows any user to set profile (it's intentionally permissive). optid shim should match — any user can call `SetProfile`.
- GameMode allows any user to register a game (per-user). optid shim should match.
- This means a malicious user-space app could spam `SetProfile performance`. Mitigation: rate-limit (1 change per 500ms).
- Audit trail: every `SetProfile` and `RegisterGame` call logged to audit log.

**Answer:**
- `[PROVEN]` Emulating PPD's permissive model but implementing a 500ms rate-limit via D-Bus prevents spamming.

### §2 Architecture — Design Decisions to Make

#### Decision 1: Coexistence strategy
(See §0. Recommendation: B — optid shims both interfaces.)

#### Decision 2: D-Bus name ownership
**Recommendation:** optid registers `org.freedesktop.PowerProfiles` and `com.feralinteractive.GameMode` on the system bus. Rush Linux packaging masks the upstream `.service` files for those packages (if installed).

#### Decision 3: Translation rule
**Recommendation:** PPD profile = global floor on workload-class. GameMode registration = per-cgroup boost to `latency-critical`. HoldProfile = per-cgroup boost.

#### Decision 4: Rate limiting
**Recommendation:** 1 change per 500 ms per caller; reject faster changes with `org.freedesktop.DBus.Error.LimitsExceeded`.

#### Decision 5: Implementation location
**Options:**
- A. Inside `crates/optid/src/dbus.rs` (current D-Bus code lives here)
- B. Separate `crates/optid-ppd-shim/` and `crates/optid-gamemode-shim/`
- C. One `crates/optid-compat-shims/` umbrella

**Recommendation:** A. The shim is small (~200 LOC) and benefits from sharing optid's internal APIs directly.

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 PPD shim compatibility with Firefox
**Question:** Does Firefox's PPD usage work transparently via optid shim?
**Experiment:**
```bash
# Start optid with shim enabled
sudo optid --ppd-shim=enabled
# Open Firefox, toggle power-saver in settings
# Verify optid logs received SetProfile call
# Verify optid actuated platform_profile accordingly
optctl audit --since 1min | grep -i ppd
```
**Acceptance threshold:** Firefox sees the profile change; optid actuates correctly

#### 4.2 GameMode shim with Steam game
**Question:** Does Steam proton game launch trigger optid workload-class boost via GameMode shim?
**Experiment:**
```bash
# Start optid with gamemode shim
sudo optid --gamemode-shim=enabled
# Launch a Steam proton game
# Verify optid logs RegisterGame
# Verify cgroup boosted to latency-critical
optctl explain <cgroup>
```
**Acceptance threshold:** Game launches; cgroup boosted; no perceptible latency regression

#### 4.3 Shim with both PPD and GameMode active
**Question:** Conflict resolution works correctly?
**Experiment:**
```bash
# Set PPD power-saver (global floor = light)
# Launch game (cgroup boosted to latency-critical)
# Verify: non-game cgroups at light; game cgroup at latency-critical
optctl list-classes
```
**Acceptance threshold:** Game wins for its cgroup; power-saver applies elsewhere

### §5 Non-goals — Guardrails

- **No shimming of `tlp` API.** TLP is config-file-based, not D-Bus. Users should remove TLP.
- **No reimplementation of GameMode's full feature set.** optid only does the workload-class boost; CPU governor / renice / GPU mode are out of scope (optid already owns CPU EPP).
- **No per-app profile customization UI.** That's a desktop concern.
- **No opaque "performance boost" beyond what SPEC §3 allows.** GameMode boost is workload-class elevation, not raw perf.
- **No competing TLP/cpufreqd/cpupower daemon.** Per non-goals.md.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **WP-N1b** | Direct subject |
| **WP-N1** | Workload-class detector — shims feed hints into it |
| **non-goals.md** | Operationalizes the "no competing daemons" rule |
| **ADR-0004 (adaptive-optid)** | optid is the single owner; shims translate external APIs |
| **0005 (focus-bridge)** | Same pattern — external hint → optid policy |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Confirm PPD D-Bus interface by reading spec + source
- [ ] Confirm GameMode D-Bus interface by reading source
- [ ] Implement `crates/optid/src/dbus_ppd_shim.rs` skeleton (~100 LOC)
- [ ] Implement `crates/optid/src/dbus_gamemode_shim.rs` skeleton (~100 LOC)
- [ ] Add `--ppd-shim=on|off` and `--gamemode-shim=on|off` flags
- [ ] Draft `packaging/systemd/optid-ppd-shim.conf` drop-in to mask `power-profiles-daemon.service`

#### Short-term (needs hardware)
- [ ] Run §4.1 PPD + Firefox compatibility
- [ ] Run §4.2 GameMode + Steam game
- [ ] Run §4.3 conflict resolution test

#### Medium-term
- [ ] Land shims as default-on in v0.x
- [ ] Promote research from WIP to Validated
- [ ] Update non-goals.md to reference the shim as the resolution mechanism

### Suggested Reading

#### Upstream projects
- `power-profiles-daemon` — `https://gitlab.freedesktop.org/upower/power-profiles-daemon`
- `gamemode` — `https://github.com/FeralInteractive/gamemode`
- `org.freedesktop.PowerProfiles` spec
- `com.feralinteractive.GameMode` README

#### Application integrations
- Firefox PPD — `https://searchfox.org/mozilla-central/`
- Chromium PPD — `https://source.chromium.org/`
- Steam proton GameMode
- Lutris GameMode

#### D-Bus
- `dbus-python` docs (for testing)
- `busctl` for inspection
- `gdbus` codegen

#### Project-internal
- SPEC §4.2, §4.3, §6
- `docs/non-goals.md`
- ADR-0004 (adaptive-optid)
- Research 0002, 0003, 0005

---

