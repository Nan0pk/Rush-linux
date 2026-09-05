# Rush LiveDev — Developer Guide

> **Status:** skeleton (e2e phase). Documents the architecture boundaries
> and data flow of the LiveDev track.

Integration authority update: [ADR 0027](decisions/0027-delegated-reviewed-merges.md)
allows a coordinating agent to merge after independent review. The older
Builder/Verifier/Human diagrams below describe the LiveDev design history;
human-only merge labels are superseded for the coordinator. Collectors and
submission libraries still cannot self-merge or promote release truth.

## Architecture boundaries

```
┌─────────────────────────────────────────────────────────────────────┐
│                         ONLINE REPO                                  │
│  (github.com/Nan0pk/Rush-linux)                                     │
│  - release/milestones.toml (release truth)                          │
│  - release/evidence/ (evidence tree)                                │
│  - VERSION, RELEASES.md, ADRs                                       │
└──────────────────────────┬──────────────────────────────────────────┘
                           │ git push / PR
                           │
┌──────────────────────────▼──────────────────────────────────────────┐
│                    LiveDev IMAGE                                     │
│  (mkosi/mkosi.profiles/livedev/)                                    │
│  - optid.service (dry-run default)                                  │
│  - rush-livedev-autostart.service (safe countdown)                  │
│  - rush-capture.service (session start/stop)                        │
│  - rush-autopilot.service (plan generation)                         │
│  - /RUSH-DATA/ (persistent: repo, state, results, logs, ai, secrets)│
└──────────────────────────┬──────────────────────────────────────────┘
                           │ boot on real hardware
                           │
┌──────────────────────────▼──────────────────────────────────────────┐
│                    PLANNER                                           │
│  tools/rush-autopilot plan                                           │
│  - reads VERSION, git HEAD, milestones.toml                         │
│  - detects hardware (battery, AC, platform_profile, cpufreq, EPP)   │
│  - infers slot (laptop/desktop/server/ambiguous)                    │
│  - finds open hardware-gated criteria                               │
│  - generates typed Plan (PlanStep sequence)                         │
│  - safety floor: never proceed for destructive/final-approval       │
│  - deterministic: same inputs → same plan                           │
└──────────────────────────┬──────────────────────────────────────────┘
                           │ Plan JSON
                           │
┌──────────────────────────▼──────────────────────────────────────────┐
│                    RUNNER                                            │
│  tools/rush-autopilot run / tools/rush_runner_lib.py                │
│  - validates plan schema                                            │
│  - executes each step:                                              │
│    - command → rush-exec (typed argv, no shell strings)             │
│    - physical-prompt → wait-and-detect (AC/battery polling)         │
│    - validation → validate-hwtest-evidence.py                       │
│  - emits before/after events to tamper-evident chain                │
│  - saves partial results continuously                               │
│  - supports resume (skips completed steps)                          │
│  - fake mode: no real commands, fake sysfs, fake results            │
└──────────┬───────────────────────┬───────────────────────────────────┘
           │                       │
┌──────────▼──────────┐  ┌────────▼──────────────────────────────────┐
│   rush-exec          │  │   rush-capture                              │
│   (command runner)   │  │   (session manager)                        │
│   - typed argv       │  │   - start/event/finish/validate-chain      │
│   - captures:        │  │   - tamper-evident event chain (SHA-256)   │
│     argv, cwd,       │  │   - manifest, host, software, privacy      │
│     timestamps,      │  │   - summary.md                             │
│     exit code,       │  └────────────────────────────────────────────┘
│     stdout/stderr,   │
│     env (redacted),  │
│     payload hash     │
│   - redaction: 10    │
│     redactors        │
│   - rejects sh -c    │
└──────────┬───────────┘
           │ evidence bundle
           │
┌──────────▼──────────────────────────────────────────────────────────┐
│                    EVIDENCE VALIDATOR                                │
│  tools/validate-hwtest-evidence.py                                   │
│  - 14 checks: required files, manifest parses, source version/commit │
│    exist, hardware slot valid, laptop battery, battery/AC runs match,│
│    baseline/optid paired, sample count sufficient, results parse,    │
│    privacy report exists, secrets absent, AI not evidence, event     │
│    chain intact                                                      │
│  - schemas: schemas/hwtest-{manifest,plan,host,result}.schema.json  │
└──────────┬──────────────────────────────────────────────────────────┘
           │ validated evidence
           │
┌──────────▼──────────────────────────────────────────────────────────┐
│              OPTIONAL: rush-agent (dev-if-fail)                     │
│  tools/rush-agent / tools/rush_agent_lib.py                         │
│  - triggered when a run fails (--dev-if-fail)                       │
│  - builds redacted context bundle (all secrets removed)             │
│  - calls mock provider (deterministic, no network)                  │
│  - receives diagnosis + patch                                       │
│  - validates patch: no forbidden paths, no destructive patterns,    │
│    no claim_pass=True, no oversized patches                         │
│  - runs validation through rush-exec                                │
│  - AI never executes shell commands                                 │
│  - AI claim alone never marks pass                                  │
│  - preserves every attempt in ai-attempts/                          │
└──────────┬──────────────────────────────────────────────────────────┘
           │
┌──────────▼──────────────────────────────────────────────────────────┐
│                    PR SUBMISSION                                     │
│  tools/rush-autopilot submit-evidence / submit-code-pr              │
│  tools/rush_pr_lib.py                                               │
│  - validates evidence locally                                       │
│  - runs privacy/secret scan                                         │
│  - creates deterministic branch name                                │
│  - copies evidence into release/evidence/livedev-*/                 │
│  - commits with deterministic message                               │
│  - pushes branch                                                    │
│  - opens PR via gh CLI or GitHub API                                │
│  - NEVER merges                                                     │
│  - NEVER marks milestone verified                                   │
│  - NEVER modifies release truth                                     │
└──────────┬──────────────────────────────────────────────────────────┘
           │ PR opened
           │
┌──────────▼──────────────────────────────────────────────────────────┐
│                    CI VALIDATION                                     │
│  .github/workflows/ci.yml (change-aware LiveDev/evidence checks)    │
│  - schema validation                                                │
│  - semantic evidence validator                                      │
│  - privacy/secret scan                                              │
│  - provenance checks                                                │
│  - event-chain validation                                           │
│  - AI summaries not treated as evidence                             │
│  - release truth not changed by evidence PR                         │
│  - no self-merge commands                                           │
│  - all 134+ rush tool tests                                         │
└──────────┬──────────────────────────────────────────────────────────┘
           │ CI passes
           │
┌──────────▼──────────────────────────────────────────────────────────┐
│                    MAINTAINER APPROVAL                               │
│  (human only)                                                       │
│  - reviews PR                                                       │
│  - reviews VERIFICATION.md (from Verifier role)                     │
│  - merges to main (only the human merges)                           │
│  - flips verified = true in milestones.toml (only the human)        │
│  - ratifies ADRs (only the human)                                   │
└─────────────────────────────────────────────────────────────────────┘
```

## Tool boundaries

| Tool | Role | Can execute shell? | Can merge PRs? | Can modify release truth? |
|---|---|---|---|---|
| rush-autopilot (plan) | Planner | No | No | No |
| rush-autopilot (run) | Runner | Yes (via rush-exec) | No | No |
| rush-exec | Command runner | Yes (typed argv only) | No | No |
| rush-capture | Session manager | No | No | No |
| rush-agent | AI harness | No (mock only) | No | No |
| validate-hwtest-evidence | Validator | No | No | No |
| rush_pr_lib | PR submission | No | **NEVER** | **NEVER** |
| CI (PR Gate) | Server-side validation | No | No | No |
| Human | Maintainer | Yes | **YES** (only role) | **YES** (only role) |

## Data flow summary

```
online repo → LiveDev image → boot real hardware → planner
→ rush-exec tests → rush-capture evidence → validator
→ optional rush-agent repair → PR submission
→ CI validation → maintainer approval
```

## Key invariants

1. **Evidence Rule**: no claim without a committed command transcript.
2. **Builder/Verifier/Human split**: Builder opens PRs; Verifier runs
   acceptance; Human merges + marks verified.
3. **No self-merge**: there is no merge command in the rush tools.
4. **No release truth changes**: VERSION, milestones.toml, RELEASES.md,
   ADR Status lines, CI workflows are all forbidden.
5. **AI never executes shell**: the mock provider returns text only;
   the caller decides whether to execute.
6. **AI claim alone never marks pass**: `claim_pass=True` is rejected.
7. **Redaction before disk**: all secrets are redacted before any file
   is written to the evidence bundle.
8. **Tamper-evident event chain**: every event carries a SHA-256 hash
   linking to the previous event; editing any event breaks the chain.
9. **Read-only host disk**: the LiveDev image does not write to the
   host disk unless `--mutate-host-disk` is explicitly set.
10. **testOS preserved**: testOS is not modified, deprecated, or replaced.

## E2E dry run

```sh
# All three scenarios in one command:
python3 tools/livedev-e2e-dry-run.py --all

# Individual scenarios:
python3 tools/livedev-e2e-dry-run.py --success
python3 tools/livedev-e2e-dry-run.py --failure-no-ai
python3 tools/livedev-e2e-dry-run.py --failure-with-ai-fix
```

No real hardware, no real AI calls, no real PRs. All fake/mock.
