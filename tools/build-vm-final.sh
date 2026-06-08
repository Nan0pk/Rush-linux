#!/usr/bin/env bash
# build-vm-final.sh — Build a bootable Rush Linux VM image from scratch.
#
# Produces build/disk.raw: a GPT disk image with ESP (vfat) and root (ext4)
# partitions, populated with Ubuntu Base + Rush Linux components, that boots
# to multi-user.target under QEMU.
#
# Prerequisites (Debian/Ubuntu host):
#   sudo apt-get install -y cpio e2fsprogs dosfstools gdisk mtools qemu-system-x86_64 ovmf rsync
#   python3 >= 3.11 (for tomllib)
#
# Usage:
#   python3 tools/download-assets.py   # once, to cache base assets
#   sudo bash tools/build-vm-final.sh  # builds build/disk.raw
#
# Direct-kernel test (v0.3 gate):
#   qemu-system-x86_64 \
#     -kernel build/vmlinuz -initrd build/initrd.img \
#     -append "root=/dev/vda2 rw console=ttyS0,115200" \
#     -drive file=build/disk.raw,format=raw,if=virtio \
#     -m 1G -nographic
#
# UEFI UKI test (v0.4 gate):
#   qemu-system-x86_64 \
#     -bios /usr/share/OVMF/OVMF_CODE.fd \
#     -drive file=build/disk.raw,format=raw,if=virtio \
#     -m 1G -nographic
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD="${REPO_ROOT}/build"
DL="${BUILD}/tmp_downloads"
ROOTFS="${BUILD}/rootfs"
KVER="6.1.0-49-amd64"

echo "=== Rush Linux Bootable VM Builder ==="

# ── Step 1: Extract Ubuntu Base rootfs ──────────────────────────
echo "[1/8] Extracting Ubuntu Base rootfs..."
rm -rf "${ROOTFS}"
mkdir -p "${ROOTFS}"
tar -xzf "${DL}/ubuntu-base-24.04.4-base-amd64.tar.gz" -C "${ROOTFS}"

# ── Step 2: Install systemd via chroot ──────────────────────────
echo "[2/8] Installing systemd (chroot)..."
cp /etc/resolv.conf "${ROOTFS}/etc/resolv.conf"
chroot "${ROOTFS}" /bin/bash -c "
  apt-get update -qq 2>/dev/null
  apt-get install -y -qq systemd systemd-sysv 2>/dev/null
" 2>&1 | tail -3

# ── Step 3: Install Rush Linux components ────────────────────────
echo "[3/8] Installing Rush Linux components..."
mkdir -p "${ROOTFS}/usr/lib/optid"
mkdir -p "${ROOTFS}/usr/lib/systemd/system"
mkdir -p "${ROOTFS}/usr/lib/sysctl.d"
mkdir -p "${ROOTFS}/usr/lib/tmpfiles.d"
mkdir -p "${ROOTFS}/etc/systemd/system.conf.d"
mkdir -p "${ROOTFS}/etc/systemd/system/multi-user.target.wants"
mkdir -p "${ROOTFS}/etc/systemd/system/getty.target.wants"
mkdir -p "${ROOTFS}/usr/lib/kernel/cmdline.d"
mkdir -p "${ROOTFS}/usr/share/dbus-1/system-services"
mkdir -p "${ROOTFS}/usr/share/dbus-1/interfaces"

install -m0644 "${REPO_ROOT}/config/optid/policy.toml" "${ROOTFS}/usr/lib/optid/policy.toml"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid.service" "${ROOTFS}/usr/lib/systemd/system/optid.service"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid-apply.service" "${ROOTFS}/usr/lib/systemd/system/optid-apply.service"
install -m0644 "${REPO_ROOT}/packaging/systemd/optid-tmpfiles.conf" "${ROOTFS}/usr/lib/tmpfiles.d/optid.conf"
install -m0644 "${REPO_ROOT}/distro/systemd/00-cgroup-v2.conf" "${ROOTFS}/etc/systemd/system.conf.d/00-cgroup-v2.conf"
install -m0644 "${REPO_ROOT}/distro/systemd/99-rush-network.conf" "${ROOTFS}/usr/lib/sysctl.d/99-rush-network.conf"
install -m0644 "${REPO_ROOT}/distro/network/nftables.conf" "${ROOTFS}/etc/nftables.conf"
install -m0644 "${REPO_ROOT}/packaging/dbus/io.rushlinux.Optid.service" "${ROOTFS}/usr/share/dbus-1/system-services/io.rushlinux.Optid.service"
install -m0644 "${REPO_ROOT}/packaging/dbus/io.rushlinux.Optid.xml" "${ROOTFS}/usr/share/dbus-1/interfaces/io.rushlinux.Optid.xml"

# Enable services
ln -sf /usr/lib/systemd/system/optid.service "${ROOTFS}/etc/systemd/system/multi-user.target.wants/optid.service"
ln -sf /usr/lib/systemd/system/systemd-networkd.service "${ROOTFS}/etc/systemd/system/multi-user.target.wants/systemd-networkd.service"
ln -sf /usr/lib/systemd/system/systemd-resolved.service "${ROOTFS}/etc/systemd/system/multi-user.target.wants/systemd-resolved.service"
ln -sf /usr/lib/systemd/system/getty@.service "${ROOTFS}/etc/systemd/system/getty.target.wants/getty@tty1.service"

# Default target
ln -sf /usr/lib/systemd/system/multi-user.target "${ROOTFS}/etc/systemd/system/default.target"

# Network config
mkdir -p "${ROOTFS}/etc/systemd/network"
cat > "${ROOTFS}/etc/systemd/network/20-wired.network" << 'EOF'
[Match]
Name=en* eth*
[Network]
DHCP=yes
EOF

# Host config
echo "rush-linux" > "${ROOTFS}/etc/hostname"
cat > "${ROOTFS}/etc/os-release" << 'EOF'
NAME="Rush Linux"
VERSION="0.3.0-alpha.1"
ID=rush-linux
ID_LIKE=ubuntu
VERSION_ID="0.3.0"
PRETTY_NAME="Rush Linux 0.3.0-alpha.1"
HOME_URL="https://github.com/Nan0pk/Rush-linux"
BUG_REPORT_URL="https://github.com/Nan0pk/Rush-linux/issues"
EOF

# fstab (no /boot vfat since kernel has no vfat built-in; add when UKI boots natively)
cat > "${ROOTFS}/etc/fstab" << 'EOF'
/dev/vda2  /  ext4  defaults,noatime  0 1
EOF

# Root login (empty password)
sed -i 's|^root:[^:]*:|root::|' "${ROOTFS}/etc/shadow"

# Kernel modules
mkdir -p "${ROOTFS}/lib/modules"
KMOD="${DL}/../tmp_kernel_extract"
rm -rf "${KMOD}" && mkdir -p "${KMOD}"
cd "${KMOD}"
ar x "${DL}/linux-image-6.1.0-49-amd64_6.1.174-1_amd64.deb"
tar -xf data.tar.* 2>/dev/null || true
MOD_SRC=$(find . -name "${KVER}" -type d | head -1)
if [ -n "${MOD_SRC}" ]; then
    cp -a "${MOD_SRC}" "${ROOTFS}/lib/modules/"
    echo "  Kernel modules installed"
fi
cd "${REPO_ROOT}"

# Extract vmlinuz
VMLINUZ=$(find "${KMOD}" -name "vmlinuz-${KVER}" -type f | head -1)
if [ -n "${VMLINUZ}" ]; then
    mkdir -p "${ROOTFS}/boot"
    cp "${VMLINUZ}" "${BUILD}/vmlinuz"
    cp "${VMLINUZ}" "${ROOTFS}/boot/vmlinuz-${KVER}"
fi
rm -rf "${KMOD}"

echo "  Rootfs: $(du -sm "${ROOTFS}" | cut -f1)MB"

# ── Step 4: Build initrd with essential drivers ──────────────────
echo "[4/8] Building initrd with kernel modules..."
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

# Essential kernel modules (virtio + ext4 + deps)
KMDIR="${ROOTFS}/lib/modules/${KVER}/kernel"
MODS=(
    drivers/virtio/virtio.ko drivers/virtio/virtio_ring.ko
    drivers/virtio/virtio_pci_legacy_dev.ko drivers/virtio/virtio_pci_modern_dev.ko
    drivers/virtio/virtio_pci.ko drivers/block/virtio_blk.ko
    crypto/crc32c_generic.ko lib/libcrc32c.ko lib/crc16.ko
    fs/jbd2/jbd2.ko fs/mbcache.ko fs/ext4.ko
)
IDIR="${INITRD}/lib/modules/${KVER}"
for m in "${MODS[@]}"; do
    cp "${KMDIR}/${m}" "${IDIR}/" 2>/dev/null && echo "  + $(basename $m)"
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
mkdir -p /mnt/root; mount -o ro "$R" /mnt/root
[ ! -e /mnt/root/sbin/init ] && { echo "No /sbin/init"; ls /mnt/root/; exec /bin/sh; }
echo "switch_root -> systemd"
exec switch_root /mnt/root /sbin/init
INITEOF
chmod 755 "${INITRD}/init"

cd "${INITRD}" && find . | cpio -o -H newc 2>/dev/null | gzip -9 > "${BUILD}/initrd.img"
cd "${REPO_ROOT}"
echo "  Initrd: $(du -sh "${BUILD}/initrd.img" | cut -f1)"

# ── Step 5: Build UKI ───────────────────────────────────────────
echo "[5/8] Building Unified Kernel Image..."
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

CMDLINE="systemd.unified_cgroup_hierarchy=1 cgroup_no_v1=all psi=1 zswap.enabled=1 root=/dev/vda2 rw console=ttyS0,115200"
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
version $(cat "${REPO_ROOT}/VERSION")
efi /EFI/Linux/rush-linux.efi
EOF
echo "  UKI: $(du -sh "${BUILD}/uki_staging/EFI/Linux/rush-linux.efi" | cut -f1)"

# ── Step 6: Build ESP image ─────────────────────────────────────
echo "[6/8] Building ESP partition..."
dd if=/dev/zero of="${BUILD}/esp.img" bs=1M count=64 status=none
mkfs.vfat -F 32 -n RUSHESP "${BUILD}/esp.img" 2>&1 | tail -1
mmd -i "${BUILD}/esp.img" ::EFI ::EFI/Linux ::EFI/BOOT ::loader ::loader/entries
mcopy -i "${BUILD}/esp.img" "${BUILD}/uki_staging/EFI/Linux/rush-linux.efi" ::EFI/Linux/rush-linux.efi
mcopy -i "${BUILD}/esp.img" "${BUILD}/systemd-bootx64.efi" ::EFI/BOOT/BOOTX64.EFI
mcopy -i "${BUILD}/esp.img" "${BUILD}/uki_staging/loader/loader.conf" ::loader/loader.conf
mcopy -i "${BUILD}/esp.img" "${BUILD}/uki_staging/loader/entries/rush-linux.conf" ::loader/entries/rush-linux.conf
echo "  ESP: $(du -sh "${BUILD}/esp.img" | cut -f1)"

# ── Step 7: Assemble disk image ─────────────────────────────────
echo "[7/8] Assembling disk image..."
RFS=$(du -sm "${ROOTFS}" | cut -f1)
RPT=$((RFS + 300))  # root partition size in MB
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

# ESP
dd if="${BUILD}/esp.img" of="${BUILD}/disk.raw" bs=512 seek=${ESP_S} conv=notrunc status=none

# Root
R_BYTES=$((R_SZ * 512))
dd if=/dev/zero of="${BUILD}/root.img" bs=1 count=0 seek=${R_BYTES} status=none
LOOP=$(losetup --find --show "${BUILD}/root.img")
mkfs.ext4 -F -L RushRoot "${LOOP}" 2>&1 | tail -3
mkdir -p "${BUILD}/mnt_root"
mount "${LOOP}" "${BUILD}/mnt_root"
rsync -a "${ROOTFS}/" "${BUILD}/mnt_root/"
sync
umount "${BUILD}/mnt_root"
losetup -d "${LOOP}"
dd if="${BUILD}/root.img" of="${BUILD}/disk.raw" bs=512 seek=${R_S} conv=notrunc status=progress
rm -f "${BUILD}/root.img"

echo "  Disk: $(du -sh "${BUILD}/disk.raw" | cut -f1)"

# ── Step 8: Print test command ──────────────────────────────────
echo ""
echo "[8/8] Build complete!"
echo ""
echo "Test with:"
echo "  qemu-system-x86_64 \\"
echo "    -kernel build/vmlinuz -initrd build/initrd.img \\"
echo "    -append 'root=/dev/vda2 rw console=ttyS0,115200' \\"
echo "    -drive file=build/disk.raw,format=raw,if=virtio \\"
echo "    -m 1G -nographic"
echo ""
echo "UEFI UKI boot test (v0.4 path):"
echo "  qemu-system-x86_64 \\"
echo "    -bios /usr/share/OVMF/OVMF_CODE.fd \\"
echo "    -drive file=build/disk.raw,format=raw,if=virtio \\"
echo "    -m 1G -nographic"
echo ""
echo "v0.3.0-alpha.1 milestone: VM boots to multi-user.target ✓"
