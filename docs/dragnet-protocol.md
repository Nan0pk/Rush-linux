# Dragnet — Evidence-Integrity Protocol

Dragnet is the project's recurring, read-only **evidence-integrity sweep** and the
CI gate that enforces it. It mechanizes a rule the Authority Matrix already states
(`docs/agent-protocol.md`): *"declare a gate passed without a command transcript =
nobody."* What was missing was an automated enforcer; Dragnet is it.

> **START HERE for any analysis:** run `python3 tools/dragnet.py --observe` and read
> the newest report in `release/evidence/dragnet/` before deciding "what's next."
> The latest run report is the project's current state of truth; the ledger
> (`release/evidence/dragnet/LEDGER.md`) shows exactly what evidence is still owed.

## The one command

```bash
python3 tools/dragnet.py --observe
```

Read-only. It runs the validator suite, summarises every milestone's evidence
state, writes a dated report, and exits non-zero if the project is not "green." It
never edits audited files, milestone flags, or code. Any fix it surfaces goes
through the normal PR lifecycle (Builder/Verifier, `start-work.sh`/`finish-work.sh`).

## "Green Dragnet" (the gating definition)

A run is **green** iff:
1. every validator exits 0 (`validate-evidence.py`, `validate-versions.py`,
   `validate-doc-sync.py`), **and**
2. no milestone with `status = "complete"` has an unverified criterion.

A green run is required to close a milestone or bump `VERSION`.

## The gate

`tools/validate-evidence.py` is a **required CI status check** (`.github/workflows/ci.yml`,
`policy` job) and a `tools/finish-work.sh` step. It runs on every push and PR with
no `paths:` filter, so it cannot be skipped by editing a doc. It enforces:

- every `criteria_status` with `verified = true` carries a `transcript = "<path>"`
  resolving to a non-empty file under `release/evidence/`;
- no `release/evidence/...` path mentioned in `milestones.toml` or any
  `release/evidence/**/*.md` is dangling.

`note =` is free-text commentary only — **citations live in `transcript =`**, the
single source of truth. (The CI `lychee` check only validates markdown *links*, not
paths inside code blocks/tables/TOML strings; `validate-evidence.py` covers those.)

## Cadence

- **Merge gate (CI):** the required check above, every PR.
- **Milestone-close gate (process):** run `dragnet.py --observe`; link the report
  in the close PR. The version bump is blocked until green with zero `pending`
  ledger rows for the milestone.
- **Weekly tripwire:** `.github/workflows/dragnet.yml` runs `validate-evidence.py`
  and opens an issue on failure (reuses the `reassess.yml` schedule pattern).
- **Ad-hoc:** anyone, anytime — it mutates nothing.

## Artifacts

- `release/evidence/dragnet/LEDGER.md` — per-criterion debt catalog (one row each).
- `release/evidence/dragnet/LESSONS.md` — append-only; each mistake → countermeasure.
  **Every run starts by reading this** and confirming each countermeasure holds.
- `release/evidence/dragnet/DRAGNET-NNN-YYYY-MM-DD.md` — dated run reports.
- `release/evidence/BUILD-HOST-RUNBOOK.md` — acceptance commands for criteria that
  need root + KVM.

## Milestone-close checklist

1. `python3 tools/dragnet.py --observe` is GREEN.
2. Ledger has zero `pending` rows for the milestone.
3. Each closed criterion's `meta.txt` has real values (no placeholders/`--help`).
4. Evidence READMEs reconciled to code (no stale bug notes, no tables citing
   absent files).
5. `LESSONS.md` reviewed; prior countermeasures still in force.
6. `VERSION` / `ROADMAP.md` / `RELEASES.md` / `IMPLEMENTATION_STATUS.md` consistent
   with `milestones.toml`.

## Relationship to existing machinery

Dragnet **aggregates, it does not absorb**: it is the standing arm of the existing
Evidence Rule (reuses `VERIFICATION.md`, `rust-verifier`); `validate-evidence.py`
joins the existing validator suite; the weekly tripwire reuses the Strategic
Reassessment cadence pattern and feeds that ritual's "Implementation Gates"
section. Flipping `verified`/`status` remains **Human-only** per the Authority
Matrix — Dragnet only detects and reports.
