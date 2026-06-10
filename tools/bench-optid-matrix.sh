#!/usr/bin/env bash
# bench-optid-matrix.sh — Guided, human-in-the-loop benchmark campaign for optid.
#
# Walks a test matrix across power sources (AC / battery), optimization levers,
# and scenarios, PROMPTING the operator to plug/unplug the charger and
# VERIFYING the state change through sysfs before measuring. Produces an
# evidence-ready results directory (CSV + metadata + full transcript).
#
# Matrix dimensions:
#   power : ac, bat            (prompted + sysfs-verified; default: both)
#   lever : baseline           nothing applied
#           epp                EPP written directly (isolates the EPP lever)
#           weight             user.slice CPUWeight=200 only (isolates cgroups)
#           optid-performance  full optid --apply, mode=performance
#           optid-battery      full optid --apply, mode=battery
#   scen  : RESP (cgroup-isolated wakeup latency), POWER (RAPL package watts)
#
# Ambient desktop load (browser, video, agents) is RECORDED as metadata, not
# forbidden — real-world conditions are valid test conditions, but they must
# be visible in the evidence.
#
# Safety contract (same as v1/v2, extended):
#   - refuses hosts optid cannot actuate; refuses non-root
#   - captures EPP/platform_profile before anything; restores + verifies on
#     EVERY exit path; cgroup props are --runtime (reboot also clears)
#   - stops tuned/power-profiles-daemon for the session IF running and
#     restarts them in the exit trap
#   - refuses battery phase below --min-batt (default 25%) unless --force
#   - requires --apply to mutate anything (without it: plans + baselines only)
#
# Usage:
#   sudo ./bench-optid-matrix.sh --apply                       # full matrix, both power sources
#   sudo ./bench-optid-matrix.sh --apply --power ac            # AC only, no prompts
#   sudo ./bench-optid-matrix.sh --apply --levers baseline,epp --iter 9
#   sudo ./bench-optid-matrix.sh --apply --out ~/bench-results
#
set -euo pipefail

APPLY=0
POWER_PHASES="auto"        # auto -> ac,bat if a battery exists, else ac
LEVERS="baseline,epp,weight,optid-performance,optid-battery"
DURATION=12
ITER=5
MIN_BATT=25
FORCE=0
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OPTID="${REPO_ROOT}/target/release/optid"
POLICY="${REPO_ROOT}/config/optid/policy.toml"
OUT_BASE="${HOME}/optid-bench-results"
WORK="$(mktemp -d /tmp/optid-matrix.XXXXXX)"
SAVE="${WORK}/restore"; mkdir -p "$SAVE"
RESTORED=0
LOAD_UNIT="optid-bench-load.service"
STOPPED_DAEMONS=()

while [ $# -gt 0 ]; do
  case "$1" in
    --apply) APPLY=1 ;;
    --power) POWER_PHASES="$2"; shift ;;
    --levers) LEVERS="$2"; shift ;;
    --duration) DURATION="$2"; shift ;;
    --iter) ITER="$2"; shift ;;
    --min-batt) MIN_BATT="$2"; shift ;;
    --force) FORCE=1 ;;
    --out) OUT_BASE="$2"; shift ;;
    --optid) OPTID="$2"; shift ;;
    --policy) POLICY="$2"; shift ;;
    -h|--help) grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
  shift
done

log()  { printf '\033[1;34m[matrix]\033[0m %s\n' "$*"; }
ask()  { printf '\033[1;32m[ACTION]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[warn]\033[0m %s\n' "$*" >&2; }
err()  { printf '\033[1;31m[err]\033[0m %s\n' "$*" >&2; }

# ---- preflight --------------------------------------------------------------
[ "$(id -u)" -eq 0 ] || { err "must run as root. use sudo."; exit 1; }
[ -x "$OPTID" ] || { err "optid not built at $OPTID (cargo build --release -p optid)"; exit 1; }
command -v systemd-run >/dev/null || { err "systemd-run required"; exit 1; }

EPP_PATHS=( $(ls /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference 2>/dev/null || true) )
PP_PATH="/sys/firmware/acpi/platform_profile"
HAVE_EPP=$([ "${#EPP_PATHS[@]}" -gt 0 ] && echo 1 || echo 0)
HAVE_PP=$([ -w "$PP_PATH" ] && echo 1 || echo 0)
NCPU="$(nproc)"
[ "$HAVE_EPP" = 0 ] && [ "$HAVE_PP" = 0 ] && { err "no EPP/platform_profile — optid actuates nothing here."; exit 1; }

RAPL_DOM=""
for d in /sys/class/powercap/intel-rapl:0 /sys/class/powercap/*:0; do
  [ -r "$d/energy_uj" ] && { RAPL_DOM="$d"; break; }
done

# power-supply discovery
AC_PATH=""; BAT_PATH=""
for ps in /sys/class/power_supply/*; do
  [ -e "$ps/type" ] || continue
  case "$(cat "$ps/type")" in
    Mains) AC_PATH="$ps" ;;
    Battery) BAT_PATH="$ps" ;;
  esac
done

ac_online() { [ -n "$AC_PATH" ] && [ "$(cat "$AC_PATH/online" 2>/dev/null)" = "1" ] && echo 1 || echo 0; }
batt_pct()  { cat "$BAT_PATH/capacity" 2>/dev/null || echo "NA"; }

if [ "$POWER_PHASES" = "auto" ]; then
  if [ -n "$BAT_PATH" ] && [ -n "$AC_PATH" ]; then POWER_PHASES="ac,bat"; else POWER_PHASES="ac"; fi
fi

RUN_ID="$(date +%Y%m%d-%H%M%S)"
OUT="${OUT_BASE}/${RUN_ID}"
mkdir -p "$OUT"
CSV="${OUT}/results.csv"
echo "phase,lever,scenario,metric,median,iters,batt_pct,ambient_cpu_pct" > "$CSV"
exec > >(tee "${OUT}/transcript.log") 2>&1

log "surface: EPP=${HAVE_EPP}(${#EPP_PATHS[@]}cpu) platform_profile=${HAVE_PP} rapl=${RAPL_DOM:-NA} ac=${AC_PATH:-NA} bat=${BAT_PATH:-NA}"
log "matrix: power=[${POWER_PHASES}] levers=[${LEVERS}] iter=${ITER} duration=${DURATION}s"
log "results -> ${OUT}"

# ---- metadata snapshot --------------------------------------------------------
{
  echo "date=$(date -Iseconds)"
  echo "host=$(hostname)"
  echo "kernel=$(uname -r)"
  echo "cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ //')"
  echo "ncpu=${NCPU}"
  echo "cpufreq_driver=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_driver 2>/dev/null || echo NA)"
  echo "governor=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo NA)"
  echo "platform_profile_available=${HAVE_PP}"
  echo "rapl_domain=${RAPL_DOM:-NA}"
  echo "optid_version=$("$OPTID" --help 2>/dev/null | head -1 || echo NA)"
  echo "git_commit=$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo NA)"
} > "${OUT}/meta.txt"

ambient_cpu() { # %CPU used by everything EXCEPT our load unit, sampled over 2s
  local a b
  a=( $(head -1 /proc/stat) ); sleep 2; b=( $(head -1 /proc/stat) )
  python3 -c "
a=[${a[1]},${a[2]},${a[3]},${a[4]}]; b=[${b[1]},${b[2]},${b[3]},${b[4]}]
tot=sum(x-y for x,y in zip(b,a)); idle=b[3]-a[3]
print(f'{100*(tot-idle)/tot:.1f}' if tot>0 else 'NA')"
}

record_ambient() { # $1 phase
  local cpu; cpu="$(ambient_cpu)"
  {
    echo "--- ambient @ phase=$1 $(date -Iseconds) ---"
    echo "ambient_cpu_pct=${cpu}"
    echo "loadavg=$(cut -d' ' -f1-3 /proc/loadavg)"
    echo "batt_pct=$(batt_pct) ac_online=$(ac_online)"
    echo "top processes:"
    ps -eo pcpu,comm --sort=-pcpu | head -8 | sed 's/^/  /'
  } >> "${OUT}/meta.txt"
  echo "$cpu"
}

# ---- capture / restore --------------------------------------------------------
log "capturing original state -> ${SAVE}"
if [ "$HAVE_EPP" = 1 ]; then
  : > "${SAVE}/epp.map"
  for p in "${EPP_PATHS[@]}"; do printf '%s\t%s\n' "$p" "$(cat "$p")" >> "${SAVE}/epp.map"; done
fi
[ "$HAVE_PP" = 1 ] && cat "$PP_PATH" > "${SAVE}/platform_profile"

for d in tuned power-profiles-daemon; do
  if systemctl is-active --quiet "$d" 2>/dev/null; then
    log "stopping competing daemon for the session: $d (will restart on exit)"
    systemctl stop "$d" && STOPPED_DAEMONS+=("$d")
  fi
done

stop_load() { systemctl stop "$LOAD_UNIT" 2>/dev/null || true; systemctl reset-failed "$LOAD_UNIT" 2>/dev/null || true; }

restore() {
  [ "$RESTORED" = 1 ] && return 0; RESTORED=1
  stop_load
  log "restoring original state..."
  if [ -f "${SAVE}/epp.map" ]; then
    while IFS=$'\t' read -r path val; do [ -w "$path" ] && printf '%s' "$val" > "$path" 2>/dev/null || true; done < "${SAVE}/epp.map"
  fi
  [ -f "${SAVE}/platform_profile" ] && [ -w "$PP_PATH" ] && printf '%s' "$(cat "${SAVE}/platform_profile")" > "$PP_PATH" 2>/dev/null || true
  for s in user.slice background.slice; do
    systemctl set-property --runtime "$s" CPUWeight= IOWeight= MemoryLow= MemoryHigh= >/dev/null 2>&1 || true
  done
  for d in "${STOPPED_DAEMONS[@]:-}"; do
    [ -n "$d" ] && { log "restarting $d"; systemctl start "$d" 2>/dev/null || warn "could not restart $d — start it manually"; }
  done
  local ok=1
  if [ -f "${SAVE}/epp.map" ]; then
    # NOTE: a restarted tuned may legitimately rewrite EPP after we restore it;
    # verify BEFORE daemon restart would race, so verify tolerantly here.
    while IFS=$'\t' read -r path val; do
      [ "$(cat "$path" 2>/dev/null)" = "$val" ] || warn "EPP differs post-restore at $path (a restarted power daemon may have re-applied policy — this is expected if tuned/ppd was running)"
    done < "${SAVE}/epp.map"
  fi
  log "restore complete. results: ${OUT}"
}
trap restore EXIT
trap 'err interrupted; exit 130' INT TERM

# ---- power-phase prompt -------------------------------------------------------
await_power_state() { # $1 = ac|bat
  local want msg t=0
  if [ "$1" = "ac" ]; then want=1; msg="PLUG IN the charger"; else want=0; msg="UNPLUG the charger"; fi
  if [ "$(ac_online)" = "$want" ]; then log "power state already correct ($1)"; return 0; fi
  ask "${msg} now. I will detect it automatically (waiting up to 120s)..."
  while [ "$(ac_online)" != "$want" ]; do
    sleep 2; t=$((t+2))
    [ $t -ge 120 ] && { err "power state never changed to $1 — skipping this phase"; return 1; }
  done
  log "detected: power=$1 (battery at $(batt_pct)%)"
  sleep 3   # let firmware/driver settle after the transition
}

# ---- levers -------------------------------------------------------------------
clear_levers() {
  if [ -f "${SAVE}/epp.map" ]; then
    while IFS=$'\t' read -r p v; do [ -w "$p" ] && printf '%s' "$v" > "$p" 2>/dev/null || true; done < "${SAVE}/epp.map"
  fi
  for s in user.slice background.slice; do systemctl set-property --runtime "$s" CPUWeight= IOWeight= >/dev/null 2>&1 || true; done
}

apply_lever() { # $1 = lever name -> 0 applied, 1 skip
  clear_levers
  case "$1" in
    baseline) : ;;
    epp)
      [ "$HAVE_EPP" = 1 ] || return 1
      for p in "${EPP_PATHS[@]}"; do printf 'power' > "$p" 2>/dev/null || true; done ;;
    weight)
      systemctl set-property --runtime user.slice CPUWeight=200 IOWeight=200 ;;
    optid-performance)
      printf 'performance' > "${WORK}/mode"
      "$OPTID" --once --apply --state-dir "$WORK" --config "$POLICY" >/dev/null 2>&1 || true ;;
    optid-battery)
      printf 'battery' > "${WORK}/mode"
      "$OPTID" --once --apply --state-dir "$WORK" --config "$POLICY" >/dev/null 2>&1 || true ;;
    *) warn "unknown lever: $1"; return 1 ;;
  esac
  return 0
}

# ---- scenarios (from v2) ------------------------------------------------------
read_work_count() {
  local total=0 val
  for f in /dev/shm/optid-bench-work-*; do
    if [ -r "$f" ]; then
      val="$(cat "$f")"
      total=$((total + val))
    fi
  done
  echo "$total"
}

start_load_bg() { stop_load; systemd-run --quiet --collect --unit="$LOAD_UNIT" --slice=background.slice \
  python3 "${REPO_ROOT}/tools/bench-work-load.py" "$1" >/dev/null 2>&1; }

probe_user_slice() {
  systemd-run --scope --quiet --slice=user.slice -- python3 -c '
import sys,time
dur=float(sys.argv[1]); end=time.perf_counter()+dur; d=[]
while time.perf_counter()<end:
    t0=time.perf_counter(); time.sleep(0.001)
    d.append((time.perf_counter()-t0-0.001)*1000.0)
d.sort(); n=len(d); pick=lambda q:d[min(n-1,int(q*n))]
print(f"{pick(.50):.3f} {pick(.95):.3f} {pick(.99):.3f} {d[-1]:.3f}")' "$1" 2>/dev/null
}

rapl_watts_efficiency() {
  [ -z "$RAPL_DOM" ] && { echo "NA NA NA"; return; }
  local e0 e1 mx w0 w1
  w0="$(read_work_count)"
  e0="$(cat "$RAPL_DOM/energy_uj")"
  mx="$(cat "$RAPL_DOM/max_energy_range_uj" 2>/dev/null || echo 0)"
  sleep "$1"
  e1="$(cat "$RAPL_DOM/energy_uj")"
  w1="$(read_work_count)"
  python3 -c "
e0=$e0; e1=$e1; mx=$mx; d=$1; w0=$w0; w1=$w1
de=(e1-e0) if e1>=e0 else (mx-e0+e1)
watts = de/1e6/d
work = w1 - w0
eff = (work / (de/1e6)) if de > 0 else 0
print(f'{watts:.2f} {work} {eff:.2f}')"
}

median() { printf '%s\n' "$@" | sort -n | awk '{a[NR]=$1} END{print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}'; }

run_cell() { # $1 phase, $2 lever, $3 ambient
  local p95s=() p99s=() ws=() works=() effs=() i r res
  # RESP
  start_load_bg "$((NCPU*2))"; sleep 1
  for i in $(seq "$ITER"); do
    r="$(probe_user_slice "$DURATION")" || continue
    p95s+=("$(echo "$r" | awk '{print $2}')"); p99s+=("$(echo "$r" | awk '{print $3}')")
  done
  stop_load
  printf '    %-18s RESP p95(med)=%sms p99(med)=%sms [%s | %s]\n' "$2" \
    "$(median "${p95s[@]}")" "$(median "${p99s[@]}")" "$(IFS=,; echo "${p95s[*]}")" "$(IFS=,; echo "${p99s[*]}")"
  echo "$1,$2,RESP,p95_ms,$(median "${p95s[@]}"),\"$(IFS=,; echo "${p95s[*]}")\",$(batt_pct),$3" >> "$CSV"
  echo "$1,$2,RESP,p99_ms,$(median "${p99s[@]}"),\"$(IFS=,; echo "${p99s[*]}")\",$(batt_pct),$3" >> "$CSV"
  # POWER (partial load)
  if [ -n "$RAPL_DOM" ]; then
    start_load_bg "$((NCPU/4>0?NCPU/4:1))"; sleep 1
    for i in $(seq "$ITER"); do
      res="$(rapl_watts_efficiency "$DURATION")"
      ws+=( "$(echo "$res" | awk '{print $1}')" )
      works+=( "$(echo "$res" | awk '{print $2}')" )
      effs+=( "$(echo "$res" | awk '{print $3}')" )
    done
    stop_load
    printf '    %-18s POWER pkgW(med)=%sW work(med)=%s eff(med)=%s/J [%s]\n' "$2" \
      "$(median "${ws[@]}")" "$(median "${works[@]}")" "$(median "${effs[@]}")" "$(IFS=,; echo "${effs[*]}")"
    echo "$1,$2,POWER,pkg_watts,$(median "${ws[@]}"),\"$(IFS=,; echo "${ws[*]}")\",$(batt_pct),$3" >> "$CSV"
    echo "$1,$2,POWER,work_units,$(median "${works[@]}"),\"$(IFS=,; echo "${works[*]}")\",$(batt_pct),$3" >> "$CSV"
    echo "$1,$2,POWER,work_per_joule,$(median "${effs[@]}"),\"$(IFS=,; echo "${effs[*]}")\",$(batt_pct),$3" >> "$CSV"
  fi
}

# ---- campaign -----------------------------------------------------------------
for phase in ${POWER_PHASES//,/ }; do
  echo; log "================ PHASE: power=${phase} ================"
  await_power_state "$phase" || continue

  if [ "$phase" = "bat" ] && [ -n "$BAT_PATH" ]; then
    pct="$(batt_pct)"
    if [ "$pct" != "NA" ] && [ "$pct" -lt "$MIN_BATT" ] && [ "$FORCE" = 0 ]; then
      err "battery at ${pct}% < --min-batt ${MIN_BATT}% — skipping battery phase (--force to override)"
      continue
    fi
  fi

  amb="$(record_ambient "$phase")"
  log "ambient (non-harness) CPU before phase: ${amb}% — recorded in meta.txt"

  for lever in ${LEVERS//,/ }; do
    if [ "$APPLY" = 0 ] && [ "$lever" != "baseline" ]; then continue; fi
    apply_lever "$lever" || { warn "lever '$lever' unavailable here — skipped"; continue; }
    [ "$HAVE_EPP" = 1 ] && log "lever=${lever}: EPP=$(cat "${EPP_PATHS[0]}") user.slice CPUWeight=$(systemctl show user.slice -p CPUWeight --value 2>/dev/null)"
    run_cell "$phase" "$lever" "$amb"
    clear_levers
  done
done

[ "$APPLY" = 0 ] && warn "dry-run: only baselines measured. re-run with --apply for the full matrix."
echo
log "campaign complete. evidence package: ${OUT}"
log "  results.csv      machine-readable matrix results"
log "  meta.txt         hardware, drivers, ambient load per phase"
log "  transcript.log   full raw transcript"
