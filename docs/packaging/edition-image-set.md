# Build the complete edition set

`tools/build-all-editions.sh` builds a release-shaped set of Rush Linux images from one common base. The default edition list comes from the canonical product manifests, so operational profiles such as LiveDev and testOS are not published as product editions.

## Local development set

```bash
sudo tools/build-all-editions.sh --unsigned-development
```

This builds the common server base once, then composes desktop, laptop, realtime-audio, and server images from that exact base.

Default outputs:

- image set: `build/edition-set/`;
- reusable workspaces: `build/edition-workspaces/`;
- image index: `build/edition-set/edition-set.json`.

## Signed release set

```bash
sudo tools/build-all-editions.sh \
  --key /secure/path/mkosi.key \
  --certificate /secure/path/mkosi.crt \
  --clean-base
```

The generated index records the common-base hash and, for every edition, the image size, image SHA-256, composition receipt, and composition-receipt SHA-256.

## Build selected editions

```bash
sudo tools/build-all-editions.sh \
  --editions desktop laptop \
  --base-image build/rush-linux-server.raw \
  --unsigned-development
```

Supplying `--base-image` reuses an existing common base and avoids rebuilding it. The same base path is passed to every edition composition.

## Publication behavior

The set is constructed in a private staging directory. It becomes visible at the requested output path only after every selected edition succeeds and the final index is written. A failed edition leaves an existing published set unchanged. `--force` replaces the old set only after the replacement is complete.
