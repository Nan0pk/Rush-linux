# Rush Linux — Action Plan (Execution Runbook)

| Field | Value |
|---|---|
| Doc | action-plan-2026-06-10, v1 |
| Governs | tactical execution only; strategy/roles remain `work-plan-v2.md` |
| Repo state | `main` @ `6c4926c`, 11 branches, 0 PRs, 0 merges |
| Intended location | `docs/plans/action-plan-2026-06-10.md` |

---

## TL;DR

Everything is blocked on ~75 minutes of human work. **Phase 0** is the merge train — exact commands below, run today. **Phase 1** queues five agent sessions for the rest of the week (verifier-on-P3 first, then B1R). **Phase 2** is your one hardware session (KVM rollback). Success by Sunday: ≥6 merges on `main`, branch CI proven live, B1R merged with a VERIFICATION.md, branch count ≤4.

---

## Phase 0 — Human Merge Train (today, ~75 min)

Prereqs: `gh auth status` works; you're in a clean clone of the repo.

### 0.1 Land plan documents (10 min)

```sh
git checkout docs/add-work-plan-v1 && git pull

# add this file + work-plan-v2.md into docs/plans/, mark v1 superseded:
#   edit docs/plans/agent-work-plan-v1.md header: "Superseded by work-plan-v2.md"
#   register both new docs in docs/docmap.toml

python3 tools/validate-doc-sync.py
git add -A && git commit -m "docs: add work-plan-v2 and action plan; mark v1 superseded"
git push
gh pr create --title "docs: work plans v1+v2 and action runbook" --base main --fill
gh pr merge --squash --delete-branch
```

### 0.2 Merge P1 FIRST — unlocks CI for everything else (10 min)

```sh
gh pr create --head wp/p1-ci-on-work-branches --base main \
  --title "WP-P1: CI on work branches + advisory cargo-audit"
gh pr merge --squash --delete-branch

# PROVE the trigger live:
git checkout -b wp/test-ci main && git commit --allow-empty -m "test: CI trigger probe" && git push -u origin wp/test-ci
gh run list --branch wp/test-ci --limit 3        # expect jobs running
git push origin --delete wp/test-ci
```

If the probe shows no runs, stop — fix the trigger before anything else merges.

### 0.3 Merge the verified backlog, in this order (25 min)

```sh
for B in wp/a1-version-consistency wp/a2-graphify-off-main wp/p2-evidence-rule-verifier wp/p3-finish-work-pr; do
  gh pr create --head "$B" --base main --title "$B" --fill || true
  gh pr checks "$B" --watch          # wait for green (P1 now gives branches CI)
  gh pr merge "$B" --squash --delete-branch
done
```

**Expected conflict:** `docs/docmap.toml` is touched by several of these. P3 already contains P2's commit, so P3 should land clean after P2. If any merge conflicts on docmap: resolve by **union** (keep both entry sets), bump `last_verified`, run `python3 tools/validate-doc-sync.py`, then continue. Do not hand-edit anything else mid-train.

**After A2 lands:** push any trivial commit to `main` (the docs merge in 0.1 may already have done it) and confirm:

```sh
git ls-remote --heads origin graphify-data    # must exist after the workflow runs
git log --oneline -5 origin/main | grep -c graphify   # expect 0 going forward
```

### 0.4 File the P3 gap as an issue, don't block on it (5 min)

```sh
gh issue create --title "WP-P3 follow-up: add dry-run mode; PR body mislabels commit msg as acceptance block" \
  --label process --body "Found in external review 2026-06-10. P3's own acceptance required a dry-run mode (absent) and the acceptance block in the PR body (currently \$COMMIT_MSG). Builder may not self-certify the fix."
```

### 0.5 Signing evidence — defer to an agent (0 min now)

Don't cherry-pick from `wp/c1-rollback-validation` yourself; Phase 1 Session 5 preps it with a §2-compliant transcript. The rollback half of that branch stays dead.

### 0.6 Burn the dead branches (5 min)

```sh
for B in wp/b1-split-optid-wip wp/b1-split-wip wp/p1-ci-on-work-branches-clean claude/intelligent-tesla-38ciR; do
  git push origin --delete "$B"
done
```

### 0.7 Branch protection — make discipline structural (10 min)

GitHub → Settings → Branches → protect `main`: require PR, require status checks (fmt, clippy, test, policy, doc-sync), no force-push, no direct push (include administrators). From this point, the C1 failure mode cannot reach `main` even if every agent misbehaves.

### 0.8 Suspend the graphify session mandate until proven (5 min)

A2's workflow now owns graph refresh on `main`. Edit `AGENTS.md`: agents no longer run `graphify-refresh.sh` locally per session (this is what was littering noise commits onto work branches). One-line change, include in the 0.1 PR if doing this first, or as a direct docs commit before protection goes on.

---

## Phase 1 — Agent Session Queue (this week, one session each)

Run in order; each builder PR gets a verifier session before you merge.

| # | Session | Role | Prompt source |
|---|---|---|---|
| 1 | **Verify P3 retroactively** | Verifier | v2 §6 verifier prompt, target = merged P3 + issue from 0.4. First live run of the protocol; cheap, instructive, and seeds VERIFICATION.md usage. |
| 2 | **B1R module split** | Builder | v2 §6 B1R prompt, verbatim. Branch off post-train `main`. One module per commit, build-green per commit, stop on first unresolvable error. |
| 3 | **Verify B1R** | Verifier | v2 §6 verifier prompt. You merge only after its VERIFICATION.md shows all-green, including the ≥8-individually-green-commits criterion. |
| 4 | **B2 fixture tests + policy matrix** | Builder | v1 §WP-B2 spec (unchanged). Needs B1R merged first. |
| 5 | **C1R runbook + signing-evidence transcript** | Builder | v2 §6 C1R prep prompt, extended: also produce the transcript-compliant signing evidence commit for cherry-pick (from the existing 2026-06-10 artifacts). |

Rules that bind all five: builder ≠ verifier per WP; no stacking on unmerged branches; `DIRTY_STATE.md` on any abort; PRs opened by `finish-work.sh` (now merged).

---

## Phase 2 — Human Hardware Gate (weekend or week 2, 1–2 h)

Run the C1R rollback test on your KVM-capable machine using the Session-5 runbook: one command, auto-captured transcript into `release/evidence/v0.4/rollback/run-<date>/`. Then, human-only: correct the evidence README checkmarks to match reality, flip `release/milestones.toml` v0.4 to `complete` (only if all three criteria now carry transcripts), and update ROADMAP/RELEASES/IMPLEMENTATION_STATUS in the same commit. v0.4 closes honestly or stays open.

---

## Phase 3 — Following Weeks (unchanged from work-plan-v2 §4)

Week 3: B3 actuator safety → B4 D-Bus hardening (your 15-min `busctl` denial check). Week 4: B5 + D1 `rush-bench`. Week 5: your two hardware benchmark sessions (D2) + D3 report → **v0.5.0-alpha.1 "First Evidence."** B6 (zbus 5) optional filler. Plan v3 gets written from D3's data, not before.

---

## Daily Cadence (15 min/day while agents run)

```sh
gh pr list                          # anything new?
gh pr view <n> --comments           # read VERIFICATION.md verdicts only
# merge if verifier-green; reject with one-line reason if not; never debug in-line
git fetch -p && git branch -r       # branch count creeping up? kill duplicates same day
```

The single rule that keeps the system honest: **nothing merges without a verifier verdict, and you never accept a claim in place of a transcript.**

---

## Definition of Done — This Week

- [ ] `main` ahead of `6c4926c` by ≥6 merges (plans, P1, A1, A2, P2, P3)
- [ ] CI proven to run on a `wp/**` push (0.2 probe transcript kept)
- [ ] `graphify-data` branch live; zero new graphify commits on `main` or work branches
- [ ] Branch protection on; dead branches deleted (total remote branches ≤4)
- [ ] B1R merged with all-green VERIFICATION.md
- [ ] P3 follow-up issue filed; signing evidence transcript prepped
- [ ] C1R runbook ready for your KVM session
