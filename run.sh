#!/usr/bin/env bash
# run.sh — lean, rugged Rush Linux hardware run. Never aborts; logs everything.
set +e -u +o pipefail
export LC_ALL=C
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
export PATH="$ROOT/target/release:$PATH"
OUT="$ROOT/run"; LOGS="$OUT/logs"; mkdir -p "$LOGS"
REPORT="$OUT/REPORT.md"; : > "$REPORT"
TS(){ date -u +%Y-%m-%dT%H:%M:%SZ; }
say(){ printf '%s %s\n' "$(TS)" "$*"; printf -- '- %s\n' "$*" >> "$REPORT"; }
have(){ command -v "$1" >/dev/null 2>&1; }
SUDO=""; PRIV=no
if [ "$(id -u)" -eq 0 ]; then PRIV=yes
elif command -v sudo >/dev/null 2>&1; then
  echo "Not root. Benchmarks need root to actuate EPP/cgroups and read RAPL." >&2
  if sudo -n -v 2>/dev/null; then SUDO="sudo"; PRIV=yes
  else echo "sudo declined/unavailable — will build+test only, benches will SKIP." >&2; fi
fi
APPLY=""; [ "$PRIV" = yes ] && APPLY="--apply"

run(){ # run <label> <logfile> <cmd...>  — never aborts
  local label="$1" lf="$LOGS/$2"; shift 2
  say "RUN  $label"
  ( "$@" ) >"$lf" 2>&1; local rc=$?
  if [ $rc -eq 0 ]; then say "OK   $label"; else
    say "FAIL $label (rc=$rc) — see logs/$2"
    { echo "### FAIL $label (rc=$rc)"; echo '```'; tail -n 40 "$lf"; echo '```'; } >> "$REPORT"
  fi
  return $rc
}

runroot(){ # like run(), but SKIP cleanly when we can't actuate
  local label="$1"
  if [ "$PRIV" != yes ]; then
    say "SKIP $label — needs root; re-run as: sudo bash run.sh  (for real evidence)"; return 0; fi
  if [ "$REAL_HW" != yes ] && [ "${ENVKIND%%[:/]*}" = container ]; then
    say "SKIP $label — env=$ENVKIND has no writable hardware lever"; return 0; fi
  run "$@"
}

# Daemons we stop during the session (to free the governor/EPP lever) get
# restored on exit, no matter how the script ends.
RESTORE_DAEMONS=""
restore_daemons(){ for d in ${RESTORE_DAEMONS:-}; do
    $SUDO systemctl start "$d.service" 2>/dev/null && say "restored $d"; done; }
trap restore_daemons EXIT

echo "# Rush Linux hardware run — $(TS)" >> "$REPORT"

# --- environment & actuation surface ----------------------------------------
{
  echo "## Environment"; echo '```'
  echo "host=$(hostname 2>/dev/null) kernel=$(uname -r) ncpu=$(nproc 2>/dev/null)"
  VIRT_RAW="$(systemd-detect-virt 2>/dev/null)"; VIRT_RC=$?
  # systemd-detect-virt exits 1 on bare metal — that's not an error
  if [ $VIRT_RC -ne 0 ] || [ -z "$VIRT_RAW" ]; then VIRT_RAW="none"; fi
  echo "virt=$VIRT_RAW"
  echo "cpu=$( (grep -m1 'model name' /proc/cpuinfo 2>/dev/null || echo NA) | cut -d: -f2- | sed 's/^ //')"
  echo "uptime/load:$(uptime 2>/dev/null)"
  EPP=$(ls /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference 2>/dev/null | wc -l)
  echo "EPP_cpus=$EPP"
  echo "platform_profile=$(cat /sys/firmware/acpi/platform_profile 2>/dev/null || echo ABSENT)"
  RAPL=""; for d in /sys/class/powercap/*:0; do [ -r "$d/energy_uj" ] && RAPL="$d" && break; done
  echo "rapl=${RAPL:-NA}"
  for ps in /sys/class/power_supply/*; do [ -d "$ps" ] && echo "psupply $ps=$(cat "$ps/type" 2>/dev/null)"; done
  echo "PSI=$( [ -e /proc/pressure/cpu ] && echo yes || echo MISSING )"
  echo "cpu_dma_latency=$( [ -e /dev/cpu_dma_latency ] && echo yes || echo no )"
  echo '```'
} >> "$REPORT"

# --- environment awareness (fixed: virt detection + HW detection) ------------
ENVKIND="baremetal"; ENVNOTE=""
VIRT="$VIRT_RAW"
VIRTC="$(systemd-detect-virt -c 2>/dev/null)" || VIRTC="none"
[ -z "$VIRTC" ] && VIRTC="none"
if [ -f /.dockerenv ] || [ -f /run/.containerenv ] \
   || grep -qaE '(docker|containerd|kubepods|lxc|libpod)' /proc/1/cgroup 2>/dev/null \
   || { [ "$VIRTC" != "none" ]; }; then
  ENVKIND="container${VIRTC:+:$VIRTC}"
elif [ "$VIRT" != "none" ]; then
  ENVKIND="vm:$VIRT"
fi
[ -n "${CI:-}${GITHUB_ACTIONS:-}" ] && ENVKIND="$ENVKIND/ci"

# Hardware detection: EPP *exists* = real hardware (writability is a privilege
# question, not a hardware question). Also check platform_profile existence.
EPP1="$(ls /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference 2>/dev/null | head -1)"
REAL_HW=no
[ -n "$EPP1" ] && REAL_HW=yes
[ -e /sys/firmware/acpi/platform_profile ] && REAL_HW=yes

# downgrade actuation if containerized — but NOT just because non-root on real HW
if [ "${ENVKIND%%[:/]*}" = "container" ]; then
  APPLY=""; ENVNOTE="dry-run forced: env=$ENVKIND → results NOT representative of real hardware"
fi
{ echo "## Environment awareness"; echo '```';
  echo "env_kind=$ENVKIND  virt=$VIRT  container=$VIRTC  ci=${CI:-no}";
  echo "real_writable_hw=$REAL_HW  priv=$PRIV  actuation=${APPLY:-dry-run}";
  echo "distro=$( . /etc/os-release 2>/dev/null; echo "${PRETTY_NAME:-unknown}")";
  [ -n "$ENVNOTE" ] && echo "NOTE: $ENVNOTE";
  echo '```'; } >> "$REPORT"
say "env=$ENVKIND real_hw=$REAL_HW priv=$PRIV actuation=${APPLY:-dry-run}${ENVNOTE:+ — $ENVNOTE}"

# --- power handling audit (runs before benchmarks) ---------------------------
# Catches misconfigured governors, competing daemons, unusual kernel params.
# Auto-fixes governor to powersave if root (EPP is ignored under performance).
{
  echo "## Power handling audit"; echo '```'
  POWER_WARNINGS=0

  # 1. cpufreq driver
  CPUFREQ_DRV="$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_driver 2>/dev/null || echo UNKNOWN)"
  echo "cpufreq_driver=$CPUFREQ_DRV"

  # 2. Intel HWP (Hardware P-states)
  HWP="no"
  grep -q 'hwp ' /proc/cpuinfo 2>/dev/null && HWP="yes"
  HWP_ACTIVE="$(cat /sys/devices/system/cpu/intel_pstate/hwp_dynamic_boost 2>/dev/null || echo N/A)"
  echo "intel_hwp_capable=$HWP  hwp_dynamic_boost=$HWP_ACTIVE"
  PSTATE_STATUS="$(cat /sys/devices/system/cpu/intel_pstate/status 2>/dev/null || echo N/A)"
  echo "intel_pstate_status=$PSTATE_STATUS"

  # 3. CPU governor — the critical check. EPP is IGNORED if governor=performance.
  GOV0="$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo UNKNOWN)"
  GOVS="$(sort -u /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null | tr '\n' ',')"
  echo "scaling_governor(cpu0)=$GOV0  all_unique=${GOVS%,}"
  if [ "$GOV0" = "performance" ]; then
    echo "WARNING: governor=performance — EPP hints are IGNORED, all levers will be inert"
    POWER_WARNINGS=$((POWER_WARNINGS+1))
    if [ "$PRIV" = yes ]; then
      echo "AUTO-FIX: stopping daemons that pin the governor, then switching to powersave..."
      # power-profiles-daemon / tuned on intel_pstate can pin governor=performance;
      # thermald can re-assert EPP. Stop them for the session; restored on exit.
      for d in power-profiles-daemon thermald tuned; do
        if systemctl is-active --quiet "$d.service" 2>/dev/null; then
          $SUDO systemctl stop "$d.service" 2>/dev/null && RESTORE_DAEMONS="$RESTORE_DAEMONS $d"
        fi
      done
      [ -n "$RESTORE_DAEMONS" ] && echo "stopped for session (restored on exit):$RESTORE_DAEMONS"
      for g in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
        $SUDO sh -c "echo powersave > '$g'" 2>/dev/null
      done
      GOV_AFTER="$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null)"
      echo "governor_after_fix=$GOV_AFTER"
      if [ "$GOV_AFTER" = "powersave" ]; then echo "FIX OK: governor now powersave — EPP lever active"
      else echo "FIX FAILED: governor still $GOV_AFTER — EPP levers will be inert"; fi
    else
      echo "CANNOT FIX: not root. Re-run as: sudo bash run.sh"
    fi
  else
    echo "OK: governor=$GOV0 — EPP lever should work"
  fi

  # 4. Current EPP values
  EPP_VAL0="$(cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference 2>/dev/null || echo N/A)"
  EPP_VALS="$(sort -u /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference 2>/dev/null | tr '\n' ',')"
  echo "epp(cpu0)=$EPP_VAL0  all_unique=${EPP_VALS%,}"

  # 5. Competing power management services (expanded list)
  echo "power_services:"
  for s in tuned power-profiles-daemon tlp thermald auto-cpufreq gamemode \
           system76-power cpupower acpid laptop-mode-tools; do
    # NB: systemctl prints one status word but exits non-zero for inactive/missing
    # units — do NOT chain `|| echo`, it appends a spurious second line.
    ST="$(systemctl is-active "$s.service" 2>/dev/null)";  ST="${ST:-unknown}"
    EN="$(systemctl is-enabled "$s.service" 2>/dev/null)"; EN="${EN:-unknown}"
    echo "  $s: active=$ST enabled=$EN"
    if [ "$ST" = "active" ]; then
      echo "  WARNING: $s is running and may override EPP/governor"
      POWER_WARNINGS=$((POWER_WARNINGS+1))
    fi
  done

  # 6. GNOME power profile (via D-Bus, if available)
  if have busctl; then
    PP="$(busctl get-property net.hadess.PowerProfiles /net/hadess/PowerProfiles \
          net.hadess.PowerProfiles ActiveProfile 2>/dev/null | awk '{print $2}' | tr -d '"')"
    echo "gnome_power_profile=${PP:-N/A}"
  fi

  # 7. Kernel command line power-related params
  echo "kernel_cmdline_power:"
  for kw in intel_pstate processor.max_cstate intel_idle.max_cstate \
            cpufreq.default_governor amd_pstate nowatchdog nohz; do
    MATCH="$(grep -oE "${kw}[^ ]*" /proc/cmdline 2>/dev/null)"
    [ -n "$MATCH" ] && echo "  $MATCH"
  done
  # If nothing matched, say so
  grep -qE '(intel_pstate|processor\.max_cstate|cpufreq\.default_governor|amd_pstate)' /proc/cmdline 2>/dev/null \
    || echo "  (none found — defaults in effect)"

  # 8. Relevant sysctl values
  echo "sysctl_power:"
  for k in vm.dirty_ratio vm.dirty_background_ratio vm.swappiness \
           kernel.sched_energy_aware kernel.nmi_watchdog; do
    V="$(sysctl -n "$k" 2>/dev/null || echo N/A)"
    echo "  $k=$V"
  done

  # 9. Summary
  if [ $POWER_WARNINGS -eq 0 ]; then
    echo "POWER AUDIT OK: no unusual power handling detected"
  else
    echo "POWER AUDIT: $POWER_WARNINGS warning(s) found — review above"
  fi
  echo '```'
} >> "$REPORT"
say "power audit: $POWER_WARNINGS warning(s)"

# --- toolchain + build deps (try every package manager, then build) ----------
if ! have cargo; then
  run "install rust" rustup.log bash -c 'curl -sSf https://sh.rustup.rs | sh -s -- -y'
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
fi
for pm in "apt-get install -y libdbus-1-dev pkg-config build-essential" \
          "pacman -S --noconfirm dbus pkgconf base-devel" \
          "dnf install -y dbus-devel pkgconf-pkg-config gcc"; do
  set -- $pm; have "$1" && { run "deps via $1" "deps-$1.log" $SUDO $pm; break; }
done

run "cargo build --release" build.log cargo build --workspace --release
run "cargo test"            test.log  cargo test  --workspace
OPTID="$ROOT/target/release/optid"; OPTCTL="$ROOT/target/release/optctl"; RUSHB="$ROOT/target/release/rushbench"
run "optid --once (smoke)"  smoke.log "$OPTID" --once

# --- optid pin sanity (catches the resolved_floors:-1 / class_mismatch bug) --
run "optid apply pin sanity" pin.log bash -c "
  $SUDO '$OPTID' --apply --interval-sec 2 --state-dir /tmp/optid-pin & p=\$!; sleep 3
  '$OPTCTL' --state-dir /tmp/optid-pin pin --global latency-critical 2>/dev/null
  sleep 3; '$OPTCTL' --state-dir /tmp/optid-pin status --json 2>/dev/null
  $SUDO kill -TERM \$p 2>/dev/null"
grep -q '\-1' "$LOGS/pin.log" 2>/dev/null && say "NOTE pin shows -1 floors → class pinning likely still broken (see logs/pin.log)"

# --- benchmarks: matrix harness stops competing daemons + restores on exit ---
runroot "bench host-v2" benchv2.log $SUDO bash "$ROOT/tools/bench-optid-host-v2.sh" $APPLY --iter 3
runroot "bench matrix"  matrix.log  $SUDO bash "$ROOT/tools/bench-optid-matrix.sh" $APPLY --iter 3 --out "$OUT/evidence"
run     "rushbench"     rushbench.log $SUDO "$RUSHB" matrix --ac-ok

# --- optional image build (best-effort; never fatal) -------------------------
if have mkosi; then runroot "build mkosi disk.raw" mkosi.log $SUDO bash "$ROOT/tools/build-mkosi-image.sh"
else say "SKIP mkosi (not installed) — image build skipped"; fi
[ -f "$ROOT/build/disk.raw" ] && run "validate uefi boot" uefiboot.log bash "$ROOT/tools/validate-uefi-boot.sh" "$ROOT/build/disk.raw"

# --- summary -----------------------------------------------------------------
{
  echo; echo "## Result summary"
  echo "OK:   $(grep -c '^- OK '   "$REPORT")"
  echo "FAIL: $(grep -c '^- FAIL ' "$REPORT")"
  echo "SKIP: $(grep -c '^- SKIP ' "$REPORT")"
  echo "power_warnings: $POWER_WARNINGS"
  echo "logs: $LOGS"
  echo "evidence csv: $(ls "$OUT"/evidence/*/results.csv 2>/dev/null | tail -1 || echo none)"
} >> "$REPORT"
say "DONE — read $REPORT"
cat "$REPORT"
