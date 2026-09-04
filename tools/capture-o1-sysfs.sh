#!/usr/bin/env bash
# Capture the real kernel sysfs layout that O1's reporter reads.
#
# O1's first two cold-verification defects both came from a hand-written test
# fixture that encoded the same wrong assumption as the code: a wakeup file
# named `total_time` (the kernel exports `total_time_ms`) and a `runtime_status`
# value set that omitted `unsupported`. A fixture written from a real capture
# cannot make either mistake, so this script produces one.
#
# The output is a flat, sorted, diffable text format read by
# `crates/optid/src/runtime_observability.rs` tests through `include_str!`:
#
#   # comment / provenance header
#   D<TAB><directory><TAB><entry>      one directory entry
#   F<TAB><path><TAB><value>           one file, newlines escaped as \n
#
# Nothing is written outside standard output. No root is required; a file the
# invoking user cannot read is recorded as a `U` line so the fixture reflects
# what an unprivileged reporter actually sees.
#
# Usage:
#   bash tools/capture-o1-sysfs.sh                 # representative subset
#   bash tools/capture-o1-sysfs.sh --full          # every device, unfiltered

set -uo pipefail

MODE="representative"
case "${1:-}" in
    --full) MODE="full" ;;
    --representative | "") MODE="representative" ;;
    *)
        echo "Usage: bash tools/capture-o1-sysfs.sh [--representative|--full]" >&2
        exit 2
        ;;
esac

# Files the reporter reads, per surface. Keeping the list here rather than in
# the Rust test means the capture is a record of the kernel, not a mirror of
# the code's current guesses.
WAKEUP_FILES=(name event_count wakeup_count total_time_ms)
PM_FILES=(runtime_status control runtime_active_time runtime_suspended_time pm_qos_resume_latency_us)
CPUIDLE_FILES=(name time usage)
BACKLIGHT_FILES=(brightness actual_brightness max_brightness)

emit_dir() { printf 'D\t%s\t%s\n' "$1" "$2"; }

emit_file() {
    local path="$1"
    if [[ ! -e "$path" ]]; then
        return
    fi
    if [[ ! -r "$path" ]]; then
        printf 'U\t%s\n' "$path"
        return
    fi
    local value
    if ! value="$(cat -- "$path" 2>/dev/null)"; then
        printf 'U\t%s\n' "$path"
        return
    fi
    # Collapse embedded newlines into the two-character escape the parser
    # understands, so one file is always exactly one line.
    value="${value//$'\n'/\\n}"
    printf 'F\t%s\t%s\n' "$path" "$value"
}

echo "# O1 sysfs capture — real kernel layout for the runtime observability fixture"
echo "# generated-by: tools/capture-o1-sysfs.sh --$MODE"
echo "# host-kernel: $(uname -sr)"
echo "# host-arch: $(uname -m)"
echo "# captured-utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "# privileged: $([[ "$(id -u)" == 0 ]] && echo yes || echo 'no — unprivileged, as the reporter runs')"
if [[ "$MODE" == representative ]]; then
    echo "# selection: first 3 /sys/class/wakeup sources; one device per"
    echo "#            (bus, runtime_status) pair across pci/usb/platform/i2c/hid;"
    echo "#            cpu0 cpuidle states; every backlight, scsi_host and nvme node."
else
    echo "# selection: unfiltered — every node the reporter enumerates."
fi
echo "# format: D<TAB>dir<TAB>entry | F<TAB>path<TAB>value | U<TAB>path (unreadable)"

# ── wakeup sources ───────────────────────────────────────────────────────────
WAKEUP_ROOT=/sys/class/wakeup
if [[ -d "$WAKEUP_ROOT" ]]; then
    mapfile -t sources < <(find "$WAKEUP_ROOT" -mindepth 1 -maxdepth 1 | sort -V)
    if [[ "$MODE" == representative ]]; then
        sources=("${sources[@]:0:3}")
    fi
    for source in "${sources[@]}"; do
        emit_dir "$WAKEUP_ROOT" "$source"
        for file in "${WAKEUP_FILES[@]}"; do
            emit_file "$source/$file"
        done
    done
fi

# ── per-device runtime PM ────────────────────────────────────────────────────
for bus in pci usb platform i2c hid; do
    root="/sys/bus/$bus/devices"
    [[ -d "$root" ]] || continue
    seen=""
    mapfile -t devices < <(find "$root" -mindepth 1 -maxdepth 1 | sort)
    for device in "${devices[@]}"; do
        status_path="$device/power/runtime_status"
        [[ -r "$status_path" ]] || continue
        if [[ "$MODE" == representative ]]; then
            state="$(cat -- "$status_path" 2>/dev/null)"
            case " $seen " in
                *" $state "*) continue ;;
            esac
            seen="$seen $state"
        fi
        emit_dir "$root" "$device"
        for file in "${PM_FILES[@]}"; do
            emit_file "$device/power/$file"
        done
    done
done

# ── cpuidle residency ────────────────────────────────────────────────────────
CPU_ROOT=/sys/devices/system/cpu
if [[ -d "$CPU_ROOT" ]]; then
    mapfile -t cpus < <(find "$CPU_ROOT" -mindepth 1 -maxdepth 1 -name 'cpu[0-9]*' | sort -V)
    if [[ "$MODE" == representative ]]; then
        cpus=("${cpus[@]:0:1}")
    fi
    for cpu in "${cpus[@]}"; do
        idle="$cpu/cpuidle"
        [[ -d "$idle" ]] || continue
        emit_dir "$CPU_ROOT" "$cpu"
        mapfile -t states < <(find "$idle" -mindepth 1 -maxdepth 1 | sort -V)
        for state in "${states[@]}"; do
            emit_dir "$idle" "$state"
            for file in "${CPUIDLE_FILES[@]}"; do
                emit_file "$state/$file"
            done
        done
    done
fi

# ── PM QoS (debugfs; usually root-only) ──────────────────────────────────────
emit_file /sys/kernel/debug/pm_qos/cpu_latency_constraints

# ── storage link-power surfaces ──────────────────────────────────────────────
SCSI_ROOT=/sys/class/scsi_host
if [[ -d "$SCSI_ROOT" ]]; then
    mapfile -t hosts < <(find "$SCSI_ROOT" -mindepth 1 -maxdepth 1 | sort -V)
    for host in "${hosts[@]}"; do
        emit_dir "$SCSI_ROOT" "$host"
        emit_file "$host/link_power_management_policy"
    done
fi

PCI_ROOT=/sys/bus/pci/devices
if [[ -d "$PCI_ROOT" ]]; then
    mapfile -t devices < <(find "$PCI_ROOT" -mindepth 1 -maxdepth 1 | sort)
    for device in "${devices[@]}"; do
        [[ -e "$device/link/l1_aspm" ]] || continue
        emit_dir "$PCI_ROOT" "$device"
        emit_file "$device/link/l1_aspm"
    done
fi

NVME_ROOT=/sys/class/nvme
if [[ -d "$NVME_ROOT" ]]; then
    mapfile -t controllers < <(find "$NVME_ROOT" -mindepth 1 -maxdepth 1 | sort -V)
    for controller in "${controllers[@]}"; do
        emit_dir "$NVME_ROOT" "$controller"
        emit_file "$controller/power/runtime_status"
        emit_file "$controller/device/power/runtime_status"
    done
fi

# ── backlights ───────────────────────────────────────────────────────────────
BACKLIGHT_ROOT=/sys/class/backlight
if [[ -d "$BACKLIGHT_ROOT" ]]; then
    mapfile -t panels < <(find "$BACKLIGHT_ROOT" -mindepth 1 -maxdepth 1 | sort -V)
    for panel in "${panels[@]}"; do
        emit_dir "$BACKLIGHT_ROOT" "$panel"
        for file in "${BACKLIGHT_FILES[@]}"; do
            emit_file "$panel/$file"
        done
    done
fi
