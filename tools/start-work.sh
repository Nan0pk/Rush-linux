#!/usr/bin/env bash
# Start a Rush work session without mutating main or blocking unrelated work on
# missing compilers. Full, change-specific checks run at finish time and in CI.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TASK="${*:-}"
if [[ -z "$TASK" ]]; then
    echo 'Usage: bash tools/start-work.sh "short task description"' >&2
    exit 2
fi

echo "Rush Linux — start work"
echo "Task: $TASK"

if [[ -n "$(git status --porcelain)" ]]; then
    echo
    echo "BLOCKED: the checkout already contains uncommitted work."
    echo "Risk: starting a second task could mix with or overwrite someone else's work."
    echo "Root: AGENTS.md sections 8 and 13."
    echo "Ways forward: continue the existing work, commit it on its branch, or use a clean checkout."
    git status --short
    exit 1
fi

BRANCH="$(git branch --show-current)"
if [[ "$BRANCH" == "main" || "$BRANCH" == "master" ]]; then
    SLUG="$(printf '%s' "$TASK" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/^-//; s/-$//' | cut -c1-48)"
    BRANCH="work/$(date -u +%Y%m%d)-${SLUG:-task}"
    git switch -c "$BRANCH"
fi

echo "Branch: $BRANCH"
echo "Base: $(git rev-parse --short HEAD)"
echo
bash tools/checks.sh --quick
echo
echo "Ready. Implement the smallest coherent change, then run:"
echo "  bash tools/finish-work.sh --dry-run"
