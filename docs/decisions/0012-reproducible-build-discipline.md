# ADR 0012: Reproducible Build Discipline

Status: proposed

> Marked **proposed**; needs human ratification. Addresses review item B5.

## Context

The packaging docs state builds "should produce reproducible binary packages"
and the v0.3 exit criteria mention comparing file manifests for reproducibility.
But `tools/rush-builder.py` was optimised for "no external dependencies", not
determinism: it walks the tree with `os.walk()` (filesystem-order dependent),
writes gzip tarballs with embedded mtimes, and pins no tool versions or
environment. The stated goal and the implementation are not yet compatible.

## Decision (proposed)

Adopt the reproducible-builds baseline in the builder and make it a checked
invariant:

1. **Deterministic file ordering** — sort entries before adding to archives
   (do not rely on `os.walk()` order).
2. **Deterministic timestamps** — honour `SOURCE_DATE_EPOCH`; clamp archive
   member mtimes to it.
3. **Deterministic archive metadata** — normalise uid/gid/owner/mode; use a
   reproducible gzip (no embedded name/mtime) or prefer `zstd`/`tar` with fixed
   settings.
4. **Pinned toolchain** — record the toolchain versions used to build each
   package in its metadata (`--locked` is already used for cargo).
5. **Verification** — the build acceptance step builds twice and asserts
   identical content checksums (not just file-name manifests).

## Consequences

- `tools/rush-builder.py` changes: sorted walks, `SOURCE_DATE_EPOCH` support,
  normalised tar metadata, toolchain capture.
- The v0.3 "compare file manifests" criterion is upgraded to "byte-identical
  rebuild" where practical.
- Reproducibility becomes verifiable in CI, which the v0.9 RC integrity gate
  depends on.
