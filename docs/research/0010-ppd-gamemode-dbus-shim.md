# 0010 — PPD / GameMode D-Bus Shim Design

_This document is a **research WIP** specifying how optid coexists with application-facing
power/performance D-Bus APIs (`power-profiles-daemon`, `GameMode`) by shimming both interfaces
itself. Fills WP-N1b. Design decisions are tagged `[PROVEN]` (verified by upstream source,
spec, or established pattern) or `[HYPOTHESIS]` (plausible design, needs validation)._

**Status:** WIP — design complete, integration tests pending.
**Author:** Nan0pk
**Date:** 2026-06-19
**Depends on:** `docs/SPEC-northstar.md`, `docs/non-goals.md`,
`docs/decisions/0004-adaptive-optid.md`, research 0005 (focus-bridge pattern)
**No hardware deps** — all verification is software-only.

* * *

## 0. Motivation

`docs/non-goals.md` is explicit: "Running multiple competing power/performance daemons by
default" is a non-goal. But applications already call two D-Bus APIs that look like power daemons:

1. **`power-profiles-daemon` (PPD)** — `org.freedesktop.PowerProfiles` — applications set
   `power-saver` / `balanced` / `performance` profiles. GNOME Settings, KDE PowerDevil,
   Firefox, Chromium all use this interface.
2. **GameMode** (`com.feralinteractive.GameMode`) — games request a performance boost. Steam
   Proton games call this automatically before launching.

If Rush Linux ships optid but disables PPD/GameMode, those applications break silently or
noisily. If Rush Linux ships optid *alongside* PPD/GameMode, they fight over the same sysfs
knobs (`/sys/firmware/acpi/platform_profile`, `energy_performance_preference`).

**This research specifies option B: optid shims both D-Bus interfaces**, translating external
app hints into optid's own workload-class boost. optid is the single policy engine; apps
believe they're talking to PPD/GameMode.

* * *

## 1. Findings

### 1.1 PPD D-Bus Interface

**Interface specification (as of PPD 0.21, 2024)** **[PROVEN — upower/ppd source and D-Bus XML]**

Well-known name: `org.freedesktop.PowerProfiles` on the system bus.
Object path: `/org/freedesktop/PowerProfiles`.

**Methods:**
```xml
<interface name="org.freedesktop.PowerProfiles">
  <!-- Deprecated simple setter (still widely used) -->
  <method name="SetProfile">
    <arg name="profile" type="s" direction="in"/>   <!-- power-saver|balanced|performance -->
  </method>
  <!-- Per-app profile hold (PPD 0.12+) -->
  <method name="HoldProfile">
    <arg name="profile"          type="s" direction="in"/>
    <arg name="reason"           type="s" direction="in"/>
    <arg name="application_id"  type="s" direction="in"/>
    <arg name="cookie"          type="u" direction="out"/>
  </method>
  <method name="ReleaseProfile">
    <arg name="cookie" type="u" direction="in"/>
  </method>
</interface>
```

**Properties** (on the same interface):
- `ActiveProfile` (s, read): current active profile
- `Profiles` (as, read): list of supported profiles (`["power-saver","balanced","performance"]`)
- `PerformanceInhibited` (s, read): non-empty string if performance is degraded (e.g., "lap-detected")
- `PerformanceDegraded` (s, read): same, for explicit degradation signal
- `ActiveProfileHolds` (aa{sv}, read): array of active `HoldProfile` holds

**Signal:** `ProfileChanged(s profile)` — fired when `ActiveProfile` changes.

**Profile semantics per PPD's own implementation:**

| Profile | platform_profile | EPP sysfs value |
|---------|-----------------|-----------------|
| `power-saver` | `low-power` | `power` |
| `balanced` | `balanced` | `balance_performance` |
| `performance` | `performance` | `performance` |

Verified in `src/ppd-driver-platform-profile.c` and `src/ppd-driver-amd-pstate.c` in the
PPD source tree.

**How applications use PPD:**

- **Firefox** (`browser/base/content/browser-sitePermissionPanel.js`): calls
  `HoldProfile("power-saver", "video", "org.mozilla.Firefox")` when a video is playing
  fullscreen and battery saver is active; releases hold when video stops.
- **Chromium**: calls `SetProfile("performance")` when it detects AC power and the user
  has performance mode enabled in chrome://flags.
- **GNOME Settings**: exposes the three-position toggle; calls `SetProfile` directly.
- **KDE PowerDevil**: uses `ActiveProfile` property to read and `SetProfile` to write.
- **Steam (Proton)**: does NOT use PPD; uses GameMode instead (see §1.2).

### 1.2 GameMode D-Bus Interface

**Interface specification (GameMode 1.8, 2024)** **[PROVEN — FeralInteractive/gamemode source]**

Well-known name: `com.feralinteractive.GameMode` on the session bus (not system bus).
Object path: `/com/feralinteractive/GameMode`.

```xml
<interface name="com.feralinteractive.GameMode">
  <method name="RegisterGame">
    <arg name="pid" type="i" direction="in"/>
    <arg name="status" type="i" direction="out"/>  <!-- 0=error, 1=ok, 2=already registered -->
  </method>
  <method name="UnregisterGame">
    <arg name="pid" type="i" direction="in"/>
    <arg name="status" type="i" direction="out"/>
  </method>
  <method name="QueryStatus">
    <arg name="status" type="i" direction="out"/>  <!-- 0=inactive, 1=active, 2=active+registered -->
  </method>
  <method name="RegisterGameByPID">
    <arg name="caller_pid" type="i" direction="in"/>
    <arg name="game_pid"   type="i" direction="in"/>
    <arg name="status"     type="i" direction="out"/>
  </method>
  <method name="UnregisterGameByPID">
    <arg name="caller_pid" type="i" direction="in"/>
    <arg name="game_pid"   type="i" direction="in"/>
    <arg name="status"     type="i" direction="out"/>
  </method>
  <method name="QueryStatusByPID">
    <arg name="pid"    type="i" direction="in"/>
    <arg name="status" type="i" direction="out"/>
  </method>
</interface>
```

**What GameMode does today:**

1. Sets CPU governor to `performance` (via `/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor`)
2. Sets process niceness (`renice -n -5 <pid>`)
3. Sets CPU scheduling to `SCHED_RR` for some games (configurable in `/etc/gamemode.ini`)
4. Disables screensaver (via `org.freedesktop.ScreenSaver.Inhibit`)
5. Optionally sets GPU performance mode (NVIDIA: `nvidia-smi -pm 1`; AMD: TDP config)

In optid's shim, actions 1, 5 are replaced by workload-class boost. Actions 2, 3, 4 are
intentionally not replicated (they are not optid's responsibility; the game can do this
itself, or a separate compatibility shim can handle it).

**How Steam Proton uses GameMode:**

Steam auto-calls `RegisterGame(pid)` via the `GameMode` D-Bus interface when launching any
game, regardless of whether the game explicitly requests it. This is configured in Proton's
launch script. The call goes to the session bus (`com.feralinteractive.GameMode`).

### 1.3 Translation Semantics

**PPD profile → optid global floor** **[HYPOTHESIS — fits SPEC §0 objective]**

PPD `SetProfile` is a global signal (affects the whole system). optid treats it as a global
*floor* on workload class assignment:

| PPD Profile | optid global floor |
|------------|-------------------|
| `power-saver` | `light` — workload detector may not boost above `light` globally |
| `balanced` (default) | `interactive` — default optid behavior, no constraint |
| `performance` | `throughput` — floor raised; optid does not drop below `throughput` for any cgroup |

Within the floor, optid's workload-class detector still operates normally: a cgroup doing
active compile under `power-saver` is still classified as `throughput`, but the *actuation*
decisions are bounded below by `light`. This preserves SPEC §0's "subject to per-class
responsiveness floor" semantics while respecting the user's explicit PPD preference.

**`HoldProfile` per app → per-cgroup boost** **[HYPOTHESIS]**

`HoldProfile("performance", "game", "com.steam.Steam")` from a specific app translates to a
per-cgroup boost to `latency-critical` for the duration of the hold. When the cookie is
released, the cgroup returns to its detected class. This mirrors the focus-bridge boost
pattern from research 0005.

**GameMode `RegisterGame(pid)` → per-cgroup boost to `latency-critical`** **[HYPOTHESIS]**

optid resolves the registered PID to its cgroup (via `/proc/<pid>/cgroup`), then boosts that
cgroup to `latency-critical` for the duration of registration. This is semantically equivalent
to `HoldProfile("performance", ...)` scoped to one cgroup.

**Conflict resolution: PPD power-saver + GameMode simultaneously** **[PROVEN design — SPEC §0]**

SPEC §0: "per-workload-class responsiveness floor." The floor is per-class, not global. So:
- Non-game cgroups: bounded by PPD power-saver floor = `light`
- Game cgroup: boosted to `latency-critical` by GameMode (floor = latency-critical > light)

The game's cgroup floor *overrides* the global PPD floor for that cgroup. PPD power-saver
applies to everything else. This is the correct interpretation: the user set power-saver for
background tasks; the game still needs performance.

### 1.4 D-Bus Ownership

**Registering the well-known names** **[PROVEN — D-Bus specification]**

optid registers `org.freedesktop.PowerProfiles` on the **system bus** (PPD runs as root/system
service). optid registers `com.feralinteractive.GameMode` on the **session bus** (GameMode
runs as the user's session). optid is a system daemon; for the session bus GameMode interface,
optid must launch a per-user `systemd --user` bridge service (`optid-gamemode-bridge.service`)
that registers the session-bus name and relays calls to optid's system service via D-Bus.

**Masking upstream service files** **[PROVEN — systemd masking convention]**

Rush Linux packaging includes:
```
/etc/systemd/system/power-profiles-daemon.service → /dev/null
```

This is a systemd "mask" — it completely disables the service and prevents it from being
started even if the package is installed. Same for `gamemoded.service`:
```
/etc/systemd/system/gamemoded.service → /dev/null
```

These symlinks ship in the `optid` package and take precedence over the vendor service files.

**D-Bus activation guard** **[PROVEN]**

Removing the `.service` file alone is insufficient because D-Bus has its own activation
mechanism. Rush Linux must also install:
```
/etc/dbus-1/system.d/optid-ppd-shim.conf   ← allows optid to own org.freedesktop.PowerProfiles
/etc/dbus-1/session.d/optid-gamemode.conf  ← allows optid-gamemode-bridge to own GameMode name
```

### 1.5 Polkit / Permissions

**PPD authorization** **[PROVEN — PPD source]**

PPD today uses polkit action `net.hadess.PowerProfiles.switch-profile`. The default polkit
policy allows any active session user (not just root) to call `SetProfile` without
authentication. optid's shim replicates this: no polkit auth required for `SetProfile`.

**GameMode authorization** **[PROVEN — gamemode source]**

GameMode uses no polkit; any user process can call `RegisterGame` on the session bus. optid's
session-bus bridge replicates this: any process may register.

**Rate limiting** **[HYPOTHESIS]**

A malicious or buggy app could spam `SetProfile("performance")` causing optid to hold an
elevated floor indefinitely. Mitigation in optid's shim: deduplicate `SetProfile` calls;
only act if the new profile differs from the current state; apply a 500 ms cooldown between
actuation decisions (separate from the call acceptance rate).

Audit trail: every `SetProfile`, `HoldProfile`, `RegisterGame`, `UnregisterGame` call is
logged to `/var/log/optid/audit.jsonl` with caller PID and D-Bus sender.

* * *

## 2. Architecture — Design Decisions

### Decision 1: Coexistence strategy
**B — optid shims both interfaces.** optid is the single policy engine; apps believe they
are talking to PPD and GameMode. (See §0 above.)

### Decision 2: D-Bus name ownership
optid system service registers `org.freedesktop.PowerProfiles` on the system bus.
`optid-gamemode-bridge` (per-user `systemd --user` service) registers
`com.feralinteractive.GameMode` on the session bus and relays to optid.
Rush Linux packaging masks both upstream `.service` files.

### Decision 3: Translation rule
PPD profile = global floor on workload-class. GameMode/HoldProfile = per-cgroup boost to
`latency-critical`. Conflict: per-cgroup boost always wins for that cgroup; global floor
applies to everything else. (§1.3 above.)

### Decision 4: Rate limiting
Deduplicate identical consecutive `SetProfile` calls. Cooldown: 500 ms between floor
changes. Log all calls regardless.

### Decision 5: Implementation location
**Inside `crates/optid/src/dbus_shims.rs`** — two modules:
- `ppd_shim`: registers `org.freedesktop.PowerProfiles`, handles `SetProfile`/`HoldProfile`/`ReleaseProfile`
- `gamemode_relay`: listens on D-Bus session bus for relay calls from `optid-gamemode-bridge`

The session-bus bridge lives in `crates/optid-session-bridges/src/gamemode.rs` (same umbrella
crate as `optid-focus-bridge` from research 0005).

~200 LOC total for both shims; benefits from sharing optid's internal policy APIs directly.

* * *

## 4. Evidence Gaps

### 4.1 PPD Shim Compatibility — Firefox

```bash
# Start optid with PPD shim enabled
sudo optid --ppd-shim=enabled
# Verify optid owns the well-known name:
busctl status org.freedesktop.PowerProfiles
# Open Firefox; toggle Energy Saving in Firefox Settings
# Verify optid received SetProfile:
optctl audit --since 1min | grep -i 'ppd\|SetProfile'
# Verify platform_profile was actuated:
cat /sys/firmware/acpi/platform_profile
```

**Acceptance threshold:** Firefox sees no error; `platform_profile` changes to match;
optid audit log shows the call.

### 4.2 GameMode Shim — Steam Proton Launch

```bash
# Start optid with session bridge enabled
systemctl --user start optid-gamemode-bridge
# Verify bridge owns the name on session bus:
busctl --user status com.feralinteractive.GameMode
# Launch a Steam game (with GameMode enabled in Steam launch options)
# Check optid received RegisterGame:
optctl audit --since 1min | grep -i 'RegisterGame\|gamemode'
# Check cgroup classification:
optctl explain --cgroup <steam-cgroup>
```

**Acceptance threshold:** Game launches normally; optid audit shows `RegisterGame` call;
cgroup classification shows `latency-critical` during game session.

### 4.3 Conflict: PPD power-saver + GameMode active

```bash
# Set global PPD floor to power-saver
optctl ppd set power-saver
# Launch game (should boost its cgroup to latency-critical despite global floor)
DRI_PRIME=1 steam &
# Verify dual state:
optctl list-classes
# Expected: game cgroup = latency-critical; all other cgroups ≤ light
```

**Acceptance threshold:** Game cgroup shows `latency-critical`; non-game cgroups show ≤ `light`.

* * *

## 5. Non-Goals

- **No shimming of `tlp` API.** TLP is config-file-based, not D-Bus. Users should remove TLP.
- **No reimplementation of GameMode's full feature set.** optid only does the workload-class
  boost; CPU governor / renice / GPU mode are out of scope (optid already owns CPU EPP).
- **No per-app profile customization UI.** That is a desktop concern.
- **No opaque "performance boost" beyond what SPEC §3 allows.** GameMode boost is workload-class
  elevation, not raw override.
- **No competing TLP/cpufreqd/cpupower daemon.** Per non-goals.md.
- **No PPD `PerformanceDegraded` signal without real evidence.** optid emits this only when
  the thermal governor is actually throttling.

* * *

## 6. WP Relationship Map

| Workplan / Doc | Relationship |
|----------------|-------------|
| **WP-N1b** | Direct subject |
| **WP-N1** | Workload-class detector — PPD/GameMode hints feed into it |
| **non-goals.md** | Operationalizes the "no competing daemons" rule |
| **ADR-0004 (adaptive-optid)** | optid is the single owner; shims translate external APIs |
| **0005 (focus-bridge)** | Same bridge pattern; `optid-session-bridges` umbrella crate |

* * *

## 7. Next Steps

### Immediate (no hardware needed)
- [ ] Read PPD D-Bus XML spec + confirm `HoldProfile` cookie semantics from PPD 0.21 source
- [ ] Read GameMode 1.8 source for exact D-Bus interface (especially `RegisterGameByPID`)
- [ ] Implement `crates/optid/src/dbus_shims/ppd.rs` (~100 LOC) and `gamemode_relay.rs` (~80 LOC)
- [ ] Implement `crates/optid-session-bridges/src/gamemode.rs` (~120 LOC, relays to system D-Bus)
- [ ] Add `--ppd-shim=on|off` and `--gamemode-shim=on|off` flags to optid
- [ ] Draft `/etc/systemd/system/power-profiles-daemon.service → /dev/null` symlink in packaging
- [ ] Draft `/etc/systemd/system/gamemoded.service → /dev/null` symlink
- [ ] Draft D-Bus policy files in `packaging/dbus/`

### Short-term (needs a desktop session)
- [ ] Run §4.1 PPD + Firefox compatibility test
- [ ] Run §4.2 GameMode + Steam game test
- [ ] Run §4.3 conflict resolution test

### Medium-term
- [ ] Land shims as default-on in v0.x
- [ ] Promote research from WIP to Validated
- [ ] Update `non-goals.md` to reference the shim as the resolution mechanism for the
  "no competing daemons" constraint

* * *

## Appendix: Suggested Reading

### Upstream projects
- `power-profiles-daemon` — `https://gitlab.freedesktop.org/upower/power-profiles-daemon`
- `gamemode` — `https://github.com/FeralInteractive/gamemode`
- `busctl` man page — D-Bus inspection

### Application integrations
- Firefox energy saver PPD integration
- Steam Proton GameMode integration (in `steam-runtime-tools`)
- Chromium PPD integration (`chrome/browser/performance_manager/policies/`)

### Project-internal
- SPEC §4.2 (platform_profile, EPP), §4.3, §6
- `docs/non-goals.md`
- ADR-0004 (`docs/decisions/0004-adaptive-optid.md`)
- Research 0005 (focus-bridge, umbrella crate pattern)
