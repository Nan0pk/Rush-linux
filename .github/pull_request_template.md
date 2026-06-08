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
- [ ] New or updated tests added for this change

## Documentation

**Documentation updates are required for changes to behavior, defaults,
policy, boot/update flow, kernel fragments, recipes, or service behavior.**

- [ ] No documentation update needed (explain why below)
- [ ] `IMPLEMENTATION_STATUS.md` updated
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
