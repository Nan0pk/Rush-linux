#!/usr/bin/env bash
# Fires once at session open. Injects branch, dirty-file count, and a
# doc-sync reminder into Claude's context — zero per-turn overhead after this.
set -euo pipefail

ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

BRANCH=$(git -C "$ROOT" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
DIRTY=$(git -C "$ROOT" status --short 2>/dev/null | wc -l | tr -d ' ')
LAST_COMMIT=$(git -C "$ROOT" log -1 --format="%h %s" 2>/dev/null || echo "none")

jq -n \
  --arg branch "$BRANCH" \
  --arg dirty "$DIRTY" \
  --arg last "$LAST_COMMIT" \
'{
  "hookSpecificOutput": {
    "hookEventName": "SessionStart",
    "sessionTitle": $branch,
    "additionalContext": (
      "Branch: " + $branch + "\n" +
      "Dirty files: " + $dirty + "\n" +
      "Last commit: " + $last + "\n" +
      "Doc-sync: run python3 tools/validate-doc-sync.py after any code+doc change.\n" +
      "Evidence rule: literal command transcript required — do not certify your own work."
    )
  }
}'
