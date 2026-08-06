# Deploying edition system extensions

`tools/deploy-edition-sysext.py` turns a built edition artifact into an installed Rush Linux edition. It consumes the canonical plan and build receipt emitted by `tools/build-edition-sysext.py`, verifies their hashes and target compatibility, and installs the image in the systemd-standard `/var/lib/extensions/` location.

## Install into a prepared root tree

```bash
sudo python3 tools/deploy-edition-sysext.py install \
  --plan build/desktop/edition-plan.json \
  --receipt build/desktop/output/rush-linux-desktop.build.json \
  --root build/rootfs
```

Release installs require a signed build receipt. An unsigned local artifact is accepted only with `--allow-unsigned-development`.

The command:

- verifies the plan hash, artifact size, and artifact SHA-256;
- checks the target root's `ID` and `VERSION_ID` against the extension plan;
- atomically installs the image as `/var/lib/extensions/<extension-id>.raw`;
- records installed state under `/var/lib/rush-linux/editions/`;
- enables `systemd-sysext.service` for early boot activation.

This makes the command directly usable while composing a VM root tree before the disk image is packed.

## Install or update a running system

```bash
sudo python3 tools/deploy-edition-sysext.py install \
  --plan edition-plan.json \
  --receipt output/rush-linux-desktop.build.json \
  --root / \
  --activate \
  --force
```

`--activate` runs `systemd-sysext --root=/ refresh` after the atomic replacement. If activation fails, the previous artifact and installed-state record are restored automatically.

## List installed editions

```bash
python3 tools/deploy-edition-sysext.py list --root /
python3 tools/deploy-edition-sysext.py list --root / --json
```

## Remove an edition

```bash
sudo python3 tools/deploy-edition-sysext.py remove \
  --extension-id rush-linux-desktop \
  --root / \
  --activate
```

Removal is also transactional: a failed refresh restores the removed image and its state record.

## Product boundary

This tool owns deployment lifecycle, not artifact construction. `build-edition-sysext.py` creates and signs the image; `deploy-edition-sysext.py` installs, activates, updates, lists, and removes it. The extension remains subject to systemd-sysext's normal compatibility and additive-content model.
