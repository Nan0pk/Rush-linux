# 0016 — mkosi and Arch Linux Archive Snapshot Pinning

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md
**Code:** packaging/mkosi/, tools/build-image.sh

* * *

## 0. Motivation

Rush Linux is built from source using `mkosi` (Make OS Image), which creates system images
from a Pacman package tree. To achieve **reproducible builds**, the Pacman package database
must be pinned to a specific snapshot from the Arch Linux Archive (ALA) rather than using
the current rolling-release mirror. Without snapshot pinning:
- Builds on different days produce different package versions
- A new package version might break optid's kernel ABI assumptions
- CI/CD cannot deterministically reproduce the build artefact

Research questions: How does mkosi interact with ALA snapshots? How is the snapshot URL
configured? What is the workflow for updating to a newer snapshot? How does optid's build
system verify the snapshot hash? What are the storage implications of pinned snapshots?

* * *

## 1. Findings

### 1.1 Arch Linux Archive (ALA) Structure

**Q: What is the ALA URL structure and how does pinning work?**

The Arch Linux Archive is a snapshot service maintained by the Arch community at
`https://archive.archlinux.org/` [PROVEN — documented on ArchWiki "Arch Linux Archive"]:

```
https://archive.archlinux.org/repos/YYYY/MM/DD/
├── core/          # core packages at that date's state
├── extra/         # extra packages
├── community/     # (removed in 2023; merged into extra)
└── multilib/      # 32-bit compat packages
```

To use ALA as the Pacman mirror, set in `/etc/pacman.d/mirrorlist`:
```
Server = https://archive.archlinux.org/repos/2026/06/01/$repo/os/$arch
```

This pins all `pacman -S` operations to the package versions available on 2026-06-01.
The ALA stores snapshots daily back to 2013-11-08 [PROVEN — ALA "about" page].

**Package database files** are also archived:
```
https://archive.archlinux.org/repos/2026/06/01/core/os/x86_64/core.db.tar.gz
```

### 1.2 mkosi Integration

**Q: How does mkosi consume the ALA snapshot URL?**

mkosi ≥ 20 (the version Rush Linux targets) reads distribution configuration from
`mkosi.conf` or `mkosi.conf.d/` [PROVEN — mkosi documentation, `mkosi.conf` man page]:

```ini
# packaging/mkosi/mkosi.conf
[Distribution]
Distribution=arch
Architecture=x86_64
Mirror=https://archive.archlinux.org/repos/2026/06/01

[Content]
Packages=
        linux
        linux-headers
        optid
        ...
```

The `Mirror=` key sets the Pacman `Server` for all repositories. mkosi writes a
temporary `mirrorlist` pointing to this URL when building the image.

**Snapshot date management** [HYPOTHESIS — best practice; mkosi has no built-in snapshot
version manager]:
Rush Linux stores the pinned date in a dedicated file:
```
packaging/mkosi/SNAPSHOT_DATE   # content: "2026-06-01"
```
`tools/build-image.sh` reads this file and substitutes it into `mkosi.conf` before
calling `mkosi build`. The CI job reads the same file, ensuring identical snapshots in
local and CI builds.

### 1.3 Package Hash Verification

**Q: How does the build system verify that the pinned packages have not changed?**

Pacman verifies package signatures using the `pacman-key` keyring [PROVEN — Arch Linux
package signing uses GnuPG, with keys in `/etc/pacman.d/gnupg/`]. mkosi imports the
Arch Linux keyring during image build.

**ALA package integrity**: Each package in ALA retains its original signature from when
it was built. A package downloaded from ALA on 2026-06-01 has the same GPG signature as
when it was first published — ALA does not re-sign packages [PROVEN — ALA is a static
mirror, not a re-packaging service].

**Rush Linux additional verification** [HYPOTHESIS — design; not yet implemented]:
`tools/build-image.sh` records a lock file `packaging/mkosi/SNAPSHOT_LOCK.toml` after
each successful build:
```toml
snapshot_date = "2026-06-01"
[packages]
linux = { version = "6.9.7.arch1-1", sha256 = "abc123..." }
optid = { version = "0.1.0-1", sha256 = "def456..." }
```
This lock file is committed to git, enabling reproducibility checks: `tools/verify-snapshot.sh`
downloads the listed packages and verifies their SHA256 hashes.

### 1.4 Snapshot Update Workflow

**Q: How does a maintainer update to a newer ALA snapshot?**

1. Edit `packaging/mkosi/SNAPSHOT_DATE` to the new date (e.g., `2026-07-01`)
2. Run `mkosi build` locally to test
3. If build succeeds and tests pass, update `SNAPSHOT_LOCK.toml` with the new package
   versions and hashes
4. Commit both `SNAPSHOT_DATE` and `SNAPSHOT_LOCK.toml` in the same git commit
5. CI runs the build against the new snapshot date in the PR

**Blocking conditions for snapshot update** [HYPOTHESIS — process design]:
- `linux` kernel version must be compatible with optid's sched_ext and BPF requirements
  (kernel ≥ 6.12 for sched_ext merge; ≥ 6.1 for MGLRU stable)
- `scx` package version must match the kernel's sched_ext ABI
- No `optid` binary package is available in ALA — it is built from source; only its
  dependencies (Rust toolchain, libdbus, etc.) are pinned

### 1.5 Local Mirror Fallback

**Q: What happens if ALA is unreachable during CI?**

ALA has historically had availability issues during high-traffic periods [PROVEN —
community reports on Arch forums].

**Fallback options** [HYPOTHESIS — design choices]:
1. **Cached Pacman DB and packages**: CI caches the pacman database and downloaded `.pkg.tar.zst`
   files in a persistent CI cache keyed on `SNAPSHOT_DATE`. If ALA is unreachable, CI uses
   the cache (only valid for the same snapshot date).
2. **Self-hosted ALA mirror**: Rush Linux project can host a minimal ALA mirror containing
   only the packages needed for the Rush Linux build. Storage: ~2 GB per snapshot date
   for the required package set [HYPOTHESIS — depends on package count; full ALA is ~40 GB
   per snapshot].
3. **GitHub Actions cache**: `actions/cache@v4` with key `ala-${{ env.SNAPSHOT_DATE }}`.
   Cache invalidation is automatic when SNAPSHOT_DATE changes.

Recommended: option 3 (GitHub Actions cache) for simplicity; self-hosted mirror as
long-term backup for critical builds.

### 1.6 mkosi Image Types

**Q: What image type does Rush Linux produce?**

mkosi supports multiple output formats [PROVEN — mkosi documentation]:
- `disk` — GPT disk image (for real hardware, UEFI boot)
- `uki` — Unified Kernel Image (kernel + initrd + cmdline signed as a single EFI binary)
- `tar` — tarball (for containers / further processing)
- `directory` — unpacked rootfs (fast iteration)

Rush Linux builds two artefacts from the same mkosi config [HYPOTHESIS — design]:
1. A `disk` image for installation on target hardware
2. A `uki` image for secure boot (see 0017 for UKI signing)

Both are built from the same `SNAPSHOT_DATE`-pinned package tree, ensuring the kernel
and initrd in the UKI match the rootfs packages.

* * *

## 2. Architecture Decisions

### Decision A: ALA Snapshot Date vs. Package Lock File

**Selected: Both** — SNAPSHOT_DATE determines which ALA mirror URL is used; SNAPSHOT_LOCK.toml
records the actual package versions and hashes for verification [HYPOTHESIS — date alone is
not sufficient because ALA packages can be retracted; hash lock is the reproducibility guarantee].

### Decision B: Pacman Signature Verification — Trust ALA Signatures

**Selected: Trust original Arch Linux package signatures (already verified by Arch keyring)**
[PROVEN — Arch package signing with GnuPG is a sufficient integrity guarantee for packages
that passed Arch's build system; no additional re-signing needed for the snapshot].

### Decision C: Snapshot Update Cadence

**Selected: Quarterly snapshot updates** (or when a critical kernel/driver update requires it)
[HYPOTHESIS — monthly is too frequent for stable release cycles; quarterly balances security
updates vs. build stability; emergency patches use kernel LTS backport patches instead].

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| ALA availability SLA | < 1 % build failure rate from ALA downtime | CI metrics: count ALA-related failures over 90 days |
| SNAPSHOT_LOCK verification time | < 30 s for full lock file verification | `tools/verify-snapshot.sh` timing on CI runner |
| Package set size for snapshot | Total download size ≤ 2 GB for required packages | `mkosi --dry-run` or list packages; sum `pacman -Si` sizes |
| Reproducibility | Identical SHA256 of root.img on two independent builds from same SNAPSHOT_DATE | Build twice on fresh CI runners; compare image hashes |
| Snapshot update CI time | Full image build ≤ 20 min | `time mkosi build` on standard CI runner (8 vCPU, 16 GB RAM) |

* * *

## 5. Non-Goals

- optid (the daemon) does not participate in build infrastructure — this brief covers the
  build tooling only.
- Rush Linux does not maintain a full ALA mirror — only the packages needed for the build.
- This brief does not cover binary package signing for the Rush Linux package repository
  (a separate future topic).
- This brief does not cover Pacman hook management inside the image.
- This brief does not cover `mkosi.extra/` file overlays — that is packaging configuration.

* * *

## 6. WP Relationship Map

| WP tag | How this brief addresses it |
|--------|-----------------------------|
| WP-N14 | Snapshot pinning ensures reproducible builds that are prerequisite for UKI signing (0017) |
| WP-N15 | SNAPSHOT_LOCK.toml provides the bill-of-materials for supply chain integrity |

* * *

## 7. Next Steps

**Immediate**
- Create `packaging/mkosi/SNAPSHOT_DATE` with current pinned date.
- Update `packaging/mkosi/mkosi.conf` to use `Mirror=` with the snapshot URL template.
- Implement `tools/build-image.sh` to substitute SNAPSHOT_DATE into mkosi.conf.

**Short-term**
- Implement `tools/verify-snapshot.sh` to validate SNAPSHOT_LOCK.toml hashes.
- Set up GitHub Actions cache for the ALA package cache.

**Medium-term**
- Implement automated snapshot update PRs: weekly CI job checks for new ALA snapshots
  and opens a PR updating SNAPSHOT_DATE + SNAPSHOT_LOCK if the build succeeds.
- Evaluate self-hosted ALA mirror for build reliability independence from ALA uptime.

* * *

## Appendix: Suggested Reading

- ArchWiki: "Arch Linux Archive" — ALA URL structure and usage
- mkosi documentation: `mkosi.conf` manual page
- Arch Linux package signing: `pacman-key --help` and `pacman.conf SigLevel`
- mkosi GitHub: `systemd/mkosi` — examples and configuration reference
- SLSA (Supply chain Levels for Software Artifacts): https://slsa.dev — framework for
  supply chain integrity levels; Rush Linux targets SLSA Level 2
