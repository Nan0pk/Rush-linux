# Edition system extensions

Rush Linux editions have two canonical inputs:

- `distro/editions/<edition>.toml` defines product identity, policy defaults,
  inheritance, and base-system requirements.
- `mkosi/mkosi.profiles/<edition>/mkosi.conf` defines the complete package
  payload that belongs in that edition's system extension.

`tools/build-edition-sysext.py` resolves both inputs and generates a standalone
mkosi workspace using `Format=sysext` and `Overlay=yes`.

## Why both inputs are required

The edition manifest is not a package-manager input. Names such as
`adaptive-desktop-plasma`, `linux-adaptive-rt`, and `optid` describe Rush
components and base requirements. They must not be passed blindly to pacman.

The mkosi profile contains the actual extension package payload. The generated
plan keeps the two sets separate:

- `packages.edition_requirements` records the complete product requirements;
- `packages.sysext` records only packages installed into the extension image.

This is especially important for the realtime-audio edition. Its
`linux-adaptive-rt` requirement belongs to the bootable base/kernel build, not
to a system extension that is merged after the system has already booted.

Edition inheritance merges policy defaults. Each mkosi profile remains a
complete, explicit extension payload; package lists are never silently unioned
across editions.

## Inspect the resolved plan

```sh
python3 tools/build-edition-sysext.py list

python3 tools/build-edition-sysext.py plan \
  --edition laptop
```

The plan includes source hashes for every inherited edition manifest and the
matching mkosi profile, effective defaults, base OS identity, extension image
identity, and both package sets.

## Prepare a build workspace

The base tree may be a directory or image accepted by mkosi `BaseTrees=`:

```sh
python3 tools/build-edition-sysext.py prepare \
  --edition desktop \
  --base-tree build/base \
  --workspace build/sysext/desktop
```

The workspace contains:

```text
edition-plan.json
mkosi.conf
mkosi.images/rush-linux-<edition>/mkosi.conf
tree/usr/lib/extension-release.d/extension-release.rush-linux-<edition>
tree/usr/lib/rush-linux/editions/<edition>.json
base -> <selected base tree or image>
```

The extension filename and extension-release filename use the same
`rush-linux-<edition>` identity. Compatibility is bound to:

```text
ID=rush-linux
VERSION_ID=<Rush base version without prerelease suffix>
ARCHITECTURE=<systemd architecture identifier>
```

The full prerelease version remains recorded as `SYSEXT_VERSION_ID`.

A non-empty workspace is never deleted unless it contains the builder's marker
file and `--force` is supplied.

## Build a signed extension

Production builds require both signing inputs:

```sh
python3 tools/build-edition-sysext.py build \
  --edition laptop \
  --base-tree build/base \
  --workspace build/sysext/laptop \
  --key /secure/release/mkosi.key \
  --certificate /secure/release/mkosi.crt
```

The key and certificate are exposed to mkosi through temporary symlinks and are
removed after the invocation, including failure paths. Private key material is
not copied into the generated workspace or build receipt.

For a local throwaway build only:

```sh
python3 tools/build-edition-sysext.py build \
  --edition server \
  --base-tree build/base \
  --workspace build/sysext/server \
  --unsigned-development
```

Unsigned output is explicitly marked in the build receipt and must not be
published as a release artifact.

Successful builds produce:

```text
output/rush-linux-<edition>.raw
output/rush-linux-<edition>.raw.sha256
output/rush-linux-<edition>.build.json
```

The receipt records the artifact size and SHA-256, the canonical plan hash,
whether signing was requested, and the signing certificate hash without
recording private key paths.

## Current boundary

This builder creates edition sysext artifacts from the existing profiles. It
does not claim that the base image, PREEMPT_RT kernel package, repository
metadata, key enrollment, VM boot, or physical-hardware acceptance has already
been completed. Those remain separate build and release tasks.
