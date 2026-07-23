# Optid Package Verification Receipts

This directory stores cold-verification receipts for packages in
`../optid-package-status.toml`.

A builder may make a package `candidate`. A different worker checks the exact
implementation commit without repairing it. Only that verifier may add a
receipt and propose `completed`.

Use one file per package, named `<package-id-lowercase>.toml`:

```toml
schema_version = 1
package = "F1"
implementation_pr = 324
verified_commit = "0123456789abcdef0123456789abcdef01234567"
verifier = "independent worker identity"
result = "pass"
commands = [
  "cargo test -p optid --test production_domain_modes",
  "cargo test --workspace",
]
runtime_proofs = [
  "production decision path preserves suppressed actions in observe mode",
  "all-off production path emits no privileged cgroup or hardware action",
]
unresolved = []
```

The receipt is evidence only when:

- it names the exact 40-character commit checked;
- its implementation PR matches the ledger;
- commands are exact and reproducible;
- runtime proofs enter through a production daemon, CLI, service, or public
  integration surface rather than calling only a new module; and
- unresolved findings are empty.

`tools/validate-optid-packages.py` checks this structure. It does not turn a
false statement into truth; the verifier remains responsible for running the
commands and inspecting the runtime path.
