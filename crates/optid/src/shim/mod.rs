//! v0.6 Phase B — compatibility shims and conflict detection.
//!
//! This module groups the D-Bus compatibility shims (PPD, GameMode) and the
//! conflict detector that refuses `--apply` when a competing policy daemon is
//! already running. Phase B3 (conflict detection) shipped first because it
//! has no D-Bus dependency and is fully testable in-container; Phase B1
//! (PPD shim) and Phase B2 (GameMode shim) followed.
//!
//! See `docs/plans/v0.6-hardware-aware-optid-proposal.md` §3 Phase B.

pub(crate) mod conflict;
pub(crate) mod gamemode;
pub(crate) mod ppd;

// `detect_conflicts` is the production entrypoint used by `main.rs`. The
// `detect_conflicts_with` and `ConflictReport` symbols are pub(crate)-used
// only inside `conflict::tests`; re-exporting them here would be dead code
// in the binary. Tests reach them via `crate::shim::conflict::...` directly.
pub(crate) use conflict::detect_conflicts;
pub(crate) use gamemode::GameModeServer;
pub(crate) use ppd::PpdServer;
