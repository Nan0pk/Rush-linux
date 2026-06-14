# Agent Bus — Protocol Reference

## Roles in detail

### BUILDER (typically: Gemini / a coding agent)
- **Owns**: any branch except `agent-bus`, `main`, and `docs/strategy/*`.
- **Commits**: code, data, tests, fixtures, non-strategy docs.
- **Does NOT**: edit the ledger (`agent-bus` branch), open the verdict, set STATE.
- **Opens PRs** targeting `main` once the work-package is ready for verdict.
- **Receives handoffs**: from VERIFIER (specs, follow-ups, trim asks) or HUMAN (decisions).

### VERIFIER (typically: Claude / a reasoning agent)
- **Owns**: `agent-bus` branch + `docs/strategy/*` branches.
- **Commits**: BATON.md, STATUS.md, WP-*.verdict.md, WP-*.spec.md, strategic docs.
- **Does NOT**: edit code in `crates/`, runner scripts, or CI configs.
- **Reads PR diffs against `origin/main`**, not local main (avoids stale-local drift).
- **Writes verdict** when the work-package evidence is complete. Numeric caveats,
  not vague ones. Never self-certifies a gate that requires hardware.

### HUMAN (authority)
- **Owns**: merge to `main`, final trim decisions, scope triage.
- **Reads**: `STATUS.md` first. Has 60 seconds to understand where the project is.
- **Acts**: merge PR, flip baton back to BUILDER for follow-ups, accept research output.

## Branches in detail

### `main`
- Production-ready. Only HUMAN merges.
- Every commit lands via PR. No direct pushes.
- Branch protection: require CI green, require review (where GitHub allows).

### `agent-bus`
- Single ledger branch. VERIFIER pushes here.
- Contains `docs/agent-bus/{BATON.md, STATUS.md, WP-*.md}`.
- Branch protection: VERIFIER is the only committer; no PR needed for ledger commits
  (they are coordination, not code).

### Code/data branches
- `feat/<slug>` — new feature
- `fix/<slug>` or `issue-N-fix` — fix
- `research/<slug>` — research output (PRs #50/#51/#52 in Rush-linux)
- All merge to `main` via PR. CI must be green.

## Work-package lifecycle

1. **Plan (optional)** — `WP-{NAME}.plan.md` on `agent-bus`. Either role can author.
2. **Build** — BUILDER works on a code branch. Commits code + tests + data.
3. **Handoff** — BUILDER pushes final commit, opens PR, writes
   `WP-{NAME}.handoff.md` on `agent-bus`, flips BATON to VERIFIER.
4. **Verify** — VERIFIER reads the PR diff, downloads evidence, writes
   `WP-{NAME}.verdict.md` (ACCEPT / CONDITIONAL PASS / REJECT / DEFER).
5. **Act** — HUMAN merges (if ACCEPT/CONDITIONAL) or sends fix ask (if REJECT).
6. **Follow-ups** — caveat-numbered. Each follow-up is either:
   - A new WP-{NAME}-N.handoff.md (own work-package), or
   - A bare GitHub issue (small enough to file and forget).

## Verdict format (mandatory sections)

```markdown
# WP-{NAME} (PR #N) — VERIFIER VERDICT: <ACCEPT|CONDITIONAL|REJECT|DEFER>

**Branch**: `<branch>` @ `<sha>`
**PR**: #N
**Tracks**: <issue>
**Verdict by**: <role> (<session id>), <date>

## Evidence (<one-line source>)
... (verifier must enumerate every fact checked)

## Gate: <OPEN|CLOSED>
... (cite the SPEC section if applicable)

## Caveats (numbered)
1. ...
2. ...

## Next
- HUMAN: <one-line>
- BUILDER: <one-line>
- Verifier: <one-line>

Baton → <role>. Verdict by <role>, <date>.
```

## Flip rules

A flip is a **5-line minimum** change to BATON.md:

```
OWNER: <role>
TASK: <one-line>
STATE: <one-line, evidence-backed>
VERDICT: <path or "—">
UPDATED: <ISO> by <role> (<session>)
```

You may add more sections below — but the 5-line header is mandatory.
If a flip doesn't change those 5 lines, it's not a flip.

## Cross-agent communication rules

1. **PR comments are first-class.** Every REJECT / ACCEPT verdict also posts as a
   PR comment with the same content. Future agents grep PR comments.
2. **Handoff documents live forever.** Even after a work-package is closed, its
   handoff remains on `agent-bus` as historical context.
3. **Commit messages cite the WP.** `feat: ... (#74)` or `verdict(WP-NAME): ...` so
   `git log --grep=WP-NAME` works.
4. **No silent reopens.** If you reject a verdict, the next WP-* starts fresh.

## Anti-patterns (full list)

1. **Self-certifying hardware gates from CI.** CI has no battery / no RAPL.
   The verifier on CI cannot close a hardware-required gate.
2. **Stale-local diffs.** `git diff main` against your local main, not
   `origin/main`. Local main is often behind — diff against origin.
3. **Phantom claims.** PR body says "implemented RAPL priority" but
   `git show --stat` shows only `main.rs` and `runner.rs` touched.
   Verifier flags this as a flag.
4. **Hollow-green tests.** A test that passes by `return`/`SKIP` without asserting
   the new behaviour. Verifier demands an actual assertion.
5. **Out-of-scope bundling.** A critical fix PR also adds a speculative refactor.
   Verifier demands a split.
6. **Bloat in committed data.** 30 MB of all-zero `samples` arrays. Verifier
   flags repo bloat and demands trim-before/at-merge.
7. **Closed gates that re-open silently.** If a new commit reverts a closed
   gate's evidence, the gate must re-open and a new WP must start.
