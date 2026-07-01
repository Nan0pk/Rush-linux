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
CLEAN=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --clean|-c) CLEAN=true; shift ;;
        --help|-h)
            echo "Usage: $0 [--clean]"
            exit 0
            ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MKOSI_DIR="${REPO_ROOT}/mkosi"
EXTRA_DIR="${MKOSI_DIR}/mkosi.extra"
VERSION="$(cat "${REPO_ROOT}/VERSION" 2>/dev/null || echo "0.7.0-beta.1")"

echo "════════════════════════════════════════════════════"
echo "  testOS Builder"
echo "════════════════════════════════════════════════════"
echo "  Version:  ${VERSION}"
echo "  Clean:    ${CLEAN}"
echo ""

# ── Step 1: Build all workspace binaries (including testos-runner) ──
echo ">> [1/6] Building workspace in release mode..."
cd "${REPO_ROOT}"
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

# systemd units
install -m0644 "${REPO_ROOT}/packaging/systemd/optid.service" "${EXTRA_DIR}/usr/lib/systemd/system/optid.service"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid-apply.service" "${EXTRA_DIR}/usr/lib/systemd/system/optid-apply.service"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid-tmpfiles.conf" "${EXTRA_DIR}/usr/lib/tmpfiles.d/optid.conf"

# systemd presets
cat > "${EXTRA_DIR}/usr/lib/systemd/system-preset/00-rush.preset" << 'EOF'
enable optid.service
enable optid-boot-assess.service
enable systemd-networkd.service
enable systemd-resolved.service
enable systemd-oomd.service
enable nftables.service
enable testos-usb-mount.service
enable testos-runner.service
EOF

# Boot assessment
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

# Enable services via symlinks
ln -sf /usr/lib/systemd/system/optid.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/optid.service"
ln -sf /usr/lib/systemd/system/optid-boot-assess.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/optid-boot-assess.service"
ln -sf /usr/lib/systemd/system/systemd-networkd.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/systemd-networkd.service"
ln -sf /usr/lib/systemd/system/systemd-resolved.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/systemd-resolved.service"
ln -sf /usr/lib/systemd/system/systemd-oomd.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/systemd-oomd.service"
ln -sf /usr/lib/systemd/system/nftables.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/nftables.service"

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

# OS metadata
cat > "${EXTRA_DIR}/etc/os-release" << EOF
NAME="testOS"
VERSION="${VERSION}"
ID=testos
ID_LIKE=arch
VERSION_ID="$(echo "${VERSION}" | sed 's/-.*//')"
PRETTY_NAME="testOS (Rush Linux ${VERSION})"
HOME_URL="https://github.com/Nan0pk/Rush-linux"
BUG_REPORT_URL="https://github.com/Nan0pk/Rush-linux/issues"
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
cat > "${EXTRA_DIR}/usr/lib/systemd/system/testos-usb-mount.service" << 'EOF'
[Unit]
Description=testOS - mount USB ESP partition at /run/testos/usb
DefaultDependencies=no
After=local-fs-pre.target
Before=local-fs.target
# No ConditionKernelCommandLine: this service file only exists in the
# testOS image, so it only runs when testOS boots. The condition was
# preventing the service from starting because systemd's bare-word match
# for 'testos.usb_label' doesn't match 'testos.usb_label=RUSHESP'
# (assignment form) on systemd 261.

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/libexec/testos-usb-mount

[Install]
WantedBy=multi-user.target
EOF

# testos-usb-mount script
#
# This script is critical: if the mount fails or mounts the wrong thing,
# the runner will either fail to start or silently write results to tmpfs
# (which evaporates on reboot, losing all benchmark data). Two robustness
# measures:
#   1. udev settle + retry loop for blkid (fixes first-boot race where udev
#      hasn't populated partition labels yet — root cause of the first-boot
#      root-prompt issue)
#   2. Post-mount verification: bench-list.toml must exist at the expected
#      path, proving we mounted the real ESP (not an empty/wrong partition)
#   3. If verification fails, unmount and exit 1 so the runner fails loudly
#      with diagnostics instead of silently writing to tmpfs
cat > "${EXTRA_DIR}/usr/libexec/testos-usb-mount" << 'EOF'
#!/usr/bin/env bash
set -euo pipefail

# Parse testos.usb_label= from the kernel command line.
LABEL=""
for arg in $(cat /proc/cmdline); do
    case "$arg" in
        testos.usb_label=*) LABEL="${arg#testos.usb_label=}" ;;
    esac
done

if [[ -z "$LABEL" ]]; then
    echo "testos-usb-mount: no testos.usb_label= on kernel command line" >&2
    exit 1
fi

# Wait for udev to settle so blkid can see partition labels. On first boot,
# udev may not have processed the USB's partitions yet, causing blkid to
# return empty — this is the root cause of the first-boot root-prompt issue.
udevadm settle --timeout=10 2>/dev/null || true

mkdir -p /run/testos/usb

# Find the partition by label, with retries. On first boot the label may
# not be visible to blkid until udev finishes processing the block devices.
PART=""
for attempt in 1 2 3 4 5; do
    PART=$(blkid -t LABEL="$LABEL" -o device 2>/dev/null | head -1 || true)
    if [[ -n "$PART" ]]; then
        echo "testos-usb-mount: found partition '$LABEL' at $PART (attempt $attempt)"
        break
    fi
    echo "testos-usb-mount: attempt $attempt - partition '$LABEL' not found yet, retrying..." >&2
    # Force udev to re-scan block devices and re-settle
    udevadm trigger --subsystem-match=block 2>/dev/null || true
    udevadm settle --timeout=5 2>/dev/null || true
    sleep 2
done

if [[ -z "$PART" ]]; then
    echo "testos-usb-mount: no partition with label '$LABEL' found after 5 attempts" >&2
    echo "  Available partitions (blkid):"
    blkid 2>/dev/null || true
    echo "  Block devices (lsblk):"
    lsblk -o NAME,SIZE,TYPE,FSTYPE,LABEL 2>/dev/null || true
    exit 1
fi

echo "testos-usb-mount: mounting $PART at /run/testos/usb"
# Mount options:
#   umask=0000 : all files world-writable (FAT32 has no real permissions)
#   flush      : write data more often (safer for USB, reduces data loss on unplug)
#   utf8       : handle non-ASCII filenames
if ! mount -t vfat "$PART" /run/testos/usb -o rw,flush,umask=0000,utf8; then
    echo "testos-usb-mount: mount failed for $PART" >&2
    exit 1
fi

sync

# CRITICAL: verify we mounted the real ESP, not an empty/wrong partition.
# The bench-list.toml is copied to the ESP at build time (via mkosi repart
# CopyFiles=/boot:/). If it's not here, we mounted the wrong thing — unmount
# and fail so the runner fails loudly instead of silently writing results
# to a useless mount point (which is what caused the "results not found"
# bug: the runner wrote to tmpfs because the mount silently failed).
if [[ ! -f /run/testos/usb/testos/bench-list.toml ]]; then
    echo "testos-usb-mount: MOUNTED $PART BUT bench-list.toml NOT FOUND at /run/testos/usb/testos/bench-list.toml" >&2
    echo "  This means we mounted the wrong partition or the ESP is corrupt." >&2
    echo "  Contents of /run/testos/usb:" >&2
    ls -la /run/testos/usb/ >&2 || true
    echo "  Unmounting and failing so the runner reports the error loudly." >&2
    umount /run/testos/usb 2>/dev/null || true
    exit 1
fi

echo "testos-usb-mount: mounted successfully, bench-list.toml verified"
EOF
chmod +x "${EXTRA_DIR}/usr/libexec/testos-usb-mount"

# testos-runner.service — starts the runner on tty1.
cat > "${EXTRA_DIR}/usr/lib/systemd/system/testos-runner.service" << 'EOF'
[Unit]
Description=testOS - benchmark runner
After=testos-usb-mount.service network-online.target
Wants=testos-usb-mount.service
# Stop the default getty on tty1 so it doesn't race with the runner for
# the console. Without this, if the runner fails (e.g. mount issue), the
# user sees a login prompt from getty@tty1 instead of the runner's
# diagnostic output — making it look like the runner never started.
# When the runner exits, systemd restarts getty@tty1 (conflict is gone).
Conflicts=getty@tty1.service
# No ConditionKernelCommandLine: this service file only exists in the
# testOS image. The condition 'testos.runner' was not matching
# 'testos.runner=1' (assignment form) on systemd 261, causing the
# service to be skipped and the user to get a login prompt instead
# of the benchmark menu.

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

[Install]
WantedBy=multi-user.target
EOF

# Enable testos services via symlinks
ln -sf /usr/lib/systemd/system/testos-usb-mount.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/testos-usb-mount.service"
ln -sf /usr/lib/systemd/system/testos-runner.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/testos-runner.service"

# Explicitly disable getty@tty1 so it can't race the runner for the console.
# The runner service has Conflicts=getty@tty1.service, but that only stops
# getty@tty1 when the runner is *starting* — it doesn't prevent getty@tty1
# from being wanted by getty.target at boot. We mask it here so systemd
# never even tries to start it on tty1.
mkdir -p "${EXTRA_DIR}/etc/systemd/system"
ln -sf /dev/null "${EXTRA_DIR}/etc/systemd/system/getty@tty1.service"

echo "   Done."
echo ""

# ── Step 4: Clean if requested ───────────────────────────────────
if [[ "${CLEAN}" == true ]]; then
    echo ">> [4/6] Cleaning previous build artifacts..."
    rm -rf "${REPO_ROOT}/build"
    rm -rf "${MKOSI_DIR}/.mkosi-private"
    echo "   Done."
    echo ""
else
    echo ">> [4/6] Skipping clean (--clean not specified)."
    echo ""
fi

# ── Step 5: Invoke mkosi with the testos profile ─────────────────
echo ">> [5/6] Invoking mkosi build (profile: testos)..."
cd "${MKOSI_DIR}"

MKOSI_ARGS=(
    --profile="testos"
    --force
)

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
