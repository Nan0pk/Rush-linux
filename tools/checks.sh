#!/usr/bin/env bash
# One change-aware check runner for local work and the Linux CI lane.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || { echo "fatal: could not cd to $ROOT" >&2; exit 1; }

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

files_matching() {
    local pattern="$1"
    if [[ "$CHANGED" == "ALL" ]]; then
        git ls-files | grep -E "$pattern" || true
    else
        printf '%s\n' "$CHANGED" | grep -E "$pattern" || true
    fi
}

shell_join() {
    local rendered=""
    printf -v rendered '%q ' "$@"
    printf '%s' "${rendered% }"
}

github_escape() {
    local value="$1"
    value="${value//'%'/'%25'}"
    value="${value//$'\r'/'%0D'}"
    value="${value//$'\n'/'%0A'}"
    printf '%s' "$value"
}

markdown_escape() {
    local value="$1"
    value="${value//\\/\\\\}"
    value="${value//|/\\|}"
    value="${value//$'\r'/}"
    value="${value//$'\n'/<br>}"
    printf '%s' "$value"
}

FAILURES=0
FAILED_RISKS=()
FAILED_COMMANDS=()
FAILED_STATUSES=()

record_failure() {
    local risk="$1"
    local command="$2"
    local status="${3:-1}"

    FAILURES=$((FAILURES + 1))
    FAILED_RISKS+=("$risk")
    FAILED_COMMANDS+=("$command")
    FAILED_STATUSES+=("$status")

    if [[ "${GITHUB_ACTIONS:-false}" == "true" ]]; then
        local message
        message="$(github_escape "$risk | exit $status | reproduce: $command")"
        printf '::error title=Rush CI check failed::%s\n' "$message"
    fi
}

write_failure_summary() {
    local index

    echo
    echo "================ RUSH CI FAILURE SUMMARY ================"
    echo "$FAILURES blocker(s) must be fixed:"
    for index in "${!FAILED_RISKS[@]}"; do
        printf '  %d. %s\n' "$((index + 1))" "${FAILED_RISKS[$index]}"
        printf '     exit: %s\n' "${FAILED_STATUSES[$index]}"
        printf '     reproduce: %s\n' "${FAILED_COMMANDS[$index]}"
    done
    echo "========================================================="

    if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
        {
            echo "## Rush CI failure summary"
            echo
            echo "**$FAILURES blocker(s) must be fixed.**"
            echo
            echo "| # | Check | Exit | Reproduce |"
            echo "|---:|---|---:|---|"
            for index in "${!FAILED_RISKS[@]}"; do
                printf '| %d | %s | `%s` | `%s` |\n' \
                    "$((index + 1))" \
                    "$(markdown_escape "${FAILED_RISKS[$index]}")" \
                    "${FAILED_STATUSES[$index]}" \
                    "$(markdown_escape "${FAILED_COMMANDS[$index]}")"
            done
            echo
            echo "The same indexed blockers are printed at the end of the job log."
        } >> "$GITHUB_STEP_SUMMARY"
    fi
}

run() {
    local risk="$1"; shift
    local command
    command="$(shell_join "$@")"
    echo
    echo ">> $command"
    echo "   Protects: $risk"
    "$@"
}

attempt() {
    "$@"
    local status=$?

    if (( status != 0 )); then
        if [[ "${1:-}" == "run" && $# -ge 3 ]]; then
            record_failure "$2" "$(shell_join "${@:3}")" "$status"
        else
            record_failure "Unclassified repository check failed" "$(shell_join "$@")" "$status"
        fi
    fi
    return 0
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

PYTHON=()
for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1 &&
       "$candidate" -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)' >/dev/null 2>&1; then
        PYTHON=("$candidate")
        break
    fi
done
if (( ${#PYTHON[@]} == 0 )); then
    echo "BLOCKED: Python 3.11+ is required for repository checks." >&2
    record_failure "R5/R8 — repository checks cannot start" "install Python 3.11+" 127
    write_failure_summary
    exit 1
fi
case "${OSTYPE:-}" in
    msys*|cygwin*) export PYTHONUTF8=1 ;;
esac

# Fast repository-integrity checks. These are deliberately canonical and run
# once; CI does not duplicate them in separate jobs.
attempt run "R5 — whitespace or conflict-marker damage entered the patch" \
    git diff --check "$BASE"...HEAD
attempt run "R5 — uncommitted whitespace damage entered the patch" \
    git diff --check
attempt run "R4/R8 — automation can merge or project truth is unsafe" \
    "${PYTHON[@]}" tools/check-workflow-safety.py
attempt run "R5/R8 — generated or compiled output entered source control" \
    "${PYTHON[@]}" tools/check-repo-hygiene.py --base "$BASE"
attempt run "R8 — public versions contradict canonical release truth" \
    "${PYTHON[@]}" tools/validate-versions.py
attempt run "R8 — documentation is missing or contradicts canonical sources" \
    "${PYTHON[@]}" tools/validate-doc-sync.py
DOCS_IMPACT_ARGS=(--base "$BASE")
if [[ "${RUSH_DOCS_NOT_NEEDED:-false}" == "true" ]]; then
    DOCS_IMPACT_ARGS+=(--allow-docs-not-needed)
fi
attempt run "R8 — a user-facing change has no matching guide update" \
    "${PYTHON[@]}" tools/check-docs-impact.py "${DOCS_IMPACT_ARGS[@]}"
attempt run "R8 — the generated practical README is stale" \
    "${PYTHON[@]}" tools/render-frontpage.py --check
attempt run "R1/R5 — optid package claims outrun integrated, verified behavior" \
    "${PYTHON[@]}" tools/validate-optid-packages.py --base "$BASE"
attempt run "R1 — a verified or release claim lacks matching proof" \
    "${PYTHON[@]}" tools/validate-evidence.py

if need pwsh "repository policy"; then
    attempt run "R4/R8 — a core invariant or unratified decision slipped in" \
        pwsh -NoProfile -File tools/validate-repo.ps1
elif $STRICT; then
    record_failure "R4/R8 — repository policy checks could not run" \
        "install pwsh and rerun bash tools/checks.sh --ci" 127
fi

if [[ "$MODE" == "quick" ]]; then
    if (( FAILURES > 0 )); then
        write_failure_summary
        exit 1
    fi
    echo
    echo "Quick starting-state checks passed."
    exit 0
fi

if matches '^\.github/workflows/.*\.ya?ml$'; then
    if need actionlint "GitHub Actions workflow"; then
        WORKFLOW_FILES=()
        while IFS= read -r file; do
            [[ -f "$file" ]] && WORKFLOW_FILES+=("$file")
        done < <(files_matching '^\.github/workflows/.*\.ya?ml$')
        if (( ${#WORKFLOW_FILES[@]} > 0 )); then
            attempt run "R5 — a changed workflow cannot execute as written" \
                actionlint -shellcheck= "${WORKFLOW_FILES[@]}"
        else
            echo "No changed workflow remains to lint (deleted workflow only)."
        fi
    elif $STRICT; then
        record_failure "R5 — GitHub Actions workflow checks could not run" \
            "install actionlint and rerun bash tools/checks.sh --ci" 127
    fi
fi

SHELL_FILES=()
while IFS= read -r file; do
    [[ -f "$file" ]] || continue
    if [[ "$file" == *.sh ]] || head -n 1 "$file" | grep -Eq '^#!.*\b(ba|da|k)?sh\b'; then
        SHELL_FILES+=("$file")
    fi
done < <(files_matching '(^|/)[^/]+$|\.sh$')

if (( ${#SHELL_FILES[@]} > 0 )); then
    for file in "${SHELL_FILES[@]}"; do
        attempt run "R5 — a changed shell entry point cannot parse" bash -n "$file"
    done
    if need shellcheck "shell static analysis"; then
        attempt run "R5 — a changed shell entry point has a static defect" \
            shellcheck --external-sources --exclude=SC1090,SC1091 "${SHELL_FILES[@]}"
    elif $STRICT; then
        record_failure "R5 — shell static analysis could not run" \
            "install shellcheck and rerun bash tools/checks.sh --ci" 127
    fi
fi

if matches '\.ps1$'; then
    if need pwsh "PowerShell parser"; then
        while IFS= read -r file; do
            [[ -f "$file" ]] || continue
            # shellcheck disable=SC2016
            # PowerShell must receive its own $variables literally.
            attempt run "R5 — a changed Windows entry point cannot parse" \
                env RUSH_PS_FILE="$file" pwsh -NoProfile -Command \
                '$tokens=$null; $errors=$null; [void][System.Management.Automation.Language.Parser]::ParseFile($env:RUSH_PS_FILE,[ref]$tokens,[ref]$errors); if ($errors.Count) { $errors | ForEach-Object { Write-Error $_ }; exit 1 }'
        done < <(files_matching '\.ps1$')
    elif $STRICT; then
        record_failure "R5 — changed PowerShell could not be parsed" \
            "install pwsh and rerun bash tools/checks.sh --ci" 127
    fi
fi

if matches '(^|/).*\.py$|^pyproject\.toml$|^schemas/|^release/evidence/livedev-'; then
    PY_FILES=()
    while IFS= read -r file; do
        [[ -f "$file" ]] && PY_FILES+=("$file")
    done < <(files_matching '\.py$')
    if (( ${#PY_FILES[@]} > 0 )); then
        attempt run "R5 — changed Python cannot compile" \
            "${PYTHON[@]}" -m py_compile "${PY_FILES[@]}"
        if need ruff "Python static analysis"; then
            attempt run "R5 — changed Python has a static defect" ruff check "${PY_FILES[@]}"
        elif $STRICT; then
            record_failure "R5 — Python static analysis could not run" \
                "install ruff and rerun bash tools/checks.sh --ci" 127
        fi
    fi
    if "${PYTHON[@]}" -c 'import pytest' >/dev/null 2>&1; then
        attempt run "R5/R6 — tooling or evidence behavior regressed" \
            "${PYTHON[@]}" -m pytest -q
    elif $STRICT; then
        echo "BLOCKED: pytest is required for Python/tooling changes in CI." >&2
        record_failure "R5/R6 — Python tests could not run" \
            "install pytest and rerun bash tools/checks.sh --ci" 127
    else
        echo "SKIP locally: pytest is unavailable; CI will run the Python tests."
    fi
    attempt run "R1/R6 — hardware evidence is incomplete or unsafe to publish" \
        "${PYTHON[@]}" tools/validate-hwtest-evidence.py --fixtures
fi

if matches '^(Cargo\.(toml|lock)|crates/|rust-toolchain)'; then
    if need cargo "Rust"; then
        attempt run "R5 — Rust formatting drifted" cargo fmt --all -- --check
        attempt run "R3/R5 — safety behavior or existing Rust behavior regressed" \
            cargo test --workspace
        attempt run "R5 — Rust defects were found by static analysis" \
            cargo clippy --workspace --all-targets -- -D warnings
        attempt run "R5 — an optional feature or target no longer compiles" \
            cargo check --workspace --all-targets --all-features
    elif $STRICT; then
        record_failure "R3/R5 — Rust checks could not run" \
            "install cargo and rerun bash tools/checks.sh --ci" 127
    fi
fi

echo
if (( FAILURES > 0 )); then
    write_failure_summary
    exit 1
fi
echo "PASS: all checks relevant to this change passed."
