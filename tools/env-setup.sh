#!/usr/bin/env bash
# env-setup.sh — Source this to add all user-space tools to PATH.
#
# Usage:
#   source tools/env-setup.sh
#
# This adds:
#   - Rust toolchain (cargo, rustc, etc.)
#   - QEMU 10.0.8 (qemu-system-x86_64)
#   - mkosi 25.3
#   - systemd-repart 257
#   - mtools, cpio, mkfs.ext4, mkfs.vfat, sgdisk
#   - OVMF firmware paths
#   - Python path for mkosi module

TOOLBASE="/home/z/my-project/tmp-debs"

# Rust
export PATH="/home/z/.cargo/bin:${PATH}"

# User-space extracted binaries
export PATH="${TOOLBASE}/usr/bin:${TOOLBASE}/usr/sbin:${PATH}"

# Shared libraries for extracted binaries
export LD_LIBRARY_PATH="${TOOLBASE}/usr/lib/x86_64-linux-gnu:${TOOLBASE}/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# Python modules (mkosi)
export PYTHONPATH="${TOOLBASE}/usr/lib/python3/dist-packages${PYTHONPATH:+:$PYTHONPATH}"

# OVMF firmware
export OVMF_FIRMWARE="${TOOLBASE}/usr/share/OVMF/OVMF_CODE_4M.fd"

# Confirm key tools
echo "Environment ready:"
echo "  cargo:       $(cargo --version 2>/dev/null || echo 'MISSING')"
echo "  qemu:        $(qemu-system-x86_64 --version 2>/dev/null | head -1 || echo 'MISSING')"
echo "  mkosi:       $(python3 -m mkosi --version 2>/dev/null || echo 'MISSING')"
echo "  systemd-rep:  $(systemd-repart --version 2>/dev/null | head -1 || echo 'MISSING')"
echo "  mcopy:       $(mcopy --version 2>/dev/null | head -1 || echo 'MISSING')"
echo "  mkfs.ext4:   $(mkfs.ext4 -V 2>/dev/null | head -1 || echo 'MISSING')"
echo "  sgdisk:      $(sgdisk --version 2>/dev/null | head -1 || echo 'MISSING')"
echo "  cpio:        $(cpio --version 2>/dev/null | head -1 || echo 'MISSING')"
echo "  OVMF:        ${OVMF_FIRMWARE}"
