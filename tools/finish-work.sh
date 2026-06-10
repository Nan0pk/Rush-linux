#!/usr/bin/env bash
# Rush Linux — Finish Work
#
# Run this AFTER finishing work. It:
#   1. Updates last_verified dates in docmap.toml for all changed docs
#   2. Runs ALL validators (fmt, test, clippy, policy, doc-sync)
#   3. Removes the DIRTY_STATE.md flag
#   4. Commits and pushes (if all checks pass)
#
# Usage:
#   bash tools/finish-work.sh                    # interactive
#   bash tools/finish-work.sh "commit message"   # with message
#
# Environment:
#   RUSH_AGENT — your name/ID (default: whoami)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WHO="${RUSH_AGENT:-$(whoami)}"
NOW="$(date -u '+%Y-%m-%d %H:%M UTC')"
DIRTY_FILE="DIRTY_STATE.md"
COMMIT_MSG="${1:-}"

echo "════════════════════════════════════════════════════"
echo "  Rush Linux — Finishing Work Session"
echo "════════════════════════════════════════════════════"
echo ""

# ── Step 0: Check there's something to finish ────────
if [ -z "$(git status --porcelain)" ] && [ ! -f "$DIRTY_FILE" ]; then
    echo "✅ Nothing to finish — working tree is clean and no dirty flag."
    exit 0
fi

# ── Step 1: Update docmap last_verified dates ───────
echo ">> Updating doc verification dates..."
python3 tools/update-docmap-dates.py 2>/dev/null || {
    echo "⚠️  Could not auto-update docmap dates. Update manually if needed."
}
echo ""

# ── Step 2: Run all validators ─────────────────────
echo ">> Running full validation suite..."
echo ""

ERRORS=0

# Check 1: Format
echo "  [1/6] cargo fmt..."
if cargo fmt --all -- --check 2>/dev/null; then
    echo "        ✅ Formatted"
else
    echo "        ❌ Formatting issues — run 'cargo fmt --all'"
    ERRORS=$((ERRORS + 1))
fi

# Check 2: Tests
echo "  [2/6] cargo test..."
if cargo test --workspace 2>/dev/null; then
    echo "        ✅ Tests pass"
else
    echo "        ❌ Tests fail"
    ERRORS=$((ERRORS + 1))
fi

# Check 3: Clippy
echo "  [3/6] cargo clippy..."
if cargo clippy --workspace --all-targets -- -D warnings 2>/dev/null; then
    echo "        ✅ No clippy warnings"
else
    echo "        ❌ Clippy warnings — fix before committing"
    ERRORS=$((ERRORS + 1))
fi

# Check 4: Repository policy
echo "  [4/6] Repository policy..."
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

# Check 5: Doc sync
echo "  [5/6] Documentation sync..."
if python3 tools/validate-doc-sync.py --max-age 90 2>/dev/null; then
    echo "        ✅ Docs in sync"
else
    echo "        ❌ Docs out of sync — fix before committing"
    ERRORS=$((ERRORS + 1))
fi

# Check 6: No leftover TODO/FIXME placeholders in changed files
echo "  [6/6] Placeholder check..."
CHANGED_RUST=$(git diff --name-only HEAD -- '*.rs' 2>/dev/null || true)
if [ -n "$CHANGED_RUST" ]; then
    TODO_COUNT=$(echo "$CHANGED_RUST" | xargs grep -c "todo!\|unimplemented!\|FIXME\|HACK" 2>/dev/null || echo "0")
    if [ "$TODO_COUNT" = "0" ]; then
        echo "        ✅ No leftover placeholders"
    else
        echo "        ⚠️  Found TODO/FIXME placeholders in changed files — review before release"
    fi
else
    echo "        ✅ No Rust files changed"
fi

echo ""

if [ "$ERRORS" -gt 0 ]; then
    echo "❌ Validation found $ERRORS error(s). Fix these before finishing."
    echo ""
    echo "   Your changes are preserved. Fix the issues above, then re-run:"
    echo "   bash tools/finish-work.sh"
    echo ""
    echo "   DIRTY_STATE.md is kept so the next agent knows work is incomplete."
    exit 1
fi

# ── Step 3: Remove dirty flag ──────────────────────
if [ -f "$DIRTY_FILE" ]; then
    rm "$DIRTY_FILE"
    echo ">> ✅ DIRTY_STATE.md removed"
fi

# ── Step 4: Stage everything ───────────────────────
echo ">> Staging all changes..."
git add -A

STAGED=$(git diff --cached --stat)
if [ -z "$STAGED" ]; then
    echo "   No changes to commit."
    echo ""
    echo "════════════════════════════════════════════════════"
    echo "  ✅ Work session finished. Repo is clean."
    echo "════════════════════════════════════════════════════"
    exit 0
fi

echo "$STAGED"
echo ""

# ── Step 5: Commit ─────────────────────────────────
if [ -z "$COMMIT_MSG" ]; then
    echo ">> No commit message provided. Changes are staged but NOT committed."
    echo "   To commit manually:"
    echo "     git commit -m \"your message\""
    echo "     git push origin main"
    echo ""
    echo "   Or re-run with a message:"
    echo "     git commit -m \"your message\" && git push origin main"
    echo ""
    echo "════════════════════════════════════════════════════"
    echo "  ✅ Validation passed. Changes staged. Dirty flag removed."
    echo "════════════════════════════════════════════════════"
    exit 0
fi

echo ">> Committing..."
git commit -m "$COMMIT_MSG" --author="$WHO <$WHO@users.noreply.github.com>" || \
    git commit -m "$COMMIT_MSG"

# ── Step 6: Push ───────────────────────────────────
echo ">> Pushing..."
if git push origin main 2>/dev/null; then
    echo "   ✅ Pushed to origin/main"
else
    # Maybe graphify auto-commit happened
    git pull --rebase origin main 2>/dev/null || true
    if git push origin main 2>/dev/null; then
        echo "   ✅ Pushed to origin/main (after rebase)"
    else
        echo "   ⚠️  Could not push. Push manually: git push origin main"
    fi
fi

echo ""
echo "════════════════════════════════════════════════════"
echo "  ✅ Work session finished. Repo is clean and pushed."
echo "════════════════════════════════════════════════════"
# ── Step 7 (NEW in WP-P3): Open PR if gh is available ─────────────────
echo ""
echo ">> Checking for GitHub CLI to open PR..."

if command -v gh &>/dev/null; then
    echo "   gh CLI detected — attempting to create PR..."
    
    # Get current branch
    CURRENT_BRANCH=$(git branch --show-current)
    
    # Check if we're not on main
    if [ "$CURRENT_BRANCH" = "main" ]; then
        echo "   ⚠️  On main branch — skipping PR creation"
    else
        # Try to create PR
        PR_TITLE="${COMMIT_MSG:-Work session: $CURRENT_BRANCH}"
        PR_BODY="This PR was created automatically by finish-work.sh (WP-P3).

**Branch:** $CURRENT_BRANCH  
**Agent:** $WHO  
**Date:** $NOW

## Acceptance block (from work session)
$COMMIT_MSG

---
*Opened via tools/finish-work.sh — see work-plan-v2 WP-P3*"
        
        if gh pr create --title "$PR_TITLE" --body "$PR_BODY" --base main 2>/dev/null; then
            echo "   ✅ PR created successfully"
        else
            echo "   ⚠️  Could not create PR automatically (may already exist or no gh auth)"
            echo "   Compare URL: https://github.com/Nan0pk/Rush-linux/compare/$CURRENT_BRANCH"
        fi
    fi
else
    echo "   gh CLI not found — printing compare URL for manual PR creation"
    CURRENT_BRANCH=$(git branch --show-current)
    if [ "$CURRENT_BRANCH" != "main" ]; then
        echo "   Compare URL: https://github.com/Nan0pk/Rush-linux/compare/$CURRENT_BRANCH"
        echo "   Please open a PR manually with the WP id in the title."
    fi
fi
