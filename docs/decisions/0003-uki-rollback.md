# ADR 0003: Use UKI-First Boot With Rollback

Status: accepted

## Context

Kernel and optimizer policy changes can break boot or degrade hardware behavior.
The distro needs a boot path that supports signed artifacts, measured boot, and
rollback.

## Decision

Use Unified Kernel Images as the default UEFI boot artifact, systemd-boot where
supported, and GRUB as a compatibility fallback. Keep multiple rollback entries.

## Consequences

- Kernel packages must produce UKI outputs.
- Update descriptors must account for kernel and base OS rollback.
- Installable releases must test failed-boot recovery.
- Boot docs and `distro/boot/uki.toml` must stay aligned.

