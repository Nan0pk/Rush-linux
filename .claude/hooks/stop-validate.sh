#!/usr/bin/env bash
# Stop hook — only blocks session end when RUSH_ENFORCE_DOCSYNC=1 is set.
# Default: silent pass-through (zero overhead in normal sessions).
# Activate before commits: RUSH_ENFORCE_DOCSYNC=1 claude
set -euo pipefail

if [ "${RUSH_ENFORCE_DOCSYNC:-0}" != "1" ]; then
  exit 0
fi

ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
cd "$ROOT"

OUTPUT=$(python3 tools/validate-doc-sync.py 2>&1)
STATUS=$?

if [ $STATUS -ne 0 ]; then
  jq -n --arg out "$OUTPUT" '{
    "hookSpecificOutput": {
      "hookEventName": "Stop",
      "decision": "block",
      "additionalContext": ("doc-sync validation failed — fix before finishing:\n" + $out)
    }
  }'
  exit 2
fi

exit 0
