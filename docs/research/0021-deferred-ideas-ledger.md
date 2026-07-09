# Research 0021: Deferred Ideas Ledger

**Date:** 2026-07-10  
**Status:** Living research ledger — not a decision record  
**Purpose:** Preserve project ideas that are strategically relevant but not ready to become immediate work packages.

## Scope and rule

An idea belongs here when it is part of the Rush Linux imagination thread but cannot yet clear one of these gates:

1. it does not map cleanly to a SPEC role: contract-setter, depth-enabler, or budget-arbitrator;
2. it needs kernel/compositor/firmware cooperation not currently available;
3. it requires hardware evidence that does not exist yet;
4. it would increase safety/privacy/release risk if implemented before supporting infrastructure lands;
5. it is strategically useful but too broad for one worker-agent PR.

Ideas filed here are not rejected. They are parked with a trigger condition.

## Ledger

| ID | Idea | Current bucket | Why deferred | Trigger to reopen |
|---|---|---|---|---|
| D-001 | Full Apple-equivalent power orchestrator across CPU, scheduler, devices, memory, display, GPU, thermal, sleep, telemetry | Long-range architecture | Correct destination, too broad for one phase | Each domain has a module spec, safety gate, synthetic test, and at least one hardware campaign path |
| D-002 | Power Intent Bridge using portals + compositor plugins | Research → staged implementation | Wayland focus/intent APIs are fragmented; portal support is not enough today | GameMode + cgroup/scope v1 lands; then choose first compositor plugin target |
| D-003 | xdg-desktop-portal upstream intent API | Upstream proposal | Requires multi-project consensus and will not ship quickly | Rush has local evidence showing intent hints improve energy without harming responsiveness |
| D-004 | Per-app battery drain attribution | Explainability research | Linux lacks per-PID energy truth for many domains; overclaim risk is high | Confidence schema exists and `--show-work` can expose raw evidence |
| D-005 | App-to-hardware telemetry pipeline | Local-first research | Privacy and attribution risk; `rush_telemetry` state must be decided first | Snapshot schema + privacy audit land; telemetry crate has an ADR outcome |
| D-006 | Render scaling for battery savings | Display/GPU research | Requires compositor cooperation and UX tuning; wrong defaults can visibly degrade user experience | Backlight/DPMS/PSR basics land and a compositor integration target is chosen |
| D-007 | Thermal-acoustic comfort model | Budget-arbitrator research | Fan/skin/noise interfaces vary wildly by vendor | Powercap/thermal observability matrix exists for reference hardware |
| D-008 | AI-agent-friendly operating system as a product feature | Governance/UX research | Agent scaffolding exists in repo, but OS-level agent workflows need security policy | LiveDev and AI interface policy are stable; no self-verify/no self-merge remains enforced |
| D-009 | Community hardware auto-promotion | Evidence policy | Good idea, but needs snapshot trust, dedup, and privacy model | N independent clean snapshots and promotion PR workflow are defined |
| D-010 | S0ix-only modern sleep stance | Adjusted concept | S0ix should be preferred, not mandated; S3 fallback avoids breaking older laptops | Sleep-test probe proves S0ix works on target hardware; otherwise fallback remains allowed |
| D-011 | Standalone `optid` as package across distros | Active plan | Not deferred; now Phase 2A hardening/packaging | Arch package path works without enabling mutation silently |
| D-012 | `DIRTY_STATE.md` replacement by `.okf` | Adjusted concept | Filename is wired into scripts; replacement creates process breakage | Adopt `.okf` TOML schema inside existing `DIRTY_STATE.md` |
| D-013 | Delete `.claude/` and `.codex/` | Rejected as stated | These directories contain tool-internal safety hooks/settings | Only instruction prose should consolidate into `AGENTS.md`; config dirs stay |
| D-014 | Full planner/actuator split into separate processes | Staged architecture | Correct boundary, but deployment/IPC/watchdog complexity is non-trivial | ADR chooses staged trait boundary vs immediate process split |
| D-015 | Hardware-contributor LiveDev campaign | Active plan with privacy gate | Needed, but risky if snapshot schema is vague | Strict default snapshot schema and validator land before public campaign |

## Filing method for future agents

When adding an idea:

1. assign the next `D-###` ID;
2. write the idea in one sentence;
3. state the current bucket: `active plan`, `research`, `ADR-needed`, `rejected as stated`, or `long-range`;
4. state why it is deferred;
5. state the trigger to reopen;
6. if it matures, move it into a plan/ADR and leave a pointer here instead of deleting it.

## Current course-correction notes

- The project should not hide big ideas, but it must not let them become unverified release claims.
- Ideas become work only when they can be expressed as a small PR with acceptance commands and a separate verifier path.
- This ledger should be reviewed during each strategic reassessment cycle, but it should not be allowed to dominate the active milestone queue.
