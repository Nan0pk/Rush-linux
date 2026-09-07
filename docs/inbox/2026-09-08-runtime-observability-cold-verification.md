# Runtime-state observability cold verification: pass

Package: **Add truthful runtime-state observability** (`O1`), implementation PR #450,
including repairs merged in PRs #454, #461, and #463.

Verifier: independent ChatGPT GitHub verifier, separate from the implementation and
repair authors. Verification date: 2026-09-08.

## Whole-project assessment

Rush is pursuing a responsive, efficient OS while keeping measurement claims separate
from implementation claims. This package is a read-only observation surface. Passing
it proves that the reporter truthfully distinguishes kernel runtime state and read
failures on the exercised production paths; it does **not** prove energy savings,
performance benefit, safe automatic actuation, or broad hardware compatibility.

The package remains architecturally separate from the daemon control loop and does not
write hardware state. The intentionally unwired `optctl status` integration is not
silently treated as implemented; current package acceptance is through the standalone
`optid-observe` production executable, as recorded in the ledger.

## Reconstruction of the prior failure

The 2026-09-06 independent verification correctly blocked completion because real
hardware exposed defects that the then-green synthetic suite did not catch:

- PM QoS permission denial could be folded into `unsupported`;
- per-device runtime-PM QoS read errors could be discarded;
- requested backlight brightness could be presented as actual brightness; and
- wakeup, CPU-idle, and backlight directory-discovery failures could be hidden as
  empty or stale state without source-level failure status.

Source inspection on the repaired code confirms that these paths now fail truthfully:
PM QoS is opened directly so ancestor permission errors survive; optional runtime-PM
QoS treats only true absence as unsupported; actual brightness is never fabricated;
and source-directory failures carry their own status while preserving prior entries as
stale rather than erasing them.

## Fresh independent execution

I independently re-ran the repository's root Rust workspace job for PR #463. GitHub
Actions run `34044904615`, rerun job `101844030189`, used Ubuntu 24.04 and executed:

```text
sudo -E env ... cargo test --workspace
```

Result: **PASS**.

The fresh run exercised the repaired observability code and reported:

- `optid-observe` unit suite: **18 passed, 0 failed**;
- `crates/optid/tests/o1_runtime_status.rs`: **5 passed, 0 failed**;
- full `optid` main suite: **548 passed, 0 failed**;
- full root Rust workspace: **PASS**.

The observability suite specifically passed the regressions for inaccessible PM QoS,
runtime-PM QoS read failures, missing actual brightness, directory-discovery failures,
successful empty sources, wakeup/runtime-PM units, stale/delta behavior, and unsupported
runtime status. The production integration suite also passed the live-kernel test,
repeated live sampling, off-mode zero-read behavior, production status surface, and
kernel PM-QoS error preservation.

The rerun checked out synthetic PR merge commit
`b4dd665e6afbd0b7eba4fe139b0f98185b92b29f`. GitHub comparison shows that synthetic
merge and the actual PR #463 merge commit
`898654437a90c33b5371791fce73cb7afe3ac09b` have no file differences. A separate
comparison from that actual merge commit to current `main`
`1dcd18b30e438d7099f67460e4e106d78ecd0479` shows later changes are confined to
benchmark/source-build files; none of the declared observability proof paths changed.
The verified package content is therefore the content now on `main`.

## Verdict

**PASS for package completion.** The previously blocking software defects are repaired,
the mapped acceptance behavior passes a fresh independent production/live-kernel run,
and the relevant proof paths have not changed since the repaired merge.

This verdict does not claim physical energy or performance improvement and does not
promote any hardware for automatic actuation. Those require their own evidence.

Unresolved package-completion findings: **none**.
