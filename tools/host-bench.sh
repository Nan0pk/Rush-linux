#!/usr/bin/env bash
# host-bench.sh — run v0.6 Phase D host benchmarks STRAIGHT FROM YOUR EXISTING OS.
#
# No USB. No QEMU. No mkosi image. No reboot. This is the shortest path to
# the two numbers v0.6 needs: responsiveness (cyclictest + PSI) and power
# (RAPL or battery) under "mainstream baseline" vs "optid --apply".
#
# Designed for your existing Fedora (or any systemd-based distro). Run from
# a TTY (Ctrl+Alt+F3) after closing Chrome, Discord, IDEs, etc. — the script
# records ambient load so residual noise is honest, but quit browsers anyway.
#
# Usage:
#   sudo bash tools/host-bench.sh --tag victus-2026-07-16
#   sudo bash tools/host-bench.sh --tag victus-2026-07-16 --skip-baseline
#
# Output lands under benchmarks/host-runs/<tag>/{baseline,optid}/ in the
# shape release/evidence/host-bench/_TEMPLATE/ expects. Copy the directory
# into release/evidence/host-bench/<date>-<hostname>/, write VERDICT.md, commit.

set -euo pipefail

TAG=""
SKIP_BASELINE=0
N=5
PHASE_SEC=30
WARMUP_SEC=5
STATE_DIR=/run/optid

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tag) TAG="$2"; shift 2 ;;
        --skip-baseline) SKIP_BASELINE=1; shift ;;
        --n) N="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,28p' "$0"
            exit 0
            ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

if [[ -z "$TAG" ]]; then
    echo "error: --tag is required (e.g. --tag victus-2026-07-16)" >&2
    exit 2
fi

if [[ $EUID -ne 0 ]]; then
    echo "error: run as root (sudo)" >&2
    exit 2
fi

for tool in cargo cyclictest bc; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: required tool '$tool' not found in PATH" >&2
        echo "  cargo:       install via rustup (https://rustup.rs)"
        echo "  cyclictest:  'sudo dnf install realtime-tests' (Fedora)"
        echo "               'sudo apt install rt-tests' (Debian/Ubuntu)"
        echo "  bc:          'sudo dnf install bc' / 'sudo apt install bc'"
        exit 2
    fi
done

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

OUT="$REPO_ROOT/benchmarks/host-runs/$TAG"
rm -rf "$OUT"
mkdir -p "$OUT/baseline" "$OUT/optid"

echo "============================================================"
echo " Rush Linux host-bench (direct-on-host, no USB/QEMU)"
echo " tag        : $TAG"
echo " iterations : $N per leg"
echo " phase sec  : $PHASE_SEC"
echo " output     : $OUT"
echo "============================================================"
echo
echo "[preflight] Quit browsers / IDEs / Discord before continuing."
echo "            The script records ambient CPU so mild background"
echo "            load is honest, but 50% Chrome ruins the numbers."
echo
read -r -p "Ready? (type 'yes' to continue) " CONFIRM
if [[ "$CONFIRM" != "yes" ]]; then
    echo "aborted."
    exit 1
fi

echo "[build] cargo build --release (optid, optctl, rushbench) ..."
cargo build --release -p optid -p optctl -p rushbench 2>&1 | sed 's/^/  /'

OPTID="$REPO_ROOT/target/release/optid"
OPTCTL="$REPO_ROOT/target/release/optctl"

# Sanity check --version flag works (was the cause of the 2026-06-10 defect)
if ! "$OPTID" --version >/dev/null 2>&1; then
    echo "error: optid --version failed (this was the Dragnet-001 meta.txt defect)" >&2
    exit 1
fi
echo "[build] optid --version: $($OPTID --version)"

# Stop competing daemons; restart on exit.
STOPPED=()
stop_if_active() {
    local svc="$1"
    if systemctl is-active --quiet "$svc" 2>/dev/null; then
        echo "[daemons] stopping $svc (will restart on exit)"
        systemctl stop "$svc"
        STOPPED+=("$svc")
    fi
}
cleanup() {
    set +e
    echo
    echo "[cleanup] restoring system state"
    if [[ -n "${OPTID_PID:-}" ]] && kill -0 "$OPTID_PID" 2>/dev/null; then
        echo "[cleanup] stopping optid (pid=$OPTID_PID, revert journal restores knobs)"
        kill "$OPTID_PID" 2>/dev/null || true
        sleep 2
    fi
    pkill -x optid 2>/dev/null || true
    for svc in "${STOPPED[@]:-}"; do
        echo "[cleanup] restarting $svc"
        systemctl start "$svc" 2>/dev/null || true
    done
}
trap cleanup EXIT

stop_if_active tuned
stop_if_active power-profiles-daemon
pkill -x optid 2>/dev/null || true
sleep 1
rm -rf "$STATE_DIR"
mkdir -p "$STATE_DIR"

# ------------------------------------------------------------- helpers
HOSTNAME_VAL="$(hostname)"
KERNEL="$(uname -r)"
CPU="$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs)"
DMI="$(cat /sys/class/dmi/id/board_name 2>/dev/null || echo unknown)"
BATTERY_UWH="$(cat /sys/class/power_supply/BAT0/energy_full_design 2>/dev/null || echo 0)"
GIT_SHA="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
OPTID_VERSION="$($OPTID --version 2>/dev/null | tr -d '\n')"

detect_ac_path() {
    for f in /sys/class/power_supply/AC/online \
             /sys/class/power_supply/ACAD/online \
             /sys/class/power_supply/ADP*/online; do
        if [[ -r "$f" ]]; then echo "$f"; return; fi
    done
}
detect_bat_path() {
    for f in /sys/class/power_supply/BAT*/energy_now; do
        if [[ -r "$f" ]]; then echo "$f"; return; fi
    done
}
AC_PATH="$(detect_ac_path || echo /dev/null)"
BAT_PATH="$(detect_bat_path || echo /dev/null)"

write_meta() {
    local dest="$1" leg="$2"
    local ac_online batt_pct
    ac_online="$(cat "$AC_PATH" 2>/dev/null || echo unknown)"
    batt_pct="$(cat "${BAT_PATH%/energy_now}/capacity" 2>/dev/null || echo unknown)"
    cat > "$dest/meta.txt" <<EOF
date=$(date -u +%Y-%m-%dT%H:%M:%SZ)
host=$HOSTNAME_VAL
kernel=$KERNEL
cpu=$CPU
ncpu=$(nproc)
dmi_board=$DMI
battery_design_uwh=$BATTERY_UWH
git_commit=$GIT_SHA
optid_version=$OPTID_VERSION
leg=$leg
ambient_loadavg=$(cat /proc/loadavg | awk '{print $1, $2, $3}')
ac_online=$ac_online
batt_pct=$batt_pct
EOF
}

set_baseline_governor() {
    echo "[baseline] resetting to distro-default CPU governor"
    if [[ -f /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference ]]; then
        for c in /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference; do
            echo balance_performance > "$c" 2>/dev/null || true
        done
        if [[ -f /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ]]; then
            for c in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
                echo powersave > "$c" 2>/dev/null || true
            done
        fi
    elif [[ -f /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ]]; then
        for c in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
            echo schedutil > "$c" 2>/dev/null || true
        done
    fi
}

# Read current energy counter in joules.
read_energy_joules() {
    # Intel RAPL package domain
    if [[ -r /sys/class/powercap/intel-rapl:0/energy_uj ]]; then
        local uj; uj=$(cat /sys/class/powercap/intel-rapl:0/energy_uj)
        echo "scale=6; $uj / 1000000" | bc -l
        return
    fi
    # AMD energy
    for f in /sys/class/hwmon/hwmon*/energy1_input; do
        [[ -r "$f" ]] || continue
        local name; name=$(cat "$(dirname "$f")/name" 2>/dev/null || echo "")
        if [[ "$name" == "amd_energy" || "$name" == "rapl" ]]; then
            local uj; uj=$(cat "$f")
            echo "scale=6; $uj / 1000000" | bc -l
            return
        fi
    done
    # Battery
    if [[ -r "$BAT_PATH" ]]; then
        local uwh; uwh=$(cat "$BAT_PATH")
        # µWh × 3.6 = µJ; /1e6 = J
        echo "scale=6; $uwh * 3.6 / 1000000" | bc -l
        return
    fi
    echo "0"
}

# Read /proc/pressure/<name> avg10 value (e.g. "avg10=0.02 avg60=..." -> "0.02")
read_psi_avg10() {
    local name="$1"
    awk '/^avg10=/ { sub("avg10=", "", $1); print $1 }' "/proc/pressure/$name"
}

median_of() {
    # args are values on stdin, one per line; prints median
    sort -g | awk '
        { a[NR]=$1 }
        END {
            if (NR % 2 == 1) print a[(NR+1)/2]
            else print (a[NR/2] + a[NR/2+1])/2
        }'
}

# run_leg <name> <dir> <optid_running:0|1> — runs in a subshell so the
# stdout/stderr redirect to the transcript is contained.
run_leg() {
    local name="$1" dir="$2" optid_running="$3"
    local TRANSCRIPT="$dir/transcript.log"
    local CSV="$dir/results.csv"

    (
        echo "================ LEG: $name ================"
        echo "[leg:$name] date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo "[leg:$name] optid_running=$optid_running"
        echo "[leg:$name] cpu=$CPU"
        echo "[leg:$name] kernel=$KERNEL"
        echo "[leg:$name] loadavg=$(cat /proc/loadavg)"
        echo
        if [[ "$optid_running" == "1" ]]; then
            echo "[leg:$name] waiting ${WARMUP_SEC}s for optid to stabilize..."
            sleep "$WARMUP_SEC"
        fi

        echo "phase,scenario,metric,median,iters" > "$CSV"

        # ---- cyclictest (latency-critical responsiveness)
        echo "[leg:$name] ==== cyclictest (max wakeup latency us) x$N, ${PHASE_SEC}s each"
        local cyc_vals=()
        for i in $(seq 1 "$N"); do
            # cyclictest output final line format (typical):
            #   T: 0 (111222) P:99 I:200 C:  149980 Min:1 Act:1 Avg:1 Max:  14
            local line max_us
            line=$(cyclictest -p99 -t1 -i200 -D"$PHASE_SEC" -m -q 2>&1 | tail -n 1)
            max_us=$(echo "$line" | grep -oE 'Max:[[:space:]]*[0-9]+' | awk '{print $2}')
            if [[ -z "$max_us" ]]; then
                echo "  iter $i: FAILED to parse cyclictest output: $line"
                max_us="ERR"
            else
                echo "  iter $i: max_latency_us=$max_us"
            fi
            cyc_vals+=("$max_us")
            sleep 2
        done
        local cyc_med
        cyc_med=$(printf '%s\n' "${cyc_vals[@]}" | median_of)
        echo "latency-critical,cyclictest,cyclictest-max-us,$cyc_med,${cyc_vals[*]}" >> "$CSV"

        # ---- PSI readings
        echo "[leg:$name] ==== PSI (cpu + io avg10) x$N"
        sleep "$WARMUP_SEC"
        local pc_vals=() pi_vals=()
        for i in $(seq 1 "$N"); do
            local pc pi
            pc=$(read_psi_avg10 cpu)
            pi=$(read_psi_avg10 io)
            echo "  iter $i: psi_cpu_avg10=$pc  psi_io_avg10=$pi"
            pc_vals+=("$pc"); pi_vals+=("$pi")
            sleep 3
        done
        local pc_med pi_med
        pc_med=$(printf '%s\n' "${pc_vals[@]}" | median_of)
        pi_med=$(printf '%s\n' "${pi_vals[@]}" | median_of)
        echo "interactive,psi,psi-cpu-avg10,$pc_med,${pc_vals[*]}" >> "$CSV"
        echo "throughput,psi,psi-io-avg10,$pi_med,${pi_vals[*]}" >> "$CSV"

        # ---- idle energy window
        echo "[leg:$name] ==== idle energy window (${PHASE_SEC}s — do not touch the machine)"
        local e_start e_end t_start t_end delta_j elapsed watts
        e_start=$(read_energy_joules)
        t_start=$(date +%s.%N)
        sleep "$PHASE_SEC"
        t_end=$(date +%s.%N)
        e_end=$(read_energy_joules)
        elapsed=$(echo "$t_end - $t_start" | bc -l)
        delta_j=$(echo "$e_end - $e_start" | bc -l)
        watts=$(echo "scale=2; if ($elapsed == 0) 0 else $delta_j / $elapsed" | bc -l)
        echo "  energy_start_j=$e_start  energy_end_j=$e_end"
        echo "  delta_j=$delta_j over ${elapsed}s -> avg_watts=$watts"
        echo "idle,energy,avg_watts,$watts,start_j=$e_start end_j=$e_end elapsed_s=$elapsed" >> "$CSV"

        # ---- optctl snapshot (only meaningful when optid is running)
        echo
        echo "[leg:$name] ==== optctl status --json"
        if "$OPTCTL" --state-dir "$STATE_DIR" status --json 2>&1; then
            :
        else
            echo "(optctl status unavailable — baseline leg is expected to print this)"
        fi
        echo
        echo "[leg:$name] DONE"
    ) 2>&1 | tee "$TRANSCRIPT"
    echo "[leg:$name] wrote $CSV and $TRANSCRIPT"
}

# =========================================================================
# BASELINE LEG
# =========================================================================
if [[ "$SKIP_BASELINE" == "0" ]]; then
    set_baseline_governor
    write_meta "$OUT/baseline" "baseline"
    run_leg "baseline" "$OUT/baseline" 0
else
    echo "[skip] baseline leg skipped (--skip-baseline)"
fi

# =========================================================================
# OPTID LEG
# =========================================================================
pkill -x optid 2>/dev/null || true
sleep 1
rm -rf "$STATE_DIR"; mkdir -p "$STATE_DIR"

# Use the repo's policy.toml so thresholds, shim config, and conflict list
# are the committed ones, not the /usr/lib path (which doesn't exist on a
# non-Rush host). Fallback to the curated baseline if the path is wrong.
POLICY_PATH="$REPO_ROOT/config/optid/policy.toml"
echo "[optid] starting $OPTID --apply --state-dir $STATE_DIR --config $POLICY_PATH"
"$OPTID" --apply --state-dir "$STATE_DIR" --config "$POLICY_PATH" \
    >"$OUT/optid/optid.stdout.log" 2>"$OUT/optid/optid.stderr.log" &
OPTID_PID=$!
echo "[optid] pid=$OPTID_PID — waiting ${WARMUP_SEC}s for initial decisions..."
sleep "$WARMUP_SEC"

if ! kill -0 "$OPTID_PID" 2>/dev/null; then
    echo >&2
    echo "[optid] ERROR: optid exited early. stderr:" >&2
    cat "$OUT/optid/optid.stderr.log" >&2
    exit 1
fi

write_meta "$OUT/optid" "optid"
run_leg "optid" "$OUT/optid" 1

echo
echo "============================================================"
echo " DONE. Results are in:"
echo "   $OUT/baseline/"
echo "   $OUT/optid/"
echo
echo " QUICK COMPARISON:"
echo "   (lower cyclictest max + lower PSI + lower watts = better)"
echo "------------------------------------------------------------"
echo " baseline cyclictest max us : $(awk -F, '/cyclictest/ {print $4}' "$OUT/baseline/results.csv")"
echo " optid    cyclictest max us : $(awk -F, '/cyclictest/ {print $4}' "$OUT/optid/results.csv")"
echo " baseline psi-cpu avg10     : $(awk -F, '/psi-cpu/ {print $4}' "$OUT/baseline/results.csv")"
echo " optid    psi-cpu avg10     : $(awk -F, '/psi-cpu/ {print $4}' "$OUT/optid/results.csv")"
echo " baseline idle watts        : $(awk -F, '/avg_watts/ {print $4}' "$OUT/baseline/results.csv")"
echo " optid    idle watts        : $(awk -F, '/avg_watts/ {print $4}' "$OUT/optid/results.csv")"
echo "------------------------------------------------------------"
echo
echo " NEXT STEPS:"
echo "   1. Inspect $OUT/*/results.csv and $OUT/*/transcript.log."
echo "   2. If numbers look sane, copy the tree into the evidence"
echo "      template shape:"
echo "        cp -r $OUT release/evidence/host-bench/<date>-<hostname>"
echo "      (rename 'baseline'/'optid' if you want to match _TEMPLATE,"
echo "       or just commit the layout as-is with a note.)"
echo "   3. Fill in VERDICT.md in that directory comparing the legs."
echo "   4. git add + commit. That's one (laptop) slot of Phase D."
echo "   5. Run the same thing on AC power, then on battery, so you"
echo "      have data for BOTH v0.6 exit criteria (responsiveness"
echo "      and battery behavior)."
echo "============================================================"
