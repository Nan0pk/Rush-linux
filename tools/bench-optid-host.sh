#!/usr/bin/env bash
# bench-optid-host.sh — Safe, self-restoring optid benchmark for a real host.
#
# Runs ONLY on real hardware (needs EPP and/or platform_profile). Captures every
# knob optid can touch, benchmarks a baseline, applies optid in one or more
# modes, benchmarks each, then restores the captured state and VERIFIES the
# restore. Restore runs on any exit path (success, error, Ctrl-C, kill).
#
# Default is DRY-RUN: it shows optid's plan and benchmarks the baseline only,
# mutating nothing. Pass --apply to actually let optid change knobs (still fully
# restored afterward).
#
# Usage:
#   sudo ./bench-optid-host.sh                       # dry-run + baseline only
#   sudo ./bench-optid-host.sh --apply               # apply 'performance', restore
#   sudo ./bench-optid-host.sh --apply --modes performance,battery
#   sudo ./bench-optid-host.sh --apply --duration 20 --optid /path/to/optid
#
set -euo pipefail

# ---- config / args ----------------------------------------------------------
APPLY=0
MODES="performance"
DURATION=15            # seconds of load per benchmark sample
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OPTID="${REPO_ROOT}/target/release/optid"
POLICY="${REPO_ROOT}/config/optid/policy.toml"
WORK="$(mktemp -d /tmp/optid-bench.XXXXXX)"
SAVE="${WORK}/restore"; mkdir -p "$SAVE"
RESTORED=0

while [ $# -gt 0 ]; do
  case "$1" in
    --apply) APPLY=1 ;;
    --modes) MODES="$2"; shift ;;
    --duration) DURATION="$2"; shift ;;
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
[ "$(id -u)" -eq 0 ] || { err "must run as root (writes sysfs). use sudo."; exit 1; }
[ -x "$OPTID" ] || { err "optid binary not found/executable at: $OPTID
  build it first: cargo build --release -p optid"; exit 1; }

EPP_PATHS=( $(ls /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference 2>/dev/null || true) )
PP_PATH="/sys/firmware/acpi/platform_profile"
HAVE_EPP=$([ "${#EPP_PATHS[@]}" -gt 0 ] && echo 1 || echo 0)
HAVE_PP=$([ -w "$PP_PATH" ] && echo 1 || echo 0)

if [ "$HAVE_EPP" = 0 ] && [ "$HAVE_PP" = 0 ]; then
  err "no EPP and no platform_profile on this host — optid would actuate nothing.
  This host is not a useful target (are you in a VM/container?)."
  exit 1
fi
log "host actuation surface: EPP=${HAVE_EPP} (${#EPP_PATHS[@]} cpus), platform_profile=${HAVE_PP}"

# ---- capture original state -------------------------------------------------
log "capturing original state -> ${SAVE}"
if [ "$HAVE_EPP" = 1 ]; then
  : > "${SAVE}/epp.map"
  for p in "${EPP_PATHS[@]}"; do printf '%s\t%s\n' "$p" "$(cat "$p")" >> "${SAVE}/epp.map"; done
fi
[ "$HAVE_PP" = 1 ] && cat "$PP_PATH" > "${SAVE}/platform_profile"
# cgroup weights are --runtime (reboot clears them) but capture for explicit reset:
for slice in user.slice background.slice system.slice; do
  systemctl show "$slice" -p CPUWeight -p IOWeight -p MemoryLow -p MemoryHigh \
    2>/dev/null > "${SAVE}/${slice}.props" || true
done

# ---- restore (runs on ANY exit) --------------------------------------------
restore() {
  [ "$RESTORED" = 1 ] && return 0
  RESTORED=1
  log "restoring original state..."
  if [ -f "${SAVE}/epp.map" ]; then
    while IFS=$'\t' read -r path val; do
      [ -w "$path" ] && printf '%s' "$val" > "$path" 2>/dev/null || true
    done < "${SAVE}/epp.map"
  fi
  if [ -f "${SAVE}/platform_profile" ] && [ -w "$PP_PATH" ]; then
    printf '%s' "$(cat "${SAVE}/platform_profile")" > "$PP_PATH" 2>/dev/null || true
  fi
  # reset transient cgroup props (these are also auto-cleared on reboot):
  for slice in user.slice background.slice; do
    systemctl set-property --runtime "$slice" CPUWeight= IOWeight= MemoryLow= MemoryHigh= \
      >/dev/null 2>&1 || true
  done

  # verify the persistent knobs came back
  local ok=1
  if [ -f "${SAVE}/epp.map" ]; then
    while IFS=$'\t' read -r path val; do
      [ "$(cat "$path" 2>/dev/null)" = "$val" ] || { ok=0; err "EPP NOT restored: $path"; }
    done < "${SAVE}/epp.map"
  fi
  if [ -f "${SAVE}/platform_profile" ]; then
    [ "$(cat "$PP_PATH" 2>/dev/null)" = "$(cat "${SAVE}/platform_profile")" ] || \
      { ok=0; err "platform_profile NOT restored"; }
  fi
  if [ "$ok" = 1 ]; then
    log "RESTORE VERIFIED: persistent knobs match original. (cgroup props are --runtime; a reboot also clears them.)"
  else
    err "RESTORE INCOMPLETE. Manual fix — original values saved in: ${SAVE}"
    err "  EPP:  while IFS=\$'\\t' read -r p v; do echo \"\$v\" > \"\$p\"; done < ${SAVE}/epp.map"
    err "  PP:   cat ${SAVE}/platform_profile > ${PP_PATH}"
  fi
}
trap restore EXIT
trap 'err "interrupted"; exit 130' INT TERM

# ---- benchmark: wakeup-latency under mixed load (input-latency proxy) -------
# Spawns CPU + IO load, then measures how late 1ms timer wakeups actually fire.
# Reports p50/p95/p99 in ms. Lower = more responsive under contention.
bench() {
  local label="$1"
  local loadpids=() ncpu; ncpu="$(nproc)"
  # CPU load: keep all cores busy
  if command -v stress-ng >/dev/null 2>&1; then
    stress-ng --cpu "$ncpu" --io 2 --timeout "$((DURATION+3))s" >/dev/null 2>&1 &
    loadpids+=($!)
  else
    for _ in $(seq "$ncpu"); do ( timeout "$((DURATION+3))" bash -c 'while :; do :; done' ) & loadpids+=($!); done
    ( timeout "$((DURATION+3))" bash -c 'while :; do dd if=/dev/zero of='"$WORK"'/io.tmp bs=1M count=64 oflag=dsync 2>/dev/null; done' ) & loadpids+=($!)
  fi
  sleep 1  # let load ramp

  python3 - "$DURATION" <<'PY'
import sys, time
dur = float(sys.argv[1]); end = time.perf_counter() + dur; d = []
while time.perf_counter() < end:
    t0 = time.perf_counter(); time.sleep(0.001)
    d.append((time.perf_counter() - t0 - 0.001) * 1000.0)  # overshoot ms
d.sort(); n = len(d)
pick = lambda q: d[min(n-1, int(q*n))]
print(f"    samples={n}  p50={pick(.50):.3f}ms  p95={pick(.95):.3f}ms  p99={pick(.99):.3f}ms  max={d[-1]:.3f}ms")
PY

  for pid in "${loadpids[@]}"; do kill "$pid" >/dev/null 2>&1 || true; wait "$pid" 2>/dev/null || true; done
  rm -f "$WORK"/io.tmp
}

apply_mode() {
  local mode="$1"
  printf '%s' "$mode" > "${WORK}/mode"
  "$OPTID" --once --apply --state-dir "$WORK" --config "$POLICY" >/dev/null 2>&1 || true
}

# ---- run --------------------------------------------------------------------
echo
log "=== optid plan (dry-run) for each mode ==="
for m in ${MODES//,/ }; do
  printf '%s' "$m" > "${WORK}/mode"
  echo "--- mode: $m ---"
  "$OPTID" --once --state-dir "$WORK" --config "$POLICY" >/dev/null 2>&1 || true
  grep -A20 '^actions:' "${WORK}/status" 2>/dev/null || true
done

echo
log "=== BASELINE (optid not applied) ==="
bench baseline

if [ "$APPLY" = 1 ]; then
  for m in ${MODES//,/ }; do
    echo
    log "=== APPLY mode=${m} ==="
    apply_mode "$m"
    [ "$HAVE_EPP" = 1 ] && log "EPP now: $(cat "${EPP_PATHS[0]}")"
    [ "$HAVE_PP" = 1 ]  && log "platform_profile now: $(cat "$PP_PATH")"
    bench "$m"
  done
else
  echo
  warn "dry-run only (no --apply): baseline measured, nothing mutated."
  warn "re-run with --apply to benchmark applied modes (state is still restored)."
fi

echo
log "done. restore will run now via trap."
# trap restore fires on EXIT
