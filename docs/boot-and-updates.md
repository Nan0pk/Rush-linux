# Boot And Updates

Rush Linux uses a UKI-first boot and update model with rollback as a core
safety requirement.

## Boot Direction

Default direction:

- UEFI systems use Unified Kernel Images.
- systemd-boot is preferred where supported.
- The builder stages the fallback UEFI path (`EFI/BOOT/BOOTX64.EFI`) plus a
  systemd-boot loader configuration (`loader/loader.conf`) and entry
  (`loader/entries/rush-linux.conf`) that boots `/EFI/Linux/rush-linux.efi`.
  This closes the earlier "UKI exists on the ESP but no boot menu entry points
  at it" gap.
- `tools/validate-uefi-boot.sh` validates the v0.4 VM boot path under
  QEMU/OVMF. Verified 2026-06-08: OVMF starts the fallback bootloader,
  systemd-boot displays the Rush Linux entry, the UKI loads its embedded
  initrd, `/dev/vda2` mounts as root, systemd reaches `multi-user.target`, and
  `optid.service` starts.
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

### Implemented Rollback Infrastructure (v0.4)

- **Boot entry manager** (`tools/manage-boot-entries.sh`):
  Rotates the current main UKI (`rush-linux.efi`) into a versioned and
  timestamped rollback entry (e.g., `rush-linux-0.4.0-alpha.1-20260608120000.efi`).
  Prunes entries beyond `INSTANCES_MAX` (default: 3). Updates systemd-boot
  loader entries accordingly.

- **Boot assessment service** (`optid-boot-assess.service`):
  A systemd oneshot service that runs after `multi-user.target` and calls
  `/usr/libexec/optid-boot-assess mark-good` to record a successful boot
  on the ESP (`/boot/loader/rush-assess/current-boot`).

- **Boot assessment tool** (`tools/optid-boot-assess`):
  Manages boot-good markers. Commands:
  - `mark-good` — Record current boot as successful
  - `check` — Check if previous boot was good (exit 0/1)
  - `count-failed` — Count consecutive failed boots
  - `reset` — Clear failure counters

- **Rollback test** (`tools/test-rollback.sh`):
  Validates all three v0.4 rollback exit criteria:
  1. VM boots through UKI (calls `validate-uefi-boot.sh`)
  2. Three rollback entries are retained after simulated updates
  3. Simulated bad kernel is detected, system rolls back, and recovers

### Update Signing (v0.4)

- **Signing tools**:
  - `tools/sign_updates.py` — Python module using Ed25519 (`cryptography` library)
  - `tools/sign-updates.sh` — Shell wrapper using OpenSSL (fallback)
- **Key management**:
  - Test Ed25519 keys stored in `config/keys/`
  - Private key: `testing.private.pem` (git-ignored)
  - Public key: `testing.public.pem` (bundled in images)
- **Builder integration**: `rush-builder.py repo-init` uses real Ed25519
  signatures when keys are present, falls back to mock stubs otherwise.
- **Signing test** (`tools/test-sign-updates.sh`): Validates key generation,
  signing, verification, and tamper detection.

## Acceptance Criteria

Boot/update changes must update this file, `distro/boot/uki.toml`,
`distro/sysupdate/`, and the roadmap/status docs in the same change.

