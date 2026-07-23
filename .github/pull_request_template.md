## What Does This Change?

A clear description of what this PR changes and why.

## Motivation

Why is this change needed? Link to the issue it resolves (e.g., `Closes #123`)
or explain the problem it solves.

## Type of Change

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that changes existing behavior)
- [ ] Refactor (no functional change)
- [ ] Documentation update
- [ ] CI / build system change
- [ ] Kernel config change
- [ ] Policy / defaults change

## Testing

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo fmt --all -- --check` passes
- [ ] `pwsh ./tools/validate-repo.ps1` passes
- [ ] `python3 tools/validate-evidence.py` passes (Dragnet gate)
- [ ] New or updated tests added for this change

### Evidence (Dragnet)

- [ ] If this PR sets any milestone criterion `verified = true`, it commits the
      acceptance `transcript` and `python3 tools/dragnet.py --observe` is GREEN
      with zero `pending` ledger rows for that milestone. (See `docs/dragnet-protocol.md`.)

### Optid package contract

- Package ID (or `not optid`): ___
- [ ] This optid code PR updates exactly one entry in
      `docs/plans/optid-package-status.toml`.
- [ ] The package is `candidate` or honestly `merged_incomplete`; the builder
      did not self-certify it as `completed`.
- [ ] Ledger paths identify production runtime entry points, integration tests,
      and committed evidence (required for `candidate`).
- [ ] Integration tests enter through a production daemon/CLI/service surface,
      not only through a newly added module.
- [ ] No new production `allow(dead_code)` or module-only wiring hides
      incomplete integration.
- [ ] `python3 tools/validate-optid-packages.py --base origin/main` passes.

## Documentation

**Documentation updates are required for changes to behavior, defaults,
policy, boot/update flow, kernel fragments, recipes, or service behavior.**

- [ ] No documentation update needed (explain why below)
- [ ] `docs/IMPLEMENTATION_STATUS.md` updated
- [ ] Relevant docs updated (list which ones):
  - ___
- [ ] ADR added or amended (if this changes architectural direction)

### If no doc update is needed, explain why:

___

## Checklist

- [ ] I have read `CONTRIBUTING.md`
- [ ] My changes follow the project's design rules (modern defaults,
      one adaptive policy owner, explainable behavior)
- [ ] No legacy defaults were introduced without justification
- [ ] No guardrails were weakened
- [ ] If this changes privileged actions, `SECURITY.md` and relevant
      ADRs are updated
