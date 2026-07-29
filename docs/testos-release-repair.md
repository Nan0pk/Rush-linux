# testOS release repair trigger

LiveDev USB preparation requires release metadata that binds the downloadable image to its source commit and canonical testOS version. Legacy releases that predate `testos-image-commit.txt` and `testos-version.txt` are intentionally rejected by the installer.

To repair a missing or malformed prebuilt release without asking an operator to run GitHub Actions commands:

1. Update `testos/release-request.txt` on a reviewed branch to a new release tag.
2. Merge the change to `main`.
3. `.github/workflows/repair-testos-release.yml` validates the tag and dispatches `release-testos.yml` with a real image build.
4. The existing release workflow builds the workspace and image, publishes checksummed provenance sidecars, injects a verified smoke-test run intent, boots the image in QEMU, and publishes only after the smoke test succeeds.

The repair dispatcher is idempotent. If the requested GitHub Release already exists, it exits successfully without starting another build. Changing unrelated repository files does not trigger a release build.

The current repair request is `v0.7.0-beta.4-r1`. The release tag carries the correction suffix, while `testos-version.txt` continues to state the canonical image version from the repository `VERSION` file.
