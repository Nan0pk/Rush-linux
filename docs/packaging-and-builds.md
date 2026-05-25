# Packaging And Builds

Adaptive Linux is source-built by the project, but users should install signed
binary packages. Compiling the world locally is not the default user experience.

## Model

- Source recipes live under `recipes/`.
- Recipes describe source, verification, build features, and installed outputs.
- Builds should produce reproducible binary packages.
- Package metadata and packages must be signed before any installable release.
- The base OS and fast hardware enablement layer should be separable.

## Current State

Implemented recipe skeletons:

- `recipes/core/linux.toml`
- `recipes/core/linux-rt.toml`
- `recipes/core/optid.toml`
- `recipes/core/systemd.toml`
- `recipes/desktop/plasma-wayland.toml`
- `recipes/server/minimal.toml`

Implemented helper:

- `tools/build-rootfs.sh` creates a Linux rootfs skeleton from checked-in
  config files. It is not yet a package builder.

## Package Backend Direction

Do not build a custom dependency solver early. Use a mature signed metadata
backend such as a DNF5/libdnf5-style stack or an equivalent modern package
backend once package production begins.

## Build Acceptance Criteria

- Source recipe changes must keep docs updated.
- Build outputs must be reproducible where practical.
- Package metadata must be signed before installable release artifacts.
- CI must reject missing core recipes and obsolete default choices.

