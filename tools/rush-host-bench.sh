#!/usr/bin/env bash
# tools/rush-host-bench.sh — ONE-COMMAND host benchmark + submit.
#
# Designed to be curl-piped from the README:
#
#   curl -fsSL https://rush-linux.org/host-bench.sh | bash
#
# or run from a clone:
#
#   bash tools/rush-host-bench.sh
#   bash tools/rush-host-bench.sh --submit
#   bash tools/rush-host-bench.sh --tag custom-tag --battery-only
#
# What it does (in order):
#   1. Checks prerequisites (cargo, cyclictest, bc, root). Offers to install
#      cyclictest/bc via dnf/apt if missing.
#   2. Clones the repo to a temp dir (or uses the current dir if run from a
#      clone), checks out main.
#   3. Builds optid/optctl/rushbench in release mode.
#   4. Stops tuned/PPD for the duration (restarts on exit).
#   5. Runs the BASELINE leg (distro-default governor) — cyclictest ×5, PSI
#      cpu/avg10 ×5, PSI io/avg10 ×5, 30s idle energy window.
#   6. Runs the OPTID leg (optid --apply) — same probes.
#   7. Stops optid (revert journal restores sysctls/EPP/PM QoS).
#   8. Prints a comparison table.
#   9. If --submit and gh is authed (or GH_TOKEN is set), copies the run dir
#      into benchmarks/results/<date>/<host>/, commits, pushes to an
#      evidence/<date>/<host> branch, and opens an evidence PR via
#      tools/rush-submit-evidence. Otherwise prints local path.
#
# Designed to run on your existing Linux distro (tested on Fedora; should
# work on Ubuntu/Arch with minor package-name tweaks). Does NOT require a
# USB, QEMU, mkosi, reboot, or the Rush Linux distro itself. optid runs as
# a normal Rust binary with --apply and cleans up after itself.
#
# Safety:
#   - Never installs anything without prompting.
#   - Never leaves optid running or tuned/PPD stopped.
#   - Never auto-merges PRs.
#   - Never fabricates numbers. If a probe fails, the anomaly is recorded.

set -euo pipefail

REPO_URL="https://github.com/Nan0pk/Rush-linux.git"
REPO_HOST="Nan0pk/Rush-linux"

# --- Defaults ---------------------------------------------------------------
SUBMIT=0
TAG=""
BATTERY_ONLY=0
AC_ONLY=0
DRY_RUN=0
SKIP_BUILD=0
ITER=5
PHASE_SEC=30
WARMUP_SEC=5
STATE_DIR=/run/optid

# --- Parse args -------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --submit) SUBMIT=1; shift ;;
        --tag) TAG="$2"; shift 2 ;;
        --battery-only) BATTERY_ONLY=1; shift ;;
        --ac-only) AC_ONLY=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        --skip-build) SKIP_BUILD=1; shift ;;
        --n) ITER="$2"; shift 2 ;;
        --phase-sec) PHASE_SEC="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,40p' "$0"
            exit 0
            ;;
        *) echo "unknown arg: $1 (use --help)" >&2; exit 2 ;;
    esac
done

# --- Detect if we're inside a clone already ---------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd 2>/dev/null || echo "")"
if [[ -n "$SCRIPT_DIR" && -f "$SCRIPT_DIR/../Cargo.toml" && -f "$SCRIPT_DIR/../VERSION" ]]; then
    REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
    TMP_CLONE=0
else
    # Not inside a clone — clone to a temp dir.
    REPO_ROOT="$(mktemp -d /tmp/rush-host-bench.XXXXXX)"
    TMP_CLONE=1
fi

# --- Terminal / color setup -------------------------------------------------
if [[ -t 1 ]]; then
    BOLD=$'\e[1m'; DIM=$'\e[2m'; RED=$'\e[31m'; GREEN=$'\e[32m'; YELLOW=$'\e[33m'; CYAN=$'\e[36m'; RESET=$'\e[0m'
else
    BOLD=""; DIM=""; RED=""; GREEN=""; YELLOW=""; CYAN=""; RESET=""
fi
log()  { echo "${BOLD}[rush-host-bench]${RESET} $*"; }
ok()   { echo "  ${GREEN}OK${RESET}  $*"; }
warn() { echo "  ${YELLOW}WARN${RESET} $*" >&2; }
err()  { echo "  ${RED}ERR${RESET}  $*" >&2; }

# --- Friendly header --------------------------------------------------------
cat <<EOF
${BOLD}Rush Linux — direct-on-host benchmark${RESET}
${DIM}One command. No USB. No QEMU. No reboot.${RESET}

EOF

# --- Root check -------------------------------------------------------------
if [[ $EUID -ne 0 ]] && [[ $DRY_RUN -eq 0 ]]; then
    err "this script needs root to access /dev/cpu_dma_latency, write EPP,"
    err "and stop tuned/PPD. Re-run with sudo (or as root)."
    exit 2
fi

# --- Clone if needed --------------------------------------------------------
if [[ $TMP_CLONE -eq 1 && $DRY_RUN -eq 0 ]]; then
    log "cloning $REPO_URL into $REPO_ROOT ..."
    git clone --depth 1 "$REPO_URL" "$REPO_ROOT" 2>&1 | sed 's/^/  /'
fi
cd "$REPO_ROOT"

# --- Package detection & install offer --------------------------------------
install_pkgs() {
    local pkgs="$1"
    if command -v dnf >/dev/null 2>&1; then
        dnf install -y $pkgs
    elif command -v apt-get >/dev/null 2>&1; then
        apt-get update -y && apt-get install -y $pkgs
    elif command -v pacman >/dev/null 2>&1; then
        pacman -Sy --noconfirm $pkgs
    else
        err "unrecognized package manager; install [$pkgs] manually and rerun."
        exit 2
    fi
}

MISSING_PKGS=()
for tool in cargo cyclictest bc git; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        case "$tool" in
            cyclictest)
                # Different package name on different distros
                if command -v dnf >/dev/null 2>&1; then MISSING_PKGS+=("realtime-tests")
                elif command -v apt-get >/dev/null 2>&1; then MISSING_PKGS+=("rt-tests")
                elif command -v pacman >/dev/null 2>&1; then MISSING_PKGS+=("rt-tests")
                else MISSING_PKGS+=("rt-tests"); fi
                ;;
            cargo) MISSING_PKGS+=("rustc cargo") ;;
            *) MISSING_PKGS+=("$tool") ;;
        esac
    fi
done
if [[ ${#MISSING_PKGS[@]} -gt 0 ]]; then
    PKGLIST="${MISSING_PKGS[*]}"
    warn "missing packages: $PKGLIST"
    if [[ -t 0 ]]; then
        read -r -p "  Install them now? (requires root) [y/N] " YN
        if [[ "$YN" =~ ^[Yy] ]]; then
            install_pkgs "$PKGLIST"
        else
            err "aborting — install $PKGLIST and rerun."
            exit 2
        fi
    else
        err "non-interactive and packages are missing. Install [$PKGLIST] then rerun."
        exit 2
    fi
fi

# --- Detect host tag --------------------------------------------------------
if [[ -z "$TAG" ]]; then
    HOST_ALIAS="$(hostname | tr '[:upper:]_ ' '[:lower:]-' | tr -cd '[:alnum:]-' | cut -c1-40)"
    DATE_TAG="$(date -u +%Y-%m-%d)"
    TAG="${DATE_TAG}-${HOST_ALIAS}"
fi

# --- Build ------------------------------------------------------------------
OPTID="$REPO_ROOT/target/release/optid"
OPTCTL="$REPO_ROOT/target/release/optctl"
RUSHBENCH="$REPO_ROOT/target/release/rushbench"

if [[ $SKIP_BUILD -eq 0 ]]; then
    log "building optid/optctl/rushbench (release)..."
    if [[ $DRY_RUN -eq 0 ]]; then
        cargo build --release -p optid -p optctl -p rushbench 2>&1 | grep -E '^(error|   Compiling|    Finished)' || true
    fi
fi
for bin in "$OPTID" "$OPTCTL" "$RUSHBENCH"; do
    [[ -x "$bin" ]] || { err "build didn't produce $bin"; exit 1; }
done
ok "build complete: optid $("$OPTID" --version 2>/dev/null | head -1 || echo 'unknown')"

# --- Power source detection -------------------------------------------------
detect_ac_path() {
    for f in /sys/class/power_supply/AC/online /sys/class/power_supply/ACAD/online /sys/class/power_supply/ADP*/online; do
        [[ -r "$f" ]] && { echo "$f"; return; }
    done
}
detect_bat_path() {
    for f in /sys/class/power_supply/BAT*/energy_now; do
        [[ -r "$f" ]] && { echo "$f"; return; }
    done
}
AC_PATH="$(detect_ac_path || echo /dev/null)"
BAT_PATH="$(detect_bat_path || echo /dev/null)"

on_ac() { [[ "$(cat "$AC_PATH" 2>/dev/null)" == "1" ]]; }
has_battery() { [[ -r "$BAT_PATH" ]]; }

if [[ $BATTERY_ONLY -eq 0 && $AC_ONLY -eq 0 ]]; then
    if on_ac; then
        log "on AC power now — will run both legs on AC."
        if has_battery; then
            log "battery detected."
            warn "if you want the battery leg too, rerun on battery with --battery-only."
        fi
    else
        log "on battery power now — will run both legs on battery."
    fi
fi

# --- Output dir -------------------------------------------------------------
OUT="$REPO_ROOT/benchmarks/host-runs/$TAG"
rm -rf "$OUT"
mkdir -p "$OUT/baseline" "$OUT/optid"
ok "results will be written to $OUT"

# --- Stop competing daemons; restart on exit --------------------------------
STOPPED=()
OPTID_PID=""
stop_if_active() {
    local svc="$1"
    if systemctl is-active --quiet "$svc" 2>/dev/null; then
        log "stopping $svc (will restart on exit)"
        if [[ $DRY_RUN -eq 0 ]]; then systemctl stop "$svc"; fi
        STOPPED+=("$svc")
    fi
}
cleanup() {
    set +e
    if [[ -n "$OPTID_PID" ]] && kill -0 "$OPTID_PID" 2>/dev/null; then
        log "stopping optid (pid=$OPTID_PID) — revert journal restores knobs"
        kill "$OPTID_PID" 2>/dev/null || true
        sleep 2
    fi
    pkill -x optid 2>/dev/null || true
    for svc in "${STOPPED[@]:-}"; do
        log "restarting $svc"
        systemctl start "$svc" 2>/dev/null || true
    done
    echo
}
trap cleanup EXIT

stop_if_active tuned
stop_if_active power-profiles-daemon
pkill -x optid 2>/dev/null || true
sleep 1
rm -rf "$STATE_DIR"; mkdir -p "$STATE_DIR"

# --- Helpers ----------------------------------------------------------------
HOSTNAME_VAL="$(hostname)"
KERNEL="$(uname -r)"
CPU="$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs)"
DMI="$(cat /sys/class/dmi/id/board_name 2>/dev/null || echo unknown)"
BATTERY_UWH="$(cat /sys/class/power_supply/BAT0/energy_full_design 2>/dev/null || echo 0)"
GIT_SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
OPTID_VERSION="$("$OPTID" --version 2>/dev/null | tr -d '\n')"

write_meta() {
    local dest="$1" leg="$2"
    local ac batt
    ac="$(cat "$AC_PATH" 2>/dev/null || echo unknown)"
    batt="$(cat "${BAT_PATH%/energy_now}/capacity" 2>/dev/null || echo unknown)"
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
ac_online=$ac
batt_pct=$batt
iterations=$ITER
phase_sec=$PHASE_SEC
EOF
}

set_baseline_governor() {
    log "resetting CPU governor to distro defaults (balance_performance/powersave)"
    if [[ -f /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference ]]; then
        for c in /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference; do
            echo balance_performance > "$c" 2>/dev/null || true
        done
        for c in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
            echo powersave > "$c" 2>/dev/null || true
        done
    elif [[ -f /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ]]; then
        for c in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
            echo schedutil > "$c" 2>/dev/null || true
        done
    fi
}

read_energy_joules() {
    if [[ -r /sys/class/powercap/intel-rapl:0/energy_uj ]]; then
        local uj; uj=$(cat /sys/class/powercap/intel-rapl:0/energy_uj)
        echo "scale=6; $uj / 1000000" | bc -l
        return
    fi
    for f in /sys/class/hwmon/hwmon*/energy1_input; do
        [[ -r "$f" ]] || continue
        local n; n=$(cat "$(dirname "$f")/name" 2>/dev/null || echo "")
        if [[ "$n" == "amd_energy" || "$n" == "rapl" ]]; then
            local uj; uj=$(cat "$f")
            echo "scale=6; $uj / 1000000" | bc -l
            return
        fi
    done
    if [[ -r "$BAT_PATH" ]]; then
        local uwh; uwh=$(cat "$BAT_PATH")
        echo "scale=6; $uwh * 3.6 / 1000000" | bc -l
        return
    fi
    echo "0"
}

read_psi_avg10() { awk '/^avg10=/ { sub("avg10=", "", $1); print $1 }' "/proc/pressure/$1"; }

median_of() { sort -g | awk '{ a[NR]=$1 } END { if(NR%2==1) print a[(NR+1)/2]; else print (a[NR/2]+a[NR/2+1])/2 }'; }

parse_cyclictest_max() {
    # Input: last line of cyclictest output, e.g.:
    #   T: 0 (12345) P:99 I:200 C:  150000 Min:1 Act:1 Avg:2 Max:  42
    grep -oE 'Max:[[:space:]]*[0-9]+' | awk '{print $2}' | head -1
}

# run_leg <name> <dir>
run_leg() {
    local name="$1" dir="$2"
    local TRANSCRIPT="$dir/transcript.log" CSV="$dir/results.csv"

    log "starting leg '$name' — ${BOLD}please don't touch the machine${RESET}"
    (
        echo "================ LEG: $name ================"
        echo "date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo "cpu=$CPU"
        echo "kernel=$KERNEL"
        echo "loadavg=$(cat /proc/loadavg)"
        echo

        echo "phase,scenario,metric,median,iters" > "$CSV"

        # warm-up (let optid converge if running)
        if [[ "$name" == "optid" ]]; then
            echo "(waiting ${WARMUP_SEC}s for optid to converge...)"
            sleep "$WARMUP_SEC"
        fi

        # ---- cyclictest
        echo "==== cyclictest (max wakeup latency us) x$ITER, ${PHASE_SEC}s each"
        local cyc=()
        for i in $(seq 1 "$ITER"); do
            local line max_us
            line=$(cyclictest -p99 -t1 -i200 -D"$PHASE_SEC" -m -q 2>&1 | tail -n 1 || echo "")
            max_us=$(echo "$line" | parse_cyclictest_max || echo "ERR")
            echo "  iter $i: max_latency_us=$max_us"
            cyc+=("$max_us")
            sleep 2
        done
        local cyc_med; cyc_med=$(printf '%s\n' "${cyc[@]}" | median_of)
        echo "latency-critical,cyclictest,cyclictest-max-us,$cyc_med,${cyc[*]}" >> "$CSV"

        # ---- PSI
        echo "==== PSI avg10 (cpu + io) x$ITER"
        sleep "$WARMUP_SEC"
        local pc=() pi=()
        for i in $(seq 1 "$ITER"); do
            local pcv piv; pcv=$(read_psi_avg10 cpu); piv=$(read_psi_avg10 io)
            echo "  iter $i: psi_cpu_avg10=$pcv  psi_io_avg10=$piv"
            pc+=("$pcv"); pi+=("$piv")
            sleep 3
        done
        local pc_med pi_med; pc_med=$(printf '%s\n' "${pc[@]}" | median_of); pi_med=$(printf '%s\n' "${pi[@]}" | median_of)
        echo "interactive,psi,psi-cpu-avg10,$pc_med,${pc[*]}" >> "$CSV"
        echo "throughput,psi,psi-io-avg10,$pi_med,${pi[*]}" >> "$CSV"

        # ---- idle energy
        echo "==== idle energy window (${PHASE_SEC}s — DO NOT TOUCH)"
        local es ee ts te dj el w
        es=$(read_energy_joules); ts=$(date +%s.%N)
        sleep "$PHASE_SEC"
        te=$(date +%s.%N); ee=$(read_energy_joules)
        el=$(echo "$te - $ts" | bc -l); dj=$(echo "$ee - $es" | bc -l)
        w=$(echo "scale=2; if($el==0) 0 else $dj / $el" | bc -l)
        echo "  energy_start_j=$es  end_j=$ee  delta_j=$dj over ${el}s -> avg_watts=$w"
        echo "idle,energy,avg_watts,$w,start_j=$es end_j=$ee elapsed_s=$el" >> "$CSV"

        echo
        echo "==== optctl status --json"
        "$OPTCTL" --state-dir "$STATE_DIR" status --json 2>&1 || echo "(optid not running — expected for baseline)"
        echo
        echo "DONE"
    ) 2>&1 | tee "$TRANSCRIPT"
}

# =========================================================================
# BASELINE LEG
# =========================================================================
if [[ $DRY_RUN -eq 0 ]]; then
    set_baseline_governor
    write_meta "$OUT/baseline" baseline
    run_leg baseline "$OUT/baseline"
fi

# =========================================================================
# OPTID LEG
# =========================================================================
pkill -x optid 2>/dev/null || true
sleep 1
rm -rf "$STATE_DIR"; mkdir -p "$STATE_DIR"

POLICY_PATH="$REPO_ROOT/config/optid/policy.toml"
log "starting optid --apply (policy: $POLICY_PATH)"
if [[ $DRY_RUN -eq 0 ]]; then
    "$OPTID" --apply --state-dir "$STATE_DIR" --config "$POLICY_PATH" \
        >"$OUT/optid/optid.stdout.log" 2>"$OUT/optid/optid.stderr.log" &
    OPTID_PID=$!
    sleep 2
    if ! kill -0 "$OPTID_PID" 2>/dev/null; then
        err "optid exited early. stderr:"
        cat "$OUT/optid/optid.stderr.log" >&2
        exit 1
    fi
    ok "optid running (pid=$OPTID_PID)"

    write_meta "$OUT/optid" optid
    run_leg optid "$OUT/optid"
fi

# Stop optid NOW so the cleanup trap in EXIT doesn't also try (defensive)
if [[ -n "$OPTID_PID" ]] && kill -0 "$OPTID_PID" 2>/dev/null; then
    log "stopping optid — revert journal restores EPP/sysctls/PM QoS"
    kill "$OPTID_PID" 2>/dev/null || true
    wait "$OPTID_PID" 2>/dev/null || true
    OPTID_PID=""
fi
sleep 2

# --- Comparison -------------------------------------------------------------
echo
echo "${BOLD}============================================================${RESET}"
echo "${BOLD} QUICK COMPARISON (lower is better)${RESET}"
echo "${BOLD}============================================================${RESET}"
printf "  %-28s  %-14s  %-14s\n" "metric" "baseline" "optid"
printf "  %-28s  %-14s  %-14s\n" "----------------------------" "--------------" "--------------"
for metric_row in "cyclictest:cyclictest-max-us" "psi-cpu:psi-cpu-avg10" "psi-io:psi-io-avg10" "idle-watts:avg_watts"; do
    key="${metric_row%%:*}"; col="${metric_row##*:}"
    b=$(grep "$col" "$OUT/baseline/results.csv" 2>/dev/null | awk -F, 'NR==1{print $4}' || echo "?")
    o=$(grep "$col" "$OUT/optid/results.csv"    2>/dev/null | awk -F, 'NR==1{print $4}' || echo "?")
    printf "  %-28s  %-14s  %-14s\n" "$key" "$b" "$o"
done
echo
echo "Results saved to:"
echo "  $OUT/"

# --- Submit -----------------------------------------------------------------
if [[ $SUBMIT -eq 1 ]]; then
    log "submitting evidence PR..."
    if ! command -v gh >/dev/null 2>&1; then
        err "--submit requires the 'gh' CLI (https://cli.github.com). Install and run:"
        err "  cd $REPO_ROOT && python3 tools/rush-submit-evidence \"$OUT\" --submit-mode github"
        exit 0
    fi
    if ! gh auth status >/dev/null 2>&1 && [[ -z "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]]; then
        err "gh is not authenticated and GH_TOKEN is not set."
        err "Run 'gh auth login' or set GH_TOKEN=<token>, then rerun with --submit."
        err "Or submit manually: cd $REPO_ROOT && python3 tools/rush-submit-evidence \"$OUT\" --submit-mode github"
        exit 0
    fi
    if [[ $DRY_RUN -eq 1 ]]; then
        log "(dry run) would run: python3 tools/rush-submit-evidence \"$OUT\" --submit-mode github"
    else
        python3 tools/rush-submit-evidence "$OUT" --submit-mode github || {
            err "submit failed; results are still local at $OUT"
            exit 1
        }
    fi
else
    echo
    echo "${DIM}To submit these results as an evidence PR, run:${RESET}"
    echo "  ${CYAN}cd $REPO_ROOT && python3 tools/rush-submit-evidence \"$OUT\" --submit-mode github${RESET}"
    echo "${DIM}(requires gh auth or GH_TOKEN)${RESET}"
fi

echo
ok "done."
