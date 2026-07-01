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
cat > "${EXTRA_DIR}/usr/libexec/testos-usb-mount" << 'EOF'
#!/usr/bin/env bash
# Mount the USB's ESP at /run/testos/usb so testos-runner can find the bench list
# and write results back to it.
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

mkdir -p /run/testos/usb

# Try to find a partition with the given label.
PART=$(blkid -t LABEL="$LABEL" -o device 2>/dev/null | head -1 || true)

if [[ -z "$PART" ]]; then
    echo "testos-usb-mount: no partition with label '$LABEL' found" >&2
    echo "  Available partitions:"
    blkid 2>/dev/null || true
    exit 1
fi

echo "testos-usb-mount: mounting $PART at /run/testos/usb"
mount -t vfat "$PART" /run/testos/usb -o rw,flush,umask=0000
sync
echo "testos-usb-mount: mounted successfully"
EOF
chmod +x "${EXTRA_DIR}/usr/libexec/testos-usb-mount"

# testos-runner.service — starts the runner on tty1.
cat > "${EXTRA_DIR}/usr/lib/systemd/system/testos-runner.service" << 'EOF'
[Unit]
Description=testOS - benchmark runner
After=testos-usb-mount.service network-online.target
Wants=testos-usb-mount.service
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

# Suppress the normal getty on tty1 (testos-runner takes it over).
ln -sf /usr/lib/systemd/system/testos-runner.service "${EXTRA_DIR}/etc/systemd/system/getty.target.wants/testos-runner.service"

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
