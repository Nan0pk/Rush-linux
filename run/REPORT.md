# Rush Linux hardware run — 2026-06-20T09:06:46Z
## Environment
```
host=fedora kernel=7.0.12-201.fc44.x86_64 ncpu=24
virt=none
cpu=13th Gen Intel(R) Core(TM) i7-13700HX
uptime/load: 14:06:46 up 17:31,  2 users,  load average: 0.42, 0.35, 0.43
EPP_cpus=24
platform_profile=ABSENT
rapl=/sys/class/powercap/intel-rapl-mmio:0
psupply /sys/class/power_supply/ACAD=Mains
psupply /sys/class/power_supply/BAT1=Battery
psupply /sys/class/power_supply/ucsi-source-psy-USBC000:001=USB
PSI=yes
cpu_dma_latency=yes
```
## Environment awareness
```
env_kind=baremetal  virt=none  container=none  ci=no
real_writable_hw=yes  priv=yes  actuation=--apply
distro=Fedora Linux 44 (Workstation Edition)
```
- env=baremetal real_hw=yes priv=yes actuation=--apply
## Power handling audit
```
cpufreq_driver=intel_pstate
intel_hwp_capable=yes  hwp_dynamic_boost=0
intel_pstate_status=active
scaling_governor(cpu0)=performance  all_unique=performance
WARNING: governor=performance — EPP hints are IGNORED, all levers will be inert
AUTO-FIX: stopping daemons that pin the governor, then switching to powersave...
stopped for session (restored on exit): power-profiles-daemon thermald
governor_after_fix=powersave
FIX OK: governor now powersave — EPP lever active
epp(cpu0)=balance_performance  all_unique=balance_performance,balance_power
power_services:
  tuned: active=inactive enabled=enabled
  power-profiles-daemon: active=inactive enabled=enabled
  tlp: active=inactive enabled=not-found
  thermald: active=inactive enabled=enabled
  auto-cpufreq: active=inactive enabled=not-found
  gamemode: active=inactive enabled=not-found
  system76-power: active=inactive enabled=not-found
  cpupower: active=inactive enabled=disabled
  acpid: active=inactive enabled=not-found
  laptop-mode-tools: active=inactive enabled=not-found
gnome_power_profile=balanced
kernel_cmdline_power:
  (none found — defaults in effect)
sysctl_power:
  vm.dirty_ratio=20
  vm.dirty_background_ratio=10
  vm.swappiness=60
  kernel.sched_energy_aware=
  kernel.nmi_watchdog=1
POWER AUDIT: 1 warning(s) found — review above
```
- power audit: 1 warning(s)
- RUN  deps via pacman
- FAIL deps via pacman (rc=1) — see logs/-S
### FAIL deps via pacman (rc=1)
```
warning: database file for 'core' does not exist (use '-Sy' to download)
warning: database file for 'extra' does not exist (use '-Sy' to download)
error: target not found: dbus
error: target not found: pkgconf
error: target not found: base-devel
```
- RUN  cargo build --release
- OK   cargo build --release
- RUN  cargo test
- FAIL cargo test (rc=101) — see logs/test
### FAIL cargo test (rc=101)
```
test tests::test_n2_t6_no_thrash ... ok
test tests::test_n2_t4_per_device_revert ... ok
test tests::test_n2_t2_dry_run_no_op ... ok
test tests::test_n2_t8_explainability ... ok
test tests::test_t1_dry_run_no_op ... ok
test tests::test_n1_t9_global_pin_loop_boundary_precedence ... ok

test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.00s

     Running unittests src/main.rs (target/debug/deps/rushbench-4f125c805d887b32)

running 12 tests
test tests::test_t4_schema_freeze ... ok
test tests::test_t2_energy_probe_arithmetic ... ok
test tests::test_t9_avg_watts_positive ... ok
test tests::test_t1_energy_probe_wrap_ac_switch_rejection ... ok
test tests::test_report_energy_analysis_workload_filter ... ok
test tests::test_t7_provenance_completeness ... ok
test tests::test_t10_real_energy_advance ... ok
test tests::test_t5_n_less_than_5_honesty ... ok
test tests::test_t3_class_readback_enforcement ... ok
test tests::test_t9_energy_detection_priority ... FAILED
test tests::test_t8_latency_critical_honesty_path ... ok
test tests::test_t9_host_reject_when_no_energy_counter ... ok

failures:

---- tests::test_t9_energy_detection_priority stdout ----

thread 'tests::test_t9_energy_detection_priority' (865258) panicked at crates/rushbench/src/main.rs:573:9:
assertion failed: matches!(source, EnergySource::Battery(_))
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    tests::test_t9_energy_detection_priority

test result: FAILED. 11 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s

error: test failed, to rerun pass `-p rushbench --bin rushbench`
```
- RUN  optid --once (smoke)
- OK   optid --once (smoke)
- RUN  optid apply pin sanity
- OK   optid apply pin sanity
- RUN  bench host-v2
- OK   bench host-v2
- RUN  bench matrix
- OK   bench matrix
- RUN  rushbench
- FAIL rushbench (rc=1) — see logs/matrix
### FAIL rushbench (rc=1)
```

--- Running cell: class=interactive, workload=psi-cpu ---
Wrote results to /home/victus/Rush-linux/benchmarks/results/2026-06-20/fedora/interactive/psi-cpu.json
Cell failed: class_mismatch: requested=interactive, observed=idle

--- Running cell: class=interactive, workload=psi-io ---
Wrote results to /home/victus/Rush-linux/benchmarks/results/2026-06-20/fedora/interactive/psi-io.json
Cell failed: class_mismatch: requested=interactive, observed=idle

--- Running cell: class=latency-critical, workload=foreground-launch ---
Wrote results to /home/victus/Rush-linux/benchmarks/results/2026-06-20/fedora/latency-critical/foreground-launch.json
Cell failed: class_mismatch: requested=latency-critical, observed=idle

--- Running cell: class=latency-critical, workload=cyclictest ---
Wrote results to /home/victus/Rush-linux/benchmarks/results/2026-06-20/fedora/latency-critical/cyclictest.json
Cell failed: class_mismatch: requested=latency-critical, observed=idle

--- Running cell: class=latency-critical, workload=psi-cpu ---
Wrote results to /home/victus/Rush-linux/benchmarks/results/2026-06-20/fedora/latency-critical/psi-cpu.json
Cell failed: class_mismatch: requested=latency-critical, observed=idle

--- Running cell: class=latency-critical, workload=psi-io ---
Wrote results to /home/victus/Rush-linux/benchmarks/results/2026-06-20/fedora/latency-critical/psi-io.json
Cell failed: class_mismatch: requested=latency-critical, observed=idle

--- Running cell: class=throughput, workload=foreground-launch ---
Wrote results to /home/victus/Rush-linux/benchmarks/results/2026-06-20/fedora/throughput/foreground-launch.json
Cell failed: class_mismatch: requested=throughput, observed=idle

--- Running cell: class=throughput, workload=cyclictest ---
Wrote results to /home/victus/Rush-linux/benchmarks/results/2026-06-20/fedora/throughput/cyclictest.json
Cell failed: class_mismatch: requested=throughput, observed=idle

--- Running cell: class=throughput, workload=psi-cpu ---
Wrote results to /home/victus/Rush-linux/benchmarks/results/2026-06-20/fedora/throughput/psi-cpu.json
Cell failed: class_mismatch: requested=throughput, observed=idle

--- Running cell: class=throughput, workload=psi-io ---
Wrote results to /home/victus/Rush-linux/benchmarks/results/2026-06-20/fedora/throughput/psi-io.json
Cell failed: class_mismatch: requested=throughput, observed=idle
```
- RUN  build mkosi disk.raw
- FAIL build mkosi disk.raw (rc=127) — see logs//home/victus/Rush-linux/tools/build-mkosi-image.sh
### FAIL build mkosi disk.raw (rc=127)
```
bash: /home/victus/Rush-linux/tools/build-mkosi-image.sh: No such file or directory
```
- RUN  validate uefi boot
- OK   validate uefi boot

## Result summary
OK:   6
FAIL: 4
SKIP: 0
power_warnings: 1
logs: /home/victus/Rush-linux/run/logs
evidence csv: /home/victus/Rush-linux/run/evidence/20260620-141117/results.csv
- DONE — read /home/victus/Rush-linux/run/REPORT.md
- restored power-profiles-daemon
- restored thermald
