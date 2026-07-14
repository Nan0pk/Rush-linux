# ADR 0007: Project And D-Bus Naming

Status: accepted

## Context

The project is called **Rush Linux**, but the codebase used "Adaptive Linux"
interchangeably as a brand, and the public D-Bus identity was `io.adaptive.*`.
The D-Bus well-known name, object path, and interface name are public API: once
desktop environments or power-management GUIs integrate against them, renaming
requires a deprecation cycle and coordination with every downstream caller.
Fixing this before any external integration exists costs one change; fixing it
afterwards is a breaking change.

## Decision

1. The project's single brand name is **Rush Linux**. The brand string
   "Adaptive Linux" is retired from all docs, comments, and configuration.
2. The public D-Bus identity is:
   - interface: `io.rushlinux.Optid1`
   - well-known name: `io.rushlinux.Optid`
   - object path: `/io/rushlinux/Optid`
3. "adaptive" survives only as a **technical adjective** for the optimization
   approach and the kernel flavour (e.g. the `linux-adaptive` package, the
   "adaptive engine"). It is not a brand.

## Consequences

- `optid`, `optctl`, the D-Bus packaging files, and all docs use the
  `io.rushlinux` identity.
- Any future public interface follows the `io.rushlinux.*` namespace.
- The interface keeps its trailing version digit (`Optid1`) so a future
  incompatible interface can coexist as `Optid2`.
