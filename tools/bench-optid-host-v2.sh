#!/usr/bin/env bash
# bench-optid-host-v2.sh — Isolating optid benchmark for a real host.
#
# Fixes the three confounds found in v1 on real hardware:
#   1. cgroup isolation — load runs in background.slice, the latency probe runs
#      in user.slice, so optid's user.slice CPUWeight boost can actually be
#      measured (v1 put both in the same slice, cancelling the effect).
#   2. oversubscription — load spawns ~2x nproc busy threads so the run queue is
#      contended and weight arbitration actually matters.
#   3. power scenario — at PARTIAL load, sample CPU package watts via RAPL so
#      EPP=power (battery mode) has something to show (latency can't measure it).
# Plus: N iterations per config, median reported, to beat down p99/max noise.
#
# Two scenarios:
#   RESP  : full-core load in background.slice vs single-thread probe in
#           user.slice -> p95/p99 wakeup latency (ms). Tests the cgroup lever.
#   POWER : partial load -> avg CPU package watts. Tests the EPP lever.
#
# Same safety contract as v1: refuses hosts optid can't actuate, captures all
# knobs, restores+verifies on every exit, dry-run unless --apply.
#
# Usage:
#   sudo ./bench-optid-host-v2.sh                          # dry-run + baselines
#   sudo ./bench-optid-host-v2.sh --apply                  # baseline vs performance,battery
#   sudo ./bench-optid-host-v2.sh --apply --modes performance --iter 5
#
set -euo pipefail

APPLY=0
MODES="performance,battery"
DURATION=12
ITER=3
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OPTID="${REPO_ROOT}/target/release/optid"
POLICY="${REPO_ROOT}/config/optid/policy.toml"
WORK="$(mktemp -d /tmp/optid-bench2.XXXXXX)"
SAVE="${WORK}/restore"; mkdir -p "$SAVE"
RESTORED=0
LOAD_UNIT="optid-bench-load.service"

while [ $# -gt 0 ]; do
  case "$1" in
    --apply) APPLY=1 ;;
    --modes) MODES="$2"; shift ;;
    --duration) DURATION="$2"; shift ;;
    --iter) ITER="$2"; shift ;;
    --optid) OPTID="$2"; shift ;;
    --policy) POLICY="$2"; shift ;;
    -h|--help) grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
  shift
done

log()  { printf '\033[1;34m[bench]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[warn]\033[0m %s\n' "$*" >&2; }
err()  { printf '\033[1;31m[err]\033[0m %s\n' "$*" >&2; }

# ---- preflight --------------------------------------------------------------
[ "$(id -u)" -eq 0 ] || { err "must run as root (writes sysfs, places cgroups). use sudo."; exit 1; }
[ -x "$OPTID" ] || { err "optid not built at: $OPTID  (cargo build --release -p optid)"; exit 1; }
command -v systemd-run >/dev/null || { err "systemd-run required"; exit 1; }

EPP_PATHS=( $(ls /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference 2>/dev/null || true) )
PP_PATH="/sys/firmware/acpi/platform_profile"
HAVE_EPP=$([ "${#EPP_PATHS[@]}" -gt 0 ] && echo 1 || echo 0)
HAVE_PP=$([ -w "$PP_PATH" ] && echo 1 || echo 0)
NCPU="$(nproc)"
[ "$HAVE_EPP" = 0 ] && [ "$HAVE_PP" = 0 ] && { err "no EPP/platform_profile here — optid actuates nothing."; exit 1; }

# RAPL package-energy domain (Intel & most AMD via powercap). NA if absent.
RAPL_DOM=""
for d in /sys/class/powercap/intel-rapl:0 /sys/class/powercap/*:0; do
  [ -r "$d/energy_uj" ] && { RAPL_DOM="$d"; break; }
done
log "surface: EPP=${HAVE_EPP}(${#EPP_PATHS[@]}cpu) platform_profile=${HAVE_PP} cpus=${NCPU} rapl=${RAPL_DOM:-NA}"

# ---- capture / restore ------------------------------------------------------
log "capturing original state -> ${SAVE}"
if [ "$HAVE_EPP" = 1 ]; then
  : > "${SAVE}/epp.map"
  for p in "${EPP_PATHS[@]}"; do printf '%s\t%s\n' "$p" "$(cat "$p")" >> "${SAVE}/epp.map"; done
fi
[ "$HAVE_PP" = 1 ] && cat "$PP_PATH" > "${SAVE}/platform_profile"

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
  local ok=1
  if [ -f "${SAVE}/epp.map" ]; then
    while IFS=$'\t' read -r path val; do [ "$(cat "$path" 2>/dev/null)" = "$val" ] || { ok=0; err "EPP NOT restored: $path"; }; done < "${SAVE}/epp.map"
  fi
  [ -f "${SAVE}/platform_profile" ] && { [ "$(cat "$PP_PATH" 2>/dev/null)" = "$(cat "${SAVE}/platform_profile")" ] || { ok=0; err "platform_profile NOT restored"; }; }
  [ "$ok" = 1 ] && log "RESTORE VERIFIED (persistent knobs match; cgroup props are --runtime)." \
                || err "RESTORE INCOMPLETE — originals in ${SAVE}"
}
trap restore EXIT
trap 'err interrupted; exit 130' INT TERM

# ---- primitives -------------------------------------------------------------
# start oversubscribed CPU load inside background.slice (async service)
start_load_bg() { # $1 = nthreads
  stop_load
  systemd-run --quiet --collect --unit="$LOAD_UNIT" --slice=background.slice \
    bash -c 'for i in $(seq '"$1"'); do (while :; do :; done) & done; wait' >/dev/null 2>&1
}

# single-thread wakeup-latency probe placed in user.slice; prints "p50 p95 p99 max"
probe_user_slice() { # $1 = duration
  local out="${WORK}/probe.$$"; : > "$out"
  systemd-run --scope --quiet --slice=user.slice -- \
    python3 -c '
import sys,time
dur=float(sys.argv[1]); end=time.perf_counter()+dur; d=[]
while time.perf_counter()<end:
    t0=time.perf_counter(); time.sleep(0.001)
    d.append((time.perf_counter()-t0-0.001)*1000.0)
d.sort(); n=len(d); pick=lambda q:d[min(n-1,int(q*n))]
print(f"{pick(.50):.3f} {pick(.95):.3f} {pick(.99):.3f} {d[-1]:.3f}")' "$1" > "$out" 2>/dev/null
  cat "$out"
}

# avg CPU package watts over duration via RAPL (handles counter wrap). "NA" if no RAPL.
rapl_watts() { # $1 = duration
  [ -z "$RAPL_DOM" ] && { echo NA; return; }
  local e0 e1 mx; e0="$(cat "$RAPL_DOM/energy_uj")"; mx="$(cat "$RAPL_DOM/max_energy_range_uj" 2>/dev/null || echo 0)"
  sleep "$1"; e1="$(cat "$RAPL_DOM/energy_uj")"
  python3 -c "e0=$e0;e1=$e1;mx=$mx;d=$1;de=(e1-e0) if e1>=e0 else (mx-e0+e1);print(f'{de/1e6/d:.2f}')"
}

median() { printf '%s\n' "$@" | sort -n | awk '{a[NR]=$1} END{print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}'; }

apply_mode() { printf '%s' "$1" > "${WORK}/mode"; "$OPTID" --once --apply --state-dir "$WORK" --config "$POLICY" >/dev/null 2>&1 || true; }
clear_mode() { # restore knobs to captured baseline between configs
  if [ -f "${SAVE}/epp.map" ]; then while IFS=$'\t' read -r p v; do [ -w "$p" ] && printf '%s' "$v" > "$p" 2>/dev/null || true; done < "${SAVE}/epp.map"; fi
  for s in user.slice background.slice; do systemctl set-property --runtime "$s" CPUWeight= IOWeight= >/dev/null 2>&1 || true; done
}

# ---- scenarios --------------------------------------------------------------
run_resp() { # $1 = label ; assumes knobs already set for this config
  local p95s=() p99s=() i r
  start_load_bg "$((NCPU*2))"; sleep 1
  for i in $(seq "$ITER"); do
    r="$(probe_user_slice "$DURATION")"
    p95s+=("$(echo "$r" | awk '{print $2}')"); p99s+=("$(echo "$r" | awk '{print $3}')")
  done
  stop_load
  printf '    %-14s p95(med)=%sms  p99(med)=%sms   [iters: %s | %s]\n' \
    "$1" "$(median "${p95s[@]}")" "$(median "${p99s[@]}")" "$(IFS=,; echo "${p95s[*]}")" "$(IFS=,; echo "${p99s[*]}")"
}

run_power() { # $1 = label
  [ -z "$RAPL_DOM" ] && { printf '    %-14s watts=NA (no RAPL on this host)\n' "$1"; return; }
  local ws=() i; start_load_bg "$((NCPU/4>0?NCPU/4:1))"; sleep 1
  for i in $(seq "$ITER"); do ws+=("$(rapl_watts "$DURATION")"); done
  stop_load
  printf '    %-14s pkgW(med)=%sW   [iters: %s]\n' "$1" "$(median "${ws[@]}")" "$(IFS=,; echo "${ws[*]}")"
}

# ---- run --------------------------------------------------------------------
echo; log "=== plan (dry-run) ==="
for m in ${MODES//,/ }; do
  printf '%s' "$m" > "${WORK}/mode"; "$OPTID" --once --state-dir "$WORK" --config "$POLICY" >/dev/null 2>&1 || true
  echo "--- $m ---"; grep -A6 '^actions:' "${WORK}/status" 2>/dev/null || true
done

echo; log "=== SCENARIO RESP (cgroup-isolated wakeup latency, load=${NCPU}x2 in background.slice) ==="
clear_mode; run_resp baseline
echo; log "=== SCENARIO POWER (partial load, CPU package watts) ==="
clear_mode; run_power baseline

if [ "$APPLY" = 1 ]; then
  for m in ${MODES//,/ }; do
    echo; log "=== APPLY mode=${m} ==="
    clear_mode; apply_mode "$m"
    [ "$HAVE_EPP" = 1 ] && log "EPP now: $(cat "${EPP_PATHS[0]}")  (user.slice CPUWeight: $(systemctl show user.slice -p CPUWeight --value 2>/dev/null))"
    run_resp "$m"; clear_mode; apply_mode "$m"; run_power "$m"
  done
else
  echo; warn "dry-run only — re-run with --apply to measure applied modes (state still restored)."
fi
echo; log "done. restore runs now via trap."
