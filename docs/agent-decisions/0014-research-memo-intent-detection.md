# Research Memo 001: Intent Detection Architecture

| Field | Value |
|---|---|
| ID | RM-001 |
| Strategic Questions | Q1, Q9, Q4 |
| Track | Track A: Intent & Interaction |
| Complexity Class | Complicated |
| Date | 2026-06-12 |
| Driver | Arena Agent |

## 1. Context & Hypothesis
Rush Linux aims to move `optid` from reactive monitoring (PSI/Thermal) to proactive intent awareness. We hypothesize that existing Linux desktop signals (Wayland, PipeWire, logind) provide sufficient metadata to identify "High Priority Foreground" vs. "Idle" vs. "Content Consumption" vs. "Communication" states without custom per-app patches.

## 2. Methodology
- **Audit:** Reviewed Wayland protocols (`ext-idle-notify-v1`, `xdg-activation-v1`).
- **Discovery:** Investigated D-Bus interfaces for GNOME Shell, KDE KWin, and systemd-logind.
- **Analysis:** Cross-referenced PipeWire/WirePlumber stream state capabilities.

## 3. Evidence & Data
- **Wayland Standard:** `ext-idle-notify-v1` is the current standard for idle/resume detection. It is supported by Hyprland, Sway, and increasingly others.
- **Foreground Metadata Gap:** No universal Wayland protocol exists to retrieve the *title* or *AppID* of the currently focused window for security reasons.
    - **GNOME:** Requires `org.gnome.Shell.Eval` or a custom Shell Extension (D-Bus).
    - **KDE:** Uses `org.kde.PlasmaWindowManagement` (D-Bus).
    - **Wlroots:** Uses `wlr-foreign-toplevel-management-v1`.
- **Media/Communication Signals:** PipeWire/WirePlumber successfully tracks active camera and microphone streams. `wpctl status` and D-Bus properties of `org.freedesktop.portal.ScreenCast` provide reliable indicators of "Communication Mode" (active mic/cam).
- **Session State:** `systemd-logind` provides `IdleHint`, `LockedHint`, and `Active` properties via D-Bus on `/org/freedesktop/login1/session/auto`.

## 4. Option Comparison

| Option | Pros | Cons | MCDA Score (H/M/L) |
|---|---|---|---|
| **A: Universal Protocol** | Future-proof, clean architecture. | High latency to adoption (years). | **M** (Slow) |
| **B: Compositor Adapters** | Immediate data, high resolution. | High maintenance, fragmented. | **M** (Brittle) |
| **C: Hybrid (Optid D-Bus + Shims)** | Native speed in `optid`, simple shims. | Requires one shim per DE. | **H** (Recommended) |

## 5. Pre-Mortem Analysis
**Failure Scenario:** If we pick Option C and a major DE (e.g., GNOME) locks down D-Bus access to window metadata, `optid` loses its primary intent signal.
**Mitigation:** Fall back to `logind` session activity and PipeWire stream presence, which are DE-agnostic.

## 6. Decision Hint
- **Q1 (Architecture):** Adopt **Hybrid (Option C)**. Implement a generic `io.rushlinux.Optid.Intent` D-Bus interface. Write a reference GNOME extension and KWin script to push "FocusedApp" info to `optid`.
- **Q9 (Standardization):** Use the data from v0.6 to propose `ext-user-intent-v1` to the Wayland protocols group.
- **Q4 (Market):** Focus on "Developer/Builder" signals first (detecting `make`, `ninja`, `cargo` via eBPF/process tree) as it’s DE-independent.

## 7. Reversal Plan
If D-Bus shims become unmaintainable, pivot `optid` to a "Pure Signal" model: use eBPF to track process importance and PipeWire for media, ignoring the Wayland window title entirely.
