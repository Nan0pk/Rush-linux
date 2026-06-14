# Agent Bus — Multi-Agent Collaboration Protocol (Skill)

> A lightweight, ledger-driven protocol for multi-agent software projects.
> One ledger branch (`agent-bus`) carries the shared state across agents;
> each agent owns a narrow commit surface. Roles, branches, and templates below.

## When to use this skill

Use when **two or more agents** are collaborating on the same repo, especially when
they have **different commit authorities** (e.g. one writes code, one writes docs,
a human holds merge). It scales down to a single human + single agent too — just
collapse `Builder` and `Verifier` into one role.

Don't use when: a single agent holds the whole repo, or merge authority and code
authority are the same person. The discipline is overhead in that case.

## At a glance

| Concept        | Value                                                              |
|----------------|--------------------------------------------------------------------|
| Ledger branch  | `agent-bus` — single source of truth for cross-agent state         |
| Roles          | **BUILDER** (code/data), **VERIFIER** (ledger+strategy), **HUMAN** (merge authority) |
| Commit rules   | BUILDER → any branch except `agent-bus` and `main`. VERIFIER → only `agent-bus` and `docs/strategy/*`. HUMAN → merges `main` and edits anything. |
| Flip discipline| Every baton ownership change writes **exactly 5 lines** to `BATON.md`. No silent flips. |
| Work units     | Each work-package gets a `WP-{NAME}.{plan,handoff,verdict}.md` triplet. Optional `.plan.md`. |
| Failure mode   | REJECT verdicts never re-open silently. New evidence → new flip → new verdict. |

## Branches

| Branch            | Owner        | Purpose                                       | Direct push? |
|-------------------|--------------|-----------------------------------------------|--------------|
| `main`            | HUMAN        | Production-ready code                         | No (PR only) |
| `agent-bus`       | VERIFIER     | Cross-agent shared state                      | VERIFIER only |
| `docs/strategy/*` | VERIFIER     | North-star / strategic docs                   | VERIFIER only |
| `feat/*`, `fix/*`, `issue-*-fix`, `research/*` | BUILDER | Code + data work                  | BUILDER      |
| Strategy branches (e.g. `claude/...`)  | VERIFIER | Verifier-led exploration                  | VERIFIER     |

## Ledger files (all under `docs/agent-bus/`)

| File                | Owner        | Updated when                                    |
|---------------------|--------------|-------------------------------------------------|
| `BATON.md`          | VERIFIER     | **Every baton flip.** Exactly 5 lines minimum.  |
| `STATUS.md`         | VERIFIER     | **Every baton flip.** Human-facing summary.     |
| `WP-*.handoff.md`   | BUILDER      | When BUILDER hands a work-package to VERIFIER.  |
| `WP-*.verdict.md`   | VERIFIER     | When VERIFIER closes the work-package.          |
| `WP-*.plan.md`      | either       | Optional pre-work plan (BUILDER or VERIFIER).   |
| `WP-*.{spec,...}.md`| VERIFIER     | Optional specs issued from VERIFIER → BUILDER.  |

## Flip protocol (5-line minimum)

```
OWNER:  <role>                # BUILDER | VERIFIER | HUMAN
TASK:   <one-sentence ask>    # what the next agent must do
STATE:  <one-sentence state>  # evidence-backed, not narrative
VERDICT:<path or "—">         # docs/agent-bus/WP-*.verdict.md if closed
UPDATED:<ISO timestamp> by <role> (<session>)
```

Plus any extra sections the flip requires (next-track, action-for-human, etc).
The discipline: **never flip without updating all three (BATON, STATUS, the relevant ledger docs).**

## Verdict taxonomy

| Verdict                | Meaning                                                    |
|------------------------|------------------------------------------------------------|
| **ACCEPT**             | Evidence meets gate. Merge OK.                             |
| **CONDITIONAL PASS**   | Meets evidence but with N conditions before merge.         |
| **REJECT**             | Fails evidence. Specific blocker + specific fix path.      |
| **DEFER**              | Out of verifier scope; human decides.                      |

Every verdict names the **exact commits/SHAs verified**, the **gate cleared**, and
the **caveats** (numbered, never vague). A verdict is final unless a new WP opens.

## Templates

- [`protocol.md`](./protocol.md) — full reference (conventions, anti-patterns, recovery).
- [`BATON.md.template`](./BATON.md.template) — minimal 5-line flip block + sections.
- [`STATUS.md.template`](./STATUS.md.template) — human-facing one-screen summary.
- [`handoff.md.template`](./handoff.md.template) — BUILDER → VERIFIER handoff.
- [`verdict.md.template`](./verdict.md.template) — VERIFIER verdict (ACCEPT / REJECT).
- [`tools/flip.py`](./tools/flip.py) — helper that updates BATON + STATUS atomically.

## Anti-patterns (audit checklist)

- [ ] BATON.md updated on every flip (5-line minimum).
- [ ] No REJECT verdict reused to ACCEPT silently — open a new WP.
- [ ] No fake change claims (e.g. PR body says "implemented X" but `git show` shows nothing).
- [ ] No self-certification of hardware-required gates from a CI container.
- [ ] No bloat commits — trim large arrays before/at merge, never silently.
- [ ] No out-of-scope changes bundled into a critical fix PR (split it).

## Provenance

Authored 2026-06-15 from a real WP-ENERGY-PROBE / WP-B1E-REDO2 / WP-N1 sequence
in `Nan0pk/Rush-linux` (Rush Linux project, Fedora Workstation + Rush tooling).
