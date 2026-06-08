#!/usr/bin/env bash
# Rush Linux — Start Work
#
# Run this BEFORE making any changes. It:
#   1. Checks for a dirty state from a previous session
#   2. Pulls latest changes
#   3. Validates the repo is in a good state to start from
#   4. Creates or updates the DIRTY_STATE.md flag
#
# Usage: bash tools/start-work.sh "description of what you're about to do"

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TASK="${1:-General work session}"
WHO="${RUSH_AGENT:-$(whoami)}"
NOW="$(date -u '+%Y-%m-%d %H:%M UTC')"
DIRTY_FILE="DIRTY_STATE.md"

echo "════════════════════════════════════════════════════"
echo "  Rush Linux — Starting Work Session"
echo "════════════════════════════════════════════════════"
echo ""

# ── Step 1: Pull latest ──────────────────────────────
echo ">> Pulling latest changes..."
git pull --rebase origin main 2>/dev/null || {
    echo "⚠️  Could not pull (maybe offline or no remote). Continuing with local state."
}

# ── Step 2: Check for existing dirty state ───────────
if [ -f "$DIRTY_FILE" ]; then
    echo ""
    echo "⚠️  DIRTY STATE DETECTED — previous session left work in progress."
    echo "    Contents:"
    echo "    ─────────────────────────────────"
    head -30 "$DIRTY_FILE" | sed 's/^/    /'
    echo "    ─────────────────────────────────"
    echo ""
    echo "    Options:"
    echo "      1. Continue the previous task (default)."
    echo "      2. Run 'bash tools/finish-work.sh' to reset to clean state first."
    echo ""
fi

# ── Step 3: Validate current state ──────────────────
echo ">> Running pre-work validation..."
echo ""

ERRORS=0

# Check 1: Rust
echo "  [1/5] cargo check..."
if cargo check --workspace --quiet 2>/dev/null; then
    echo "        ✅ Compiles"
else
    echo "        ❌ Does not compile — fix before starting work"
    ERRORS=$((ERRORS + 1))
fi

# Check 2: Tests
echo "  [2/5] cargo test..."
if cargo test --workspace --quiet 2>/dev/null; then
    echo "        ✅ Tests pass"
else
    echo "        ❌ Tests fail — fix before starting work"
    ERRORS=$((ERRORS + 1))
fi

# Check 3: Policy validator
echo "  [3/5] Repository policy..."
if command -v pwsh &>/dev/null; then
    if pwsh -File ./tools/validate-repo.ps1 2>/dev/null; then
        echo "        ✅ Policy valid"
    else
        echo "        ❌ Policy check failed"
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "        ⚠️  Skipped (pwsh not available)"
fi

# Check 4: Doc sync
echo "  [4/5] Documentation sync..."
if python3 tools/validate-doc-sync.py --max-age 90 2>/dev/null; then
    echo "        ✅ Docs in sync"
else
    echo "        ❌ Docs out of sync — run 'python3 tools/validate-doc-sync.py --verbose' for details"
    ERRORS=$((ERRORS + 1))
fi

# Check 5: Dirty flag
echo "  [5/5] Dirty state flag..."
if [ -f "$DIRTY_FILE" ]; then
    echo "        ⚠️  DIRTY_STATE.md exists (previous session incomplete)"
else
    echo "        ✅ Clean state"
fi

echo ""

if [ "$ERRORS" -gt 0 ]; then
    echo "❌ Pre-work validation found $ERRORS error(s). Fix these before starting."
    echo "   The repo may have been left in a broken state."
    echo "   If the previous agent left notes in DIRTY_STATE.md, review them."
    exit 1
fi

# ── Step 4: Create/update dirty flag ────────────────
echo ">> Creating dirty state flag..."
cat > "$DIRTY_FILE" << EOF
# Work In Progress

This file exists because the repository is mid-work.
**Delete this file only by running \`tools/finish-work.sh\`.**

## Status

- **Started:** $NOW
- **Agent/Person:** $WHO
- **Task:** $TASK
- **What's done so far:** Nothing yet — just starting.
- **What's left:** Everything.
- **Known issues:** None yet.

## For the next agent

1. Read the fields above to understand what was in progress.
2. Run \`bash tools/start-work.sh\` to validate the current state and
   pick up where the previous session left off.
3. If the task above is stale or abandoned, run \`bash tools/finish-work.sh\`
   to reset to a clean validated state before starting new work.
EOF

echo "   ✅ DIRTY_STATE.md created"
echo ""
echo "════════════════════════════════════════════════════"
echo "  ✅ Ready to work. Task: $TASK"
echo "  Run 'bash tools/finish-work.sh' when done."
echo "════════════════════════════════════════════════════"
