Run the full Builder/Verifier protocol check. Produce a literal command transcript for each step — do not summarise or skip output.

Steps (in order, stop on first failure):
1. `cargo check --workspace`
2. `cargo test --workspace`
3. `cargo clippy --workspace -- -D warnings`
4. `python3 tools/validate-doc-sync.py`

After all steps, report:
- PASS or FAIL
- Exact stdout/stderr for any failing step
- Do not certify work as passing unless all four commands exit 0.
