#!/usr/bin/env bash
# Validate the actual changed areas. With a commit message, commit, push the
# branch, and open a draft PR. This script never merges and never pushes main.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DRY_RUN=false
COMMIT_MSG=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run|-n) DRY_RUN=true; shift ;;
        --) shift; COMMIT_MSG="$*"; break ;;
        *) COMMIT_MSG="${COMMIT_MSG:+$COMMIT_MSG }$1"; shift ;;
    esac
done

if [[ -z "$(git status --porcelain)" ]]; then
    echo "Nothing to finish: the working tree is clean."
    exit 0
fi

BRANCH="$(git branch --show-current)"
if [[ "$BRANCH" == "main" || "$BRANCH" == "master" ]]; then
    echo "BLOCKED: work cannot be finished directly on $BRANCH."
    echo "Risk: unreviewed work could enter the protected project history."
    echo "Root: AGENTS.md section 13."
    echo "Ways forward: create a work branch, keep these changes, and rerun this command."
    exit 1
fi

echo "Rush Linux — finish work"
echo "Branch: $BRANCH"
echo
bash tools/checks.sh

if $DRY_RUN; then
    echo
    echo "Validation passed. Dry run made no commit and pushed nothing."
    exit 0
fi

if [[ -z "$COMMIT_MSG" ]]; then
    echo
    echo "Validation passed. Supply a commit message to publish the branch:"
    echo '  bash tools/finish-work.sh "type(scope): plain-English change"'
    exit 0
fi

git add -A
git commit -m "$COMMIT_MSG"
git push -u origin "$BRANCH"

if command -v gh >/dev/null 2>&1; then
    if ! gh pr view "$BRANCH" >/dev/null 2>&1; then
        gh pr create --draft --base main --head "$BRANCH" --title "$COMMIT_MSG" --body \
            "Builder checks passed via \`bash tools/checks.sh\`. This is a draft for maintainer review; automation will not merge it."
    fi
    gh pr view "$BRANCH" --json url --jq .url
else
    echo "Branch pushed. Open a draft PR:"
    echo "https://github.com/Nan0pk/Rush-linux/compare/$BRANCH?expand=1"
fi
