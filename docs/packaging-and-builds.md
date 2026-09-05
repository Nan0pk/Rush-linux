# Packaging And Builds

Rush currently composes Arch packages with mkosi and builds its own Rust
components from source. It is not yet a fully source-built distribution. Users
should receive built, signed release artifacts; compiling the world locally is
not the intended user experience.

The supported whole-image entrypoint is `tools/build-mkosi-image.sh` (common
server base or operational LiveDev image). Product editions use
`tools/build-edition-image.sh`. For isolated snapshot/local-package experiments,
see [build-system.md](build-system.md) and the
[source-build comparison plan](plans/source-build-experiment.md).

## Historical recipe model

- Source recipes live under `recipes/`.
- Recipes describe source, verification, build features, and installed outputs.
- Builds should produce reproducible binary packages.
- Package metadata and packages must be signed before any installable release.
- The base OS and fast hardware enablement layer should be separable.

## Retained bootstrap implementation

Implemented recipe skeletons:

- `recipes/core/linux.toml`
- `recipes/core/linux-rt.toml`
- `recipes/core/optid.toml`
- `recipes/core/systemd.toml`
- `recipes/desktop/plasma-wayland.toml`
- `recipes/server/minimal.toml`

Implemented helpers:

- `tools/build-rootfs.sh` creates a Linux rootfs skeleton from checked-in
  config files. It remains a lightweight scaffold helper.
- `tools/rush-builder.py` is the package/rootfs/VM bootstrap builder. It can
  build recipe archives, initialize local package metadata and mock signatures,
  populate a rootfs from recipe dependencies, assemble a UKI plus initrd from
  cached base assets, stage the systemd-boot fallback loader and UKI menu entry,
  and ask `systemd-repart` to produce a GPT VM disk image.
- `tools/build-vm-final.sh` is the current Linux-host integration path for the
  v0.3/v0.4 VM image. It installs release-built `optid`/`optctl` binaries,
  installs `udev` so generated serial/device units are satisfied, repairs
  Ubuntu Base `/dev` nodes after extraction, writes the same systemd-boot loader
  files into the ESP staging tree used by the UKI boot path, and includes the
  ext4 module in the initrd.
- `tools/validate-uefi-boot.sh` runs the generated `disk.raw` under QEMU/OVMF
  and checks the boot log for the fallback UEFI boot path, UKI initrd load,
  VM root command line, `multi-user.target`, and `optid.service`.

## Recipe Schema Versioning

Every recipe declares its schema version explicitly:

```toml
[package]
schema_version = 0
name = "..."
```

Rules:

- `tools/rush-builder.py` records the highest version it understands in
  `SUPPORTED_SCHEMA_VERSION`. It **rejects** a recipe whose `schema_version`
  is newer than it supports, and **warns** when the field is missing.
- The version is propagated into per-package metadata (and therefore
  `repodata.json`) so consumers can tell which schema produced a package.
- When the schema changes incompatibly, bump `SUPPORTED_SCHEMA_VERSION`, keep
  the builder able to read the previous version for at least one release, and
  ship a migration note (and, if churn is large, a `recipe-migrate`
  subcommand) so existing recipes can be upgraded mechanically rather than by
  hand. This is the migration path the v0.9 schema freeze depends on.

## Package Backend Direction

Do not build a custom dependency solver early. Use a mature signed metadata
backend such as a DNF5/libdnf5-style stack or an equivalent modern package
backend once package production begins. The `deps`-list resolution in
`tools/rush-builder.py` is an MVP bootstrap aid, not the long-term solver; the
historical package-backend proposal is ADR 0008 and the mkosi/Arch proposal is
ADR 0014. Both remain proposed; neither should be misrepresented as ratified.
Current code uses mkosi/Arch, as described above.

## Build Acceptance Criteria

- Source recipe changes must keep docs updated.
- Build outputs must be reproducible where practical.
- Package metadata must be signed before installable release artifacts.
- CI must reject missing core recipes and obsolete default choices.
