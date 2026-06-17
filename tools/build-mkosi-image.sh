#!/usr/bin/env bash
# tools/build-vm-mkosi.sh — Build the bootable Arch-based Rush Linux image using mkosi.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MKOSI_DIR="${REPO_ROOT}/mkosi"
EXTRA_DIR="${MKOSI_DIR}/mkosi.extra"

echo "=== Building Rush Linux Arch/mkosi VM Image ==="

# 1. Compile host binaries
echo "  Building optid and optctl in release mode..."
cargo build --workspace --release

# 2. Re-create a clean mkosi.extra directory
echo "  Staging files in mkosi.extra..."
rm -rf "${EXTRA_DIR}"
mkdir -p "${EXTRA_DIR}"/{usr/bin,usr/libexec,usr/lib/optid,usr/lib/systemd/system,usr/lib/tmpfiles.d}
mkdir -p "${EXTRA_DIR}"/{etc/systemd/system.conf.d,usr/lib/sysctl.d,usr/lib/systemd/network,etc}
mkdir -p "${EXTRA_DIR}"/{usr/share/dbus-1/system-services,usr/share/dbus-1/interfaces}

# 3. Copy binaries and configs
cp "${REPO_ROOT}/target/release/optid" "${EXTRA_DIR}/usr/libexec/optid"
cp "${REPO_ROOT}/target/release/optctl" "${EXTRA_DIR}/usr/bin/optctl"
cp "${REPO_ROOT}/config/optid/policy.toml" "${EXTRA_DIR}/usr/lib/optid/policy.toml"
cp "${REPO_ROOT}/packaging/systemd/optid.service" "${EXTRA_DIR}/usr/lib/systemd/system/optid.service"
cp "${REPO_ROOT}/packaging/systemd/optid-apply.service" "${EXTRA_DIR}/usr/lib/systemd/system/optid-apply.service"
cp "${REPO_ROOT}/packaging/systemd/optid-tmpfiles.conf" "${EXTRA_DIR}/usr/lib/tmpfiles.d/optid.conf"
cp "${REPO_ROOT}/distro/systemd/00-cgroup-v2.conf" "${EXTRA_DIR}/etc/systemd/system.conf.d/00-cgroup-v2.conf"
cp "${REPO_ROOT}/distro/systemd/99-rush-network.conf" "${EXTRA_DIR}/usr/lib/sysctl.d/99-rush-network.conf"
cp "${REPO_ROOT}/distro/systemd/zram-generator.conf" "${EXTRA_DIR}/usr/lib/systemd/zram-generator.conf"
cp "${REPO_ROOT}/distro/network/nftables.conf" "${EXTRA_DIR}/etc/nftables.conf"
cp "${REPO_ROOT}/packaging/dbus/io.rushlinux.Optid.service" "${EXTRA_DIR}/usr/share/dbus-1/system-services/io.rushlinux.Optid.service"
cp "${REPO_ROOT}/packaging/dbus/io.rushlinux.Optid.xml" "${EXTRA_DIR}/usr/share/dbus-1/interfaces/io.rushlinux.Optid.xml"
cp "${REPO_ROOT}/tools/optid-boot-assess" "${EXTRA_DIR}/usr/libexec/optid-boot-assess"
cp "${REPO_ROOT}/packaging/systemd/optid-boot-assess.service" "${EXTRA_DIR}/usr/lib/systemd/system/optid-boot-assess.service"

# Network configuration
cat > "${EXTRA_DIR}/usr/lib/systemd/network/20-wired.network" << 'EOF'
[Match]
Name=en* eth*
[Network]
DHCP=yes
EOF

# Hostname
echo "rush-linux" > "${EXTRA_DIR}/etc/hostname"

# OS metadata
cat > "${EXTRA_DIR}/etc/os-release" << 'EOF'
NAME="Rush Linux"
VERSION="0.4.0-alpha.1"
ID=rush-linux
ID_LIKE=arch
VERSION_ID="0.4.0"
PRETTY_NAME="Rush Linux 0.4.0-alpha.1"
HOME_URL="https://github.com/Nan0pk/Rush-linux"
BUG_REPORT_URL="https://github.com/Nan0pk/Rush-linux/issues"
EOF

# fstab
cat > "${EXTRA_DIR}/etc/fstab" << 'EOF'
/dev/vda2  /  ext4  defaults,noatime  0 1
EOF

# 4. Invoke mkosi
echo "  Invoking mkosi build..."
cd "${MKOSI_DIR}"
mkosi build --force


# 5. Fix permissions of build artifacts
echo "  Fixing permissions of build artifacts..."
chown -R "${SUDO_USER:-$USER}:${SUDO_USER:-$USER}" "${REPO_ROOT}/build"

echo "=== Build finished ==="

