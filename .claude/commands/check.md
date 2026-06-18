Fast iteration check scoped to a single crate. Usage: /check optid

Run for the named crate (default: optid if none given):
1. `cargo check -p <crate>`
2. `cargo clippy -p <crate> -- -D warnings`

Report errors with file:line references. Do not run workspace-wide commands.
