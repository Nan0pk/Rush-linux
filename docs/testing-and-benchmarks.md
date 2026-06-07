# Testing And Benchmarks

Rush Linux must prove optimization claims with repeatable tests. Benchmarks
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

Release gates are tiered. The source of truth is `release/test-tiers.toml`.

- T0 repository policy: required docs, ADRs, no obsolete defaults, recipe
  presence.
- T1 Rust tests: unit, fixture, config parsing, D-Bus, CLI, policy decisions.
- T2 VM tests: rootfs boot, service start, cgroup v2, PSI, nftables, update,
  rollback.
- T3 hardware tests: laptop battery, thermals, suspend/resume, CPU/GPU/storage
  policy.
- T4 comparative benchmarks: Fedora, Ubuntu, Arch, minimal tuned baseline.
- T5 security tests: privileged write allowlists, service sandboxing, signature
  checks, rollback, config fuzzing, and D-Bus input fuzzing.

An RC must show:

- better mixed-load foreground latency than mainstream defaults;
- competitive or better laptop battery behavior;
- no unacceptable throughput loss;
- successful rollback tests;
- `optctl explain` coverage for optimizer actions.

Channel requirements:

- Alpha requires T0-T1 passing and basic VM smoke tests once rootfs exists.
- Beta requires T0-T3 passing.
- RC requires T0-T5 passing plus benchmark publication.
- Stable requires no release-blocker regressions for at least one RC cycle.

## Documentation Gate

Docs are part of acceptance criteria. CI must require the core docs and ADRs to
exist. Any behavior change must update the relevant docs in the same commit.
