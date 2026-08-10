#!/usr/bin/env bash
# tools/build-mkosi-image.sh — Build the bootable Arch-based Rush Linux base/operational image using mkosi.
#
# This is the primary whole-image build entry point for v0.5+. Product editions
# are now composed from the common server base plus system extensions via
# tools/build-edition-image.sh; this script directly builds only the unprofiled
# common server base and the operational LiveDev profile.
#
# Usage:
#   sudo bash tools/build-mkosi-image.sh                          # common server base
#   sudo bash tools/build-mkosi-image.sh --edition server        # same common base
#   sudo bash tools/build-mkosi-image.sh --edition livedev       # LiveDev (benchmark/CI)
#   sudo bash tools/build-mkosi-image.sh --edition server --clean
#
# Product edition example:
#   sudo tools/build-edition-image.sh --edition desktop --unsigned-development
#
# Prerequisites (Arch host):
#   pacman -S mkosi archlinux-keyring base-devel rust
#   Or on Debian/Ubuntu: apt-get download mkosi (see tools/env-setup.sh)
#
# Output:
#   build/rush-linux.raw — bootable GPT disk image with ESP + root partitions
#
# Validate:
#   tools/validate-uefi-boot.sh build/rush-linux.raw
#   tools/test-rollback.sh build/rush-linux.raw

set -euo pipefail

# ── Parse arguments ──────────────────────────────────────────────
EDITION="server"
CLEAN=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --edition|-e)
            EDITION="$2"
            shift 2
            ;;
        --clean|-c)
            CLEAN=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [--edition server|livedev] [--clean]"
            echo "Product editions: use tools/build-edition-image.sh --edition desktop|laptop|realtime-audio"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

# Product edition profiles are system-extension payloads, not whole-image
# overlays. Applying one here would replace the base Packages= list and can
# produce an unbootable image. Keep that architectural boundary fail-closed.
case "${EDITION}" in
    server|livedev)
        ;;
    desktop|laptop|realtime-audio)
        echo "Product edition '${EDITION}' must be composed from the common server base plus its system extension." >&2
        echo "Use: tools/build-edition-image.sh --edition ${EDITION} --unsigned-development" >&2
        exit 2
        ;;
    *)
        echo "Unsupported whole-image edition: ${EDITION} (expected server or livedev)" >&2
        exit 2
        ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MKOSI_DIR="${REPO_ROOT}/mkosi"
EXTRA_DIR="${MKOSI_DIR}/mkosi.extra"
VERSION="$(cat "${REPO_ROOT}/VERSION" 2>/dev/null || echo "0.5.0-beta.1")"
VERSION_ID="${VERSION%%-*}"

echo "════════════════════════════════════════════════════"
echo "  Rush Linux mkosi Builder"
echo "════════════════════════════════════════════════════"
echo "  Edition:  ${EDITION}"
echo "  Version:  ${VERSION}"
echo "  Clean:    ${CLEAN}"
echo ""

# ── Step 1: Compile host binaries ────────────────────────────────
echo ">> [1/5] Building optid and optctl in release mode..."
cargo build --workspace --release
echo "   Done."
echo ""

# ── Step 2: Re-create clean mkosi.extra overlay ─────────────────
echo ">> [2/5] Staging files in mkosi.extra..."
rm -rf "${EXTRA_DIR}"
mkdir -p "${EXTRA_DIR}"/{usr/bin,usr/libexec,usr/lib/optid,usr/lib/systemd/system,usr/lib/systemd/system-preset,usr/lib/tmpfiles.d}
mkdir -p "${EXTRA_DIR}"/{etc/systemd/system.conf.d,usr/lib/sysctl.d,usr/lib/systemd/network,etc}
mkdir -p "${EXTRA_DIR}"/{usr/share/dbus-1/system-services,usr/share/dbus-1/interfaces}
mkdir -p "${EXTRA_DIR}"/{etc/systemd/system/multi-user.target.wants,etc/systemd/system/getty.target.wants}

# Rust binaries
install -m0755 "${REPO_ROOT}/target/release/optid" "${EXTRA_DIR}/usr/libexec/optid"
install -m0755 "${REPO_ROOT}/target/release/optctl" "${EXTRA_DIR}/usr/bin/optctl"

# Config
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
EOF

# Boot assessment (v0.4 rollback support)
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

# Enable services via symlinks (for mkosi.extra overlay)
ln -sf /usr/lib/systemd/system/optid.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/optid.service"
ln -sf /usr/lib/systemd/system/optid-boot-assess.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/optid-boot-assess.service"
ln -sf /usr/lib/systemd/system/systemd-networkd.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/systemd-networkd.service"
ln -sf /usr/lib/systemd/system/systemd-resolved.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/systemd-resolved.service"
ln -sf /usr/lib/systemd/system/systemd-oomd.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/systemd-oomd.service"
ln -sf /usr/lib/systemd/system/nftables.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/nftables.service"
ln -sf /usr/lib/systemd/system/getty@.service "${EXTRA_DIR}/etc/systemd/system/getty.target.wants/getty@tty1.service"

# Default target
ln -sf /usr/lib/systemd/system/multi-user.target "${EXTRA_DIR}/etc/systemd/system/default.target"

# Network configuration
mkdir -p "${EXTRA_DIR}/etc/systemd/network"
cat > "${EXTRA_DIR}/usr/lib/systemd/network/20-wired.network" << 'EOF'
[Match]
Name=en* eth*
[Network]
DHCP=yes
EOF

# Hostname
echo "rush-linux" > "${EXTRA_DIR}/etc/hostname"

# OS metadata (dynamic version)
cat > "${EXTRA_DIR}/etc/os-release" << EOF
NAME="Rush Linux"
VERSION="${VERSION}"
ID=rush-linux
ID_LIKE=arch
VERSION_ID="${VERSION_ID}"
PRETTY_NAME="Rush Linux ${VERSION}"
HOME_URL="https://github.com/Nan0pk/Rush-linux"
BUG_REPORT_URL="https://github.com/Nan0pk/Rush-linux/issues"
EOF

# ── LiveDev edition: install rush-* tools + support libraries + units ─
if [[ "${EDITION}" == "livedev" ]]; then
    echo "   Staging LiveDev tools..."

    # Rush LiveDev Python tools (installed to /usr/bin)
    for tool in rush-exec rush-capture rush-autopilot rush-agent rush-livedev-autostart rush-livedev-runner rush-livedev-orchestrator; do
        install -m0755 "${REPO_ROOT}/tools/${tool}" "${EXTRA_DIR}/usr/bin/${tool}"
    done

    # Rush LiveDev Python support libraries (installed to /usr/lib/rush)
    mkdir -p "${EXTRA_DIR}/usr/lib/rush"
    for lib in rush_capture_lib.py rush_runner_lib.py rush_agent_lib.py rush_livedev_state.py rush_livedev_markers.py rush_livedev_submit.py; do
        install -m0644 "${REPO_ROOT}/tools/${lib}" "${EXTRA_DIR}/usr/lib/rush/${lib}"
    done

    # Evidence validator + schemas
    install -m0755 "${REPO_ROOT}/tools/validate-hwtest-evidence.py" "${EXTRA_DIR}/usr/bin/validate-hwtest-evidence"
    mkdir -p "${EXTRA_DIR}/usr/share/rush/schemas"
    for schema in "${REPO_ROOT}"/schemas/hwtest-*.schema.json; do
        install -m0644 "${schema}" "${EXTRA_DIR}/usr/share/rush/schemas/$(basename "${schema}")"
    done

    # LiveDev systemd units
    install -m0644 "${REPO_ROOT}/packaging/systemd/rush-capture.service" "${EXTRA_DIR}/usr/lib/systemd/system/rush-capture.service"
    install -m0644 "${REPO_ROOT}/packaging/systemd/rush-autopilot.service" "${EXTRA_DIR}/usr/lib/systemd/system/rush-autopilot.service"
    install -m0644 "${REPO_ROOT}/packaging/systemd/rush-livedev-autostart.service" "${EXTRA_DIR}/usr/lib/systemd/system/rush-livedev-autostart.service"
    install -m0644 "${REPO_ROOT}/packaging/systemd/rush-livedev-test.service" "${EXTRA_DIR}/usr/lib/systemd/system/rush-livedev-test.service"
    install -m0644 "${REPO_ROOT}/packaging/systemd/rush-livedev-failure.service" "${EXTRA_DIR}/usr/lib/systemd/system/rush-livedev-failure.service"

    # RUSH-DATA tmpfiles
    install -m0644 "${REPO_ROOT}/packaging/systemd/rush-livedev-tmpfiles.conf" "${EXTRA_DIR}/usr/lib/tmpfiles.d/rush-livedev.conf"

    # Enable LiveDev services in the preset.
    # rush-livedev-test.service is the post-reboot test runner — it only
    # runs when /RUSH-DATA/state/livedev-state.json exists (ConditionPathExists).
    # rush-livedev-autostart.service is skipped when the state file exists
    # (its own ConditionPathExists=!...).
    # rush-livedev-failure.service is the fail-closed handler — it is
    # triggered by OnFailure= on the test service, never started directly.
    cat >> "${EXTRA_DIR}/usr/lib/systemd/system-preset/00-rush.preset" << 'EOF'
enable rush-livedev-test.service
enable rush-livedev-failure.service
enable rush-livedev-autostart.service
enable rush-capture.service
enable rush-autopilot.service
EOF

    # Symlink LiveDev service enablement
    ln -sf /usr/lib/systemd/system/rush-livedev-test.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/rush-livedev-test.service"
    ln -sf /usr/lib/systemd/system/rush-livedev-failure.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/rush-livedev-failure.service"
    ln -sf /usr/lib/systemd/system/rush-livedev-autostart.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/rush-livedev-autostart.service"
    ln -sf /usr/lib/systemd/system/rush-capture.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/rush-capture.service"
    ln -sf /usr/lib/systemd/system/rush-autopilot.service "${EXTRA_DIR}/etc/systemd/system/multi-user.target.wants/rush-autopilot.service"

    # Mask the tty1 getty on the livedev image. When the test runner is
    # active, it owns tty1 via the systemd unit's StandardOutput=journal+console.
    # When the test runner is NOT active (idle boot), rush-livedev-autostart
    # owns tty1 and offers the countdown. A bare root getty on tty1 is the
    # failure mode we are eliminating: it leaves the user at a root prompt
    # with no test status. The autostart service still drops to bash on ESC.
    # (You can still get a root shell by pressing ESC during the countdown,
    # or by logging in via ssh/getty on tty2-tty6 if those are enabled.)
    rm -f "${EXTRA_DIR}/etc/systemd/system/getty.target.wants/getty@tty1.service"
    ln -sf /dev/null "${EXTRA_DIR}/etc/systemd/system/getty.target.wants/getty@tty1.service"

    # PYTHONPATH for rush-* tools to find their support libraries
    mkdir -p "${EXTRA_DIR}/etc/profile.d"
    # shellcheck disable=SC2016
    # PYTHONPATH must expand when a user sources the generated profile.
    echo 'export PYTHONPATH="/usr/lib/rush:${PYTHONPATH}"' > "${EXTRA_DIR}/etc/profile.d/rush-livedev.sh"

    echo "   Done."
fi

# fstab (systemd-gpt-auto-generator handles root=, but explicit is clearer for VMs)
cat > "${EXTRA_DIR}/etc/fstab" << 'EOF'
# Rush Linux fstab — root partition is auto-mounted by systemd-gpt-auto-generator.
# Explicit entry for QEMU virtio VMs where GPT auto-detection may not work:
#/dev/vda2  /  ext4  defaults,noatime  0 1
EOF

echo "   Done."
echo ""

# ── Step 3: Clean previous build artifacts if requested ──────────
if [[ "${CLEAN}" == true ]]; then
    echo ">> [3/5] Cleaning previous build artifacts..."
    rm -rf "${REPO_ROOT}/build"
    rm -rf "${MKOSI_DIR}/.mkosi-private"
    echo "   Done."
    echo ""
else
    echo ">> [3/5] Skipping clean (--clean not specified)."
    echo ""
fi

# ── Step 4: Invoke mkosi ────────────────────────────────────────
echo ">> [4/5] Invoking mkosi build (edition: ${EDITION})..."
cd "${MKOSI_DIR}"

MKOSI_ARGS=(--force)

# The server target *is* the common base described by mkosi/mkosi.conf, so it
# must not consume mkosi.profiles/server: that profile is intentionally the
# empty server sysext payload. LiveDev remains a whole-image operational profile.
if [[ "${EDITION}" == "livedev" ]]; then
    MKOSI_ARGS+=(--profile="${EDITION}")
fi

if [[ -n "${MKOSI_CACHE:-}" ]]; then
    MKOSI_ARGS+=(--cache-dir="${MKOSI_CACHE}")
fi

mkosi build "${MKOSI_ARGS[@]}"

echo "   Done."
echo ""

# ── Step 5: Fix permissions and report ───────────────────────────
echo ">> [5/5] Fixing permissions of build artifacts..."
if [[ -n "${SUDO_USER:-}" ]]; then
    chown -R "${SUDO_USER}:${SUDO_USER}" "${REPO_ROOT}/build" 2>/dev/null || true
fi

DISK="${REPO_ROOT}/build/rush-linux.raw"
if [[ ! -f "${DISK}" ]]; then
    if [[ -f "${REPO_ROOT}/build/rush-linux-${EDITION}.raw" ]]; then
        ln -sf "rush-linux-${EDITION}.raw" "${DISK}"
    elif [[ -f "${REPO_ROOT}/build/rush-linux-server.raw" ]]; then
        ln -sf "rush-linux-server.raw" "${DISK}"
    fi
fi
if [[ -f "${DISK}" ]]; then
    SIZE=$(du -sh "${DISK}" | cut -f1)
    echo "   Image: ${DISK} (${SIZE})"
else
    echo "   Warning: Expected image not found at ${DISK}"
    echo "   Check mkosi output above for errors."
fi

echo ""
echo "════════════════════════════════════════════════════"
echo "  ✅ Build finished (edition: ${EDITION})"
echo ""
echo "  Validate:"
echo "    tools/validate-uefi-boot.sh build/rush-linux.raw"
echo "    tools/test-rollback.sh build/rush-linux.raw"
echo ""
echo "  Boot manually (UEFI):"
echo "    qemu-system-x86_64 -bios /usr/share/OVMF/OVMF_CODE.fd \\"
echo "      -drive file=build/rush-linux.raw,format=raw,if=virtio \\"
echo "      -m 1G -nographic"
echo "════════════════════════════════════════════════════"
