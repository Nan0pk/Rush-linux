# Versioning

Rush Linux uses SemVer for project releases and a Rush revision suffix for
distribution packages.

## Project Versions

Format:

```text
MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]
```

Examples:

- `0.1.0-alpha.1`
- `0.5.0-beta.1`
- `1.0.0-rc.1`
- `1.0.0`
- `0.8.0-dev+20260525.2862ac4`

## Meaning

- `MAJOR`: incompatible upgrade path, distro architecture change, policy API
  break, package format break, or unsupported migration.
- `MINOR`: new subsystem, edition, installer capability, optimizer feature,
  supported hardware class, or compatible package/update feature.
- `PATCH`: security fix, bug fix, packaging fix, benchmark harness fix, docs
  correction, or non-breaking policy improvement.

## Pre-Release Labels

- `alpha`: architecture is still moving; not installable for normal users.
- `beta`: installable test images exist; defaults can still change.
- `rc`: feature freeze; only release blockers should change.
- no suffix: stable release.

## Current Version

`VERSION` is the source of truth for the repository version. The current value
is `0.3.0-alpha.1`, which means the repository has implemented the rootfs
and package builder MVP, and is preparing for the `v0.4.0-alpha.1` (UKI, Boot, Rollback, Updates) gates.

## Package Versions

Package versions keep the upstream version plus a Rush revision:

```text
linux-adaptive-6.x.y-rush1
optid-0.3.0-rush1
systemd-<upstream>-rush1
```

Increment the Rush revision when Rush packaging, patches, build flags, policy,
or integration changes while the upstream version stays the same.

## Channels

- `unstable`: every merged development build.
- `alpha`: early milestone images and packages.
- `beta`: installable test releases.
- `rc`: release candidates.
- `stable`: supported user-facing releases after `1.0.0`.
- `security`: urgent security updates for supported stable releases.

Only `stable` and `security` are user-facing after `1.0.0`.

## Tagging Rules

- Tag names must match `v<version>`, for example `v0.1.0-alpha.1`.
- Every tag must have release notes in `RELEASES.md`.
- Every release must identify the passed test tiers.
- Never tag a version by weakening tests or documentation requirements.

