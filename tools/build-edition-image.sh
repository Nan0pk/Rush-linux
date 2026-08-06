#!/usr/bin/env bash
# One-command Rush Linux edition image composition.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "${REPO_ROOT}/tools/compose-edition-image.py" "$@"
