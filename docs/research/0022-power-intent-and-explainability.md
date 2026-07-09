# Research 0022: Power Intent and Explainability

**Date:** 2026-07-10  
**Status:** Research note — not implementation approval  
**Related:** `docs/SPEC-northstar.md`, `docs/adaptive-engine.md`, `docs/research/0021-deferred-ideas-ledger.md`

## Problem

Rush Linux needs to know enough about user intent to set the right responsiveness floor without violating the Wayland security model or inventing a fragile global-focus spy. It also needs to explain battery drain and sleep failures without pretending Linux can measure per-app energy perfectly.

## Principle

Separate three layers:

1. **Measured facts** — direct counters or kernel state.
2. **Correlated inferences** — plausible attribution from multiple signals.
3. **Heuristics** — useful guesses that must never be presented as truth.

`optctl explain` should expose the layer for every claim.

## Confidence labels

Suggested schema:

| Label | Meaning | Examples |
|---|---|---|
| HIGH | Direct measurement from kernel/device counter or exact event | wakeup source fired; PSI pressure; RAPL package energy; device runtime PM state |
| MEDIUM | Correlated inference from multiple measured facts | process active during display-on drain window; cgroup with sustained IO while battery delta rises |
| LOW | Heuristic estimate with weak attribution | battery delta divided across active apps; foreground guess without compositor confirmation |

Rules:

- `HIGH` may state what happened.
- `MEDIUM` may say “likely contributor”.
- `LOW` may say “possible contributor” only.
- `--show-work` must print the raw inputs used for each label.
- Any UI that cannot show confidence must hide inferred blame rather than flattening it into a false fact.

## Power Intent Bridge v1

Do not wait for a perfect portal or compositor API. v1 should use existing signals:

1. **GameMode shim** — explicit game/latency-critical request.
2. **PPD profile requests** — user or desktop power preference translated into `optid` mode.
3. **systemd user scopes/cgroups** — activity and pressure by user/session unit.
4. **Explicit `optctl pin`** — user or application declares class.
5. **PSI/load/thermal/battery** — fallback when user-session context is unavailable.

v1 non-goals:

- no global keyboard/mouse/focus spying;
- no compositor-specific plugin required for Phase 2A;
- no portal API proposal until local evidence proves the value.

## Power Intent Bridge v2 candidates

- wlroots plugin for power-user experiments;
- Mutter/GNOME integration for mainstream desktop edition;
- KWin integration after sleep/wake hooks are understood;
- xdg-desktop-portal proposal only after v1 produces evidence.

## Explain drain v1

Minimum useful output:

```text
Battery drain explanation
Window: last 10 minutes
Measured facts:
- battery energy_now changed by X Wh [HIGH]
- package energy counter changed by Y J [HIGH if available]
- display was on for Z minutes [HIGH/MEDIUM depending source]
Likely contributors:
- firefox.scope: sustained CPU pressure and wakeups [MEDIUM]
Possible contributors:
- unknown display/backlight cost [LOW]
Use --show-work for raw counters.
```

## Explain sleep v1

Minimum useful output:

```text
Sleep explanation
Last suspend attempt: 2026-07-10T...
Measured facts:
- system entered s2idle / S0ix / S3 [HIGH]
- wakeup source: X [HIGH if kernel source available]
- device Y never autosuspended [HIGH/MEDIUM]
Likely issue:
- USB controller remained active during suspend window [MEDIUM]
```

## Research questions

1. Which kernel interfaces are reliable enough for `HIGH` sleep attribution across Fedora/Arch/Ubuntu?
2. Can cgroup-level activity produce useful `MEDIUM` app attribution without per-PID energy?
3. What minimum compositor data would make intent classification materially better?
4. Which explanation format avoids blame inflation while still feeling useful?
5. Should explanations be logged as structured JSON events for future corpus analysis?

## Trigger to promote to implementation

Promote to a plan when:

- `optctl explain` output schema is accepted;
- confidence labels are documented in CLI help;
- fixture tests exist for HIGH/MEDIUM/LOW rendering;
- no output path can present inferred blame without its confidence label.
