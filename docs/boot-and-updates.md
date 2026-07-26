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
- **Key management (post-#338 review)**:
  - Test Ed25519 keys are **generated material** — never committed to the
    repository. The default key directory is `build/test-signing/keys/`
    (a build-output path that is already gitignored).
  - `.gitignore` rejects `*.private.pem` and `config/keys/*.private.pem`
    so a future tool cannot reintroduce private key material.
  - `tools/check-repo-hygiene.py` scans every tracked file for PEM
    private-key markers (`-----BEGIN PRIVATE KEY-----`,
    `-----BEGIN RSA PRIVATE KEY-----`,
    `-----BEGIN OPENSSH PRIVATE KEY-----`,
    `-----BEGIN EC PRIVATE KEY-----`) and rejects any match (with a
    narrow allow-list for the scanner itself and the log-capture
    redactor). This is a content-based scan, not a name-based scan, so
    a `.pem`-renamed or extension-less private key is still caught.
  - The historical `config/keys/testing.private.pem` and
    `config/keys/testing.public.pem` (added in PR #337) were deleted in
    the post-#337 repair. They were disposable test keys; no production
    code, trusted artifact, or published metadata relied on them. The
    evidence for this claim:
      1. `git grep -rn 'testing\.private\.pem' crates/ packaging/` —
         no production code references the key filename.
      2. `tools/test-sign-updates.sh` regenerates both keys at
         `build/test-signing/keys/` each run and never reads from
         `config/keys/` (the post-#337 repair removed the
         `config/keys/` fallback).
      3. `tools/rush-builder.py` looks for keys at
         `build/test-signing/keys/` first, with `config/keys/` as a
         legacy fallback for existing operator environments that
         pre-generate keys there. When neither location has keys, the
         builder falls back to mock signing (the only acceptable
         fallback — a missing dependency is a legitimate dev
         environment issue; other exceptions fail hard).
  - **No signing key rotation or history rewrite was required** because
    the deleted key was disposable test material, not a trusted
    credential. If a future audit discovers that a published image or
    trusted artifact was signed with the deleted key, rotate the key
    and document the affected artifacts at that time.
- **Public-key trust behavior**:
  - There is **no stable public test verification key bundled in
    images** at this time. Test keys are generated per-run by
    `tools/test-sign-updates.sh` and discarded after the suite exits
    (an EXIT trap cleans `build/test-signing/`). The signature manifest
    records the public-key path relative to the repo root so verifiers
    can locate the matching public key regardless of where the test
    key directory lives.
  - A future change that bundles a stable public verification key in
    images must document the trust root (where the key comes from, how
    it is rotated, and how clients verify it) in this section.
- **Builder integration**: `rush-builder.py repo-init` uses real Ed25519
  signatures when keys are present at `build/test-signing/keys/` (or the
  legacy `config/keys/` fallback), and falls back to mock signing only
  when keys are absent (a missing `cryptography` dependency or a
  missing key directory). Real signing failures (e.g. a corrupted key)
  fail hard rather than silently degrading to mock signing.
- **Signing test** (`tools/test-sign-updates.sh`): Validates key
  generation, signing, verification, and tamper detection. The suite
  includes a regression assertion that no private key material is ever
  written under `config/keys/`, and an EXIT trap that cleans
  `build/test-signing/` so the working tree stays clean.

## Acceptance Criteria

Boot/update changes must update this file, `distro/boot/uki.toml`,
`distro/sysupdate/`, and the roadmap/status docs in the same change.

