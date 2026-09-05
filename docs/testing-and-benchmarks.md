# Testing And Benchmarks

For the owner-authorized OS/source-build investigation, use the
[controlled comparison plan](plans/source-build-experiment.md). It separates
Optid's causal effect, one source-build intervention, and whole-product
comparisons. Its proposed numerical margins do not replace approved release
criteria. Build plans and stubbed software tests are not hardware evidence.

Rush Linux must prove optimization claims with repeatable tests. Benchmarks
are not optional marketing material; they are release gates.

## Current Validation

Run the checks relevant to the current change:

```sh
bash tools/checks.sh
```

The runner invokes Rust format, tests, and Clippy when Rust changed. Pull-request
CI supplies missing local tools. See `docs/project-workflow.md` for why each
check exists and what it is allowed to block.

Rust changes are also tested by the `Root Rust workspace` workflow with
`cargo test --workspace` under elevated privileges. This complements the normal
unprivileged lane and catches tests whose behavior changes when permission
checks no longer fail naturally. The root run uses a separate Cargo target
directory so it does not leave root-owned build artifacts in the normal
workspace.

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

## Measurement Rig (rushbench)

`rushbench` is a pure Rust measurement rig workspace member that gathers contract-validation evidence on a single host. It operates under a strict **no-write/observe-only** guarantee, never executing EPP, PM QoS, sysctl, or other actuator writes itself.

### Commands

- `rushbench run --class <C> --workload <W> [--n 5] [--ac-ok]`  
  Runs `n` iterations (default 5, must be >=5 for valid rollups) of the specified workload under the target workload class. Pins the class via `optctl pin` and validates the resolved floors via `optctl status --json`.
- `rushbench matrix [--ac-ok]`  
  Iterates through all supported workload classes and workloads.
- `rushbench report <results-dir>`  
  Parses the JSON records in `<results-dir>` and generates a Markdown summary verifying contract floors, energy consumption, and flagging `budget_violation` if limits are exceeded.

### Results Schema & Directory

Results are written under `benchmarks/results/<UTC-date>/<host-fingerprint>/<class>/<workload>.json`. The schema version is frozen at `1`.

### Safety & Sandboxing

The rig performs zero optimization writes. The only writes are to the `benchmarks/results/` output path, stdout/stderr, and `/tmp` scratch paths (no writes to `/proc/sys/**`, `/sys/devices/**`, etc.). This can be validated using sandboxing tools such as `strace` or `bwrap`.

## Benchmark Manifest

Scenario definitions live in `benchmarks/manifest.toml`.

When no physical machine is available, the candidate method in
[research paper 0024](research/0024-non-bare-metal-optid-validation-method.md)
separates real cloud-guest outcomes, deterministic QEMU/simulation proof, and
model-conditional power/thermal estimates. It is future work for planned
package I2; it does not replace T3 hardware evidence or T4 comparative release
evidence.

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

## Documentation Check

Docs are part of a change when the change alters a documented promise, command,
workflow, safety rule, or public behavior. The doc registry and generated
front-page checks catch missing and stale references without demanding unrelated
roadmap or handoff edits.
