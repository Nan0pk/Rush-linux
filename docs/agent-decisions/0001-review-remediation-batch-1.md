# AD-0001: Expert-review remediation, batch 1

- Date: 2026-06-07
- Agent: Claude Code (agent session)
- Human sign-off: pending review
- Prompt/intent: Triage the external "Rush Linux — Complete Expert Review" and
  begin fixing the highest-priority, lowest-risk findings; resolve the project
  naming conflict in favour of "Rush".

## Decisions

- **C4 (governance):** v0.3.0-alpha.1 is marked *in progress*, not complete.
  Its "minimal VM boots to multi-user.target" criterion is recorded as
  unverified in `release/milestones.toml`. Rationale: a milestone may not be
  flipped to complete while its primary exit criterion is unmet.
- **C9 (naming):** The project is **Rush Linux**. The public D-Bus identity is
  `io.rushlinux.Optid1` (object path `/io/rushlinux/Optid`). The brand string
  "Adaptive Linux" is retired. "adaptive" is retained only as a technical
  adjective (adaptive optimization, the `linux-adaptive` kernel flavour). See
  ADR 0007.
- **B1 (sysctl vs ADR 0004):** The unconditional static sysctl drop-in was
  split. Network defaults (BBR + `fq`) stay static in `99-rush-network.conf`;
  memory/VM/swap knobs become optid-owned and mode-dependent in
  `config/optid/policy.toml`. See ADR 0006 amendment and ADR 0004 boundary note.
- **C7 (recipe schema):** All recipes declare `schema_version = 0`; the builder
  validates and propagates it; migration policy documented.
- **B6 (GRUB fallback):** Added `recipes/boot/grub.toml` skeleton so the
  promised fallback is a buildable artifact.
- **C10:** Established this agent-decision log convention.

## Changes

- `RELEASES.md`, `release/milestones.toml`
- `crates/optid/src/main.rs`, `crates/optctl/src/main.rs`
- `packaging/dbus/io.rushlinux.Optid.{xml,service}` (renamed)
- `distro/systemd/99-rush-network.conf` (replaces `99-adaptive-performance.conf`)
- `config/optid/policy.toml`, `recipes/**`, `tools/rush-builder.py`
- `docs/**` (ADR 0004/0006 amendments, ADR 0007 proposed, packaging, boot,
  brand rename across docs)

## Follow-ups

- Implement guarded `vm.*` sysctl actuation in optid (currently policy keys are
  parsed-tolerant but not applied).
- Ratify the proposed ADRs (0007 naming, 0008 packaging, 0009 optid threat
  model, plus the realtime/benchmark/reproducibility/ML decisions).
- Long-lead items (contributor model, Linux-canonical dev env, hardware lab)
  captured in `docs/project-sustainability.md`.
