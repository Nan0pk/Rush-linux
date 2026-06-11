# ADR 0008: Software Delivery And Packaging Strategy

Status: proposed

> Superseded in part by ADR 0014 (proposed): the base-OS build mechanics move
> to mkosi on an Arch base, and the package plane's DNF5/RPM backend choice is
> withdrawn. The two-plane delivery model (image-based base, Flatpak-first
> applications) remains in force here.

> This ADR makes a binding architectural call about what kind of distro Rush
> Linux is. It is the single biggest gap in the project (review items A1, B2,
> C6). It is marked **proposed** and needs human ratification before it becomes
> accepted.

## Context

Rush Linux has a recipe format, a rootfs builder, a `repodata.json` metadata
format, and signing stubs — i.e. the beginnings of a package ecosystem — but no
answer to the question a real user asks first: *"how do I install a browser?"*
Specifically, the project currently has:

- no package manager binary and no client that consumes `repodata.json`;
- no committed dependency-solver decision (the `deps`-list resolution in
  `tools/rush-builder.py` is a primitive ad-hoc solver, contradicting the
  packaging guidance to "not build a custom solver early");
- no application-layer stance (Flatpak / AppImage / Snap unaddressed);
- no resolution between **image-based** updates (`systemd-sysupdate`, whole
  partition flips) and **package-based** updates implied by the recipe system —
  these are different models being used at once (review C6);
- no story for who upgrades `optid` itself, or what happens to user-modified
  `policy.toml` on upgrade.

These are entangled: the recipe schema was frozen at v0 without knowing which
way package resolution goes, and the v0.4 update work risks being wasted if the
image-vs-package question is forced later.

## Decision (proposed)

Adopt a **two-plane** model and stop pretending the project owns a from-scratch
package ecosystem:

1. **Base OS plane — image-based.** The base system (kernel/UKI, systemd,
   `optid`, core network/firewall) is delivered and updated as signed images via
   `systemd-sysupdate` with A/B-style rollback (ADR 0003). `optid` itself
   upgrades on this plane, so there is always a single owner of the base.
2. **Package plane — reuse a mature backend.** For additional system packages,
   do **not** ship a custom solver. Produce RPMs and serve them with a
   **DNF5/libdnf5** stack. `repodata.json` and `tools/rush-builder.py`'s
   `deps` resolution are explicitly **MVP bootstrap scaffolding**, not the
   shipping mechanism; they will be replaced by (or made to emit) standard RPM
   repo metadata. This retires review item B2.
3. **Application plane — Flatpak.** For a Wayland-first desktop, end-user
   applications are delivered as **Flatpak** by default (Flathub or a Rush
   remote). AppImage is supported but not curated; Snap is out of scope.
4. **Config upgrades.** Default policy ships under `/usr/lib/optid/policy.toml`;
   user overrides live under `/etc`. New defaults never overwrite user files
   (standard drop-in/`/etc` precedence). `/run/optid` is runtime-only state and
   is not preserved across upgrades.

## Consequences

- The recipe/`repodata.json` path is reframed as a build/bootstrap tool, which
  unblocks the v0 schema freeze (it no longer has to also be the user-facing
  package format).
- A concrete user answer exists: base via image updates, system packages via
  DNF5, apps via Flatpak.
- Work items this creates: choose RPM as the package output format; produce
  libdnf5-compatible repo metadata; add a Flatpak remote/recipe; document the
  `optid` self-upgrade path. These become roadmap items for v0.4–v0.6.

## Alternatives considered

- **Own full package ecosystem (custom solver + format).** Rejected: invents
  enough to be incompatible with everything without finishing enough to be
  usable, and contradicts existing packaging guidance.
- **Pure image distro, no package plane.** Viable for the server edition but
  leaves desktop users unable to install system-level software; rejected as the
  only model.
