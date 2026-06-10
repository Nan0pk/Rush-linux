#!/usr/bin/env bash
# Rush Linux — Finish Work
#
# Run this AFTER finishing work. It:
#   1. Updates last_verified dates in docmap.toml for all changed docs
#   2. Runs ALL validators (fmt, test, clippy, policy, doc-sync)
#   3. Removes the DIRTY_STATE.md flag
#   4. Commits and pushes the CURRENT BRANCH (never main — main is PR-only)
#   5. Opens a PR with the WP id in the title and the real validation
#      transcript in the body (WP-P3 / WP-P3.1)
#
# Usage:
#   bash tools/finish-work.sh                      # interactive
#   bash tools/finish-work.sh "commit message"     # with message
#   bash tools/finish-work.sh --dry-run            # validate + report only,
#                                                  # changes NOTHING (WP-P3.1)
#   bash tools/finish-work.sh --dry-run "msg"      # dry-run with message
#
# Environment:
#   RUSH_AGENT — your name/ID (default: whoami)
#
# Exit codes: 0 = success / dry-run clean, 1 = validation or push/PR failure.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WHO="${RUSH_AGENT:-$(whoami)}"
NOW="$(date -u '+%Y-%m-%d %H:%M UTC')"
HOST="$(uname -srm)"
DIRTY_FILE="DIRTY_STATE.md"

# ── Argument parsing (WP-P3.1 Gap 1: real dry-run mode) ──────────────
DRY_RUN=0
COMMIT_MSG=""
for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        -h|--help)
            sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        --*)
            echo "❌ Unknown flag: $arg (see --help)"
            exit 1
            ;;
        *)
            if [ -z "$COMMIT_MSG" ]; then COMMIT_MSG="$arg"; else
                echo "❌ Unexpected extra argument: $arg"; exit 1
            fi
            ;;
    esac
done

echo "════════════════════════════════════════════════════"
echo "  Rush Linux — Finishing Work Session"
if [ "$DRY_RUN" -eq 1 ]; then
    echo "  MODE: DRY RUN — nothing will be modified, committed,"
    echo "        pushed, or opened as a PR."
fi
echo "════════════════════════════════════════════════════"
echo ""

# ── Step 0: Check there's something to finish ────────
if [ -z "$(git status --porcelain)" ] && [ ! -f "$DIRTY_FILE" ]; then
    echo "✅ Nothing to finish — working tree is clean and no dirty flag."
    exit 0
fi

# ── Step 1: Update docmap last_verified dates ───────
if [ "$DRY_RUN" -eq 1 ]; then
    echo ">> [dry-run] Would update doc verification dates (skipped — mutates docmap.toml)."
else
    echo ">> Updating doc verification dates..."
    python3 tools/update-docmap-dates.py || {
        echo "⚠️  Could not auto-update docmap dates. Update manually if needed."
    }
fi
echo ""

# ── Step 2: Run all validators ─────────────────────
# WP-P3.1 Gap 2: every check is recorded as a literal command + exit code
# transcript, which becomes the acceptance block in the PR body.
echo ">> Running full validation suite..."
echo ""

ERRORS=0
TRANSCRIPT=""
CHECK_LOG="$(mktemp)"
trap 'rm -f "$CHECK_LOG"' EXIT

record() { TRANSCRIPT="${TRANSCRIPT}\$ ${1}
exit=${2}
"; }

show_failure_tail() {
    # WP-P3.1 Gap 5: failure output is shown, never swallowed.
    tail -n 15 "$CHECK_LOG" | sed 's/^/        | /'
}

# Check 1: Format
echo "  [1/6] cargo fmt..."
if ! command -v cargo &>/dev/null; then
    echo "        ⚠️  Skipped (cargo not available on this host)"
    record "cargo fmt --all -- --check" "SKIPPED (cargo not installed)"
elif cargo fmt --all -- --check >"$CHECK_LOG" 2>&1; then
    echo "        ✅ Formatted"
    record "cargo fmt --all -- --check" "0"
else
    echo "        ❌ Formatting issues — run 'cargo fmt --all'"
    show_failure_tail
    record "cargo fmt --all -- --check" "1"
    ERRORS=$((ERRORS + 1))
fi

# Check 2: Tests
echo "  [2/6] cargo test..."
if ! command -v cargo &>/dev/null; then
    echo "        ⚠️  Skipped (cargo not available on this host)"
    record "cargo test --workspace" "SKIPPED (cargo not installed)"
elif cargo test --workspace >"$CHECK_LOG" 2>&1; then
    echo "        ✅ Tests pass"
    record "cargo test --workspace" "0"
else
    echo "        ❌ Tests fail"
    show_failure_tail
    record "cargo test --workspace" "1"
    ERRORS=$((ERRORS + 1))
fi

# Check 3: Clippy
echo "  [3/6] cargo clippy..."
if ! command -v cargo &>/dev/null; then
    echo "        ⚠️  Skipped (cargo not available on this host)"
    record "cargo clippy --workspace --all-targets -- -D warnings" "SKIPPED (cargo not installed)"
elif cargo clippy --workspace --all-targets -- -D warnings >"$CHECK_LOG" 2>&1; then
    echo "        ✅ No clippy warnings"
    record "cargo clippy --workspace --all-targets -- -D warnings" "0"
else
    echo "        ❌ Clippy warnings — fix before committing"
    show_failure_tail
    record "cargo clippy --workspace --all-targets -- -D warnings" "1"
    ERRORS=$((ERRORS + 1))
fi

# Check 4: Repository policy
echo "  [4/6] Repository policy..."
if command -v pwsh &>/dev/null; then
    if pwsh -File ./tools/validate-repo.ps1 >"$CHECK_LOG" 2>&1; then
        echo "        ✅ Policy valid"
        record "pwsh -File ./tools/validate-repo.ps1" "0"
    else
        echo "        ❌ Policy check failed"
        show_failure_tail
        record "pwsh -File ./tools/validate-repo.ps1" "1"
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "        ⚠️  Skipped (pwsh not available)"
    record "pwsh -File ./tools/validate-repo.ps1" "SKIPPED (pwsh not installed)"
fi

# Check 5: Doc sync
echo "  [5/6] Documentation sync..."
if python3 tools/validate-doc-sync.py --max-age 90 >"$CHECK_LOG" 2>&1; then
    echo "        ✅ Docs in sync"
    record "python3 tools/validate-doc-sync.py --max-age 90" "0"
else
    echo "        ❌ Docs out of sync — fix before committing"
    show_failure_tail
    record "python3 tools/validate-doc-sync.py --max-age 90" "1"
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
    record "git diff --name-only HEAD -- '*.rs' | xargs grep -c 'todo!|unimplemented!|FIXME|HACK'" "$TODO_COUNT placeholders"
else
    echo "        ✅ No Rust files changed"
    record "git diff --name-only HEAD -- '*.rs'" "0 (no Rust files changed)"
fi

echo ""

# ── WP id detection (WP-P3.1 Gap 4) ──────────────────────────────────
CURRENT_BRANCH=$(git branch --show-current)
WP_ID=""
# Try the commit message first: e.g. "WP-P3.1: ..." / "wp-b2 ..."
if [ -n "$COMMIT_MSG" ]; then
    WP_ID=$(echo "$COMMIT_MSG" | grep -o -i -E 'WP-[A-Z]+[0-9]+(\.[0-9]+)?(R)?' | head -1 | tr '[:lower:]' '[:upper:]' || true)
fi
# Fall back to the branch name: e.g. wp/p3.1-finish-work-gaps → WP-P3.1
if [ -z "$WP_ID" ]; then
    WP_ID=$(echo "$CURRENT_BRANCH" | grep -o -i -E '^wp/[a-z]+[0-9]+(\.[0-9]+)?(r)?' | sed 's|^wp/|WP-|' | tr '[:lower:]' '[:upper:]' || true)
fi

# ── Dry-run report and exit (WP-P3.1 Gap 1) ──────────────────────────
if [ "$DRY_RUN" -eq 1 ]; then
    echo "════════════════════════════════════════════════════"
    echo "  DRY RUN REPORT — no changes were made"
    echo "════════════════════════════════════════════════════"
    echo "  Validation errors:   $ERRORS"
    echo "  Would remove flag:   $([ -f "$DIRTY_FILE" ] && echo "$DIRTY_FILE" || echo "(absent)")"
    echo "  Would stage:"
    git status --porcelain | sed 's/^/      /'
    echo "  Commit message:      ${COMMIT_MSG:-(none — would stage only)}"
    echo "  Would push branch:   ${CURRENT_BRANCH:-(detached HEAD)}"
    echo "  WP id for PR title:  ${WP_ID:-(NOT DETECTED — PR creation would refuse)}"
    echo ""
    echo "  Validation transcript:"
    echo -e "$TRANSCRIPT" | sed 's/^/      /'
    if [ "$ERRORS" -gt 0 ]; then
        echo "  ❌ Validation would block finishing ($ERRORS error(s))."
        exit 1
    fi
    echo "  ✅ A real run would proceed to commit, push, and PR."
    exit 0
fi

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
    echo "     git push -u origin <your-branch>   # never push main directly"
    echo ""
    echo "   Or re-run with a message:"
    echo "     bash tools/finish-work.sh \"your message\""
    echo ""
    echo "════════════════════════════════════════════════════"
    echo "  ✅ Validation passed. Changes staged. Dirty flag removed."
    echo "════════════════════════════════════════════════════"
    exit 0
fi

echo ">> Committing..."
git commit -m "$COMMIT_MSG" --author="$WHO <$WHO@users.noreply.github.com>" || \
    git commit -m "$COMMIT_MSG"

# ── Step 6: Push the CURRENT BRANCH (WP-P3.1 Gap 3) ──────────────────
# main is PR-only (protect-main ruleset): direct pushes are refused here
# and would be rejected by GitHub anyway. Work branches are pushed with
# -u so the PR step below has a remote ref to target.
echo ">> Pushing..."
if [ -z "$CURRENT_BRANCH" ]; then
    echo "   ❌ Detached HEAD — cannot push. Create a branch first:"
    echo "      git checkout -b wp/<id>-<slug>"
    exit 1
fi
if [ "$CURRENT_BRANCH" = "main" ]; then
    echo "   ❌ Refusing to push main directly — main is PR-only (protect-main ruleset)."
    echo "      Move your work to a branch:"
    echo "      git checkout -b wp/<id>-<slug> && bash tools/finish-work.sh \"$COMMIT_MSG\""
    exit 1
fi
# WP-P3.1 Gap 5: push output/errors are shown, not suppressed.
if git push -u origin "$CURRENT_BRANCH"; then
    echo "   ✅ Pushed $CURRENT_BRANCH to origin"
else
    echo "   ❌ Push failed (see output above). Fix and re-run."
    exit 1
fi

echo ""
echo "════════════════════════════════════════════════════"
echo "  ✅ Work session finished and pushed."
echo "════════════════════════════════════════════════════"

# ── Step 7: Open PR (WP-P3 / WP-P3.1) ────────────────────────────────
echo ""
echo ">> Checking for GitHub CLI to open PR..."

COMPARE_URL="https://github.com/Nan0pk/Rush-linux/compare/main...$CURRENT_BRANCH"

if ! command -v gh &>/dev/null; then
    echo "   gh CLI not found — open the PR manually:"
    echo "   $COMPARE_URL"
    echo "   Put the WP id in the title and paste the validation transcript"
    echo "   from this session into the body, then report the PR URL."
    exit 0
fi

# WP-P3.1 Gap 4: the WP id is guaranteed in the title or we refuse.
if [ -z "$WP_ID" ]; then
    echo "   ❌ No WP id found in the commit message or branch name."
    echo "      PR titles must contain the WP id (work-plan-v2 WP-P3)."
    echo "      Open manually with a corrected title: $COMPARE_URL"
    exit 1
fi

case "$COMMIT_MSG" in
    *"$WP_ID"*) PR_TITLE="$COMMIT_MSG" ;;
    *)          PR_TITLE="$WP_ID: $COMMIT_MSG" ;;
esac

# WP-P3.1 Gap 2: the body carries the REAL acceptance block — the literal
# validation transcript from this run (command + exit code + date + host),
# per the §2 evidence rule. The commit message is labeled as what it is.
PR_BODY="Opened automatically by tools/finish-work.sh (WP-P3, remediated in WP-P3.1).

**Branch:** $CURRENT_BRANCH
**Agent:** $WHO
**Date:** $NOW
**Host:** $HOST

## Commit message

\`\`\`
$COMMIT_MSG
\`\`\`

## Acceptance block (validation transcript from this run)

\`\`\`
$(echo -e "$TRANSCRIPT")
\`\`\`

Checks marked SKIPPED could not run on this host and MUST be covered by CI
before merge. This PR requires verification by a separate verifier session
(builders never certify their own work — work-plan-v2 §2).

---
*Opened via tools/finish-work.sh — see work-plan-v2 WP-P3 and issue #20.*"

# WP-P3.1 Gap 5: gh output/errors are shown, not suppressed.
echo "   gh CLI detected — creating PR..."
if gh pr create --title "$PR_TITLE" --body "$PR_BODY" --base main --head "$CURRENT_BRANCH"; then
    echo "   ✅ PR created"
else
    echo "   ❌ gh pr create failed (see output above)."
    echo "      If the PR already exists, update it instead: gh pr view $CURRENT_BRANCH"
    echo "      Otherwise open manually: $COMPARE_URL"
    exit 1
fi
