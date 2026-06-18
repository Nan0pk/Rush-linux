#!/usr/bin/env bash
# PreToolUse hook — only fires when the `if: Bash(rm -rf *)` matcher triggers.
# Blocks the command and tells Claude why.
INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // ""')

jq -n --arg cmd "$CMD" '{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": ("Blocked: destructive command not allowed in this repo. Command was: " + $cmd + ". Use git to revert files instead.")
  }
}'
exit 2
