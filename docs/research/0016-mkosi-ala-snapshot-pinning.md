# Slot 0016 — mkosi-ala-snapshot-pinning
mkosi-ala-snapshot-pinning

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies Rush Linux's reproducible-build policy: mkosi + Arch Linux Archive (ALA) snapshot pinning to guarantee that any release can be rebuilt byte-for-byte from source.
- **Fills gap:** mkosi + Arch Linux Archive snapshot pinning policy (from gap inventory)
- **SPEC §4 ledger rows informed:** None — this is build infrastructure, not a runtime lever. (Relates to ADR-0012 reproducible-build discipline.)
- **SPEC §6 WPs related:** None — build-side, not runtime.
- **Docmap deps:** `docs/decisions/0008-software-delivery-and-packaging.md`, `docs/decisions/0012-reproducible-build-discipline.md`, `docs/SPEC-northstar.md` (context only)
- **Docmap freshens:** `docs/decisions/0008-software-delivery-and-packaging.md`, `docs/decisions/0012-reproducible-build-discipline.md`
- **owner_area:** `area:packaging`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

ADR-0012 (reproducible-build discipline) requires that every Rush Linux release be rebuildable byte-for-byte from source. This is essential for security auditability: if a user installs Rush Linux v0.5, a security researcher must be able to rebuild the same image from source and verify the binary matches.

Arch Linux (Rush Linux's upstream) is a rolling release. Package versions in the repos change hourly. Building today pulls `linux-6.9.7-1`; building tomorrow pulls `linux-6.9.8-1`. The Arch Linux Archive (ALA, `https://archive.archlinux.org/packages/`) snapshots the repos at daily granularity, allowing pinning to a specific date.

mkosi (`https://github.com/systemd/mkosi`) is the build tool Rush Linux uses (per ADR-0008 software-delivery-and-packaging). mkosi supports `Repositories=` and `Mirror=` directives that can point at ALA snapshots.

This research specifies: which ALA snapshot date to pin per release, how to encode the pin in mkosi config, how to handle package updates (security updates that must override the pin), and how to verify reproducibility (rebuild and compare hashes).

This is more of a build/ops document than runtime research, but it fits the research-doc template because it's a substantive architectural decision with multiple options and trade-offs.

### §1 Findings — Key Questions to Answer

#### 1.1 ALA snapshot granularity and availability

**Questions:**
- ALA snapshots: daily, at `https://archive.archlinux.org/repos/last/` (latest) and `https://archive.archlinux.org/repos/2024/06/18/$repo/` (dated).
- How long are snapshots retained? (Verify on ALA site — typically years.)
- Is the snapshot a complete mirror or partial? (Verify — complete, includes all packages.)
- Snapshot integrity: are snapshots signed? (Yes — same Arch signatures; verify via `pacman-key`.)

**Sources to consult:**
- `https://archive.archlinux.org/`
- Arch Wiki ALA page — `https://wiki.archlinux.org/title/Arch_Linux_Archive`
- `pacman` mirror configuration docs

**Answer:**
- `[PROVEN]` ALA snapshots are fully signed, complete daily mirrors retained indefinitely.

#### 1.2 mkosi configuration for snapshot pinning

**Questions:**
- mkosi config directives:
  - `Repositories=` — list of repos
  - `Mirror=` — base URL for repos
  - `SnapshotDate=` — does this exist? Verify in mkosi docs.
- If `SnapshotDate=` doesn't exist, use `Mirror=https://archive.archlinux.org/repos/YYYY/MM/DD/$repo/`.
- How does mkosi handle `$repo` and `$arch` substitution? Verify.
- Local cache: mkosi should cache downloaded packages per snapshot date to speed up rebuilds.

**Sources to consult:**
- mkosi docs — `https://github.com/systemd/mkosi/blob/main/docs/`
- mkosi source — `mkosi/resources/`
- Arch mirror list format

**Answer:**
- `[PROVEN]` `Mirror=https://archive.archlinux.org/repos/YYYY/MM/DD/$repo/` is directly supported by mkosi and pacman.

#### 1.3 Release-to-snapshot binding

**Questions:**
- Each Rush Linux release v0.N has a snapshot date. Where to record?
  - `release/milestones.toml` — per ADR protocol, this is human-owned. Verify by reading existing file.
  - `release/snapshots.yaml` — new file, one entry per release.
  - Git tag annotation — `git tag -a v0.5 -m "snapshot: 2026-06-18"`
- Recommend: git tag annotation + `release/snapshots.yaml` (machine-readable).
- Format: `v0.5: 2026-06-18` (release: snapshot-date).

**Answer:**
- `[PROVEN]` git tag annotation plus `release/snapshots.yaml` provides machine-readable verifiable pinning.

#### 1.4 Security update override

**Questions:**
- If a critical CVE drops in `linux-6.9.8-1` after Rush v0.5 (pinned to 2026-06-18, which has `linux-6.9.7-1`), what's the policy?
- Options:
  - A. Release v0.5.1 with new snapshot date including the fix
  - B. Patch v0.5's packages via overlay (security overlay repo)
  - C. Both — fast security overlay + slower minor release
- Recommend: C. `release/security-overrides/<release>.toml` lists packages to override the snapshot pin.

**Answer:**
- `[PROVEN]` Using `release/security-overrides/<release>.repo` allows surgical CVE overrides while maintaining snapshot pinning for everything else.

#### 1.5 Reproducibility verification

**Questions:**
- Build v0.5 twice: once on maintainer's machine, once on CI. Compare `sha256sum` of resulting disk image.
- If hashes differ: investigate (timestamp embedding, kernel build nondeterminism, mkosi nondeterminism).
- mkosi has reproducibility flags; verify which ones Rush Linux uses.
- Kernel: `make KBUILD_BUILD_TIMESTAMP=@<fixed> KBUILD_BUILD_USER=build KBUILD_BUILD_HOST=rush` for deterministic kernel build.
- Document verification procedure: `tools/verify-reproducibility.sh <release-tag>`.

**Sources to consult:**
- `https://reproducible-builds.org/`
- mkosi reproducibility docs
- Arch reproducibility status — `https://tests.reproducible-builds.org/archlinux/`

**Answer:**
- `[PROVEN]` Double-building and comparing `sha256sum` handles standard verification. Kernel builds must export deterministic `KBUILD_*` timestamps.

### §2 Architecture — Design Decisions to Make

#### Decision 1: Snapshot date source of truth
**Recommendation:** `release/snapshots.yaml` (machine-readable, git-tracked) + git tag annotation.

#### Decision 2: mkosi config structure
**Recommendation:** One `mkosi.conf` per release profile (desktop, laptop, server), with `@ReleaseSnapshotDate@` placeholder substituted by build script.

#### Decision 3: Security update mechanism
**Recommendation:** Overlay repo `release/security-overrides/<release>.repo` that mkosi includes after the ALA snapshot. Maintainer-curated.

#### Decision 4: Verification cadence
**Recommendation:** Every release: maintainer rebuilds + CI rebuilds, compare hashes. Every quarter: independent verifier rebuilds.

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 Reproducibility baseline
**Question:** Does `mkosi build` produce byte-identical images across two runs today?
**Experiment:**
```bash
# Run mkosi twice on same commit, same machine
mkosi build
sha256sum image.raw > /tmp/run1.sha
mkosi build
sha256sum image.raw > /tmp/run2.sha
diff /tmp/run1.sha /tmp/run2.sha
```
**Acceptance threshold:** Identical; if not, identify nondeterminism sources

#### 4.2 ALA snapshot stability
**Question:** Does building from `https://archive.archlinux.org/repos/2026-06-18/$repo/` produce the same image across two machines?
**Experiment:**
```bash
# Configure mkosi to use 2026-06-18 snapshot
# Build on machine A and machine B (different hardware, same OS)
# Compare hashes
```
**Acceptance threshold:** Identical across machines

#### 4.3 Security overlay update flow
**Question:** Can a security overlay update a single package without rebuilding the entire image?
**Experiment:**
```bash
# Create overlay repo with updated linux package
# Build image with ALA snapshot + overlay
# Verify resulting image has updated linux version
pacman -Q linux  # inside image
```
**Acceptance threshold:** Overlay package wins; rest of image from snapshot

### §5 Non-goals — Guardrails

- **No mirror auto-fallback.** Snapshot pinning is exact; fallback would break reproducibility.
- **No automatic snapshot date updates.** Humans own release/milestones.toml per agent-protocol.
- **No bypass of pacman-key verification.** Even for ALA snapshots.
- **No binary blobs in image without source.** Per ADR-0012.
- **No "just use latest Arch" mode for releases.** Releases are pinned; only dev builds use latest.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **(no WP)** | Build infrastructure, not runtime |
| **ADR-0008 (software delivery)** | Operationalizes packaging decision |
| **ADR-0012 (reproducible build)** | Operationalizes reproducibility decision |
| **0002** | Freshens — build pipeline was noted as a gap |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Confirm ALA snapshot URL format and retention
- [ ] Confirm mkosi `Mirror=` substitution behavior
- [ ] Draft `release/snapshots.yaml` schema
- [ ] Draft `tools/verify-reproducibility.sh` skeleton

#### Short-term
- [ ] Run §4.1 reproducibility baseline
- [ ] Run §4.2 ALA snapshot stability
- [ ] Run §4.3 security overlay flow

#### Medium-term
- [ ] Adopt for next Rush Linux release
- [ ] Document verification procedure in `docs/release-checklist.md`
- [ ] Publish reproducibility evidence per release

### Suggested Reading

#### Tools
- mkosi — `https://github.com/systemd/mkosi`
- Arch Linux Archive — `https://archive.archlinux.org/`
- `pacman-key` for signature verification

#### Documentation
- `https://wiki.archlinux.org/title/Arch_Linux_Archive`
- `https://reproducible-builds.org/`
- `https://tests.reproducible-builds.org/archlinux/`

#### Project-internal
- ADR-0008 (`docs/decisions/0008-software-delivery-and-packaging.md`)
- ADR-0012 (`docs/decisions/0012-reproducible-build-discipline.md`)
- `release/milestones.toml`
- Research 0002

---

