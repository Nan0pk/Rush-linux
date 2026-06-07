# Boot And Updates

Rush Linux uses a UKI-first boot and update model with rollback as a core
safety requirement.

## Boot Direction

Default direction:

- UEFI systems use Unified Kernel Images.
- systemd-boot is preferred where supported.
- GRUB remains a compatibility fallback, built from `recipes/boot/grub.toml`.
  It is never the default bootloader; it is opt-in for firmware that cannot do
  UKI cleanly. The v0.4 milestone wires this recipe into the boot/rollback flow.
- Kernel command line defaults live in `distro/boot/cmdline.d/adaptive.conf`.
- UKI policy lives in `distro/boot/uki.toml`.

## Update Direction

System update descriptors live in:

- `distro/sysupdate/base.conf`
- `distro/sysupdate/uki.conf`

The current descriptors are placeholders for the future update server and
artifact naming scheme. They define the intended systemd-sysupdate direction,
not a live production update service.

## Rollback Requirements

Any installable release must support:

- multiple kernel/UKI entries;
- automatic or explicit boot-good marking;
- rollback after failed boot;
- rollback after failed optimizer or policy update where possible;
- signed update metadata.

## Acceptance Criteria

Boot/update changes must update this file, `distro/boot/uki.toml`,
`distro/sysupdate/`, and the roadmap/status docs in the same change.

