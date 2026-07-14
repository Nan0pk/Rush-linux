# Dragnet Lessons Log (append-only)

Every Dragnet run begins by reading this file and confirming each prior
countermeasure is still in force. A mistake that can be automated becomes a check
in `tools/validate-evidence.py`; one that cannot becomes a checklist line in
`docs/dragnet-protocol.md`. Never delete entries — supersede them.

---

## L-001 (Dragnet-001, 2026-06-22) — verified flags with empty evidence folders
- **Mistake:** v0.5 criteria flipped to `verified = true` while the evidence
  folders held only `*.template` placeholders (finding G1; same pattern G3 for
  v0.3/v0.4).
- **Root cause:** "scaffold created" was mistaken for "evidence captured."
- **Countermeasure:** `tools/validate-evidence.py` fails any `verified = true`
  lacking a resolving, non-empty `transcript = "<path>"`.
- **Incorporated:** required CI status check + `finish-work.sh` step + PR-template checkbox.
- **Recurrence check:** every run asserts `validate-evidence.py` is green.

## L-002 (Dragnet-001, 2026-06-22) — citation to a file that was never committed
- **Mistake:** `milestones.toml` cited `transcript-2026-06-21-qemu-tcg.log`, absent
  from the repo (G2). `lychee` missed it (path in a TOML string, not a link).
- **Root cause:** a note written as if the log existed; no enforcement scanned
  non-link path mentions.
- **Countermeasure:** `validate-evidence.py` scans `milestones.toml` and
  `release/evidence/**/*.md` for `release/evidence/...` paths that don't resolve.
- **Incorporated:** the same gate.
- **Recurrence check:** gate green.

## L-003 (Dragnet-001, 2026-06-22) — a verification table whose source is absent
- **Mistake:** the v0.3-v0.4 README presented a ✅ "Verified Markers" table as if
  transcript-derived, but the cited transcript did not exist; the same README's
  "Honest Assessment" admitted `multi-user.target` was not reached, contradicting
  the flags (G5).
- **Root cause:** honest caveat in one place, optimistic table/flags elsewhere,
  never reconciled.
- **Countermeasure:** removed the table; reconciled flags to `false`; the gate's
  path scan blocks tables citing absent files.
- **Incorporated:** gate + this audit.
- **Recurrence check:** gate green; close checklist reviews evidence READMEs.

## L-004 (Dragnet-001, 2026-06-22) — malformed, uncited "evidence" transcript
- **Mistake:** the only committed transcript captured `--help` in `optid_version`
  and began mid-line from an ANSI/`tee` artifact, and was tied to no criterion (G6).
- **Root cause:** capture script defect, never sanity-checked.
- **Countermeasure:** Dragnet `meta.txt` records real values; non-milestone samples
  are labelled as such (`NOTE.md`).
- **Incorporated:** evidence `meta.txt` convention in the protocol doc.
- **Recurrence check:** close checklist spot-checks new `meta.txt` for placeholder values.

## L-005 (Dragnet-001, 2026-06-22) — a caveat outlived its fix
- **Mistake:** the README described the `Confirms=` systemd bug as live, but it was
  already fixed in PR #163 (G7). The audit's own first pass repeated the stale claim
  before verifying the file.
- **Root cause:** asserting from a stale doc instead of checking the source.
- **Countermeasure:** verify the artifact before recording a finding; reconcile
  stale evidence notes during the close.
- **Incorporated:** checklist line "evidence docs reconciled to code."
- **Recurrence check:** manual, at each run.

## L-006 (Dragnet-001, 2026-06-22) — debt outran its tracker; three "fix" PRs missed it
- **Mistake:** Issue #162 tracked only part of the v0.3/v0.4 gap; PRs #163/#164/#165
  closed claiming "add evidence" without producing any (G4). The Authority-Matrix
  rule existed; merge-time enforcement did not.
- **Root cause:** a human-only rule with no automated, merge-blocking check.
- **Countermeasure:** `validate-evidence.py` as a *required* status check; one
  consolidated tracking issue kept in sync with this ledger.
- **Incorporated:** branch protection + tracking issue + ledger.
- **Recurrence check:** gate is a required check; ledger reconciled each run.

## L-007 (Dragnet-001, 2026-06-22) — version/status docs drifted from reality
- **Mistake:** `VERSION` never advanced after v0.5 closure (M1); `ROADMAP.md` /
  `IMPLEMENTATION_STATUS.md` disagreed with `milestones.toml` (M2).
- **Root cause:** closure work deferred; code shipped without doc edits.
- **Countermeasure:** version bump gated on a green Dragnet; status docs reconciled
  to `milestones.toml`; `validate-doc-sync.py` guards drift.
- **Incorporated:** "green Dragnet" definition; close checklist.
- **Recurrence check:** `validate-versions.py` + `validate-doc-sync.py` green each run.

## L-008 (Dragnet-001, 2026-06-22) — .gitignore silently blocked evidence transcripts
- **Mistake:** evidence files use the `transcript.log` name, but `.gitignore` had a
  global `*.log` rule, so every new transcript was silently un-committable. The lone
  pre-existing transcript survived only because it was tracked before the rule.
- **Root cause:** an unscoped ignore rule colliding with the evidence-file naming
  convention; never noticed because no one tried to commit a transcript.
- **Countermeasure:** added `!release/evidence/**/*.log` to `.gitignore`. In CI the
  evidence gate also catches this — an ignored transcript is absent from a fresh
  checkout, so `validate-evidence.py` fails there.
- **Incorporated:** `.gitignore` exception + the existing gate's on-checkout file check.
- **Recurrence check:** gate green in CI (fresh checkout) each PR.

## L-009 (Dragnet-002, 2026-06-22) — a CI refactor outran its ruleset update
- **Mistake:** PR #169 moved the evidence gate out of the required `Repository
  policy` check into a new `Evidence integrity (Dragnet)` job and merged ~2h before
  the `protect-main` ruleset was updated to require that new context. For that
  window the gate ran but did not block merges — the enforcement it provides was
  silently advisory.
- **Root cause:** "move/rename a required status check" is really two coupled
  changes (CI job + branch ruleset), but only the CI half is in-repo and reviewable;
  the ruleset half needs an admin token and was deferred, then the PR merged first.
- **Countermeasure:** treat moving/renaming a gated check as one coupled change —
  update the ruleset in the same session, before merging the CI change; if the
  ruleset can't be updated yet, keep the check in its currently-required job until
  it can. Applied retroactively as Part B (ruleset `17500512`: added the context,
  set strict).
- **Recurrence check:** required contexts in the ruleset match the gating jobs in
  `ci.yml`; spot-checked when CI job names change.
