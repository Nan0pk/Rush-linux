#!/usr/bin/env bash
# phase-d-capture.sh — capture a v0.6 Phase D arm with the mixed-load-001 preset.
#
# This is the D3 (baseline) and D4 (optid) driver defined by
# docs/strategy/mixed-load-workload.md: it runs `rushbench run
# preset=mixed-load-001` and files meta.txt / results.csv / transcript.log /
# *.json into an evidence arm directory in the shape
# release/evidence/host-bench/_TEMPLATE/ expects.
#
# It is NOT tools/host-bench.sh. That script predates the preset and measures
# its own ad-hoc cyclictest/PSI/RAPL cells; this one drives the single named
# workload the v0.6 quantitative criteria are written against.
#
# Two arms, and what differs between them:
#
#   baseline  the distribution's mainstream default power stack, left running
#             (on Fedora 44 that is tuned with its balanced profile — not the
#             workload spec's "PPD balanced", which is not what a Fedora user
#             actually runs). No optid.
#   optid     tuned stopped, `optid --apply` supervising instead. tuned is in
#             optid's competing_policy_daemons list, so leaving it up would make
#             optid downgrade its own apply mode and measure nothing.
#
# Both arms MUST run on battery: Criterion 3 is an on-battery measurement, and
# the only energy counter a non-root process can read on this class of hardware
# is the battery charge counter, which reads zero while the charger holds the
# pack full. The energy source is pinned for both arms so the two transcripts
# cannot silently end up comparing RAPL against the battery.
#
# The graphical phases (firefox, glmark2) need a logged-in session, so rushbench
# is run as the desktop user while optid stays root.
#
# Usage:
#   sudo bash tools/phase-d-capture.sh --arm baseline --dir release/evidence/host-bench/2026-08-22-victus
#   sudo bash tools/phase-d-capture.sh --arm optid    --dir release/evidence/host-bench/2026-08-22-victus
#   sudo bash tools/phase-d-capture.sh --arm both     --dir release/evidence/host-bench/2026-08-22-victus
#
# Options:
#   --cycles N        cycles per arm (default 5; fewer stamps insufficient_n)
#   --scale N         divide every phase window by N — harness validation only,
#                     stamps phase_scale_shortened on every record
#   --user NAME       desktop user for the graphical phases (default: owner of
#                     the active seat, else SUDO_USER)
#   --min-battery N   refuse to start below N% (default 70)
#   --ac-ok           allow an on-AC run (Criterion 2 only; energy unmeasurable)

set -euo pipefail

ARM=""
DIR=""
CYCLES=5
SCALE=1
DESKTOP_USER="${SUDO_USER:-}"
MIN_BATTERY=70
AC_OK=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --arm) ARM="$2"; shift 2 ;;
        --dir) DIR="$2"; shift 2 ;;
        --cycles) CYCLES="$2"; shift 2 ;;
        --scale) SCALE="$2"; shift 2 ;;
        --user) DESKTOP_USER="$2"; shift 2 ;;
        --min-battery) MIN_BATTERY="$2"; shift 2 ;;
        --ac-ok) AC_OK=1; shift ;;
        -h|--help) sed -n '2,46p' "$0"; exit 0 ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

[[ -n "$ARM" ]] || { echo "error: --arm baseline|optid|both is required" >&2; exit 2; }
[[ -n "$DIR" ]] || { echo "error: --dir <evidence arm root> is required" >&2; exit 2; }
[[ "$ARM" =~ ^(baseline|optid|both)$ ]] || { echo "error: --arm must be baseline, optid, or both" >&2; exit 2; }
[[ "$(id -u)" == "0" ]] || { echo "error: must run as root (optid --apply writes to /sys)" >&2; exit 2; }

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

RUSHBENCH="$REPO/target/release/rushbench"
OPTID="$REPO/target/release/optid"
OPTCTL="$REPO/target/release/optctl"
for binary in "$RUSHBENCH" "$OPTID" "$OPTCTL"; do
    [[ -x "$binary" ]] || { echo "error: $binary missing — run: cargo build --release" >&2; exit 1; }
done

# The 2026-06-10 sample was rejected partly because meta.txt captured usage text
# here; refuse to start if the version flag is still not real.
OPTID_VERSION="$("$OPTID" --version 2>/dev/null | tr -d '\n')"
case "${OPTID_VERSION,,}" in
    ""|usage*) echo "error: optid --version returned '$OPTID_VERSION'" >&2; exit 1 ;;
esac

if [[ -z "$DESKTOP_USER" ]]; then
    DESKTOP_USER="$(loginctl list-sessions --no-legend 2>/dev/null | awk '$3 != "" {print $3; exit}')"
fi
[[ -n "$DESKTOP_USER" ]] || { echo "error: no desktop user found; pass --user NAME" >&2; exit 2; }
id "$DESKTOP_USER" >/dev/null || exit 2
USER_UID="$(id -u "$DESKTOP_USER")"

# ------------------------------------------------------------------ preflight
missing=()
for tool in firefox ninja glmark2 mangohud xterm; do
    command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
if (( ${#missing[@]} )); then
    echo "note: absent phase drivers will record unsupported_here: ${missing[*]}"
    echo "      install with: dnf install -y ${missing[*]}"
fi

ac_online=0
for f in /sys/class/power_supply/*/online; do
    [[ -r "$f" ]] || continue
    kind="$(cat "$(dirname "$f")/type" 2>/dev/null || echo)"
    case "$kind" in
        Mains|USB|USB_C|USB_PD) [[ "$(cat "$f")" == "1" ]] && ac_online=1 ;;
    esac
done
battery_pct="$(cat /sys/class/power_supply/BAT*/capacity 2>/dev/null | head -1 || echo 0)"

if (( ac_online == 1 )) && (( AC_OK == 0 )); then
    cat >&2 <<MSG
error: the charger is attached.

Criterion 3 is an on-battery measurement, and the battery charge counter reads
zero while the charger holds the pack full, so an on-AC run produces a
real-looking 0 W for every energy metric. Unplug and re-run, or pass --ac-ok to
capture a Criterion 2-only arm with the energy metrics recorded unsupported.
MSG
    exit 1
fi
if (( ac_online == 0 )) && (( battery_pct < MIN_BATTERY )); then
    echo "error: battery at ${battery_pct}% is below --min-battery ${MIN_BATTERY}%" >&2
    exit 1
fi

# Seed the arm directory from the evidence template on first use so VERDICT.md
# and the README's required-file shape are present from the start.
if [[ ! -d "$DIR" ]]; then
    cp -r "$REPO/release/evidence/host-bench/_TEMPLATE" "$DIR"
    # The template ships placeholder arm files; a real capture writes its own.
    rm -f "$DIR"/baseline/* "$DIR"/optid/* "$DIR/README.md"
    echo "note: seeded $DIR from the evidence template; VERDICT.md still needs filling"
fi
mkdir -p "$DIR"
LOG="$DIR/capture.log"
exec > >(tee -a "$LOG") 2>&1
echo "=== phase-d-capture $(date -u +%Y-%m-%dT%H:%M:%SZ) arm=$ARM cycles=$CYCLES scale=$SCALE"
echo "=== optid_version=$OPTID_VERSION desktop_user=$DESKTOP_USER battery=${battery_pct}%"

# --------------------------------------------------------------- system state
STOPPED=()
OPTID_PID=""
cleanup() {
    set +e
    echo "[cleanup] restoring system state"
    if [[ -n "$OPTID_PID" ]] && kill -0 "$OPTID_PID" 2>/dev/null; then
        echo "[cleanup] stopping optid (pid=$OPTID_PID); its revert path restores the knobs"
        kill "$OPTID_PID"
        sleep 3
    fi
    pkill -x optid 2>/dev/null
    pkill -u "$USER_UID" -f rushbench-mixed-load-001 2>/dev/null
    for svc in "${STOPPED[@]:-}"; do
        echo "[cleanup] restarting $svc"
        systemctl start "$svc" 2>/dev/null
    done
}
trap cleanup EXIT

# rushbench runs in the desktop session so the graphical phases have a display.
run_rushbench() {
    local tag="$1" out="$2"
    mkdir -p "$out"
    chown -R "$DESKTOP_USER" "$out"
    local extra_args=()
    if (( AC_OK )); then
        extra_args+=(--ac-ok)
    fi
    runuser -u "$DESKTOP_USER" -- env \
        XDG_RUNTIME_DIR="/run/user/$USER_UID" \
        WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}" \
        DISPLAY="${DISPLAY:-:0}" \
        RUSHBENCH_ENERGY_SOURCE=battery \
        RUSHBENCH_PHASE_SCALE="$SCALE" \
        RUSHBENCH_OPTID_BIN="$OPTID" \
        RUSHBENCH_WORK_DIR="/tmp/rushbench-mixed-load-001" \
        PATH="$REPO/target/release:$PATH" \
        "$RUSHBENCH" run preset=mixed-load-001 \
            --cycles "$CYCLES" --tag="$tag" --out "$out" \
            "${extra_args[@]}"
}

capture_baseline() {
    echo "--- D3 baseline arm: mainstream defaults, no optid"
    pkill -x optid 2>/dev/null || true
    if ! systemctl is-active --quiet tuned 2>/dev/null; then
        echo "[baseline] tuned is not running; starting it so the baseline is the distro default"
        systemctl start tuned || true
    fi
    tuned-adm profile balanced 2>/dev/null || echo "[baseline] tuned-adm profile balanced failed (recorded as-is)"
    echo "[baseline] tuned profile: $(tuned-adm active 2>/dev/null || echo unknown)"
    run_rushbench "baseline-fedora44-tuned-balanced-$(hostname)" "$DIR/baseline"
}

capture_optid() {
    echo "--- D4 optid arm: tuned stopped, optid --apply supervising"
    if systemctl is-active --quiet tuned 2>/dev/null; then
        systemctl stop tuned
        STOPPED+=(tuned)
    fi
    pkill -x optid 2>/dev/null || true
    sleep 1
    rm -rf /run/optid
    mkdir -p /run/optid
    chmod 755 /run/optid
    "$OPTID" --apply --config "$REPO/config/optid/policy.toml" \
        >"$DIR/optid-daemon.log" 2>&1 &
    OPTID_PID=$!
    for _ in $(seq 1 30); do
        [[ -r /run/optid/status.json ]] && break
        sleep 1
    done
    if [[ ! -r /run/optid/status.json ]]; then
        echo "error: optid wrote no status.json in 30 s; see $DIR/optid-daemon.log" >&2
        exit 1
    fi
    chmod -R a+rX /run/optid
    echo "[optid] apply_armed line: $(grep -m1 apply_armed /run/optid/status 2>/dev/null || echo unavailable)"
    run_rushbench "optid-${OPTID_VERSION// /-}-$(hostname)" "$DIR/optid"
    # The allowlist denials are Criterion 1's evidence: unsupported/unverified
    # knobs must be skipped with a stated reason.
    for artifact in status status.json decisions.log actions.log audit.jsonl; do
        [[ -r "/run/optid/$artifact" ]] && cp "/run/optid/$artifact" "$DIR/optid/optid-$artifact"
    done
    kill "$OPTID_PID" 2>/dev/null || true
    wait "$OPTID_PID" 2>/dev/null || true
    OPTID_PID=""
}

case "$ARM" in
    baseline) capture_baseline ;;
    optid) capture_optid ;;
    both) capture_baseline; capture_optid ;;
esac

echo "=== done $(date -u +%Y-%m-%dT%H:%M:%SZ); artifacts under $DIR"
echo "=== next: rushbench report, then fill $DIR/VERDICT.md"
