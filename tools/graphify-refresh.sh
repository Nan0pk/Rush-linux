#!/usr/bin/env bash
# Refresh Rush Linux's Graphify knowledge graph in a predictable way.
#
# Default mode is `code`: AST/local extraction only, no LLM/API token use.
# Use `full` explicitly when document/paper/image semantic extraction is desired
# and the required backend credentials are available.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-code}"
if [[ $# -gt 0 ]]; then
  shift
fi

find_graphify() {
  if command -v graphify >/dev/null 2>&1; then
    printf 'graphify'
    return 0
  fi
  if python3 -c 'import graphify' >/dev/null 2>&1; then
    printf 'python3 -m graphify'
    return 0
  fi
  if command -v uvx >/dev/null 2>&1; then
    printf 'uvx graphifyy'
    return 0
  fi
  cat >&2 <<'MSG'
error: graphify is not installed.
Install it with one of:
  uv tool install graphifyy
  pipx install graphifyy
  python3 -m pip install --user graphifyy
MSG
  return 1
}

GRAPHIFY_CMD="$(find_graphify)"
export PYTHONHASHSEED="${PYTHONHASHSEED:-0}"

run_graphify() {
  # shellcheck disable=SC2086 # GRAPHIFY_CMD is intentionally a small command string.
  $GRAPHIFY_CMD "$@"
}

case "$MODE" in
  code)
    # Local/AST-only refresh. This is safe for hooks and CI because it does not
    # call an LLM backend. Use --force so deletions/refactors remove stale nodes.
    run_graphify update "$ROOT" --force
    ;;
  full)
    # Full semantic update. May call a configured backend for Markdown/YAML/etc.
    # Examples:
    #   GEMINI_API_KEY=... tools/graphify-refresh.sh full --backend gemini
    #   tools/graphify-refresh.sh full --backend ollama
    run_graphify "$ROOT" --update "$@"
    ;;
  cluster)
    run_graphify cluster-only "$ROOT" --graph "$ROOT/graphify-out/graph.json" "$@"
    ;;
  query)
    if [[ $# -lt 1 ]]; then
      echo 'usage: tools/graphify-refresh.sh query "question" [graphify query flags...]' >&2
      exit 2
    fi
    run_graphify query "$@" --graph "$ROOT/graphify-out/graph.json"
    ;;
  explain)
    if [[ $# -lt 1 ]]; then
      echo 'usage: tools/graphify-refresh.sh explain "node"' >&2
      exit 2
    fi
    run_graphify explain "$1" --graph "$ROOT/graphify-out/graph.json"
    ;;
  install-hooks)
    (cd "$ROOT" && run_graphify hook install)
    ;;
  hook-status)
    (cd "$ROOT" && run_graphify hook status)
    ;;
  *)
    cat >&2 <<'MSG'
usage: tools/graphify-refresh.sh [mode]

Modes:
  code            Refresh code graph only; AST/local; no API tokens (default)
  full [args...]  Full Graphify update for docs/assets too; may use LLM backend
  cluster         Rebuild communities/report/html from graphify-out/graph.json
  query "q"       Query the committed graph without reading raw files
  explain "node"  Explain one graph node
  install-hooks   Install local post-commit/post-checkout graph refresh hooks
  hook-status     Show local Graphify git-hook status
MSG
    exit 2
    ;;
esac
