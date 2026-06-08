# Boot And Updates

Rush Linux uses a UKI-first boot and update model with rollback as a core
safety requirement.

## Boot Direction

Default direction:

- UEFI systems use Unified Kernel Images.
- systemd-boot is preferred where supported.
- The builder now stages the fallback UEFI path (`EFI/BOOT/BOOTX64.EFI`) plus
  a systemd-boot loader configuration (`loader/loader.conf`) and entry
  (`loader/entries/rush-linux.conf`) that boots `/EFI/Linux/rush-linux.efi`.
  This closes the earlier "UKI exists on the ESP but no boot menu entry points
  at it" gap; QEMU/OVMF validation is still required before marking the v0.4
  boot gate complete.
- GRUB remains a compatibility fallback, described by `recipes/boot/grub.toml`.
  That recipe is currently a **skeleton — not yet buildable**; it records intent
  and structure only. GRUB is never the default bootloader; it is opt-in for
  firmware that cannot do UKI cleanly. The v0.4 milestone makes the recipe
  buildable and wires it into the boot/rollback flow.
- Kernel command line defaults live in `distro/boot/cmdline.d/adaptive.conf`.
  The VM/UKI builder appends image-specific `root=/dev/vda2 rw
  console=ttyS0,115200` arguments at build time so the shared default fragment
  stays hardware-neutral.
- UKI policy lives in `distro/boot/uki.toml`, including the staged
  systemd-boot fallback path, loader config path, default entry path, and UKI
  path used by the builder.

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

