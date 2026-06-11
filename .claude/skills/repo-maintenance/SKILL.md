---
name: repo-maintenance
description: Analyze the repo for completed work, tidiness, and stale/prunable branches. Use for routine repo health checks ("analyze the repo", "check tidiness", "prune branches").
model: haiku
allowed-tools: Bash(git fetch:*), Bash(git branch:*), Bash(git log:*), Bash(git rev-list:*), Bash(git merge-base:*), Bash(python3 tools/*)
---

# Repo maintenance check

Live repo state (gathered automatically, do not re-run these):

## Recent main history
!`git fetch origin --prune --quiet && git log --oneline -15 origin/main`

## Remote branches vs main
!`for b in $(git branch -r --format='%(refname:short)' | grep -v 'origin/main' | sed 's|origin/||'); do ahead=$(git rev-list --count origin/main..origin/$b); behind=$(git rev-list --count origin/$b..origin/main); merged=$(git merge-base --is-ancestor origin/$b origin/main && echo MERGED || echo open); last=$(git log -1 --format='%cs %s' origin/$b); echo "$b | ahead:$ahead behind:$behind | $merged | $last"; done`

## Repo validators
!`python3 tools/validate-doc-sync.py 2>&1 | tail -3; python3 tools/validate-versions.py 2>&1 | tail -3`

## Instructions

1. **Completed work**: summarize what merged to main since the last check (work packages, PRs referenced in commit subjects). Do not re-read files already summarized by the log.
2. **Tidiness**: report validator results above. Only investigate further if a validator fails.
3. **Branch pruning**: using the branch table above, cross-reference open PRs with the GitHub MCP tools (`list_pull_requests`, state=open) before judging any branch useless:
   - Branch is `MERGED` into main and has no open PR → safe to delete; do it.
   - Branch has an open PR → keep; flag if >7 days stale or far behind main.
   - `graphify-data` is a disposable CI snapshot branch (force-replaced on every main push, see PR #19) → always keep, never flag.
   - Never delete a branch with an open PR without asking the maintainer.

Output one short report: merged work, validator status, branches deleted/flagged with reasons. No file dumps.
