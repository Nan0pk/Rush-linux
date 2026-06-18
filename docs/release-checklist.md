# Release Checklist

Use this checklist for every tagged release.

## All Releases

- [ ] `VERSION` matches the planned tag without the leading `v`.
- [ ] `RELEASES.md` contains the release entry.
- [ ] `docs/release-plan-v1.md` milestone status is updated.
- [ ] `docs/IMPLEMENTATION_STATUS.md` and `docs/AI_CONTINUATION.md` are current.
- [ ] `docs/documentation-policy.md` was followed for all behavior, config,
      service, release, benchmark, or safety changes.
- [ ] `tools/validate-repo.ps1` passes.
- [ ] CI passes on GitHub.
- [ ] No obsolete defaults are introduced.
- [ ] Relevant docs were updated in the same change.

## Alpha Releases

- [ ] T0 repository policy passes.
- [ ] T1 Rust tests pass.
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] If rootfs exists, VM smoke boot passes.

## Beta Releases

- [ ] All alpha checks pass.
- [ ] T2 VM tests pass.
- [ ] T3 hardware tests pass on the required lab classes for that milestone.
- [ ] Install, boot, update, rollback, network, and `optid` smoke tests pass.
- [ ] Release artifacts are published to the beta channel.

## RC Releases

- [ ] All beta checks pass.
- [ ] T4 comparative benchmark report is published.
- [ ] T5 security tests pass.
- [ ] v1 schemas and public interfaces are frozen.
- [ ] Release notes and known issues are complete.
- [ ] Only release blockers are accepted after RC.

## Stable Release

- [ ] At least one RC cycle completed with no release blockers.
- [ ] All final artifacts are signed.
- [ ] Stable and security channels are configured.
- [ ] Upgrade and rollback guide is verified.
- [ ] Benchmark report supports the release claims.
- [ ] The release can be installed on mainstream x86_64 hardware and VMs.
