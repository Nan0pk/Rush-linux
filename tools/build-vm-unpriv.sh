#!/usr/bin/env bash
# tools/build-vm-unpriv.sh — Build a bootable Rush Linux VM image WITHOUT root.
#
# This is an alternative to build-vm-final.sh that avoids chroot, mount, and
# loop devices. Instead, it:
#   1. Extracts Ubuntu Base rootfs (no root needed)
#   2. Installs Rush Linux components directly (no chroot)
#   3. Builds initrd with kernel modules
#   4. Builds UKI via objcopy
#   5. Builds ESP image via mtools
#   6. Creates rootfs tarball
#   7. Assembles GPT disk image using systemd-repart
#
# The key difference: instead of mounting ext4 and rsync'ing rootfs,
# we use systemd-repart with --copy-source= to populate the root partition,
# OR we create the ext4 filesystem directly on a file using mkfs.ext4 -d
# (rootfs-in-a-directory mode, supported by e2fsprogs 1.47+).
#
# Usage:
#   bash tools/build-vm-unpriv.sh
#
# Prerequisites:
#   - cargo/rustc (for building optid/optctl)
#   - python3 >= 3.11 (for tomllib)
#   - All deps in tools/env-setup.sh (mtools, cpio, sgdisk, objcopy, mkfs.ext4)
#   - Pre-downloaded assets in build/tmp_downloads/ (run tools/download-assets.py first)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD="${REPO_ROOT}/build"
DL="${BUILD}/tmp_downloads"
ROOTFS="${BUILD}/rootfs"
KVER="6.1.0-49-amd64"
VERSION="$(cat "${REPO_ROOT}/VERSION" 2>/dev/null || echo "0.5.0-beta.1")"

echo "════════════════════════════════════════════════════"
echo "  Rush Linux Unprivileged VM Builder"
echo "════════════════════════════════════════════════════"
echo "  Version: ${VERSION}"
echo ""

# ── Step 1: Extract Ubuntu Base rootfs ──────────────────────────
echo ">> [1/8] Extracting Ubuntu Base rootfs..."
rm -rf "${ROOTFS}"
mkdir -p "${ROOTFS}"
tar -xzf "${DL}/ubuntu-base-24.04.4-base-amd64.tar.gz" -C "${ROOTFS}"
echo "   Done ($(du -sm "${ROOTFS}" | cut -f1)MB)"

# ── Step 2: Install Rush Linux components ───────────────────────
echo ">> [2/8] Installing Rush Linux components..."

# Build binaries if not already built
if [ ! -x "${REPO_ROOT}/target/release/optid" ] || [ ! -x "${REPO_ROOT}/target/release/optctl" ]; then
    (cd "${REPO_ROOT}" && cargo build --workspace --release)
fi

mkdir -p "${ROOTFS}"/{usr/bin,usr/libexec,usr/lib/optid,usr/lib/systemd/system,usr/lib/tmpfiles.d}
mkdir -p "${ROOTFS}"/{etc/systemd/system.conf.d,usr/lib/sysctl.d,usr/lib/systemd/network,etc}
mkdir -p "${ROOTFS}"/{usr/share/dbus-1/system-services,usr/share/dbus-1/interfaces}
mkdir -p "${ROOTFS}"/{etc/systemd/system/multi-user.target.wants,etc/systemd/system/getty.target.wants}
mkdir -p "${ROOTFS}"/{usr/lib/kernel/cmdline.d,boot,lib/modules}

install -m0755 "${REPO_ROOT}/target/release/optid" "${ROOTFS}/usr/libexec/optid"
install -m0755 "${REPO_ROOT}/target/release/optctl" "${ROOTFS}/usr/bin/optctl"
install -m0644 "${REPO_ROOT}/config/optid/policy.toml" "${ROOTFS}/usr/lib/optid/policy.toml"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid.service" "${ROOTFS}/usr/lib/systemd/system/optid.service"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid-apply.service" "${ROOTFS}/usr/lib/systemd/system/optid-apply.service"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid-tmpfiles.conf" "${ROOTFS}/usr/lib/tmpfiles.d/optid.conf"
install -m0644 "${REPO_ROOT}/distro/systemd/00-cgroup-v2.conf" "${ROOTFS}/etc/systemd/system.conf.d/00-cgroup-v2.conf"
install -m0644 "${REPO_ROOT}/distro/systemd/99-rush-network.conf" "${ROOTFS}/usr/lib/sysctl.d/99-rush-network.conf"
install -m0644 "${REPO_ROOT}/distro/systemd/zram-generator.conf" "${ROOTFS}/usr/lib/systemd/zram-generator.conf"
install -m0644 "${REPO_ROOT}/distro/network/nftables.conf" "${ROOTFS}/etc/nftables.conf"
install -m0644 "${REPO_ROOT}/packaging/dbus/io.rushlinux.Optid.service" "${ROOTFS}/usr/share/dbus-1/system-services/io.rushlinux.Optid.service"
install -m0644 "${REPO_ROOT}/packaging/dbus/io.rushlinux.Optid.xml" "${ROOTFS}/usr/share/dbus-1/interfaces/io.rushlinux.Optid.xml"

# Boot assessment
install -m0755 "${REPO_ROOT}/tools/optid-boot-assess" "${ROOTFS}/usr/libexec/optid-boot-assess"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid-boot-assess.service" "${ROOTFS}/usr/lib/systemd/system/optid-boot-assess.service"

# Enable services
ln -sf /usr/lib/systemd/system/optid.service "${ROOTFS}/etc/systemd/system/multi-user.target.wants/optid.service"
ln -sf /usr/lib/systemd/system/optid-boot-assess.service "${ROOTFS}/etc/systemd/system/multi-user.target.wants/optid-boot-assess.service"
ln -sf /usr/lib/systemd/system/systemd-networkd.service "${ROOTFS}/etc/systemd/system/multi-user.target.wants/systemd-networkd.service"
ln -sf /usr/lib/systemd/system/systemd-resolved.service "${ROOTFS}/etc/systemd/system/multi-user.target.wants/systemd-resolved.service"
ln -sf /usr/lib/systemd/system/systemd-oomd.service "${ROOTFS}/etc/systemd/system/multi-user.target.wants/systemd-oomd.service"
ln -sf /usr/lib/systemd/system/getty@.service "${ROOTFS}/etc/systemd/system/getty.target.wants/getty@tty1.service"
ln -sf /usr/lib/systemd/system/multi-user.target "${ROOTFS}/etc/systemd/system/default.target"

# Network
mkdir -p "${ROOTFS}/etc/systemd/network"
cat > "${ROOTFS}/usr/lib/systemd/network/20-wired.network" << 'EOF'
[Match]
Name=en* eth*
[Network]
DHCP=yes
EOF

# Host config
echo "rush-linux" > "${ROOTFS}/etc/hostname"
cat > "${ROOTFS}/etc/os-release" << EOF
NAME="Rush Linux"
VERSION="${VERSION}"
ID=rush-linux
ID_LIKE=ubuntu
VERSION_ID="$(echo "${VERSION}" | sed 's/-.*//')"
PRETTY_NAME="Rush Linux ${VERSION}"
HOME_URL="https://github.com/Nan0pk/Rush-linux"
BUG_REPORT_URL="https://github.com/Nan0pk/Rush-linux/issues"
EOF

cat > "${ROOTFS}/etc/fstab" << 'EOF'
/dev/vda2  /  ext4  defaults,noatime  0 1
EOF

# Root login (empty password)
sed -i 's|^root:[^:]*:|root::|' "${ROOTFS}/etc/shadow"

echo "   Done"

# ── Step 3: Install kernel modules ──────────────────────────────
echo ">> [3/8] Extracting kernel modules..."
KMOD="${BUILD}/tmp_kernel_extract"
rm -rf "${KMOD}" && mkdir -p "${KMOD}"
cd "${KMOD}"
ar x "${DL}/linux-image-6.1.0-49-amd64_6.1.174-1_amd64.deb"
tar -xf data.tar.* 2>/dev/null || true
MOD_SRC=$(find . -name "${KVER}" -type d | head -1)
if [ -n "${MOD_SRC}" ]; then
    cp -a "${MOD_SRC}" "${ROOTFS}/lib/modules/"
    echo "   Kernel modules installed"
else
    echo "   WARNING: Kernel modules not found in deb package"
fi

# Extract vmlinuz
VMLINUZ=$(find "${KMOD}" -name "vmlinuz-${KVER}" -type f | head -1)
if [ -n "${VMLINUZ}" ]; then
    cp "${VMLINUZ}" "${BUILD}/vmlinuz"
    cp "${VMLINUZ}" "${ROOTFS}/boot/vmlinuz-${KVER}"
    echo "   vmlinuz extracted"
fi
cd "${REPO_ROOT}"
rm -rf "${KMOD}"

# ── Step 4: Build initrd ────────────────────────────────────────
echo ">> [4/8] Building initrd with kernel modules..."
INITRD="${BUILD}/tmp_initrd"
rm -rf "${INITRD}"
mkdir -p "${INITRD}"/{bin,sbin,proc,sys,dev,mnt/root,etc,tmp,run,lib/modules/${KVER}}

# BusyBox
BB="${BUILD}/tmp_bb"
rm -rf "${BB}" && mkdir -p "${BB}"
cd "${BB}"
ar x "${DL}/busybox-static_1.35.0-4+deb12u1+b1_amd64.deb"
tar -xf data.tar.* 2>/dev/null || true
cp "${BB}/bin/busybox" "${INITRD}/bin/busybox"
chmod 755 "${INITRD}/bin/busybox"
cd "${REPO_ROOT}"
rm -rf "${BB}"

for a in sh mount cat mkdir echo ls sleep switch_root mknod modprobe insmod; do
    ln -s busybox "${INITRD}/bin/${a}"
done
ln -s ../bin/busybox "${INITRD}/sbin/switch_root"

# Essential kernel modules
KMDIR="${ROOTFS}/lib/modules/${KVER}/kernel"
MODS=(
    drivers/virtio/virtio.ko drivers/virtio/virtio_ring.ko
    drivers/virtio/virtio_pci_legacy_dev.ko drivers/virtio/virtio_pci_modern_dev.ko
    drivers/virtio/virtio_pci.ko drivers/block/virtio_blk.ko
    crypto/crc32c_generic.ko lib/libcrc32c.ko lib/crc16.ko
    fs/jbd2/jbd2.ko fs/mbcache.ko fs/ext4/ext4.ko
)
IDIR="${INITRD}/lib/modules/${KVER}"
for m in "${MODS[@]}"; do
    cp "${KMDIR}/${m}" "${IDIR}/" 2>/dev/null && echo "   + $(basename $m)"
done
touch "${IDIR}/modules.dep"

# Init script
cat > "${INITRD}/init" << 'INITEOF'
#!/bin/sh
set -e
echo "== Rush Linux initrd =="
mount -t proc proc /proc; mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev; mount -t tmpfs tmpfs /run
M="/lib/modules/6.1.0-49-amd64"
echo "Loading drivers..."
for k in virtio virtio_ring virtio_pci_legacy_dev virtio_pci_modern_dev \
         virtio_pci virtio_blk crc32c_generic libcrc32c crc16 jbd2 mbcache ext4; do
    insmod $M/${k}.ko 2>/dev/null || true
done
sleep 1
R=""; for a in $(cat /proc/cmdline); do case $a in root=*) R="${a#root=}";; esac; done
[ -z "$R" ] && { echo "No root="; ls /dev/vd* /dev/sd* 2>/dev/null; exec /bin/sh; }
echo "Root: $R"
i=0; while [ ! -e "$R" ] && [ $i -lt 50 ]; do sleep 0.2; i=$((i+1)); done
[ ! -e "$R" ] && { echo "$R not found!"; ls /dev/vd* /dev/sd* 2>/dev/null; exec /bin/sh; }
echo "Mounting $R..."
mkdir -p /mnt/root
mount -o ro "$R" /mnt/root || { echo "Mount failed"; ls /dev/vd* /dev/sd* 2>/dev/null || true; exec /bin/sh; }
[ ! -e /mnt/root/sbin/init ] && { echo "No /sbin/init"; ls /mnt/root/; exec /bin/sh; }
echo "switch_root -> systemd"
exec switch_root /mnt/root /sbin/init
INITEOF
chmod 755 "${INITRD}/init"

cd "${INITRD}" && find . | cpio -o -H newc 2>/dev/null | gzip -9 > "${BUILD}/initrd.img"
cd "${REPO_ROOT}"
echo "   Initrd: $(du -sh "${BUILD}/initrd.img" | cut -f1)"

# ── Step 5: Build UKI ───────────────────────────────────────────
echo ">> [5/8] Building Unified Kernel Image..."
EFI_W="${BUILD}/tmp_efi"
rm -rf "${EFI_W}" && mkdir -p "${EFI_W}"
cd "${EFI_W}"
ar x "${DL}/systemd-boot-efi_252.39-1~deb12u2_amd64.deb"
tar -xf data.tar.* 2>/dev/null || true
STUB=$(find . -name "linuxx64.efi.stub" -type f | head -1)
BOOTLD=$(find . -name "systemd-bootx64.efi" -type f | head -1)
cp "${STUB}" "${BUILD}/linuxx64.efi.stub"
cp "${BOOTLD}" "${BUILD}/systemd-bootx64.efi"
cd "${REPO_ROOT}"; rm -rf "${EFI_W}"

CMDLINE="systemd.unified_cgroup_hierarchy=1 cgroup_no_v1=all psi=1 zswap.enabled=1 systemd.firstboot=no root=/dev/vda2 rw console=ttyS0,115200"
echo -n "${CMDLINE}" > "${BUILD}/cmdline.txt"

mkdir -p "${BUILD}/uki_staging/EFI/Linux" "${BUILD}/uki_staging/loader/entries"

objcopy \
    --add-section ".cmdline=${BUILD}/cmdline.txt" --change-section-vma .cmdline=0x30000 \
    --add-section ".linux=${BUILD}/vmlinuz" --change-section-vma .linux=0x2000000 \
    --add-section ".initrd=${BUILD}/initrd.img" --change-section-vma .initrd=0x3000000 \
    "${BUILD}/linuxx64.efi.stub" "${BUILD}/uki_staging/EFI/Linux/rush-linux.efi"

cat > "${BUILD}/uki_staging/loader/loader.conf" << EOF
default rush-linux.conf
timeout 3
editor no
EOF

cat > "${BUILD}/uki_staging/loader/entries/rush-linux.conf" << EOF
title Rush Linux
version ${VERSION}
efi /EFI/Linux/rush-linux.efi
EOF

mkdir -p "${BUILD}/uki_staging/loader/rush-assess"

echo "   UKI: $(du -sh "${BUILD}/uki_staging/EFI/Linux/rush-linux.efi" | cut -f1)"

# ── Step 6: Build ESP image ────────────────────────────────────
echo ">> [6/8] Building ESP partition image..."
dd if=/dev/zero of="${BUILD}/esp.img" bs=1M count=64 status=none
mkfs.vfat -F 32 -n RUSHESP "${BUILD}/esp.img" 2>&1 | tail -1
mmd -i "${BUILD}/esp.img" ::EFI ::EFI/Linux ::EFI/BOOT ::loader ::loader/entries ::loader/rush-assess
mcopy -i "${BUILD}/esp.img" "${BUILD}/uki_staging/EFI/Linux/rush-linux.efi" ::EFI/Linux/rush-linux.efi
mcopy -i "${BUILD}/esp.img" "${BUILD}/systemd-bootx64.efi" ::EFI/BOOT/BOOTX64.EFI
mcopy -i "${BUILD}/esp.img" "${BUILD}/uki_staging/loader/loader.conf" ::loader/loader.conf
mcopy -i "${BUILD}/esp.img" "${BUILD}/uki_staging/loader/entries/rush-linux.conf" ::loader/entries/rush-linux.conf
echo "   ESP: $(du -sh "${BUILD}/esp.img" | cut -f1)"

# ── Step 7: Build root filesystem image ─────────────────────────
echo ">> [7/8] Building root filesystem image..."
echo "   Rootfs: $(du -sm "${ROOTFS}" | cut -f1)MB"

# Use mkfs.ext4 -d to populate the filesystem from a directory
# This is supported by e2fsprogs 1.47+ and does NOT need root/mount
RFS=$(du -sm "${ROOTFS}" | cut -f1)
RPT=$((RFS + 300))  # root partition size in MB

dd if=/dev/zero of="${BUILD}/root.img" bs=1M count=${RPT} status=none
mkfs.ext4 -F -L RushRoot -d "${ROOTFS}" "${BUILD}/root.img" 2>&1 | tail -3
echo "   Root: $(du -sh "${BUILD}/root.img" | cut -f1)"

# ── Step 8: Assemble disk image ────────────────────────────────
echo ">> [8/8] Assembling disk image..."
TOT=$((64 + RPT + 40))
rm -f "${BUILD}/disk.raw"
dd if=/dev/zero of="${BUILD}/disk.raw" bs=1M count=0 seek=${TOT} status=none

sgdisk --clear \
    --new=1::+64M  -t 1:C12A7328-F81F-11D2-BA4B-00A0C93EC93B -c 1:"RushLinux ESP" \
    --new=2::+${RPT}M -t 2:4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709 -c 2:"RushLinux Root" \
    "${BUILD}/disk.raw" 2>/dev/null

ESP_S=$(sgdisk -i 1 "${BUILD}/disk.raw" 2>/dev/null | grep "First sector:" | awk '{print $3}')
R_S=$(sgdisk -i 2 "${BUILD}/disk.raw" 2>/dev/null | grep "First sector:" | awk '{print $3}')
R_SZ=$(sgdisk -i 2 "${BUILD}/disk.raw" 2>/dev/null | grep "Partition size:" | awk '{print $3}')

# Write ESP
dd if="${BUILD}/esp.img" of="${BUILD}/disk.raw" bs=512 seek=${ESP_S} conv=notrunc status=none

# Write root
dd if="${BUILD}/root.img" of="${BUILD}/disk.raw" bs=512 seek=${R_S} count=${R_SZ} conv=notrunc status=progress

# Clean up intermediate files
rm -f "${BUILD}/root.img"

echo ""
echo "════════════════════════════════════════════════════"
echo "  ✅ Build complete (unprivileged)"
echo "  Disk: $(du -sh "${BUILD}/disk.raw" | cut -f1)"
echo ""
echo "  Test with:"
echo "    tools/validate-uefi-boot.sh build/disk.raw"
echo "    tools/test-rollback.sh build/disk.raw"
echo ""
echo "  Or manual QEMU:"
echo "    qemu-system-x86_64 -bios /usr/share/OVMF/OVMF_CODE.fd \\"
echo "      -drive file=build/disk.raw,format=raw,if=virtio \\"
echo "      -m 1G -nographic"
echo "════════════════════════════════════════════════════"
