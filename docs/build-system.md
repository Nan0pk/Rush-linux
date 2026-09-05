# Build system

Rush currently composes an Arch package base with mkosi and compiles its own Rust
components. Historical source recipes remain bootstrap material. A fully
source-built distribution is an experimental option, not a completed capability.
Rust builds use the committed Cargo lockfile (`--locked`), so a build cannot
silently rewrite dependency resolution.
The common base emits mkosi's JSON package manifest. Retain it with the image;
also record hashes for Rush binaries and other staged files, which are not
distribution packages and are not covered by the package inventory alone.

## Supported entrypoints

- `bash tools/build-mkosi-image.sh --edition server`: common headless base.
- `bash tools/build-mkosi-image.sh --edition livedev`: operational development
  image, not a substitute for a finished consumer desktop.
- `bash tools/build-edition-image.sh --edition desktop --unsigned-development`:
  compose a desktop from the common base and its extension. Laptop and realtime
  audio use the same product-edition path. Unsigned development is not release
  signing.

Use a supported Linux build host with Cargo/Rust, mkosi, the target package
manager and keys, systemd/ukify, and required image/boot tools. CI's product-image
job records its Arch build environment. The builder checks Cargo and mkosi
before changing its staging tree; real image resolution/building validates the
remaining host requirements. A successful build does not prove physical support.

## Isolated source-build experiments

```sh
bash tools/build-mkosi-image.sh --edition server --snapshot 20260904 --plan
```

`--plan` prints the intended compilation and mkosi commands and returns before
staging, compilation, cleaning or image construction. It needs neither Cargo
nor mkosi. It validates argument shape and directory existence, not snapshot
availability or package contents. `--clean --plan` also performs no cleanup.

Add repeatable `--package-dir DIR` to make locally rebuilt Arch packages
available to mkosi. Paths are resolved relative to the invocation directory,
including paths with spaces. Build packages with the existing Arch tooling and
retain their source verification and provenance. mkosi's package resolver still
selects what is installed: inspect installed versions and hashes in the resulting
image instead of assuming a supplied package was used. No signature/key-check
option is weakened by this wrapper.

`--snapshot YYYYMMDD` selects an explicit Arch archive date. Both controls and
treatments must use the same confirmed snapshot unless the snapshot itself is
examined. Pin the mkosi version and all other inputs separately; this flag alone
is not a reproducible-build guarantee.

The real build uses the same command without `--plan`. Use separate clean
checkouts to retain both arm outputs; each builder writes `build/` inside its
checkout and `--clean` deletes that checkout's prior build output. Keep product
extension inputs identical when composing from different `--base-image` files.

The [comparison plan](plans/source-build-experiment.md) defines the first
workload, evidence, proposed margins, and conditions for considering a full
source-built base. Use upstream build/packaging tools; do not introduce a custom
dependency solver or make end users compile their OS.

## Upstream interfaces

The implementation uses mkosi's `Snapshot=` and
`PackageDirectories=/--package-directory` interfaces, documented in its
[manual](https://github.com/systemd/mkosi/blob/main/mkosi/resources/man/mkosi.1.md).
Check the exact pinned tool version before real collection. Reproducibility
requires independent builds with matching unsigned artifacts, not just a
configuration comment or a successful `--plan`.
