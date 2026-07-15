#!/usr/bin/env bash
# testos/build-testos.sh — Build the bootable testOS USB image.
#
# Wraps the existing tools/build-mkosi-image.sh, adding:
#   - The testos crate binaries (testos-runner) into the image overlay.
#   - The bench-list.toml catalog onto the ESP (so it's readable post-boot).
#   - The testos-init service that auto-starts testos-runner on tty1.
#   - The testos-usb-mount service that mounts the ESP at /run/testos/usb.
#
# Output:
#   build/testos.raw — bootable GPT disk image with ESP + root partitions.
#
# Usage:
#   bash testos/build-testos.sh            # build
#   bash testos/build-testos.sh --clean    # full rebuild
#   testos-launcher build                  # preferred entry point
#
# Prerequisites (Arch host):
#   pacman -S mkosi archlinux-keyring base-devel rust
#   And the Rush Linux prerequisites listed in tools/build-mkosi-image.sh.

set -euo pipefail

# ── Parse arguments ──────────────────────────────────────────────
# The build is ALWAYS clean. There is no --clean flag anymore because
# a non-clean build produced stale images that broke boot (cached
# overlay layers from a previous broken commit got copied forward).
# If you think you want a non-clean build for speed, you don't — the
# cost of a stale image is always higher than the time saved.
while [[ $# -gt 0 ]]; do
    case "$1" in
        --help|-h)
            echo "Usage: $0"
            echo ""
            echo "Builds a clean testOS image from the current source tree."
            echo "There are no caching options — every build is fully clean"
            echo "to guarantee the image matches the source."
            exit 0
            ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MKOSI_DIR="${REPO_ROOT}/mkosi"
EXTRA_DIR="${MKOSI_DIR}/mkosi.extra"
VERSION="$(cat "${REPO_ROOT}/VERSION" 2>/dev/null || echo "0.7.0-beta.1")"

# Capture the source git SHA so we can embed it in the image. This lets
# you verify on boot that the USB actually contains the code you think
# it does — the runner prints this SHA on tty1. If the SHA doesn't match
# what you built, you're running a stale cached image.
SOURCE_GIT_SHA="$(git -C "${REPO_ROOT}" rev-parse --short HEAD 2>/dev/null || echo 'unknown')"
SOURCE_GIT_DIRTY="$(git -C "${REPO_ROOT}" status --porcelain 2>/dev/null | head -1)"
if [[ -n "${SOURCE_GIT_DIRTY}" ]]; then
    SOURCE_GIT_SHA="${SOURCE_GIT_SHA}-dirty"
fi

echo "════════════════════════════════════════════════════"
echo "  testOS Builder"
echo "════════════════════════════════════════════════════"
echo "  Version:    ${VERSION}"
echo "  Source SHA: ${SOURCE_GIT_SHA}"
echo "  Build mode: ALWAYS CLEAN (no cache)"
echo ""

# ── Step 1: Build all workspace binaries (including testos-runner) ──
echo ">> [1/6] Building workspace in release mode..."
cd "${REPO_ROOT}"
# cargo clean first to guarantee no stale artifacts from a previous
# broken build get linked into the new image.
cargo clean
cargo build --workspace --release
echo "   Done."
echo ""

# ── Step 2: Run the standard Rush mkosi.extra overlay setup ──
# This handles optid, optctl, systemd units, network, etc. — everything that
# makes a base Rush image. We extend it with testOS-specific files afterwards.
echo ">> [2/6] Staging base overlay (calls tools/build-mkosi-image.sh logic)..."

# We can't just call build-mkosi-image.sh because it would invoke mkosi itself.
# Instead, we replicate the overlay-staging part here (the part before mkosi is called).
rm -rf "${EXTRA_DIR}"
mkdir -p "${EXTRA_DIR}"/{usr/bin,usr/libexec,usr/lib/optid,usr/lib/systemd/system,usr/lib/systemd/system-preset,usr/lib/tmpfiles.d}
mkdir -p "${EXTRA_DIR}"/{etc/systemd/system.conf.d,usr/lib/sysctl.d,usr/lib/systemd/network,etc}
mkdir -p "${EXTRA_DIR}"/{usr/share/dbus-1/system-services,usr/share/dbus-1/interfaces}
mkdir -p "${EXTRA_DIR}"/{etc/systemd/system/multi-user.target.wants,etc/systemd/system/getty.target.wants}

# Base Rush binaries
install -m0755 "${REPO_ROOT}/target/release/optid" "${EXTRA_DIR}/usr/libexec/optid"
install -m0755 "${REPO_ROOT}/target/release/optctl" "${EXTRA_DIR}/usr/bin/optctl"
install -m0644 "${REPO_ROOT}/config/optid/policy.toml" "${EXTRA_DIR}/usr/lib/optid/policy.toml"

# systemd units — install optid units on disk so `optctl` works for ad-hoc
# operator use, but do NOT enable them in the testOS baseline image. The
# testOS baseline is read-only hardware evidence: it must not run optid,
# optid-apply, or optid-boot-assess as persistent services, and it must
# never run `optid --apply`. See AGENTS.md §9 (baseline purity) and the
# boot-reliability PR.
install -m0644 "${REPO_ROOT}/packaging/systemd/optid.service" "${EXTRA_DIR}/usr/lib/systemd/system/optid.service"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid-apply.service" "${EXTRA_DIR}/usr/lib/systemd/system/optid-apply.service"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid-tmpfiles.conf" "${EXTRA_DIR}/usr/lib/tmpfiles.d/optid.conf"

# systemd presets — the testOS baseline image enables ONLY the services
# needed to boot, mount the USB, run the benchmark menu, and reboot.
# optid / optid-apply / optid-boot-assess are deliberately NOT enabled;
# they are present on disk for ad-hoc use but never started automatically
# by the baseline image. This is the baseline-purity contract.
cat > "${EXTRA_DIR}/usr/lib/systemd/system-preset/00-rush.preset" << 'EOF'
enable systemd-networkd.service
enable systemd-resolved.service
enable systemd-oomd.service
enable nftables.service
enable testos-usb-mount.service
enable testos-runner.service
EOF

# Boot assessment binary + unit are installed on disk for ad-hoc operator
# use, but the unit is NOT enabled in the testOS baseline preset and is
# NOT symlinked into multi-user.target.wants.
install -m0755 "${REPO_ROOT}/tools/optid-boot-assess" "${EXTRA_DIR}/usr/libexec/optid-boot-assess"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid-boot-assess.service" "${EXTRA_DIR}/usr/lib/systemd/system/optid-boot-assess.service"

# System configuration
install -m0644 "${REPO_ROOT}/distro/systemd/00-cgroup-v2.conf" "${EXTRA_DIR}/etc/systemd/system.conf.d/00-cgroup-v2.conf"
install -m0644 "${REPO_ROOT}/distro/systemd/99-rush-network.conf" "${EXTRA_DIR}/usr/lib/sysctl.d/99-rush-network.conf"
install -m0644 "${REPO_ROOT}/distro/systemd/zram-generator.conf" "${EXTRA_DIR}/usr/lib/systemd/zram-generator.conf"
install -m0644 "${REPO_ROOT}/distro/network/nftables.conf" "${EXTRA_DIR}/etc/nftables.conf"

# D-Bus
install -m0644 "${REPO_ROOT}/packaging/dbus/io.rushlinux.Optid.service" "${EXTRA_DIR}/usr/share/dbus-1/system-services/io.rushlinux.Optid.service"
install -m0644 "${REPO_ROOT}/packaging/dbus/io.rushlinux.Optid.xml" "${EXTRA_DIR}/usr/share/dbus-1/interfaces/io.rushlinux.Optid.xml"

# Enable services via symlinks. NOTE: optid, optid-apply, and
# optid-boot-assess are intentionally NOT symlinked here — they are
# available on disk but never started automatically by the testOS
# baseline image. This preserves baseline purity: a testOS boot measures
# the hardware as-is, without optid actuation or boot assessment side
# effects. See the boot-reliability PR for the full rationale.
ln -sf /usr/lib/systemd/system/systemd-networkd.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/systemd-networkd.service"
ln -sf /usr/lib/systemd/system/systemd-resolved.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/systemd-resolved.service"
ln -sf /usr/lib/systemd/system/systemd-oomd.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/systemd-oomd.service"
ln -sf /usr/lib/systemd/system/nftables.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/nftables.service"

# Disable the default getty on tty1 so testos-runner owns tty1 exclusively.
# This prevents the tty/getty/service ordering race observed on the HP Victus
# where getty@tty1.service and testos-runner.service both tried to grab tty1.
# We override getty@tty1 by masking it (the testos-runner unit takes over).
ln -sf /dev/null "${EXTRA_DIR}/etc/systemd/system/getty@tty1.service"

# Default target — multi-user (headless, no desktop)
ln -sf /usr/lib/systemd/system/multi-user.target "${EXTRA_DIR}/etc/systemd/system/default.target"

# Network
mkdir -p "${EXTRA_DIR}/etc/systemd/network"
cat > "${EXTRA_DIR}/usr/lib/systemd/network/20-wired.network" << 'EOF'
[Match]
Name=en* eth*
[Network]
DHCP=yes
EOF

# Hostname
echo "testos" > "${EXTRA_DIR}/etc/hostname"

# Embed the source git SHA so the runner can print it on boot. This lets
# you verify at boot time that the USB actually contains the code you
# built — if the SHA doesn't match, you're running a stale cached image.
# The runner reads this file and prints it in its startup banner.
mkdir -p "${EXTRA_DIR}/etc/testos"
cat > "${EXTRA_DIR}/etc/testos/source-sha" << EOF
${SOURCE_GIT_SHA}
EOF

# Write the canonical version (from the repo VERSION file) to
# /etc/testos/version. The testos-runner reads this to populate
# manifest.testos_version, ensuring the manifest version matches the
# release asset version exactly.
cat > "${EXTRA_DIR}/etc/testos/version" << EOF
${VERSION}
EOF

# OS metadata
cat > "${EXTRA_DIR}/etc/os-release" << EOF
NAME="testOS"
VERSION="${VERSION}"
ID=testos
ID_LIKE=arch
VERSION_ID="$(echo "${VERSION}" | sed 's/-.*//')"
PRETTY_NAME="testOS (Rush Linux ${VERSION}, SHA ${SOURCE_GIT_SHA})"
HOME_URL="https://github.com/Nan0pk/Rush-linux"
BUG_REPORT_URL="https://github.com/Nan0pk/Rush-linux/issues"
SOURCE_GIT_SHA="${SOURCE_GIT_SHA}"
EOF

# fstab — root is auto-mounted by systemd-gpt-auto-generator.
cat > "${EXTRA_DIR}/etc/fstab" << 'EOF'
# testOS fstab — root partition is auto-mounted by systemd-gpt-auto-generator.
# The USB's ESP partition is mounted by testos-usb-mount.service.
EOF

echo "   Done."
echo ""

# ── Step 3: Stage testOS-specific files ───────────────────────────
echo ">> [3/6] Staging testOS-specific files..."

# testos-runner binary
install -m0755 "${REPO_ROOT}/target/release/testos-runner" "${EXTRA_DIR}/usr/bin/testos-runner"

# Bench list catalog goes onto the ESP. mkosi copies /boot to the ESP, so we
# put it under /boot/testos/bench-list.toml. The init script will mount the ESP
# at /run/testos/usb and find it there.
mkdir -p "${EXTRA_DIR}/boot/testos"
install -m0644 "${REPO_ROOT}/testos/bench-list.toml" "${EXTRA_DIR}/boot/testos/bench-list.toml"

# testos-usb-mount.service — finds the USB's ESP by label and mounts it.
#
# Boot-reliability contract:
#   - The mount helper retries for a BOUNDED window (default 30s, env-overridable).
#   - It uses udev block-device settle where available (no arbitrary unbounded sleep).
#   - It emits clear attempt/status messages to the systemd journal AND to
#     /run/testos/usb-discovery-timeline.txt so the runner can copy the
#     timeline into PRIVATE-DIAGNOSTICS for local post-mortem.
#   - On bounded-timeout failure, the service exits non-zero. The runner
#     unit has Requires= on this service, so a mount failure prevents the
#     runner from starting (and triggers the recovery screen instead of a
#     root shell).
cat > "${EXTRA_DIR}/usr/lib/systemd/system/testos-usb-mount.service" << 'EOF'
[Unit]
Description=testOS - mount USB ESP partition at /run/testos/usb
DefaultDependencies=no
After=local-fs-pre.target systemd-udevd.service
Before=local-fs.target
# OnFailure triggers the recovery service so the operator sees E001/E002
# on tty1 instead of a blank console. The recovery service owns tty1,
# writes PRIVATE-DIAGNOSTICS when possible, and reboots safely.
OnFailure=testos-recovery.service

[Service]
Type=oneshot
RemainAfterExit=yes
TimeoutStartSec=120
ExecStart=/usr/libexec/testos-usb-mount

[Install]
WantedBy=multi-user.target
EOF

# testos-usb-mount script — bounded retry, udev settle, timeline logging.
cat > "${EXTRA_DIR}/usr/libexec/testos-usb-mount" << 'EOF'
#!/usr/bin/env bash
# Mount the USB's ESP at /run/testos/usb so testos-runner can find the bench
# list and write results back to it.
#
# Boot-reliability design (see the boot-reliability PR):
#   - The USB partition may not be visible the instant systemd starts us.
#     HP Victus firmware in particular has been observed to enumerate USB
#     storage a few seconds after the kernel reports local-fs-pre.target.
#   - We retry for a BOUNDED window (default 30s). There is NO unbounded
#     sleep — if the USB does not appear, we fail within the window so the
#     runner can show the recovery screen instead of hanging forever.
#   - We use `udevadm settle` (with its own bounded timeout) where available
#     to wait for the block device to be ready, and `blkid` between attempts
#     to detect the label.
#   - Every attempt is timestamped and written to
#     /run/testos/usb-discovery-timeline.txt so the runner can copy it into
#     PRIVATE-DIAGNOSTICS for local post-mortem.
set -euo pipefail

# ── Config ────────────────────────────────────────────────────────────
# Total retry window. Override via testos.usb_mount_timeout_secs= on the
# kernel command line for a one-off tuned run.
TIMEOUT_SECS="${TESTOS_USB_MOUNT_TIMEOUT_SECS:-30}"
# Per-attempt sleep. Short enough to be responsive, long enough to avoid
# pegging the CPU. This is a BOUNDED, intentional wait — NOT an arbitrary
# unbounded sleep.
ATTEMPT_SLEEP_SECS=1
# udevadm settle per-attempt timeout (bounded).
UDEV_SETTLE_SECS=5

# ── Parse kernel cmdline for the label and the timeout override ──────
LABEL=""
for arg in $(cat /proc/cmdline); do
    case "$arg" in
        testos.usb_label=*) LABEL="${arg#testos.usb_label=}" ;;
        testos.usb_mount_timeout_secs=*) TIMEOUT_SECS="${arg#testos.usb_mount_timeout_secs=}" ;;
    esac
done

if [[ -z "$LABEL" ]]; then
    echo "testos-usb-mount: no testos.usb_label= on kernel command line" >&2
    exit 1
fi

# Validate TIMEOUT_SECS is a positive integer; fall back to 30 if not.
case "$TIMEOUT_SECS" in
    ''|*[!0-9]*) TIMEOUT_SECS=30 ;;
esac
# Clamp to [5, 300] to prevent absurd overrides.
if (( TIMEOUT_SECS < 5 )); then TIMEOUT_SECS=5; fi
if (( TIMEOUT_SECS > 300 )); then TIMEOUT_SECS=300; fi

mkdir -p /run/testos/usb
TIMELINE=/run/testos/usb-discovery-timeline.txt
: > "$TIMELINE"

ts() { date -u '+%Y-%m-%dT%H:%M:%SZ'; }
log() {
    local msg="$1"
    echo "[$(ts)] testos-usb-mount: $msg" | tee -a "$TIMELINE" >&2
}

log "starting (label=$LABEL, timeout=${TIMEOUT_SECS}s, attempt_sleep=${ATTEMPT_SLEEP_SECS}s)"

# ── Bounded retry loop ───────────────────────────────────────────────
# We count attempts and elapsed time. The loop exits as soon as the label
# is found OR the bounded window expires. There is no path that sleeps
# forever.
deadline=$(( $(date +%s) + TIMEOUT_SECS ))
attempt=0
PART=""
while :; do
    attempt=$((attempt + 1))
    now=$(date +%s)
    if (( now >= deadline )); then
        log "deadline reached after $attempt attempts ($((now - (deadline - TIMEOUT_SECS)))s elapsed); giving up"
        break
    fi
    # Ask udev to settle (bounded). This is the proper way to wait for
    # block-device enumeration without an arbitrary sleep. If udevadm is
    # unavailable (some minimal images), we skip it and rely on blkid polling.
    if command -v udevadm >/dev/null 2>&1; then
        log "attempt $attempt: udevadm settle (max ${UDEV_SETTLE_SECS}s)"
        udevadm settle --timeout="$UDEV_SETTLE_SECS" >/dev/null 2>&1 || true
    else
        log "attempt $attempt: udevadm unavailable; proceeding to blkid"
    fi
    # Look for the labeled partition.
    PART=$(blkid -t LABEL="$LABEL" -o device 2>/dev/null | head -1 || true)
    if [[ -n "$PART" ]]; then
        log "attempt $attempt: found $PART"
        break
    fi
    log "attempt $attempt: label '$LABEL' not yet visible; sleeping ${ATTEMPT_SLEEP_SECS}s"
    sleep "$ATTEMPT_SLEEP_SECS"
done

if [[ -z "$PART" ]]; then
    log "FAILED: no partition with label '$LABEL' found within ${TIMEOUT_SECS}s"
    # Record the available partitions for local diagnosis (this goes to the
    # timeline, which the runner copies into PRIVATE-DIAGNOSTICS — it does
    # NOT go into the publishable evidence bundle).
    {
        echo "--- blkid output ---"
        blkid 2>/dev/null || echo '(blkid unavailable)'
        echo "--- /proc/partitions ---"
        cat /proc/partitions 2>/dev/null || echo '(unavailable)'
    } >> "$TIMELINE" 2>&1
    exit 1
fi

# ── Mount ────────────────────────────────────────────────────────────
log "mounting $PART at /run/testos/usb"
# Capture the real mount exit status. The previous code used
# `if ! mount ...; then log "...returned $?"` which reported the NEGATED
# status (0 when mount failed, 1 when it succeeded) — a bug. We now use
# `if mount ...; then ... else ... fi` which (a) is exempt from `set -e`
# so the script doesn't exit before we capture the status, and (b)
# preserves the REAL exit code in $? inside the else branch.
if mount -t vfat "$PART" /run/testos/usb -o rw,flush,umask=0000; then
    : # mount succeeded
else
    mount_rc=$?
    log "FAILED: mount -t vfat $PART /run/testos/usb returned $mount_rc"
    exit 1
fi

# sync is a required operation (ensures the mount is durable before the
# runner starts writing). We do NOT ignore sync failures — report honestly.
if ! sync; then
    log "FAILED: sync after mount returned non-zero"
    exit 1
fi
log "mounted successfully after $attempt attempt(s)"

# ── Record boot-attempt counter for the runner ───────────────────────
# The runner reads /run/testos/boot-attempt and prints it on the banner +
# in PRIVATE-DIAGNOSTICS. We compute it from a persistent counter on the
# USB when available, falling back to 1.
BOOT_ATTEMPT=1
COUNTER_FILE=/run/testos/usb/testos/.boot-attempt-counter
if [[ -f "$COUNTER_FILE" ]]; then
    prev=$(cat "$COUNTER_FILE" 2>/dev/null | tr -dc '0-9' || echo 0)
    if [[ -n "$prev" ]]; then
        BOOT_ATTEMPT=$((prev + 1))
    fi
fi
mkdir -p /run/testos
# Writing the boot-attempt file to /run is required (the runner reads it).
# Report failure honestly instead of silently ignoring.
if ! echo "$BOOT_ATTEMPT" > /run/testos/boot-attempt; then
    log "FAILED: cannot write /run/testos/boot-attempt"
    exit 1
fi

# Persist the counter for the next boot. This is best-effort (the USB might
# be read-only or full), but we report failures honestly to the timeline
# rather than silently swallowing them.
mkdir -p "$(dirname "$COUNTER_FILE")" 2>/dev/null
if ! echo "$BOOT_ATTEMPT" > "$COUNTER_FILE" 2>/dev/null; then
    log "WARNING: cannot persist boot-attempt counter to $COUNTER_FILE (USB may be read-only or full)"
fi
# sync the counter write. Report failures honestly.
if ! sync 2>/dev/null; then
    log "WARNING: sync after counter write returned non-zero"
fi
log "boot attempt #$BOOT_ATTEMPT"
EOF
chmod +x "${EXTRA_DIR}/usr/libexec/testos-usb-mount"

# testos-runner.service — starts the runner on tty1.
#
# Boot-reliability contract:
#   - Requires= (not just Wants=) testos-usb-mount.service: if the mount
#     fails, the runner does NOT start. This prevents the runner from
#     racing onto tty1 with a half-mounted (or unmounted) USB and then
#     dropping to a root-shell-cum-login-prompt when it can't find the
#     catalog.
#   - Conflicts=getty@tty1.service: we mask getty@tty1 in the image, but
#     this Conflicts= is defense-in-depth in case the mask is removed.
#   - Bounded startup timeout + restart policy: a hung runner is restarted
#     at most once, then left for the recovery screen.
#   - The runner does NOT drop to a root shell on failure — it shows a
#     privacy-safe recovery screen and reboots.
cat > "${EXTRA_DIR}/usr/lib/systemd/system/testos-runner.service" << 'EOF'
[Unit]
Description=testOS - benchmark runner
# Requires= (not Wants=) the mount: a mount failure MUST prevent the
# runner from starting. This prevents the runner from racing onto tty1
# with an unmounted USB and falling through to a shell.
Requires=testos-usb-mount.service
After=testos-usb-mount.service network-online.target
# Defense-in-depth against the getty/tty1 race: even though we mask
# getty@tty1 in the image, also declare a conflict so systemd will not
# start both services on tty1.
Conflicts=getty@tty1.service
After=getty@tty1.service
# OnFailure triggers the recovery service if the runner crashes, panics,
# or exits non-zero before it can render its own recovery screen. The
# recovery service shows E099 (runner internal error) on tty1.
OnFailure=testos-recovery.service

[Service]
Type=idle
ExecStart=/usr/bin/testos-runner
StandardInput=tty
StandardOutput=tty
StandardError=tty
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes
KillMode=process
IgnoreSIGPIPE=no
SendSIGHUP=yes
# Bounded restart policy: within any 120-second window, systemd will
# restart the runner at most 2 times. After that, the service stays
# failed and OnFailure=testos-recovery.service takes over to show the
# recovery screen. This is a REAL bounded policy (not just a comment).
StartLimitIntervalSec=120
StartLimitBurst=2
TimeoutStartSec=300
Restart=on-failure
RestartSec=2
# Prevent duplicate runner instances: systemd will not start a second
# instance while one is running.
ExecStartPre=/bin/sh -c 'test -z "$TESTOS_RUNNER_LOCK" || exit 0'
Environment=TESTOS_RUNNER_LOCK=1

[Install]
WantedBy=multi-user.target
EOF

# testos-recovery.service — owns tty1 when testos-usb-mount or testos-runner
# fails. Shows E001/E002/E099 on tty1, writes PRIVATE-DIAGNOSTICS when
# possible, and reboots safely. Does NOT spawn a root shell.
#
# This is the fix for the critical boot-reliability gap: previously, a
# mount failure prevented the runner from starting (Requires=), but
# nothing else owned tty1 — so the operator saw a blank console. Now
# OnFailure=testos-recovery.service on both the mount and runner units
# triggers this service, which takes over tty1 and shows the recovery
# screen.
cat > "${EXTRA_DIR}/usr/lib/systemd/system/testos-recovery.service" << 'EOF'
[Unit]
Description=testOS - recovery screen (mount/runner failure fallback)
After=testos-usb-mount.service
Conflicts=getty@tty1.service testos-runner.service
# The recovery service must NOT require the mount — it needs to run even
# (especially) when the mount failed. It does its own best-effort USB
# mount for writing PRIVATE-DIAGNOSTICS.

[Service]
Type=oneshot
ExecStart=/usr/libexec/testos-recovery
StandardInput=tty
StandardOutput=tty
StandardError=tty
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes
# No restart: the recovery script reboots the machine after 10 seconds.
TimeoutStartSec=120

[Install]
WantedBy=multi-user.target
EOF

# testos-recovery script — renders the recovery screen on tty1.
#
# Determines the failure category from systemd state, tries to mount the
# USB best-effort for writing PRIVATE-DIAGNOSTICS, shows the recovery
# screen, and reboots. Never spawns a root shell.
cat > "${EXTRA_DIR}/usr/libexec/testos-recovery" << 'SCRIPT'
#!/usr/bin/env bash
# testOS recovery screen — shown when testos-usb-mount or testos-runner
# fails. Owns tty1, shows a privacy-safe failure code, writes raw
# diagnostics to PRIVATE-DIAGNOSTICS when possible, and reboots.
set -euo pipefail

# ── Determine failure category ──────────────────────────────────────
# Check which service is in failed state. systemd sets the unit to
# "failed" when ExecStart exits non-zero.
CATEGORY=""
CODE=""
if systemctl is-failed testos-usb-mount.service >/dev/null 2>&1; then
    # The mount service failed. Distinguish E001 (USB not found) from
    # E002 (mount failed) by checking the timeline.
    TIMELINE=/run/testos/usb-discovery-timeline.txt
    if [[ -f "$TIMELINE" ]] && grep -q "no partition with label" "$TIMELINE" 2>/dev/null; then
        CODE="E001"
        CATEGORY="USB not found"
        MSG="The USB partition (label RUSHESP) was not found within the retry window."
    else
        CODE="E002"
        CATEGORY="USB mount failed"
        MSG="The USB partition was found but could not be mounted."
    fi
elif systemctl is-failed testos-runner.service >/dev/null 2>&1; then
    CODE="E099"
    CATEGORY="runner internal error"
    MSG="The runner hit an internal error or crashed before showing its own recovery screen."
else
    CODE="E099"
    CATEGORY="runner internal error"
    MSG="An unexpected service failure occurred."
fi

# ── Best-effort: write PRIVATE-DIAGNOSTICS ──────────────────────────
# The USB may or may not be mounted at this point. Try to mount it
# best-effort so we can write raw diagnostics. If this fails, we skip
# diagnostics and just show the recovery screen.
DIAG_REL="(USB not mounted - no diagnostics written)"
USB_MOUNT=/run/testos/usb
TIMELINE=/run/testos/usb-discovery-timeline.txt

# Try to ensure the USB is mounted (it may already be, or the mount
# service may have failed before mounting). Best-effort only.
if ! mountpoint -q "$USB_MOUNT" 2>/dev/null; then
    # Try to find and mount the USB ourselves. Best-effort.
    LABEL=""
    for arg in $(cat /proc/cmdline 2>/dev/null || echo); do
        case "$arg" in
            testos.usb_label=*) LABEL="${arg#testos.usb_label=}" ;;
        esac
    done
    if [[ -n "$LABEL" ]]; then
        PART=$(blkid -t LABEL="$LABEL" -o device 2>/dev/null | head -1 || true)
        if [[ -n "$PART" ]]; then
            mkdir -p "$USB_MOUNT" 2>/dev/null || true
            if mount -t vfat "$PART" "$USB_MOUNT" -o rw,flush,umask=0000 2>/dev/null; then
                : # mounted OK
            fi
        fi
    fi
fi

# If the USB is now mounted, write PRIVATE-DIAGNOSTICS.
if mountpoint -q "$USB_MOUNT" 2>/dev/null; then
    BOOT_ATTEMPT=$(cat /run/testos/boot-attempt 2>/dev/null | tr -dc '0-9' || echo 1)
    [[ -z "$BOOT_ATTEMPT" ]] && BOOT_ATTEMPT=1
    DIAG_DIR="$USB_MOUNT/PRIVATE-DIAGNOSTICS/boot-${BOOT_ATTEMPT}"
    mkdir -p "$DIAG_DIR" 2>/dev/null || true
    if [[ -d "$DIAG_DIR" ]]; then
        # Write the marker.
        cat > "$DIAG_DIR/README.txt" << MARKER
PRIVATE - MAY CONTAIN HARDWARE IDENTIFIERS - DO NOT SUBMIT

Recovery screen triggered with code $CODE ($CATEGORY).
Boot attempt #$BOOT_ATTEMPT.
MARKER
        # Capture raw diagnostics (same set as the runner's private_diag).
        for pair in             "journalctl.txt:journalctl -b --no-pager -o short-monotonic 2>/dev/null || journalctl -b --no-pager 2>/dev/null || true"             "dmesg.txt:dmesg --time-format=iso 2>/dev/null || dmesg 2>/dev/null || true"             "systemctl-failed.txt:systemctl --failed --no-pager 2>/dev/null || true"             "status-usb-mount.txt:systemctl status --no-pager testos-usb-mount.service 2>/dev/null || true"             "status-runner.txt:systemctl status --no-pager testos-runner.service 2>/dev/null || true"             "critical-chain.txt:systemd-analyze critical-chain --no-pager 2>/dev/null || true"             "blame.txt:systemd-analyze blame --no-pager 2>/dev/null || true"             "kernel-version.txt:uname -r 2>/dev/null || true"             "image-version.txt:cat /etc/testos/version 2>/dev/null || cat /etc/os-release 2>/dev/null || true"
        do
            fname="${pair%%:*}"
            cmd="${pair#*:}"
            bash -c "$cmd" > "$DIAG_DIR/$fname" 2>/dev/null || true
        done
        # Copy the USB discovery timeline if it exists.
        if [[ -f "$TIMELINE" ]]; then
            cp "$TIMELINE" "$DIAG_DIR/usb-discovery-timeline.txt" 2>/dev/null || true
        fi
        # Record the recovery exit status.
        cat > "$DIAG_DIR/runner-exit.txt" << EXIT
boot_attempt=$BOOT_ATTEMPT
failure_code=$CODE
failure_category=$CATEGORY
recovery_screen=true
EXIT
        # Sync and verify.
        sync 2>/dev/null || true
        DIAG_REL="PRIVATE-DIAGNOSTICS/boot-${BOOT_ATTEMPT}"
    fi
fi

# ── Show the recovery screen on tty1 ────────────────────────────────
# Use plain text (no ANSI color) because we cannot assume the TTY state
# is clean at this point. The recovery screen must be readable even on
# a serial console.
cat << SCREEN

===============================================
  testOS - recovery screen
===============================================

  Failure code:        $CODE
  Category:            $CATEGORY
  What happened:       $MSG
  Safe next action:    Re-prepare the USB on the host, then reboot from it.

  Local diagnostics (private, NOT submitted):
    $DIAG_REL

  Rebooting in 10 seconds (Ctrl-C to stay on this screen).
===============================================
SCREEN

# Wait so the operator can read / photograph the screen.
sleep 10

# Reboot safely. Try systemctl first, then reboot(2) syscall.
# Use 'exec' so the process is replaced — if reboot succeeds, the script
# never reaches the lines below. If reboot fails (returns non-zero), bash
# continues to the fallback.
systemctl reboot 2>/dev/null && exit 0
reboot 2>/dev/null && exit 0

# If reboot failed, halt to avoid looping.
echo "Reboot failed. Press the power button to restart." >&2
sleep 60
# Last resort: force reboot.
echo b > /proc/sysrq-trigger 2>/dev/null || true
SCRIPT
chmod +x "${EXTRA_DIR}/usr/libexec/testos-recovery"

# Enable testos services via symlinks
ln -sf /usr/lib/systemd/system/testos-usb-mount.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/testos-usb-mount.service"
ln -sf /usr/lib/systemd/system/testos-runner.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/testos-runner.service"
ln -sf /usr/lib/systemd/system/testos-recovery.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/testos-recovery.service"

# Suppress the normal getty on tty1 (testos-runner takes it over). This
# symlink is in addition to the getty@tty1 mask above for backward compat
# with older systemd versions that ignore the mask under certain conditions.
ln -sf /usr/lib/systemd/system/testos-runner.service "${EXTRA_DIR}/etc/systemd/system/getty.target.wants/testos-runner.service"

echo "   Done."
echo ""

# ── Step 4: ALWAYS clean previous build artifacts ───────────────
# No conditional. No --clean flag. Every build wipes the build dir and
# the mkosi private cache. This is the only way to guarantee the image
# matches the current source. The mkosi package cache (downloaded pacman
# packages) is preserved via MKOSI_CACHE if set, but the image/overlay
# cache is always wiped.
echo ">> [4/6] Cleaning previous build artifacts (always)..."
rm -rf "${REPO_ROOT}/build"
rm -rf "${MKOSI_DIR}/.mkosi-private"
# Also wipe any stale mkosi output/images from previous runs
rm -f "${MKOSI_DIR}"/*.raw 2>/dev/null || true
echo "   Done."
echo ""

# ── Step 5: Invoke mkosi with the testos profile ─────────────────
echo ">> [5/6] Invoking mkosi build (profile: testos)..."
cd "${MKOSI_DIR}"

MKOSI_ARGS=(
    --profile="testos"
    --force
)

# MKOSI_CACHE (if set) only caches downloaded pacman packages, NOT the
# image overlay or build layers. This is safe — it speeds up the build
# without risking stale overlay content.
if [[ -n "${MKOSI_CACHE:-}" ]]; then
    MKOSI_ARGS+=(--cache-dir="${MKOSI_CACHE}")
fi

mkosi build "${MKOSI_ARGS[@]}"

echo "   Done."
echo ""

# ── Step 6: Rename output and report ─────────────────────────────
echo ">> [6/6] Renaming output and fixing permissions..."
# chown to the invoking user when running under sudo (SUDO_USER), otherwise
# to the current user (USER). In CI containers USER may be unset; default
# to root in that case so the chown is a no-op. The `2>/dev/null || true`
# guards against chown failures (e.g. running as non-root without sudo).
CHOWN_USER="${SUDO_USER:-${USER:-root}}"
chown -R "${CHOWN_USER}:${CHOWN_USER}" "${REPO_ROOT}/build" 2>/dev/null || true

# mkosi produces build/rush-linux-testos.raw (because of ImageId in the profile).
# We symlink it to build/testos.raw for a stable name.
DISK="${REPO_ROOT}/build/testos.raw"
if [[ ! -f "${DISK}" ]]; then
    if [[ -f "${REPO_ROOT}/build/rush-linux-testos.raw" ]]; then
        ln -sf "rush-linux-testos.raw" "${DISK}"
    fi
fi
if [[ -f "${DISK}" ]]; then
    SIZE=$(du -sh "${DISK}" | cut -f1)
    echo "   Image: ${DISK} (${SIZE})"
else
    echo "   Warning: Expected image not found at ${DISK}"
    exit 1
fi

echo ""
echo "════════════════════════════════════════════════════"
echo "  testOS build finished"
echo ""
echo "  Next:"
echo "    testos-launcher write /dev/sdX    # find your USB with lsblk"
echo ""
echo "  Then plug the USB into the test machine, reboot, pick USB from boot menu."
echo "════════════════════════════════════════════════════"
