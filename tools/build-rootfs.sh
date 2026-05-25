#!/usr/bin/env bash
set -euo pipefail

root="${1:-build/rootfs}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

case "$(uname -s)" in
  Linux) ;;
  *)
    echo "build-rootfs.sh must run on Linux" >&2
    exit 1
    ;;
esac

mkdir -p "$root"/{boot,efi,etc,usr/bin,usr/libexec,var/lib/adaptive}
install -Dm0644 "$repo_root/distro/boot/cmdline.d/adaptive.conf" \
  "$root/usr/lib/kernel/cmdline.d/adaptive.conf"
install -Dm0644 "$repo_root/distro/network/nftables.conf" \
  "$root/etc/nftables.conf"
install -Dm0644 "$repo_root/config/optid/policy.toml" \
  "$root/usr/lib/optid/policy.toml"
install -Dm0644 "$repo_root/packaging/systemd/optid.service" \
  "$root/usr/lib/systemd/system/optid.service"
install -Dm0644 "$repo_root/packaging/systemd/optid-tmpfiles.conf" \
  "$root/usr/lib/tmpfiles.d/optid.conf"

echo "Created rootfs skeleton at $root"
echo "Next: feed source recipes into the package builder and install signed outputs."

