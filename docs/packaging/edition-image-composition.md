# Compose a bootable edition image

`tools/build-edition-image.sh` is the one-command product path for Rush Linux editions. It builds or reuses the common headless base image, builds the selected edition as a system extension, installs that extension into a private copy of the base image, and emits a bootable edition artifact.

## Local development image

```bash
sudo tools/build-edition-image.sh \
  --edition desktop \
  --unsigned-development
```

Default outputs:

- final image: `build/rush-linux-desktop.raw`;
- edition workspace: `build/edition-desktop/`;
- composition receipt: `build/rush-linux-desktop.raw.compose.json`.

`--unsigned-development` is deliberately explicit. It is not accepted as a release-signing substitute.

## Signed image

```bash
sudo tools/build-edition-image.sh \
  --edition laptop \
  --key /secure/path/mkosi.key \
  --certificate /secure/path/mkosi.crt
```

The key and certificate are forwarded only to the sysext build. The final composition receipt records hashes and signing state, not private-key material.

## Reuse an existing common base

```bash
sudo tools/build-edition-image.sh \
  --edition realtime-audio \
  --base-image build/rush-linux.raw \
  --key /secure/path/mkosi.key \
  --certificate /secure/path/mkosi.crt
```

Without `--base-image`, the composer invokes `tools/build-mkosi-image.sh --edition server`. That command builds the unprofiled common base from `mkosi/mkosi.conf`; product profiles are sysext payloads and are never applied as whole-image profiles. `--clean-base` requests a clean common-base rebuild. Supplying `--base-image` avoids rebuilding the base when composing several editions from the same version.

## Composition flow

The command performs one product flow:

1. build or select the common server base;
2. build the requested edition with `build-edition-sysext.py`;
3. create a reflink-capable sparse copy of the common base;
4. mount only that private copy with `systemd-dissect`;
5. install the extension with `deploy-edition-sysext.py`;
6. unmount the image before publishing it;
7. atomically replace the requested output and write a composition receipt.

The common base is never modified. A failed sysext build, mount, deployment, or unmount leaves any existing output image untouched.

## Requirements

The build host needs the normal Rush/mkosi dependencies plus `systemd-dissect`. Image mounting and package/image construction normally require root privileges.

Use `--force` only when intentionally replacing an existing composed output. The common base path can never be used as the output path.
