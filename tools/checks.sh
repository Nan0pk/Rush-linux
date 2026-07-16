#!/usr/bin/env bash
# Run the checks relevant to this change. CI is authoritative; local runs skip
# only checks whose required tool is unavailable and say so plainly.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE="changed"
BASE="${RUSH_BASE:-origin/main}"
STRICT=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --all) MODE="all"; shift ;;
        --quick) MODE="quick"; shift ;;
        --ci) STRICT=true; shift ;;
        --changed-base) BASE="$2"; shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done

if [[ "$MODE" == "all" ]]; then
    CHANGED="ALL"
else
    CHANGED="$({
        git diff --name-only "$BASE"...HEAD 2>/dev/null || true
        git diff --name-only 2>/dev/null || true
        git diff --cached --name-only 2>/dev/null || true
        git ls-files --others --exclude-standard 2>/dev/null || true
    } | sort -u)"
fi

matches() {
    [[ "$CHANGED" == "ALL" ]] || printf '%s\n' "$CHANGED" | grep -Eq "$1"
}

run() {
    local risk="$1"; shift
    echo
    echo ">> $*"
    echo "   Protects: $risk"
    "$@"
}

need() {
    local command="$1" area="$2"
    if command -v "$command" >/dev/null 2>&1; then
        return 0
    fi
    if $STRICT; then
        echo "BLOCKED: '$command' is required for the $area checks in CI." >&2
        return 1
    fi
    echo "SKIP locally: '$command' is unavailable; CI will run the $area checks."
    return 1
}

FAILURES=0
attempt() { "$@" || FAILURES=$((FAILURES + 1)); }

PYTHON=()
for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1 &&
       "$candidate" -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)' >/dev/null 2>&1; then
        PYTHON=("$candidate")
        break
    fi
done
if (( ${#PYTHON[@]} == 0 )); then
    echo "BLOCKED: a working Python 3.11+ interpreter is required for repository checks." >&2
    exit 1
fi
# Python otherwise inherits the legacy Windows console code page under Git
# Bash, and validators with Unicode headings can crash before checking files.
case "${OSTYPE:-}" in
    msys*|cygwin*) export PYTHONUTF8=1 ;;
esac

attempt run "R4/R8 — unapproved direction and stale project truth" \
    "${PYTHON[@]}" tools/check-workflow-safety.py
attempt run "R8 — public docs and versions contradict the repository" \
    "${PYTHON[@]}" tools/validate-versions.py
attempt run "R8 — documentation is missing or points at stale sources" \
    "${PYTHON[@]}" tools/validate-doc-sync.py

if need pwsh "repository policy"; then
    attempt run "R4/R8 — an unratified decision or core project invariant slipped in" \
        pwsh -NoProfile -File tools/validate-repo.ps1
elif $STRICT; then
    FAILURES=$((FAILURES + 1))
fi

if [[ "$MODE" == "quick" ]]; then
    if (( FAILURES > 0 )); then exit 1; fi
    echo
    echo "Quick starting-state checks passed."
    exit 0
fi

if matches '(^|/)([^/]+\.sh)$'; then
    while IFS= read -r file; do
        [[ -f "$file" ]] && attempt run "R5 — a changed shell entry point cannot start" bash -n "$file"
    done < <(printf '%s\n' "$CHANGED" | grep -E '\.sh$' || true)
fi

if matches '\.ps1$'; then
    if need pwsh "PowerShell parser"; then
        while IFS= read -r file; do
            [[ -f "$file" ]] || continue
            attempt run "R5 — a changed Windows entry point cannot parse" \
                env RUSH_PS_FILE="$file" pwsh -NoProfile -Command \
                '$tokens=$null; $errors=$null; [void][System.Management.Automation.Language.Parser]::ParseFile($env:RUSH_PS_FILE,[ref]$tokens,[ref]$errors); if ($errors.Count) { $errors | ForEach-Object { Write-Error $_ }; exit 1 }'
        done < <(printf '%s\n' "$CHANGED" | grep -E '\.ps1$' || true)
    elif $STRICT; then
        FAILURES=$((FAILURES + 1))
    fi
fi

if matches '^(Cargo\.(toml|lock)|crates/|rust-toolchain)'; then
    if need cargo "Rust"; then
        attempt run "R5 — Rust source is malformed" cargo fmt --all -- --check
        attempt run "R3/R5 — safety behavior or existing Rust behavior regressed" cargo test --workspace
        attempt run "R5 — Rust defects caught by static analysis" cargo clippy --workspace --all-targets -- -D warnings
    elif $STRICT; then
        FAILURES=$((FAILURES + 1))
    fi
fi

if matches '^(tools/.*\.py|tools/test-|testos/|schemas/|release/evidence/livedev-)'; then
    if "${PYTHON[@]}" -c 'import pytest' >/dev/null 2>&1; then
        attempt run "R5/R6 — test and evidence tooling regressed" \
            "${PYTHON[@]}" -m pytest tools/test-*.py -q
    elif $STRICT; then
        echo "BLOCKED: pytest is required for Python/tooling changes in CI." >&2
        FAILURES=$((FAILURES + 1))
    else
        echo "SKIP locally: pytest is unavailable; CI will run the Python tests."
    fi
    attempt run "R1/R6 — hardware evidence is incomplete or unsafe to publish" \
        "${PYTHON[@]}" tools/validate-hwtest-evidence.py --fixtures
fi

if matches '^(release/evidence/|release/milestones\.toml|release/test-tiers\.toml|tools/validate-evidence\.py)'; then
    attempt run "R1 — a verified or release claim lacks matching proof" \
        "${PYTHON[@]}" tools/validate-evidence.py
fi

if matches '^(README\.md|docs/frontpage/|docs/frontpage/project\.yml|tools/render-frontpage\.py)'; then
    attempt run "R8 — the generated public front page is stale" \
        "${PYTHON[@]}" tools/render-frontpage.py --check
fi

echo
if (( FAILURES > 0 )); then
    echo "FAILED: $FAILURES relevant check(s) failed. The failing output above is the blocker."
    exit 1
fi
echo "PASS: all checks relevant to this change passed."
