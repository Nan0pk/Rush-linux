#!/usr/bin/env bash
# Build a complete Rush Linux edition set from one common base image.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "${REPO_ROOT}/tools/build-edition-set.py" "$@"
