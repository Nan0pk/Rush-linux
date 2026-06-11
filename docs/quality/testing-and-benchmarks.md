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

## Host Benchmark Harness

`tools/bench-optid-host.sh` benchmarks `optid` on a real Linux host (for
example an existing Fedora or Arch installation) before Rush Linux itself is
installable. It measures timer-wakeup latency percentiles under mixed CPU and
I/O load as a proxy for the manifest's `input-latency` metrics, first at
baseline and then with `optid --apply` in one or more modes.

Safety properties:

- Refuses to run without root, or on hosts with no EPP and no
  `platform_profile` (containers, most VMs), where `optid` cannot actuate.
- Captures every knob `optid` can mutate (per-CPU EPP, platform profile,
  systemd slice properties) before applying anything.
- Restores and verifies the captured state on every exit path via a shell
  trap, including errors and interrupts. Slice properties are set with
  `systemctl set-property --runtime`, so a reboot also clears them.
- Defaults to dry-run; `--apply` is required to mutate anything.

```sh
sudo ./tools/bench-optid-host.sh                       # dry-run + baseline
sudo ./tools/bench-optid-host.sh --apply               # benchmark 'performance'
sudo ./tools/bench-optid-host.sh --apply --modes performance,battery
```

`tools/bench-optid-host-v2.sh` is the isolating successor. The v1 harness
could not detect an effect on real hardware because the load and the latency
probe shared `user.slice` (cancelling the CPUWeight boost), the all-core load
left EPP nothing to do, and tail latency was dominated by noise. v2 fixes all
three: it runs an oversubscribed load in `background.slice` while the probe
runs in `user.slice` (so the weight lever is measurable), adds a partial-load
CPU-package-watts scenario via RAPL (so the EPP lever is measurable), and
reports the median of N iterations. Same safety contract.

```sh
sudo ./tools/bench-optid-host-v2.sh                    # dry-run + baselines
sudo ./tools/bench-optid-host-v2.sh --apply            # baseline vs performance,battery
sudo ./tools/bench-optid-host-v2.sh --apply --iter 5
```

`tools/bench-optid-matrix.sh` is the guided campaign runner. It walks a test
matrix across power sources (prompting the operator to plug/unplug the
charger and verifying the transition through sysfs before measuring),
isolates levers individually (EPP alone, cgroup weight alone, full optid
modes), records ambient desktop load as metadata instead of forbidding it,
and emits an evidence-ready results directory (`results.csv`, `meta.txt`,
`transcript.log`). It stops `tuned`/`power-profiles-daemon` for the session
if active and restarts them on exit, refuses the battery phase below a
charge floor (default 25%), and keeps the same capture/restore/verify
contract as v1/v2. It runs a work-counting background load
(`tools/bench-work-load.py`) under partial load to measure total work units
performed and compute performance-per-watt efficiency (work units per Joule).



```sh
sudo ./tools/bench-optid-matrix.sh --apply                 # full matrix, AC + battery
sudo ./tools/bench-optid-matrix.sh --apply --power ac      # AC only, no prompts
sudo ./tools/bench-optid-matrix.sh --apply --levers baseline,epp --iter 9
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
