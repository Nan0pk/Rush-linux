#!/usr/bin/env bash
# Reproduce the read-only S5D post-merge proof outside GitHub Actions.
# This script never writes a package receipt or changes repository state.

set -euo pipefail

MERGED_COMMIT="f3d785df064c9b2734509307bd1b33cf409ea9fb"
IMPLEMENTATION_HEAD="f1b38e3e4b1b1b8f2e48a65eeb84a31b600654c6"
EXPECTED_HEAD=""
OUTPUT_DIR=""
FULL_SYSTEM_PROOF=false
INSTALL_DEPS=false
NO_FETCH=false
DISPOSABLE_VM_ACK=false

usage() {
    cat <<'USAGE'
Usage: tools/verify-s5d-shadow.sh [options]

Runs the source-bound S5D checks and writes a local evidence bundle. It does not
publish an official verification receipt and cannot mark S5D completed.

Options:
  --full-system-proof       Also run root/systemd/Landlock supervisor proof.
  --install-deps            On Ubuntu, install workflow dependencies with apt.
  --disposable-vm           Confirm full proof runs in a disposable Ubuntu VM.
  --expected-head SHA       Require the current checkout to equal SHA.
  --output-dir PATH         Evidence directory (default: /tmp/rush-s5d-shadow-*).
  --no-fetch                Do not refresh origin/main before provenance checks.
  -h, --help                Show this help.
USAGE
}

while (($#)); do
    case "$1" in
        --full-system-proof) FULL_SYSTEM_PROOF=true; shift ;;
        --install-deps) INSTALL_DEPS=true; shift ;;
        --disposable-vm) DISPOSABLE_VM_ACK=true; shift ;;
        --expected-head) EXPECTED_HEAD="${2:?missing SHA}"; shift 2 ;;
        --output-dir) OUTPUT_DIR="${2:?missing path}"; shift 2 ;;
        --no-fetch) NO_FETCH=true; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$ROOT" ]]; then
    echo "fatal: run inside a Rush-linux git checkout" >&2
    exit 2
fi
cd "$ROOT"

HEAD_SHA="$(git rev-parse HEAD)"
EXPECTED_HEAD="${EXPECTED_HEAD:-$HEAD_SHA}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUTPUT_DIR="${OUTPUT_DIR:-${TMPDIR:-/tmp}/rush-s5d-shadow-${STAMP}-${HEAD_SHA:0:12}}"

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

for command in git python3 cargo jq sha256sum uname; do
    need "$command"
done

if [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
    fail "checkout is dirty; commit or remove local changes before proof"
fi

mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR="$(cd "$OUTPUT_DIR" && pwd)"
PROOF_LOG="$OUTPUT_DIR/proof.log"
exec > >(tee -a "$PROOF_LOG") 2>&1

echo "s5d_shadow_started=$STAMP"
echo "head=$HEAD_SHA"
echo "expected_head=$EXPECTED_HEAD"
echo "merged_commit=$MERGED_COMMIT"
echo "implementation_head=$IMPLEMENTATION_HEAD"
echo "output_dir=$OUTPUT_DIR"

test "$HEAD_SHA" = "$EXPECTED_HEAD" || fail "HEAD moved from expected SHA"

if ! $NO_FETCH; then
    git fetch origin main --force
fi
test "$(git rev-parse origin/main)" = "$MERGED_COMMIT" || \
    fail "origin/main is not the source-bound S5D integrated commit"
git cat-file -e "$IMPLEMENTATION_HEAD^{commit}"
git cat-file -e "$MERGED_COMMIT^{commit}"
git merge-base --is-ancestor "$IMPLEMENTATION_HEAD" "$MERGED_COMMIT"
git merge-base --is-ancestor "$MERGED_COMMIT" "$HEAD_SHA"

python3 - "$IMPLEMENTATION_HEAD" "$MERGED_COMMIT" <<'PY'
import subprocess
import sys
import tomllib
from pathlib import Path

implementation, merged = sys.argv[1:]
ledger = tomllib.loads(Path("docs/plans/optid-package-status.toml").read_text())
packages = {item["id"]: item for item in ledger["package"]}
s5d = packages["S5D"]
c1 = packages["C1"]
assert ledger["active_safety"] == "S5D"
assert s5d["status"] == "candidate"
assert s5d["pr"] == "392"
assert "verification_receipt" not in s5d
assert c1["status"] == "planned"
assert c1["depends"] == ["F1", "F3"]
assert s5d["depends"] == ["F3", "F4", "S4D"]
for dependency in set(s5d["depends"] + c1["depends"]):
    assert packages[dependency]["status"] == "completed"

paths = sorted(set(
    s5d.get("runtime_entrypoints", [])
    + s5d.get("integration_tests", [])
    + s5d.get("completion_evidence", [])
))
assert paths
for path in paths:
    subprocess.run(["git", "diff", "--quiet", implementation, merged, "--", path], check=True)
    subprocess.run(["git", "diff", "--quiet", merged, "HEAD", "--", path], check=True)
print(f"s5d_source_bound_paths={len(paths)}")
PY

python3 tools/validate-current-work.py
python3 tools/validate-optid-packages.py
python3 tools/render-frontpage.py --check
git diff --check
test ! -e docs/plans/optid-verification/s5d.toml

echo "source_binding_and_prestate=pass"

cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings

python3 - <<'PY' > "$OUTPUT_DIR/acceptance-tests.txt"
import tomllib
from pathlib import Path
ledger = tomllib.loads(Path("docs/plans/optid-package-status.toml").read_text())
package = next(item for item in ledger["package"] if item["id"] == "S5D")
tests = list(package.get("acceptance_tests", {}).values())
assert len(tests) == 13, tests
assert len(set(tests)) == 13, tests
print("\n".join(tests))
PY

count=0
while IFS= read -r short_name; do
    count=$((count + 1))
    echo "mapped_acceptance=$short_name"
    listed="$(mktemp)"
    cargo test -p optid --all-features --color never "$short_name" -- --list 2>&1 | tee "$listed"
    full_name="$(python3 - "$short_name" "$listed" <<'PY'
import sys
from pathlib import Path
short = sys.argv[1]
lines = Path(sys.argv[2]).read_text(encoding="utf-8", errors="replace").splitlines()
matches = []
for line in lines:
    if line.endswith(": test"):
        name = line[:-6]
        if name == short or name.endswith("::" + short):
            matches.append(name)
if len(matches) != 1:
    raise SystemExit(f"{short}: expected one exact mapped test, found {matches}")
print(matches[0])
PY
)"
    rm -f "$listed"
    echo "exact_acceptance=$full_name"
    cargo test -p optid --all-features --color never "$full_name" -- --exact --nocapture
done < "$OUTPUT_DIR/acceptance-tests.txt"
test "$count" -eq 13

cargo test -p optid --all-features
cargo test --workspace --all-features
echo "s5d_acceptance_and_workspace_regression=pass"

LANDLOCK_ABI=""
KERNEL_RELEASE="$(uname -r)"

if $FULL_SYSTEM_PROOF; then
    $DISPOSABLE_VM_ACK || fail "--full-system-proof requires --disposable-vm"
    [[ -r /etc/os-release ]] || fail "missing /etc/os-release"
    # shellcheck disable=SC1091
    source /etc/os-release
    [[ "${ID:-}" == "ubuntu" && "${VERSION_ID:-}" == "24.04" ]] || \
        fail "full proof requires a disposable Ubuntu 24.04 VM"
    need systemctl
    need journalctl
    need systemd-analyze
    need gzip

    if ((EUID == 0)); then
        SUDO=()
    else
        need sudo
        sudo -n true || fail "passwordless sudo is required for full proof"
        SUDO=(sudo)
    fi

    as_nobody() {
        if ((EUID == 0)); then
            runuser -u nobody -- "$@"
        else
            sudo -u nobody -- "$@"
        fi
    }

    if $INSTALL_DEPS; then
        "${SUDO[@]}" apt-get update
        "${SUDO[@]}" apt-get install -y libdbus-1-dev pkg-config jq
        rustup component add clippy rustfmt
    fi

    for protected in \
        /usr/libexec/optid \
        /usr/libexec/optid-recover \
        /usr/local/libexec/optid-capability-seal-test \
        /etc/systemd/system/optid-apply.service \
        /etc/systemd/system/optid-recover.service \
        /etc/systemd/system/optid-capability-seal-test.service; do
        [[ ! -e "$protected" ]] || fail "refusing to overwrite existing host path: $protected"
    done

    cleanup() {
        set +e
        "${SUDO[@]}" rm -f /run/systemd/system/optid-capability-seal-test.service.d/non75.conf
        "${SUDO[@]}" rm -f /run/optid-capability-seal-test-enabled
        "${SUDO[@]}" systemctl stop optid-capability-seal-test.service >/dev/null 2>&1
        "${SUDO[@]}" systemctl reset-failed optid-capability-seal-test.service >/dev/null 2>&1
        "${SUDO[@]}" rm -f \
            /usr/libexec/optid \
            /usr/libexec/optid-recover \
            /usr/local/libexec/optid-capability-seal-test \
            /usr/share/man/man8/optid.8.gz \
            /etc/systemd/system/optid-apply.service \
            /etc/systemd/system/optid-recover.service \
            /etc/systemd/system/optid-capability-seal-test.service
        "${SUDO[@]}" rm -rf /run/systemd/system/optid-capability-seal-test.service.d
        [[ -z "${proof_state:-}" ]] || "${SUDO[@]}" rm -rf "$proof_state"
        "${SUDO[@]}" systemctl daemon-reload >/dev/null 2>&1
    }
    trap cleanup EXIT

    cmp -s packaging/systemd/optid-apply.service mkosi/mkosi.extra/usr/lib/systemd/system/optid-apply.service
    cmp -s packaging/systemd/optid-recover.service mkosi/mkosi.extra/usr/lib/systemd/system/optid-recover.service

    cargo build -p optid --all-features --bins
    "${SUDO[@]}" install -D -m 0755 target/debug/optid /usr/libexec/optid
    "${SUDO[@]}" install -D -m 0755 target/debug/optid-recover /usr/libexec/optid-recover
    printf '.TH OPTID 8\n.SH NAME\noptid - Rush Linux optimization daemon\n' | \
        gzip -c | "${SUDO[@]}" tee /usr/share/man/man8/optid.8.gz >/dev/null
    "${SUDO[@]}" install -D -m 0644 packaging/systemd/optid-apply.service /etc/systemd/system/optid-apply.service
    "${SUDO[@]}" install -D -m 0644 packaging/systemd/optid-recover.service /etc/systemd/system/optid-recover.service
    systemd-analyze verify /etc/systemd/system/optid-recover.service /etc/systemd/system/optid-apply.service

    proof_state="$(mktemp -d /tmp/s5d-production-clear-proof.XXXXXX)"
    chmod 0777 "$proof_state"
    "${SUDO[@]}" /usr/libexec/optid --state-dir "$proof_state" --config config/optid/policy.toml --clear-all-circuits | tee "$OUTPUT_DIR/root-clear.log"
    grep -qF 'optid: cleared 0 S5D circuit record(s)' "$OUTPUT_DIR/root-clear.log"
    state_file="$proof_state/persistent-circuits-v1.json"
    "${SUDO[@]}" test -f "$state_file"
    test "$("${SUDO[@]}" stat -c '%a' "$state_file")" = 600
    "${SUDO[@]}" jq -e '.schema_version == 1 and .global == null and (.records | length) == 0' "$state_file" >/dev/null
    for unexpected in optid.lock status status.json circuits.json decisions.log control-cycles.jsonl; do
        test ! -e "$proof_state/$unexpected"
    done

    set +e
    as_nobody /usr/libexec/optid --state-dir "$proof_state" --config config/optid/policy.toml --clear-all-circuits > "$OUTPUT_DIR/unprivileged-clear.log" 2>&1
    clear_status=$?
    set -e
    test "$clear_status" -ne 0
    grep -qF 'S5D circuit clear requires effective UID 0' "$OUTPUT_DIR/unprivileged-clear.log"

    cargo test -p optid --features experimental-capability-sealing --bin optid-capability-seal-test
    cargo test -p optid --features experimental-capability-sealing --test capability_sealing_cli
    cargo build -p optid --features experimental-capability-sealing --bin optid-capability-seal-test
    "${SUDO[@]}" install -D -m 0755 target/debug/optid-capability-seal-test /usr/local/libexec/optid-capability-seal-test
    systemd-analyze verify packaging/systemd/optid-capability-seal-test.service
    /usr/local/libexec/optid-capability-seal-test --probe

    LANDLOCK_ABI="$(grep -Eio '(landlock[_ ]abi)[[:space:]"=:]+[0-9]+' "$PROOF_LOG" | grep -Eo '[0-9]+' | tail -n1)"
    [[ -n "$LANDLOCK_ABI" ]] || fail "could not parse Landlock ABI"

    "${SUDO[@]}" install -D -m 0644 packaging/systemd/optid-capability-seal-test.service /etc/systemd/system/optid-capability-seal-test.service
    "${SUDO[@]}" touch /run/optid-capability-seal-test-enabled
    "${SUDO[@]}" rm -rf /run/optid-capability-seal-test
    "${SUDO[@]}" systemctl daemon-reload
    "${SUDO[@]}" systemctl reset-failed optid-capability-seal-test.service || true
    proof_started="$(date --iso-8601=seconds)"
    "${SUDO[@]}" systemctl start optid-capability-seal-test.service

    for _ in $(seq 1 30); do
        active_state="$(systemctl show optid-capability-seal-test.service -p ActiveState --value)"
        result="$(systemctl show optid-capability-seal-test.service -p Result --value)"
        [[ "$active_state" == inactive && "$result" == success ]] && break
        [[ "$active_state" == failed ]] && break
        sleep 1
    done

    "${SUDO[@]}" journalctl --sync
    "${SUDO[@]}" journalctl -u optid-capability-seal-test.service --since "$proof_started" --no-pager -o cat | tee "$OUTPUT_DIR/status75.log"
    systemctl show optid-capability-seal-test.service -p ActiveState -p SubState -p Result -p ExecMainCode -p ExecMainStatus -p NRestarts | tee "$OUTPUT_DIR/status75.status"
    test "$(systemctl show optid-capability-seal-test.service -p ActiveState --value)" = inactive
    test "$(systemctl show optid-capability-seal-test.service -p Result --value)" = success
    test "$(systemctl show optid-capability-seal-test.service -p ExecMainStatus --value)" = 0
    test "$(grep -cF 'Scheduled restart job, restart counter is at 1.' "$OUTPUT_DIR/status75.log")" -eq 1
    test "$(grep -cF 'recovery step completed:' "$OUTPUT_DIR/status75.log")" -eq 2
    test "$(grep -cF 'recovery marker verified before capability discovery:' "$OUTPUT_DIR/status75.log")" -eq 2
    test "$(grep -cF 'capability-sealing proof passed (8 checks)' "$OUTPUT_DIR/status75.log")" -eq 2
    test "$(grep -cF 'first sealed cycle complete; requesting topology rebuild with status 75' "$OUTPUT_DIR/status75.log")" -eq 1
    test "$(grep -cF 'fresh sealed process started after recovery; supervisor cycle complete' "$OUTPUT_DIR/status75.log")" -eq 1

    "${SUDO[@]}" mkdir -p /run/systemd/system/optid-capability-seal-test.service.d
    printf '%s\n' '[Service]' 'ExecStart=' "ExecStart=/bin/sh -c 'exit 1'" | \
        "${SUDO[@]}" tee /run/systemd/system/optid-capability-seal-test.service.d/non75.conf >/dev/null
    "${SUDO[@]}" systemctl daemon-reload
    "${SUDO[@]}" systemctl reset-failed optid-capability-seal-test.service || true
    failure_started="$(date --iso-8601=seconds)"
    set +e
    "${SUDO[@]}" systemctl start optid-capability-seal-test.service
    start_status=$?
    set -e
    sleep 3
    "${SUDO[@]}" journalctl --sync
    "${SUDO[@]}" journalctl -u optid-capability-seal-test.service --since "$failure_started" --no-pager -o cat | tee "$OUTPUT_DIR/non75.log"
    systemctl show optid-capability-seal-test.service -p ActiveState -p SubState -p Result -p ExecMainCode -p ExecMainStatus -p NRestarts | tee "$OUTPUT_DIR/non75.status"
    test "$(systemctl show optid-capability-seal-test.service -p Result --value)" = exit-code
    test "$(systemctl show optid-capability-seal-test.service -p ExecMainStatus --value)" = 1
    test "$(systemctl show optid-capability-seal-test.service -p NRestarts --value)" = 0
    ! grep -qF 'Scheduled restart job' "$OUTPUT_DIR/non75.log"
    echo "systemctl_start_status=$start_status"
    echo "live_landlock_and_supervisor_proof=pass"
fi

VERIFIED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
export OUTPUT_DIR HEAD_SHA EXPECTED_HEAD MERGED_COMMIT IMPLEMENTATION_HEAD KERNEL_RELEASE LANDLOCK_ABI VERIFIED_AT FULL_SYSTEM_PROOF
python3 - <<'PY'
import json
import os
import platform
from pathlib import Path

out = Path(os.environ["OUTPUT_DIR"])
os_release = {}
path = Path("/etc/os-release")
if path.exists():
    for line in path.read_text(errors="replace").splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            os_release[key] = value.strip().strip('"')

payload = {
    "schema_version": 1,
    "result": "pass",
    "official_receipt": False,
    "purpose": "local shadow reproduction of S5D post-merge cold proof",
    "verified_at": os.environ["VERIFIED_AT"],
    "head": os.environ["HEAD_SHA"],
    "expected_head": os.environ["EXPECTED_HEAD"],
    "integrated_commit": os.environ["MERGED_COMMIT"],
    "implementation_head": os.environ["IMPLEMENTATION_HEAD"],
    "kernel_release": os.environ["KERNEL_RELEASE"],
    "landlock_abi": int(os.environ["LANDLOCK_ABI"]) if os.environ["LANDLOCK_ABI"] else None,
    "full_system_proof": os.environ["FULL_SYSTEM_PROOF"] == "true",
    "platform": platform.platform(),
    "os_release": os_release,
}
(out / "environment.json").write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
PY

(
    cd "$OUTPUT_DIR"
    find . -maxdepth 1 -type f ! -name manifest.sha256 -printf '%P\0' | sort -z | xargs -0 sha256sum > manifest.sha256
)

echo "shadow_evidence_manifest=$OUTPUT_DIR/manifest.sha256"
echo "result=pass"
echo "NOTE: this is shadow evidence only; it does not authorize an S5D package-state transition."
