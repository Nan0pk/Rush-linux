# ADR 0014: Image Composition via mkosi on Arch Linux

Status: proposed

> This ADR proposes adopting mkosi-based image composition on top of an Arch Linux package base, superseding the package-based plane (RPM/DNF5) proposed in ADR 0008.

## Context

ADR 0008 proposed a two-plane model: an image-based Base OS plane via `systemd-sysupdate` and a package-based plane reusing standard RPMs and DNF5. However, maintaining a custom recipe system (`tools/rush-builder.py` and `recipes/`) alongside DNF5 repositories creates significant maintenance overhead without delivering a corresponding strategic advantage. 

We need a simpler, highly reproducible way to compose the base OS images from an existing, up-to-date package repository that supports modern kernel features (such as sched_ext and MGLRU) out of the box.

## Decision (proposed)

1. **Adopt mkosi for image composition:** Replace the custom package builder (`tools/rush-builder.py`) and recipe-based system with `mkosi` as the primary image composition tool.
2. **Base distro: Arch Linux:** Use Arch Linux as the upstream package base. Arch's rolling release model provides the newest kernels and user-space components required for modern Linux schedulers and features (e.g., sched_ext/MGLRU).
3. **Reproducible builds:** Enforce build reproducibility by pinning the Arch Linux Archive snapshot dates in the mkosi configuration.
4. **Strangler-fig transition:** Retain the current recipe-based builder only as a bootstrap scaffolding. It will be retired once the mkosi-composed Arch images reach feature and validation parity.
5. **Base OS plane:** The base system remains image-composed and is delivered/updated as signed images via `systemd-sysupdate`.

## Consequences

- The custom recipe system (`recipes/`) and packaging tool (`tools/rush-builder.py`) will be deprecated and eventually removed.
- Parity validation is required: the new mkosi/Arch images must pass existing `validate-uefi-boot.sh` and `test-rollback.sh` checks.
- Image configuration becomes standard mkosi profiles.
- Dependencies on external package repositories are resolved at image build time rather than runtime on the target system.
