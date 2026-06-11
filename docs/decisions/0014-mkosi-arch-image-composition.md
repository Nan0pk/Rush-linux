# ADR 0014: Image Composition With mkosi On An Arch Base

Status: proposed

> Decided by the project owner in the 2026-06-10 strategy session (recorded in
> `docs/plans/handover-2026-06-10-ecosystem-and-benchmarks.md` §1) but marked
> **proposed** pending formal ratification: per the ADR lifecycle, only a human
> maintainer may set `accepted` with a `Ratified-by:` line.

## Context

Rush Linux currently builds its rootfs with a bespoke pipeline
(`tools/rush-builder.py`, `recipes/`, `tools/build-vm-final.sh`): source
recipes, an ad-hoc dependency resolver, and a hand-assembled boot layout. ADR
0008 already reframed that pipeline as MVP bootstrap scaffolding, but left the
replacement open. The pipeline competes for effort with the project's actual
differentiators (`optid`, boot/rollback integrity, sched_ext), and reproducing
a maintained package universe is not winnable work for this project's size.

The strategic requirements are: newest kernels (sched_ext and MGLRU mature
release-by-release), a proven path for running scx schedulers, reproducible
image builds, and compatibility with the existing systemd-native boot stack
(UKI + systemd-boot + sysupdate, ADR 0003).

## Decision (proposed)

1. **Rush images are composed with mkosi** from an upstream package base.
   mkosi is systemd's own image builder and natively produces the layout Rush
   already validates: UKI, systemd-boot, dm-verity partitions, sysupdate
   transfer definitions.
2. **The package base is Arch Linux.** Rolling release delivers the newest
   kernels fastest; CachyOS demonstrates the scx-on-Arch path in production.
   Reproducibility comes from pinning each release to an Arch Archive
   snapshot date, plus pinning the mkosi version (extends ADR 0012).
3. **The bespoke builder is retired strangler-fig style.** `tools/rush-builder.py`
   and most of `recipes/` are removed only after the mkosi image passes the
   EXISTING `tools/validate-uefi-boot.sh` and `tools/test-rollback.sh`
   unmodified, twice, with transcripts attached to the removal PR.
4. **An image-policy test gates every image build:** boot the image and assert
   cgroup v2 unified hierarchy, nftables loaded, PSI active, no `tlp.service`
   (or other competing policy daemon) present. This stops the Arch base from
   silently reintroducing banned defaults.
5. **Abort criterion:** if boot-validation parity is not reached within
   roughly three agent sessions of spike work, fall back to a hybrid: mkosi
   for rootfs assembly, current scripts for the boot layout.

## Consequences

- Supersedes the base-OS *build mechanics* of ADR 0008: the recipe/rootfs
  builder path ends. ADR 0008's two-plane *delivery* model survives with one
  amendment: the package plane's DNF5/RPM backend choice is obsolete on an
  Arch base — base-system packages enter images at compose time from pinned
  Arch repos, and there is no user-facing system package manager in the base
  image. The application plane (Flatpak-first) is unchanged.
- The custom Rush package manager work item is cancelled (it was never
  built; `repodata.json` remains build metadata only).
- Editions become mkosi profiles on one base image (roadmap v0.7 remap).
- New pins to manage per release: mkosi version, Arch Archive snapshot date.
- ParticleOS (systemd upstream) is the reference architecture for mkosi
  profiles, user-key Secure Boot, and verity layout — referenced, not
  depended on.

## Alternatives considered

- **Keep the bespoke builder.** Rejected: unbounded maintenance for zero
  differentiation; admission rule "net-negative maintenance" fails.
- **Debian/Fedora base.** Rejected for the primary editions: slower kernel
  cadence directly delays sched_ext/MGLRU improvements that Rush's thesis
  depends on. Fedora also ships competing policy defaults (tuned) that would
  have to be stripped each release.
- **OSTree/bootc/composefs.** Rejected (handover §3.3): a second
  update/integrity mechanism alongside sysupdate+UKI+dm-verity violates the
  "one mechanism per problem" admission rule.
