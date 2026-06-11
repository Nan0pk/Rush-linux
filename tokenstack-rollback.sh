#!/usr/bin/env bash
# tokenstack-rollback.sh
#
# Reverses the token-optimization stack added by commit 615721c
# (docs(claude): add token-optimization stack repo settings).
#
# Usage:
#   ./tokenstack-rollback.sh [--dry-run]
#
# --dry-run prints every action without performing it.
#
# What this script DOES (idempotent, safe to re-run):
#   - Restores any *.pre-tokenstack.bak file next to its target.
#   - Deletes the new repo files added by the stack.
#   - Removes the `headroom wrap claude` line from common shell rc files.
#   - Deregisters `codegraph` and `headroom` MCP entries from Claude Code.
#
# What this script DOES NOT do (printed as manual commands at the end):
#   - rm -rf .codegraph/                 (per-repo CodeGraph index)
#   - codegraph uninstall                (removes CodeGraph MCP wiring globally)
#   - npm uninstall -g @colbymchenry/codegraph
#   - pipx uninstall headroom-ai   OR   pip uninstall headroom-ai
#
# Those are package-/cache-level operations and are intentionally not automatic.

set -u

DRY_RUN=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    -h|--help)
      sed -n '2,28p' "$0"; exit 0 ;;
    *)
      echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$REPO_ROOT"

log() { printf '%s\n' "$*"; }
act() {
  # act <human-description> <command...>
  local desc="$1"; shift
  if [ "$DRY_RUN" -eq 1 ]; then
    log "DRY-RUN  $desc"
    log "         \$ $*"
  else
    log "DO       $desc"
    "$@" || log "         (non-fatal: command returned $?)"
  fi
}

log "tokenstack-rollback.sh  (repo: $REPO_ROOT)"
[ "$DRY_RUN" -eq 1 ] && log "MODE: dry-run (no changes will be made)"
log "----------------------------------------------------------------"

# 1. Restore *.pre-tokenstack.bak files
log ""
log "[1/4] Restore *.pre-tokenstack.bak files"
shopt -s globstar nullglob 2>/dev/null || true
found_bak=0
while IFS= read -r -d '' bak; do
  found_bak=1
  target="${bak%.pre-tokenstack.bak}"
  act "restore  $target   <- $bak" cp -- "$bak" "$target"
  act "remove   $bak" rm -f -- "$bak"
done < <(find . -type f -name '*.pre-tokenstack.bak' -print0 2>/dev/null)
[ "$found_bak" -eq 0 ] && log "  (no .pre-tokenstack.bak files found)"

# 2. Delete new files added by the stack
log ""
log "[2/4] Delete new files added by the token-optimization stack"
NEW_FILES=(
  "CLAUDE.md"
  ".claude/settings.json"
  ".claude/agents/README.md"
  "docs/token-optimization-setup.md"
)
for f in "${NEW_FILES[@]}"; do
  if [ -e "$f" ]; then
    act "delete   $f" rm -f -- "$f"
  else
    log "  skip   $f   (not present)"
  fi
done
# Prune now-empty directories (only if empty).
for d in ".claude/agents" ".claude"; do
  if [ -d "$d" ] && [ -z "$(ls -A "$d" 2>/dev/null)" ]; then
    act "rmdir    $d   (empty)" rmdir -- "$d"
  fi
done

# 3. Remove `headroom wrap claude` line from shell rc files
log ""
log "[3/4] Strip 'headroom wrap claude' from shell rc files"
for rc in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.bash_profile" "$HOME/.profile"; do
  [ -f "$rc" ] || continue
  if grep -q 'headroom wrap claude' "$rc" 2>/dev/null; then
    if [ "$DRY_RUN" -eq 1 ]; then
      log "DRY-RUN  would strip from $rc:"
      grep -n 'headroom wrap claude' "$rc" | sed 's/^/           /'
    else
      cp -- "$rc" "${rc}.tokenstack-rollback.bak"
      grep -v 'headroom wrap claude' "${rc}.tokenstack-rollback.bak" > "$rc"
      log "DO       stripped from $rc   (pre-strip copy at ${rc}.tokenstack-rollback.bak)"
    fi
  else
    log "  skip   $rc   (no matching line)"
  fi
done

# 4. Deregister codegraph + headroom MCP entries from Claude Code
log ""
log "[4/4] Deregister MCP servers from Claude Code"
if command -v claude >/dev/null 2>&1; then
  for srv in codegraph headroom; do
    if claude mcp list 2>/dev/null | grep -qi "^$srv\b\|[[:space:]]$srv[[:space:]]"; then
      act "claude mcp remove $srv" claude mcp remove "$srv"
    else
      log "  skip   MCP '$srv' not registered"
    fi
  done
else
  log "  skip   'claude' CLI not found on PATH"
fi

# Manual follow-ups
log ""
log "----------------------------------------------------------------"
log "Manual follow-ups (NOT performed automatically):"
log "  rm -rf .codegraph/                          # drop per-repo index"
log "  codegraph uninstall                          # remove MCP wiring (global)"
log "  npm uninstall -g @colbymchenry/codegraph     # remove the package"
log "  pipx uninstall headroom-ai                   # or: pip uninstall headroom-ai"
log ""
log "Done."
