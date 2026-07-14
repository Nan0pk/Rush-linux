# Build System Direction

The distro is source-built but not user-compiled by default. Source recipes
produce signed binary packages so installs and updates remain fast.

Initial build system requirements:

- Reproducible source recipes.
- Signed package metadata.
- Binary package repositories.
- Separate stable base and fast hardware enablement repositories.
- Kernel UKI outputs with rollback entries.
- Build logs and provenance retained for audit.

The recipe format in `recipes/` is intentionally small. It describes source,
verification, build features, and installed outputs. A future builder can map
these recipes to RPM/libdnf5-style repositories or an equivalent modern backend.

Do not build a custom dependency solver in the early project. That is a high
risk distraction from the distro's differentiator: adaptive runtime policy.

## Publishing Source

The source repository target is `https://github.com/Nan0pk/Rush-linux`.
Use `tools/publish-github.ps1` with `GH_TOKEN` or `GITHUB_TOKEN` when the
repository needs to be created non-interactively.
