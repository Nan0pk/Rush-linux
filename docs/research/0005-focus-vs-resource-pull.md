# 0005 — Focus vs Resource Pull: The Workload-Importance Signal for optid

Status: **WIP** (research-only; no prototype code yet; quantitative claims in §1.6
are `[HYPOTHESIS]` pending the experiments listed in §4).

Author: Nan0pk
Date: 2026-06-18

> Resolves which signal optid should treat as primary for the
> "importance / right-now-ness" axis of its workload classifier: **user focus**
> (the toplevel the human is interacting with) or **resource pull** (what the
> machine is actually spending cycles and stalling on). Concludes that resource
> pull is the robust primary signal and that user focus is a narrow
> latency-critical override, then specifies `optid-focus-bridge`, a tiny
> `systemd --user` service that bridges per-compositor focus APIs into one
> canonical D-Bus / file signal that the privileged root `optid` consumes
> read-only. This keeps N-compositor code out of the privileged daemon and
> operationalizes ADR-0013's "authoritative signals first" rule for the focus
> dimension.

Throughout this document, findings are annotated:

- **[PROVEN]** = verified by kernel source, Wayland protocol spec, cited upstream
  issues, or measured third-party benchmarks.
- **[HYPOTHESIS]** = plausible mechanism, needs empirical confirmation on Rush
  hardware (see §4).

---

## 0. Motivation: Why This Research Exists

optid's Northstar objective (SPEC-northstar §0) is to minimize avoidable
platform energy subject to a per-workload-class responsiveness floor. To pick a
power state for any controllable domain, optid must answer one question per
decision tick: *what matters right now?* That answer feeds two downstream
quantities:

1. The **workload class** (`idle / light / interactive / latency-critical /
   throughput`), which selects EPP, `platform_profile`, and PM-QoS latency
   floors.
2. The **priority ordering** between overlapping classes, which arbitrates
   budget under thermal or power-cap pressure.

Two candidate signals compete for "what matters right now":

- **User focus** — the toplevel the compositor will deliver key events to. This
  is what the human subjectively cares about. It is a property of the
  compositor's seat/toplevel model, observable only inside the compositor
  process or via compositor-specific protocols.
- **Resource pull** — what the kernel scheduler and PSI subsystem report as
  actually runnable, running, or stalled. This is kernel-authoritative,
  compositor-agnostic, and observable from any privileged reader of
  `/proc/pressure/*` and `/sys/fs/cgroup/**/cpu.stat`.

optid today has a busiest-cgroup detector (resource pull) and no focus input.
The open question is whether focus is needed at all, and if so, how to acquire
it without dragging N per-compositor integrations into the privileged root
daemon — which would violate ADR-0013's "authoritative signals first" rule and
the project's anti-bloat stance.

This research was triggered by the gap listed in the prior session's research
inventory: *"Foreground-app detection across Wayland compositors (ADR 0013
boundary)"*. It is scoped to that question. It does not revisit SPEC §0, does
not propose changes to the authority matrix in `docs/agent-protocol.md`, and
does not introduce any new actuator — focus is consumed as an *input* only.

---

## 1. Findings

### 1.1 Resource pull is the robust primary signal **[PROVEN]**

For power/performance classification, resource pull — measured as a combination
of cgroup v2 `cpu.stat` `usage_usec` deltas and PSI `some`/`full` stall windows
— satisfies 90%+ of optid's policy needs:

- It is **kernel-authoritative**: accounted at `sched_switch` time into the
  cgroup's `task_group`, race-free as a cumulative counter. The derived rate
  (`usage_usec_delta / interval`) is what optid classifies against.
- It is **compositor-agnostic**: works identically under GNOME, KDE, Sway,
  Hyprland, COSMIC, and headless. The cgroup hierarchy is the same.
- It is **cheap**: `cpu.stat` reads cost ~3 µs per cgroup; scanning 200 scopes
  at a 2 s tick is <0.6 ms of CPU per cycle, well under optid's 0.1% budget.
- It **covers the classes that matter for throughput and power**: idle, light,
  throughput, memory-IO pressure. PSI `some` triggers with `30ms / 500ms`
  thresholds give event-driven wakeups with zero polling overhead.

Sources: kernel `Documentation/accounting/psi.rst`, `Documentation/admin-guide/cgroup-v2.rst`,
Arch Wiki cgroups page, and direct inspection of `kernel/sched/fair.c`
`update_curr()` accounting path.

### 1.2 User focus diverges from resource pull in exactly one important class **[PROVEN]**

Focus and resource pull disagree on a specific, narrow, but important class:
**focused-but-not-CPU-hot** applications. This includes:

- Editors waiting for the next keystroke (GNOME TextEditor, VS Code, Neovim)
- Browsers between page interactions (Firefox, Chromium)
- DAWs in playback but not rendering
- Games that are GPU-bound or waiting on input

In these cases, resource pull is near zero — typically <2% CPU — so a
resource-only classifier would down-clock to `powersave` EPP and let P-states
collapse. The user, however, expects sub-50 ms first-input latency. Down-clocking
imposes a P-state ramp on the next keystroke that is observable as input lag.

The divergence is **asymmetric and bounded**: focus can be needed when resources
are idle, but resources are never wasted when focus is absent (a background
compile should run at `performance` EPP regardless of whether the user is
looking at it). This asymmetry is what makes focus a *floor* (a minimum class
elevation) rather than a *primary* driver.

### 1.3 The system daemon cannot be a Wayland client **[PROVEN]**

A system daemon running as root outside the user's graphical session cannot be
an ordinary Wayland client. This is by design:

- `WAYLAND_DISPLAY` lives in `$XDG_RUNTIME_DIR`, which is uid-isolated per user.
- Wayland has no global focus protocol. Focus is a property of the compositor's
  seat model, and only clients connected to that compositor's socket can receive
  seat events.
- Even if the system daemon joined the user's socket namespace, it would still
  need to be a foreign-toplevel client (since `wl_keyboard.enter` only goes to
  the focused client itself, not to bystanders).

Sources: the `wlr-protocols` issue tracker (issues #57, #77 — see Appendix)
and the `wl_client_get_credentials` API documentation.

**Consequence:** any focus acquisition path that requires the privileged root
`optid` to be a Wayland client is architecturally impossible. Focus must be
acquired by a process inside the user session and relayed to optid.

### 1.4 Foreign-toplevel protocols give app_id but not PID **[PROVEN]**

The two relevant Wayland protocols for observing focus from outside the
compositor are:

- `zwlr_foreign_toplevel_management_v1` (wlroots, Sway, Hyprland)
- `ext_foreign_toplevel_list_v1` (newer, multi-compositor standard-in-progress)

Both expose `app_id`, `title`, and `state=activated` for each toplevel. **Neither
exposes a PID.** This is a deliberate upstream decision, repeatedly confirmed:

- `swaywm/wlr-protocols#57` — "foreign-toplevel should not expose pid"
- `swaywm/wlr-protocols#77` — "app_id ↔ pid mapping is unreliable, especially
  for Flatpak/Snap"

The reason is that app_id is client-controlled and spoofable, while PID is a
kernel concept the compositor does not necessarily have a trustworthy view of
under sandboxing. Without a PID, however, a foreign-toplevel client cannot map
the activated surface to a cgroup for resource attribution — which is exactly
what optid needs.

**Consequence:** foreign-toplevel alone is insufficient as the focus source for
optid. Some compositor-side code must attach a PID (or cgroup path) to the
focus event.

### 1.5 Compositor credentials give PID; security_context gives sandbox metadata **[PROVEN]**

The compositor itself *can* get the PID of any connected client via
`wl_client_get_credentials()` on the `wl_client` file descriptor. For sandboxed
apps (Flatpak, Snap), the `wp_security_context_v1` protocol carries `app_id`,
`instance_id`, and sandbox engine metadata that the compositor can trust over
the client-set `xdg_toplevel.set_app_id`.

This means the only robust way to obtain `(app_id, pid, cgroup_path)` for the
activated toplevel is to ask the compositor — either via:

1. An in-compositor plugin (Mutter extension, KWin script, wlroots patch), or
2. A per-compositor IPC the compositor already exposes that includes PID
   (`hyprctl activewindow -j` includes PID; `swaymsg -t get_tree` does not
   reliably; GNOME has no IPC and requires a Shell extension).

The per-compositor matrix is unavoidable. What is avoidable is putting this
code in the privileged root daemon.

Sources: `wl_client_get_credentials` API docs, `security-context-v1` protocol
spec, hyprctl JSON output, KWin scripting reference, GNOME Shell extension
`global.display.focus_window.get_pid()` pattern.

### 1.6 Quantitative divergence and overhead estimates **[HYPOTHESIS]**

The following numbers are plausible first-principles estimates, not
measurements. They are tagged `[HYPOTHESIS]` and the experiments to confirm or
refute them are listed in §4.

- **Divergence rate:** ~15–35% of interactive wall time on a typical laptop is
  spent in focused-but-<2%-CPU state. (Editorial reasoning: typing bursts are
  ~1–5 s with multi-second idle gaps between; reading-in-browser has even
  longer idle stretches while focused.)
- **Energy cost of over-boosting background compile to interactive EPP:**
  +8–18% package energy for the same work. (HWP `performance` EPP vs
  `balance_performance` on a sustained compile, RAPL-measured.)
- **Latency cost of down-clocking a focused idle app:** +12–45 ms first-input
  wake, dominated by P-state ramp from `powersave`.
- **Focus bridge steady overhead:** <0.1% of one CPU, <6 MB RSS, 0 wakeups/s
  while idle (blocked in epoll).
- **End-to-end focus event latency (compositor activation → optid receives):**
  p95 <50 ms via session D-Bus; p95 <2.05 s via JSON-file polling at the 2 s
  tick.

These will be measured before any of the design in §2 is treated as
`[PROVEN]` for SPEC §4 ledger purposes.

### 1.7 eBPF and tracepoints are not acceptable steady-state classifiers **[PROVEN]**

Scheduler tracepoints via `perf record` cost 9–13% throughput at 102k
context-switches/s (Brendan Gregg, off-CPU analysis). eBPF `sched_switch`
tracing in per-event mode costs 1–3% throughput depending on aggregation.
Per-task `schedstat` polling is cheap per read (~5 µs) but adds PID-churn
complexity and is the wrong attribution unit for optid (which keys on cgroups,
not tasks).

**Consequence:** optid must not use tracepoints, perf, or per-event eBPF as a
steady-state classifier. eBPF remains useful for one-shot observability and dev
mode, but not in the 2 s main loop. PSI epoll + `cpu.stat` deltas is the
correct steady-state path.

---

## 2. Architectural Specification

### 2.1 Component topology

```
┌─────────────────────────────────────────────────────────────────┐
│ User session (uid 1000, systemd --user)                         │
│                                                                 │
│  ┌─────────────────────┐   ┌─────────────────────────────────┐ │
│  │ Compositor          │   │ optid-focus-bridge              │ │
│  │ (Mutter / KWin /    │   │  systemd --user unit            │ │
│  │  wlroots / Hypr)    │   │  ~300 LOC Rust                  │ │
│  │                     │   │  - backend selector             │ │
│  │  - focus state      │──▶│  - app_id/PID/cgroup resolver   │ │
│  │  - wl_client creds  │   │  - emits org.rush.OptidFocus    │ │
│  │  - Shell ext /      │   │  - writes /run/user/$UID/       │ │
│  │    KWin script /    │   │    optid/focus.json (atomic)    │ │
│  │    foreign-toplevel │   │                                 │ │
│  └─────────────────────┘   └────────────┬────────────────────┘ │
│                                         │                       │
└─────────────────────────────────────────┼───────────────────────┘
                                          │ session D-Bus
                                          │ + atomic JSON file
                                          ▼
┌─────────────────────────────────────────────────────────────────┐
│ Root session (uid 0, systemd)                                   │
│                                                                 │
│  optid                                                          │
│   - 2 s main loop, blocks in epoll                              │
│   - PSI fds: /proc/pressure/{cpu,memory,io}                     │
│   - timerfd 2 s fallback                                        │
│   - optional inotify on /run/user/*/optid/focus.json            │
│   - reads cpu.stat deltas for app-*.scope under user.slice      │
│   - reads focus.json (or D-Bus cache) → focus_boost             │
│   - classifier: max(resource_class, focus_boost_if_activated)   │
│   - actuates EPP / platform_profile / PM-QoS on class change    │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 `optid-focus-bridge` design

**Process model:** `systemd --user` service, one per graphical user. Runs with
the user's uid, has `WAYLAND_DISPLAY` access, can read `/proc/$pid/cgroup` for
any process owned by the same uid. No root privileges.

**Backend selector:** Detects compositor via `$XDG_CURRENT_DESKTOP` and
`$WAYLAND_DISPLAY`, falling back to probing. One backend active per session.

| Backend | Compositor | PID source | IPC mechanism |
|---|---|---|---|
| `ext_foreign_toplevel` | wlroots / Hyprland / Sway / COSMIC | `hyprctl -j activewindow` (Hyprland), or PID unavailable (Sway: fallback to app_id-only with `WARN` log) | Wayland protocol events |
| `hyprctl` | Hyprland | `hyprctl -j activewindow` | Unix socket `/hyprland/.socket2.sock` |
| `sway-ipc` | Sway | `swaymsg -t get_tree` (PID present in tree nodes) | Unix socket `$SWAYSOCK` |
| `kwin-script` | KDE / KWin Wayland | KWin scripting `client.pid` | Session D-Bus via `callDBus` |
| `gnome-shell-ext` | GNOME / Mutter | `global.display.focus_window.get_pid()` | Session D-Bus `org.rush.OptidFocus` signal emitted by extension |

**Output contract (canonical, backend-independent):**

- **D-Bus signal** on session bus:
  ```
  org.rush.Optid.FocusChanged (s app_id, s cgroup_path, u pid, t activated_ns)
  ```
  Emitted on every activation change. `cgroup_path` is the full systemd scope
  path (e.g. `/user.slice/user-1000.slice/user@1000.service/app.slice/app-org.gnome.TextEditor-abc.scope`).
  `pid` is included for debugging only. `activated_ns` is the activation
  timestamp in CLOCK_MONOTONIC ns.

- **Atomic JSON file** at `/run/user/$UID/optid/focus.json`:
  ```json
  {
    "app_id": "org.gnome.TextEditor",
    "cgroup": "/user.slice/user-1000.slice/user@1000.service/app.slice/app-org.gnome.TextEditor-abc.scope",
    "pid": 12345,
    "activated_ts": 1718700000000000000
  }
  ```
  Written via `write-temp-then-rename` for atomicity. Refreshed on every
  activation change. Root optid can poll this at its 2 s tick as a fallback
  path that does not require D-Bus.

**Failure modes:**

- Bridge crashes → focus.json goes stale. optid treats `activated_ts > now - 5s`
  as live; older than that is ignored, focus boost reverts to off. System
  degrades to resource-only behavior identical to current optid.
- Compositor unsupported → bridge logs `WARN` and exits cleanly. optid sees no
  focus.json and runs resource-only.
- Multi-seat → bridge emits one signal per seat; optid aggregates by OR-ing
  activation across seats (see §4 open question on multi-seat arbitration).

### 2.3 Root `optid` consumer

**Main loop changes** (additive to current 2 s loop):

- Block in `epoll` on:
  - PSI fds: `/proc/pressure/cpu`, `/memory`, `/io` with triggers `some 30000 500000` (30 ms / 500 ms)
  - `timerfd` 2 s fallback (existing)
  - Optional `inotify` on `/run/user/*/optid/focus.json` (only if `--focus-boost` enabled)
- On wake:
  - Scan `/sys/fs/cgroup/user.slice/user-*.slice/app-*.scope/cpu.stat`, read `usage_usec` only, compute delta vs previous, EWMA with α=0.5, hysteresis 1.2 s.
  - Read focus: either from D-Bus cache (if `--focus-boost=bus`) or `focus.json` (if `--focus-boost=file`). If `activated_ts > now - 1.5 s` → focus_boost = `interactive`; else `off`.
  - Classify: `class = max(resource_class(busiest_cgroup), focus_boost)`.
  - If class changed since last tick → actuate EPP / `platform_profile` / PM-QoS. Otherwise no-op.
- Steady overhead target: <0.6 ms CPU per 2 s tick, 0 wakeups/s while idle
  (epoll sleeps), RSS <6 MB.

**PM-QoS hold under focus boost:** When `focus_boost = interactive`, optid opens
`/dev/cpu_dma_latency` and writes `2` (microseconds, 2 ms floor). This holds
the kernel in a latency state that prevents deep C-states on the focused CPU.
On focus loss, the fd is closed, C-states collapse normally. This is the
mechanism by which focus reduces input wake latency; it is gated entirely
behind `--focus-boost`.

**Flag surface:**

- `--focus-boost=off|file|bus` (default `off` in v0.x; default `file` once §4
  experiments are closed)
- `OptidFocusEnable=false` in `/etc/optid/conf.toml` (runtime disable without
  restart)

### 2.4 Attribution contract

**Canonical key:** systemd cgroup scope path under `user.slice`.

```
/user.slice/user-$UID.slice/user@$UID.service/app.slice/app-$APP_ID-$UUID.scope
```

- Resource signal natively keyed here (`cpu.stat`).
- Focus bridge resolves `app_id`/PID → cgroup before sending to root. The root
  daemon does no PID→cgroup lookup.
- Validation: optid rejects any focus event whose `cgroup` does not start with
  `/user.slice/user-`. This prevents a malicious or buggy bridge from spoofing
  system-slice focus.
- A pinned table `app_id → workload_class` (existing in optid) is consulted at
  classification time; `cgroup → {cpu_ewma, last_focus_ts, pinned_class}` is
  the runtime hashmap.

### 2.5 Reversibility

This design is high-reversibility:

- Focus input is a pure additive floor. `--focus-boost=off` reverts optid to
  byte-identical resource-only behavior.
- The bridge can be removed without touching root `optid`; the daemon degrades
  gracefully to missing focus file.
- No kernel ABI dependency, no persistent privileged hooks, no new actuators.
- The user can uninstall the GNOME extension / KWin script / disable the
  systemd --user unit without root.

This satisfies the project's reversibility rule for additive observability
inputs.

---

## 3. Prototype Implementation

**None yet.** This research is design-only. The §2 specification is the
contract that any future prototype must satisfy; no `crates/optid-focus-bridge`
or compositor-side shims exist in the tree as of this writing.

The closest existing code is the busiest-cgroup detector inside `crates/optid/`
(modularized in PR #107). That detector already reads `cpu.stat` deltas under
`user.slice` and is the natural extension point for the §2.3 consumer changes.

What a prototype *would* contain, when authorized:

- `crates/optid-focus-bridge/` — Rust crate, `zbus` for D-Bus, `wayland-client`
  for `ext_foreign_toplevel_list`, optional `hyprctl`/`swaymsg` IPC parsers.
  Estimated 250–400 LOC.
- `crates/optid-focus-bridge/backends/gnome/` — GNOME Shell extension
  (`optid@rushlinux.org`), JavaScript, ~80 LOC, emits
  `org.rush.Optid.FocusChanged` via `dbus` extension API.
- `crates/optid-focus-bridge/backends/kde/` — KWin QML script, ~60 LOC,
  installs to `~/.local/share/kwin/scripts/optid/`.
- `crates/optid-focus-bridge/backends/wlroots/` — uses
  `ext_foreign_toplevel_list_v1` + optional `hyprctl`/`swaymsg` for PID
  resolution, ~150 LOC.
- `crates/optid/src/focus.rs` — new module in optid, ~100 LOC, reads
  `focus.json` or D-Bus cache, applies focus boost to classifier, manages
  `cpu_dma_latency` fd lifecycle.
- `crates/optid/src/main.rs` — add `--focus-boost` flag, plumb into main loop's
  epoll set (add `inotify` on `focus.json`).
- `docs/man/optid-focus-bridge.1.md` — manpage.
- `units/optid-focus-bridge.service` (user unit) and `units/optid.service`
  (system unit) updates.

Total estimated prototype surface: 600–900 LOC across Rust + JS + QML + unit
files. This is intentionally *not* in scope for this research PR — research
docs land first, prototypes land later under separate WP workplans.

---

## 4. Evidence Gaps & Open Questions

Each item below blocks promotion of this research from `WIP` to `Validated`.
Items are ordered by priority (cheapest-to-close first).

### 4.1 PSI trigger false-wake rate on idle desktop **[needs measurement]**

**Question:** With PSI triggers `some 30000 500000` on a truly idle desktop
(no user input, no background compile, just compositor + desktop shell), how
many false wakes per minute does optid see?

**Why it matters:** §2.3 claims 0 wakeups/s idle. If the desktop shell
(Mutter, plasmashell) generates enough sporadic CPU to trip PSI 30 ms/500 ms
regularly, the epoll loop will wake far more than claimed, eroding the overhead
budget.

**Experiment:**

```bash
# On a Rush Linux image (or Arch with matching kernel), GUI session up, no
# user interaction for 10 minutes:
sudo perf stat -e sched:sched_switch -- sleep 600 &
# Meanwhile, log optid wake timestamps:
optid --dry-run --focus-boost=off --log-wakes | tee /tmp/optid-wakes.log
# Compute wakes/min over the steady 10-minute window.
```

**Acceptance threshold:** <2 wakes/min idle. If exceeded, tune triggers to
`some 50000 1000000` (50 ms / 1 s) and re-measure.

### 4.2 Focus bridge end-to-end latency p95 **[needs measurement]**

**Question:** What is the p95 latency from compositor activation event to optid
receiving the focus signal, across the three backend classes (wlroots, KDE,
GNOME)?

**Why it matters:** §1.6 hypothesizes p95 <50 ms via D-Bus. If GNOME Shell
extension → session D-Bus → optid is much slower (Shell extensions run on the
GNOME Shell main loop, which can be blocked by compositing), the latency budget
is broken.

**Experiment:**

```bash
# Instrument the bridge to log activation_ns on emit.
# Instrument optid to log receive_ns on focus.json inotify fire.
# Compute delta_ns over a 100-window-switch trace (xdotool-like script under
# wlroots; manual switching under GNOME/KDE).
optid-focus-bridge --debug --log-emit-ts | tee /tmp/bridge-emit.log
optid --focus-boost=file --log-recv-ts | tee /tmp/optid-recv.log
python3 tools/analyze-focus-latency.py /tmp/bridge-emit.log /tmp/optid-recv.log
```

**Acceptance threshold:** p95 <50 ms on all three compositors. If GNOME Shell
exceeds, fall back to direct Unix socket from extension (bypass session D-Bus).

### 4.3 Flatpak/Snap `app_id → cgroup` mapping reliability **[needs measurement]**

**Question:** Over 1 week of real desktop use with mixed native + Flatpak + Snap
apps, what fraction of focus events have an `app_id` that does not cleanly map
to a cgroup scope path?

**Why it matters:** §1.4 cites upstream that app_id ↔ PID is unreliable for
sandboxed apps. If unmapped rate is >5%, the bridge cannot be the sole focus
source for those apps, and `wp_security_context_v1` integration becomes
mandatory rather than deferred.

**Experiment:**

```bash
# Run bridge with --debug --log-unmapped for 1 week of normal desktop use.
# Categorize unmapped events: app_id pattern, sandbox type (Flatpak/Snap/native),
# compositor.
optid-focus-bridge --debug --log-unmapped | tee -a /tmp/bridge-unmapped.log
# Weekly summary:
python3 tools/analyze-mapping-coverage.py /tmp/bridge-unmapped.log
```

**Acceptance threshold:** <5% unmapped. If exceeded, escalate
`wp_security_context_v1` from deferred (§5) to immediate.

### 4.4 Energy impact of focus-boost vs resource-only **[needs measurement]**

**Question:** With a focused-idle editor (e.g. GNOME TextEditor open, user
typing 1 keystroke every 3–10 s), what is the package-energy delta and
input-wake-latency delta between `--focus-boost=off` and `--focus-boost=file`?

**Why it matters:** §1.6 hypothesizes +80–250 mW idle cost and −20–40 ms wake
latency. If the energy cost is much higher (say >500 mW), the trade is not
worth it for laptop battery life. If the latency saving is much smaller (say
<10 ms), it is not user-perceptible.

**Experiment:**

```bash
# 10-minute typing trace, repeated for both modes:
# Mode A: optid --focus-boost=off
# Mode B: optid --focus-boost=file
# Measure:
#   - RAPL package energy (turbostat or powercap/sys/class/powercap/intel-rapl)
#   - Input wake latency (evtest timestamp vs compositor key event timestamp)
#   - P-state residency (turbostat MHz histogram)
sudo turbostat --quiet --show PkgWatt,Bzy_MHz,CPU%c1,CPU%c6 \
  --interval 1 > /tmp/turbostat-A.log &
evtest /dev/input/event0 | tee /tmp/evtest-A.log &
# (run typing trace for 600 s, then repeat with --focus-boost=file)
python3 tools/analyze-focus-energy.py /tmp/turbostat-{A,B}.log /tmp/evtest-{A,B}.log
```

**Acceptance threshold:** Energy cost <300 mW AND latency saving >15 ms. Both
must hold for `--focus-boost` to default to `file` in v0.x.

### 4.5 Busiest-cgroup vs focused divergence rate **[needs measurement]**

**Question:** In real user traces, how often does `busiest_cgroup !=
focused_cgroup`, and what is the CPU% delta?

**Why it matters:** §1.6 hypothesizes 15–35% divergence. If actual divergence
is <5%, focus is not worth the complexity and §2 should be descoped. If >50%,
the policy `max(resource_class, focus_boost)` may need a different composition
rule (e.g. weighted average instead of max).

**Experiment:**

```bash
# Instrument optid in dry-run to log every 2 s tick:
#   - busiest_cgroup, busiest_cgroup_cpu_pct
#   - focused_cgroup (if --focus-boost=file)
optid --dry-run --focus-boost=file --log-tick-state | tee /tmp/optid-ticks.log
# Run for 1 week of normal desktop use.
python3 tools/analyze-divergence.py /tmp/optid-ticks.log
# Report: divergence %, CPU% delta distribution, per-class breakdown.
```

**Acceptance threshold:** 10–40% divergence. <10% → descope focus. >40% →
revisit composition rule.

### 4.6 Multi-user / multi-seat focus arbitration **[needs design]**

**Question:** If two users are logged in (e.g. `user@1000.service` and
`user@1001.service` via `gdm`), each with their own bridge and `focus.json`,
how does optid arbitrate?

**Why it matters:** §2.2 says optid reads all `/run/user/*/optid/focus.json`,
but does not specify which user's focus wins when multiple are active.

**Proposed rule (needs ratification):**

1. Active local seat0 (`loginctl seat-status` says "active") wins.
2. If no active local seat (e.g. RDP/VNC), the user with the highest
   `busiest_cgroup` CPU% in the last 10 s wins.
3. Ties broken by lowest UID.

This is a design gap, not a measurement gap. Closing it requires writing the
arbitration rule into `optid` and a multi-seat test fixture.

### 4.7 PSI per-cgroup overhead at scale **[needs measurement]**

**Question:** With 200 active `app-*.scope` cgroups under `user.slice`, each
monitored for `cpu.pressure` + `memory.pressure` + `io.pressure` (600 PSI fds
total), what is the kernel memory and FD overhead?

**Why it matters:** §2.3 says optid limits PSI monitors to system-wide + top-N
cgroups dynamically. The threshold for "dynamic cap" depends on this
measurement.

**Experiment:**

```bash
# Spawn 200 transient systemd scopes under user.slice:
for i in $(seq 1 200); do
  systemd-run --user --unit=test-$i --scope sleep 3600 &
done
# Open PSI fds on each:
python3 tools/measure-psi-fd-overhead.py --scopes 200
# Report: total FDs, kernel memory (slabtop), optid RSS.
```

**Acceptance threshold:** <8 KB/fd kernel overhead, <2 MB optid RSS impact at
200 scopes. If exceeded, cap PSI monitors to system-wide + top-20 cgroups by
CPU% and document the cap.

---

## 5. What This Does NOT Change (Non-goals)

- **SPEC-northstar §0** (the objective function) — unchanged. Focus is an input
  signal; the objective remains minimizing avoidable energy subject to a
  per-class floor.
- **The authority matrix in `docs/agent-protocol.md`** — unchanged. The bridge
  is a user-session component; root `optid` retains exclusive ownership of all
  actuators (EPP, `platform_profile`, `vm.*`, PM-QoS). The bridge has no
  actuator access.
- **The 2 s main loop interval** — unchanged. What optid blocks on inside the
  2 s loop changes (epoll on PSI + inotify on focus.json), but the tick
  cadence does not.
- **The existing busiest-cgroup classifier** — unchanged in semantics. Focus is
  an additive floor via `max(resource_class, focus_boost)`, not a replacement.
- **ADR-0013's signals-vs-policy separation** — enforced, not weakened. Focus
  is a signal; `max()` is deterministic, explainable policy. `optctl explain`
  will report `focus_boost=interactive (cgroup=app-org.gnome.TextEditor-abc.scope,
  activated 1.2s ago)` when focus fired.
- **The list of actuators** — no new knobs. Focus does not get its own
  `/sys/...` write path; it only modulates the existing EPP / platform_profile
  / PM-QoS decisions.
- **Per-window (vs per-app) focus** — out of scope. The bridge emits one
  activation event for the activated toplevel, not per-window state.
- **GPU utilization tracking** — out of scope. GPU state is the compositor's
  domain; the bridge does not report GPU load.
- **A Wayland portal backend for focus** — explicitly out of scope. No focus
  portal exists today; we do not invent one.
- **`wp_security_context_v1` integration** — deferred. Required only if §4.3
  shows >5% unmapped Flatpak/Snap apps.
- **User-configurable focus policies** — out of scope. One good default ships;
  users can disable but not rewrite the policy.
- **A learned model for focus** — explicitly forbidden by ADR-0013. The
  `max(resource, focus_boost)` rule is deterministic and stays so.

---

## 6. Relationship to Existing Workplans

| Workplan / Doc | Relationship |
|---|---|
| **WP-N1 (Workload-class detector)** | Directly extends. The current busiest-cgroup detector is the resource-pull half; this research specifies the focus-boost half. The two compose via `max()`. WP-N1 cannot be marked complete without the focus-boost mechanism specified here. |
| **WP-N2 (PM QoS contract layer)** | Directly informs. The `cpu_dma_latency ≤ 2 ms` hold under focus boost is a contract-setter row in SPEC §4.2. WP-N2 must implement the fd lifecycle described in §2.3. |
| **WP-N3 (Wakeup-source + runtime-PM telemetry)** | Adjacent, not blocked. WP-N3 owns "what woke the machine"; this research owns "what should be considered important right now". Both share the cgroup attribution unit. No dependency either direction. |
| **WP-N4 (Hardware allowlist DB)** | No relationship. Focus is orthogonal to hardware allowlisting. |
| **WP-N5 (Runtime PM autosuspend policy)** | No relationship. Focus applies to CPU class + PM-QoS, not to device runtime PM. (A focused editor does not prevent a USB hub from autosuspending — that decision is owned by the per-device allowlist + floor, not by focus.) |
| **WP-N7 (Display/media depth)** | Weak relationship. A future extension could feed focus into display PSR / dGPU runtime decisions, but this is out of scope for v0.x and is not specified here. |
| **ADR-0013 (Detection and ML boundary)** | Operationalizes. ADR-0013 prescribes "authoritative signals first" — cgroup/scope identity, compositor state, kernel PSI. This research specifies exactly which authoritative signals to use for the focus dimension (compositor credentials via `wl_client_get_credentials`, cgroup path via `/proc/$pid/cgroup`, PSI via `/proc/pressure/*`). It does not weaken ADR-0013's signals-vs-policy separation; it strengthens it by giving the policy side a deterministic composition rule. |
| **0002 (Rush Linux architecture review)** | Freshens. 0002 left the workload-class-detector question open; this research resolves the focus-input sub-question of it. |
| **0003 (Unified power orchestrator paper)** | Aligned. 0003's orchestrator design has optid as the single privileged owner of all actuators; this research preserves that — the user-session bridge has no actuator access. |
| **0004 (Telemetry fidelity RCA)** | Orthogonal. 0004 is about *measurement correctness* (PSI `avg10` flattening, sysfs dead-zones). This research is about *signal selection* for the classifier. Both inform optid's observability stack but do not depend on each other. |

---

## 7. Next Steps for Continuing Agents

### 7.1 Immediate (no hardware needed)

- [ ] Draft `org.rush.Optid.FocusChanged` D-Bus interface XML in
      `docs/protocols/optid-focus.xml`.
- [ ] Add `--focus-boost=off|file|bus` flag to `crates/optid/src/args.rs`
      (default `off` in v0.x).
- [ ] Add `focus.rs` module skeleton in `crates/optid/src/` that reads
      `/run/user/*/optid/focus.json` and exposes a `FocusBoost` enum to the
      classifier. No actuation yet; dry-run logging only.
- [ ] Instrument `optid --dry-run --log-tick-state` to emit per-tick:
      `busiest_cgroup, busiest_cgroup_cpu_pct, focused_cgroup, focus_boost`.
      This unblocks §4.5.
- [ ] Write `tools/analyze-focus-latency.py`,
      `tools/analyze-mapping-coverage.py`, `tools/analyze-divergence.py`,
      `tools/analyze-focus-energy.py` skeletons. These are needed for §4
      experiments.
- [ ] Add a CI test in `crates/optid/` that verifies: with no `focus.json`
      present, optid's behavior is byte-identical to pre-focus-boost optid
      (regression guard for §2.5 reversibility).

### 7.2 Short-term (needs hardware)

- [ ] Implement `optid-focus-bridge` Rust crate with the wlroots/Hyprland
      backend (`ext_foreign_toplevel_list_v1` + `hyprctl -j activewindow`).
      Target: 250–400 LOC.
- [ ] Implement GNOME Shell extension stub (`optid@rushlinux.org`) that emits
      `org.rush.Optid.FocusChanged` from `global.display.focus_window`.
- [ ] Implement KWin script stub (`~/.local/share/kwin/scripts/optid/`) that
      pushes via `callDBus`.
- [ ] Run §4.1 (PSI false-wake rate, 10 min idle).
- [ ] Run §4.2 (bridge latency p95, 100 window switches per compositor).
- [ ] Run §4.3 (Flatpak/Snap mapping coverage, 1 week desktop use).
- [ ] Run §4.4 (energy + latency A/B, 10-min typing trace × 2 modes).
- [ ] Run §4.5 (divergence rate, 1 week dry-run log).
- [ ] Run §4.7 (PSI per-cgroup overhead at 200 scopes).

### 7.3 Medium-term

- [ ] Land `--focus-boost=file` as default in `optid` once §4.1, §4.2, §4.4,
      §4.5 meet acceptance thresholds. Bump `optid` to v0.x+1.
- [ ] Promote this research doc from `WIP` to `Validated` in
      `docs/docmap.toml` once §4.1–§4.5 + §4.7 are closed. (§4.6 multi-seat
      arbitration can remain open; it does not block single-user validation.)
- [ ] Wire multi-seat arbitration (§4.6) into `focus.rs` after the design rule
      is ratified by a human reviewer.
- [ ] If §4.3 shows >5% unmapped Flatpak/Snap, escalate
      `wp_security_context_v1` integration to immediate.
- [ ] Update SPEC §4.1 ledger row for "per-cgroup cpu.stat delta" and §4.2 row
      for "focus-derived PM-QoS cpu_dma_latency hold" to `status: implemented`
      once the corresponding code lands (separate PR, separate verifier
      session).

---

## Appendix: References

### Wayland focus and activation

- Wayland focus propagation in wlroots — https://drewdevault.com/2018/07/17/Input-handling-in-wlroots.html
- `xdg_toplevel` configuration (activation state) — https://wayland-book.com/xdg-shell-in-depth/configuration.html
- `xdg-shell` protocol reference — https://wayland.app/protocols/xdg-shell
- On window activation (xdg_activation_v1) — https://blog.broulik.de/2025/08/on-window-activation/
- `wl_client_get_credentials` API — https://wayland.freedesktop.org/docs/html/apc.html
- `security-context-v1` protocol — https://wayland.app/protocols/security-context-v1

### foreign-toplevel and the PID question

- `swaywm/wlr-protocols#57` — "foreign-toplevel should not expose pid" — https://github.com/swaywm/wlr-protocols/issues/57
- `swaywm/wlr-protocols#77` — app_id ↔ pid mapping is unreliable — https://github.com/swaywm/wlr-protocols/issues/77
- wlroots `foreign-toplevel.c` example — https://github.com/swaywm/wlroots/blob/master/examples/foreign-toplevel.c

### Resource pull: PSI and cgroups

- PSI (Pressure Stall Information) kernel docs — https://github.com/torvalds/linux/blob/master/Documentation/accounting/psi.rst
- PSI explainer (LWN, 2018) — https://lwn.net/Articles/759781/
- cgroup v2 — Arch Wiki — https://wiki.archlinux.org/title/Cgroups

### Scheduler tracing overhead

- Brendan Gregg, off-CPU analysis (9–13% perf-tracepoint overhead at 102k ctx/s) — archived snapshot: https://web.archive.org/web/20260606120419/https://www.brendangregg.com/offcpuanalysis.html

### Project-internal references

- SPEC-northstar §0 (objective function), §4 (lever ledger), §6 (WP decomposition) — `docs/SPEC-northstar.md`
- Authority matrix — `docs/agent-protocol.md`
- ADR-0013: Workload Detection And The ML Boundary — `docs/decisions/0013-detection-and-ml-boundary.md`
- Rush Linux architecture review — `docs/research/0002-rush-linux-architecture-review.md`
- Unified power orchestrator paper — `docs/research/0003-unified-power-orchestrator-paper.md`
- Telemetry fidelity RCA (orthogonal) — `docs/research/0004-telemetry-fidelity-rca-and-architecture.md`

