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
#   baseline  the distribution's mainstream default power stack, already
#             running in its expected profile. The capture never starts tuned
#             or changes its profile to manufacture a baseline. No optid.
#   optid     tuned stopped by this run when it was active, `optid --apply`
#             supervising instead. tuned is in optid's competing_policy_daemons
#             list, so leaving it up would make optid downgrade its own apply
#             mode and measure nothing. Cleanup restores only state this run
#             changed, including the prior tuned profile.
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
        -h|--help) sed -n '2,49p' "$0"; exit 0 ;;
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
command -v pgrep >/dev/null 2>&1 || { echo "error: pgrep is required for ownership checks" >&2; exit 1; }

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

if pgrep -x optid >/dev/null 2>&1; then
    echo "error: an optid process already exists; this capture will not kill or adopt a process it did not start" >&2
    exit 1
fi

current_tuned_profile() {
    tuned-adm active 2>/dev/null | sed -n 's/^Current active profile: //p' | head -1
}

TUNED_WAS_ACTIVE=0
TUNED_PROFILE_BEFORE=""
if systemctl is-active --quiet tuned 2>/dev/null; then
    TUNED_WAS_ACTIVE=1
    TUNED_PROFILE_BEFORE="$(current_tuned_profile)"
fi

# A baseline is evidence for the distro's mainstream default, not a profile the
# harness manufactured. On the nominated Fedora 44 reference host that means
# tuned must already be active in balanced. Refuse mismatched host state rather
# than silently changing it.
if [[ "$ARM" == "baseline" || "$ARM" == "both" ]]; then
    command -v tuned-adm >/dev/null 2>&1 || { echo "error: tuned-adm is required for the Fedora baseline" >&2; exit 1; }
    if (( TUNED_WAS_ACTIVE == 0 )); then
        echo "error: tuned is not active; refusing to start it just to manufacture the baseline" >&2
        exit 1
    fi
    if [[ "$TUNED_PROFILE_BEFORE" != "balanced" ]]; then
        echo "error: tuned profile is '${TUNED_PROFILE_BEFORE:-unknown}', expected the already-active distro baseline 'balanced'; refusing to change it" >&2
        exit 1
    fi
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
echo "=== tuned_initial_active=$TUNED_WAS_ACTIVE tuned_initial_profile=${TUNED_PROFILE_BEFORE:-unknown}"

# --------------------------------------------------------------- system state
OPTID_PID=""
RUSHBENCH_PID=""
TUNED_STOPPED_BY_RUN=0
RUN_WORK_DIR="/tmp/rushbench-mixed-load-001-capture-$$"

terminate_owned_tree() {
    local pid="$1"
    local child
    while read -r child; do
        [[ -n "$child" ]] && terminate_owned_tree "$child"
    done < <(pgrep -P "$pid" 2>/dev/null || true)
    kill "$pid" 2>/dev/null || true
}

cleanup() {
    local original_status=$?
    local restore_failed=0
    set +e
    echo "[cleanup] restoring capture-owned system state"

    if [[ -n "$RUSHBENCH_PID" ]] && kill -0 "$RUSHBENCH_PID" 2>/dev/null; then
        echo "[cleanup] stopping run-owned rushbench process tree (pid=$RUSHBENCH_PID)"
        terminate_owned_tree "$RUSHBENCH_PID"
        wait "$RUSHBENCH_PID" 2>/dev/null
    fi
    RUSHBENCH_PID=""

    if [[ -n "$OPTID_PID" ]] && kill -0 "$OPTID_PID" 2>/dev/null; then
        echo "[cleanup] stopping run-owned optid (pid=$OPTID_PID); its revert path restores the knobs"
        kill "$OPTID_PID" 2>/dev/null
        wait "$OPTID_PID" 2>/dev/null
    fi
    OPTID_PID=""

    if (( TUNED_STOPPED_BY_RUN == 1 )); then
        echo "[cleanup] restarting tuned because this run stopped it"
        if ! systemctl start tuned; then
            echo "[cleanup] error: failed to restart tuned" >&2
            restore_failed=1
        elif [[ -n "$TUNED_PROFILE_BEFORE" ]]; then
            local current_profile
            current_profile="$(current_tuned_profile)"
            if [[ "$current_profile" != "$TUNED_PROFILE_BEFORE" ]]; then
                echo "[cleanup] restoring tuned profile '$TUNED_PROFILE_BEFORE'"
                if ! tuned-adm profile "$TUNED_PROFILE_BEFORE"; then
                    echo "[cleanup] error: failed to restore tuned profile '$TUNED_PROFILE_BEFORE'" >&2
                    restore_failed=1
                fi
            fi
        fi
    fi

    rm -rf "$RUN_WORK_DIR"
    trap - EXIT
    if (( restore_failed == 1 && original_status == 0 )); then
        exit 1
    fi
    exit "$original_status"
}
trap cleanup EXIT

# rushbench runs in the desktop session so the graphical phases have a display.
# It is launched asynchronously only so cleanup can retain its exact PID and
# terminate that run-owned process tree on errors or interrupts; no name-based
# process killing is used.
run_rushbench() {
    local tag="$1" out="$2"
    local status
    mkdir -p "$out" "$RUN_WORK_DIR"
    chown -R "$DESKTOP_USER" "$out" "$RUN_WORK_DIR"
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
        RUSHBENCH_WORK_DIR="$RUN_WORK_DIR" \
        PATH="$REPO/target/release:$PATH" \
        "$RUSHBENCH" run preset=mixed-load-001 \
            --cycles "$CYCLES" --tag="$tag" --out "$out" \
            "${extra_args[@]}" &
    RUSHBENCH_PID=$!
    if wait "$RUSHBENCH_PID"; then
        status=0
    else
        status=$?
    fi
    RUSHBENCH_PID=""
    return "$status"
}

capture_baseline() {
    echo "--- D3 baseline arm: observed mainstream default, no optid"
    echo "[baseline] tuned was already active; profile: $(tuned-adm active 2>/dev/null || echo unknown)"
    run_rushbench "baseline-fedora44-tuned-balanced-$(hostname)" "$DIR/baseline"
}

capture_optid() {
    echo "--- D4 optid arm: competing tuned owner temporarily stopped, optid --apply supervising"
    if systemctl is-active --quiet tuned 2>/dev/null; then
        systemctl stop tuned
        TUNED_STOPPED_BY_RUN=1
    fi

    # Do not delete /run/optid: it may contain state this capture does not own.
    # Instead require status.json to change after this daemon is launched so a
    # stale file cannot be mistaken for readiness.
    mkdir -p /run/optid
    local status_before=""
    if [[ -e /run/optid/status.json ]]; then
        status_before="$(stat -c '%y:%s:%i' /run/optid/status.json 2>/dev/null || true)"
    fi

    # Mirror the packaged unit ordering: optid-apply.service Requires= and
    # After= optid-recover.service. Transaction records left by an earlier
    # generation make the daemon refuse to touch their targets, so recovery must
    # run before a new daemon. Recovery evidence is consumed through its normal
    # verified path; this wrapper never deletes the shared recovery directory.
    "$REPO/target/release/optid-recover" \
        --recovery-dir /var/lib/optid/recovery \
        --status-file /run/optid/recovery-status.json \
        >"$DIR/optid-recover.log" 2>&1
    echo "[optid] recovery exit=$?"
    "$OPTID" --apply --config "$REPO/config/optid/policy.toml" \
        >"$DIR/optid-daemon.log" 2>&1 &
    OPTID_PID=$!
    for _ in $(seq 1 30); do
        if [[ -r /run/optid/status.json ]]; then
            local status_now
            status_now="$(stat -c '%y:%s:%i' /run/optid/status.json 2>/dev/null || true)"
            if [[ -n "$status_now" && "$status_now" != "$status_before" ]]; then
                break
            fi
        fi
        sleep 1
    done
    if [[ ! -r /run/optid/status.json ]]; then
        echo "error: optid wrote no status.json in 30 s; see $DIR/optid-daemon.log" >&2
        exit 1
    fi
    local status_after
    status_after="$(stat -c '%y:%s:%i' /run/optid/status.json 2>/dev/null || true)"
    if [[ -z "$status_after" || "$status_after" == "$status_before" ]]; then
        echo "error: /run/optid/status.json did not become fresh after optid launch; refusing stale readiness" >&2
        exit 1
    fi

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