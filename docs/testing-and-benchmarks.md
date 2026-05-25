# Testing And Benchmarks

Adaptive Linux must prove optimization claims with repeatable tests. Benchmarks
are not optional marketing material; they are release gates.

## Current Validation

Local repository validation:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\validate-repo.ps1
```

Rust validation once a toolchain is available:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Benchmark Manifest

Scenario definitions live in `benchmarks/manifest.toml`.

Required comparisons:

- Fedora current.
- Ubuntu current.
- Arch current.
- Minimal tuned baseline.

Required scenarios:

- mixed-load responsiveness;
- laptop battery;
- gaming frame time;
- realtime audio;
- server throughput.

## Release Gates

A release candidate must show:

- better mixed-load foreground latency than mainstream defaults;
- competitive or better laptop battery behavior;
- no unacceptable throughput loss;
- successful rollback tests;
- `optctl explain` coverage for optimizer actions.

## Documentation Gate

Docs are part of acceptance criteria. CI must require the core docs and ADRs to
exist. Any behavior change must update the relevant docs in the same commit.

