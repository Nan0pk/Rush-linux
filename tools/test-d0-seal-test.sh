#!/usr/bin/env bash
# CI-friendly test for D0 seal-test binary
set -euo pipefail

echo "=== D0 seal-test binary smoke test ==="

cargo build --features experimental-capability-sealing --bin optid-capability-seal-test --quiet

echo "1. Testing --recovery-order (does not need Landlock)"
./target/debug/optid-capability-seal-test --recovery-order
echo "   PASS: recovery-order simulation succeeded"

echo "2. Testing --exit-75"
set +e
./target/debug/optid-capability-seal-test --exit-75
code=$?
set -e
if [[ $code -eq 75 ]]; then
    echo "   PASS: exited with 75 (topology rebuild)"
else
    echo "   FAIL: expected exit 75, got $code"
    exit 1
fi

echo "All D0 smoke tests passed."
