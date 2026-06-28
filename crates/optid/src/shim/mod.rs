//! v0.6 Phase B — compatibility shims and conflict detection.
//!
//! This module groups the D-Bus compatibility shims (PPD, GameMode) and the
//! conflict detector that refuses `--apply` when a competing policy daemon is
//! already running. The shims land in B1/B2; B3 (conflict detection) is the
//! first to ship because it has no D-Bus dependency and is fully testable
//! in-container.
//!
//! See `docs/plans/v0.6-hardware-aware-optid-proposal.md` §3 Phase B.

pub(crate) mod conflict;

// `detect_conflicts` is the production entrypoint used by `main.rs`. The
// `detect_conflicts_with` and `ConflictReport` symbols are pub(crate)-used
// only inside `conflict::tests`; re-exporting them here would be dead code
// in the binary. Tests reach them via `crate::shim::conflict::...` directly.
pub(crate) use conflict::detect_conflicts;
