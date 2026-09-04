# O1 cold verification — FAILED, no receipt. Two defects only real hardware shows.

Package: **O1 — Add truthful runtime-state observability** (ledger `candidate`, PR #450)
Verified commit: `54f6d68351b7fb192e2627d4c1095d6010ad889e` (origin/main, clean worktree)
Verifier: Claude (Opus 5), Claude Code session. Did not author O1, its tests, or its fixtures.
Host: Fedora 44, kernel `7.1.12-200.fc44.x86_64`, 13th Gen Intel Core i7-13700HX laptop.
Date: 2026-09-04

**Result: no receipt written.** O1 stays `candidate`.
`docs/plans/optid-verification/o1.toml` is untouched. Per
`docs/plans/optid-verification/README.md` I checked the commit and did **not**
repair it.

## What passed

All ten mapped acceptance tests pass individually with `-- --exact`:

```
cargo test -p optid --bin optid-observe runtime_observability::tests::<name> -- --exact
  o1_runtime_mode_defaults_to_observe_and_parses_off              ok
  o1_mock_full_state_snapshot_is_deterministic                    ok
  o1_monotonic_counters_produce_real_deltas                       ok
  o1_counter_reset_or_wrap_never_becomes_a_huge_delta             ok
  o1_disappearing_devices_are_reported_stale                      ok
  o1_permission_error_is_distinct_from_unsupported_and_malformed   ok
  o1_nonadvancing_clock_marks_snapshot_stale_and_suppresses_deltas ok
  o1_off_mode_performs_no_runtime_reads                           ok

cargo test -p optid --test o1_runtime_status <name> -- --exact
  o1_production_status_surfaces_runtime_observability              ok
  o1_production_off_mode_reports_zero_runtime_reads                ok
```

The reporter builds and runs as its own read-only binary, writes nothing, and
respects `off` mode. The ownership split described in the ledger holds: no
safety file, `crates/optid/src/main.rs`, or `crates/optctl` path is touched.

**A green test suite is why these two defects survived to a `candidate` claim.**
Both are invisible to the fixtures and visible on the first real host.

## Finding 1 — the wakeup surface is 100% inert on real hardware

`crates/optid/src/runtime_observability.rs:460`:

```rust
let total_time_us = read_u64(read, counter, &path.join("total_time"))
```

The kernel does not export `total_time`. It exports **`total_time_ms`**
(`drivers/base/power/wakeup_stats.c`). Confirmed on this host:

```
$ ls /sys/class/wakeup/wakeup0/
active_count  active_time_ms  device  event_count  expire_count
last_change_ms  max_time_ms  name  prevent_suspend_time_ms  relax_count
subsystem  total_time_ms  uevent  wakeup_count
```

`read_text` maps `NotFound` to `ObservationStatus::Unsupported` (line 369), and
that merges into the whole entry's status. So every wakeup source degrades, and
its deltas are suppressed. Actual output from the production binary on this
host, all 57 sources identical in shape:

```
wakeup.wakeup0  name=device:00 events_delta=unavailable wakeups_delta=unavailable total_time_delta_us=unavailable status=unsupported
wakeup.wakeup37 name=serio0    events_delta=unavailable wakeups_delta=unavailable total_time_delta_us=unavailable status=unsupported
```

`event_count` and `wakeup_count` are world-readable and parse fine on this host —
the entry is downgraded solely by the one missing path. WP-N3's whole point
("`optctl` reports what woke the machine") reads as `unsupported` on every
source of a working machine.

**Why no test catches it:** the fixture at line 1043 creates the non-existent
file.

```rust
add_file(&kernel, "/sys/class/wakeup/wakeup0/total_time", "1200\n");
```

`o1_monotonic_counters_produce_real_deltas` and
`o1_counter_reset_or_wrap_never_becomes_a_huge_delta` both prove delta
arithmetic against a sysfs layout that does not exist. The tests are correct
about the arithmetic and wrong about the kernel.

**Unit hazard attached to the fix:** the field is `total_time_us` /
`total_time_delta_us`, and `total_time_ms` is milliseconds. A rename alone
would report a value 1000× too small under a microsecond label. The sibling
`active_time_ms`, `max_time_ms`, `last_change_ms` and
`prevent_suspend_time_ms` are all milliseconds too.

## Finding 2 — a valid kernel state is reported as `malformed`

`crates/optid/src/runtime_observability.rs:~562`:

```rust
if matches!(
    value.as_str(),
    "active" | "suspended" | "suspending" | "resuming" | "error"
) { Some(value) } else {
    status = status.merge(ObservationStatus::Malformed);
```

`power/runtime_status` has a sixth documented value: **`unsupported`**, for a
device whose driver does not implement runtime PM. It is not corrupt data; it is
the kernel answering the question. Counted on this host:

```
$ for f in /sys/bus/*/devices/*/power/runtime_status; do cat "$f"; done | sort | uniq -c
    713 unsupported
     60 suspended
     18 active
```

One example read directly, whose every field is valid:

```
/sys/bus/i2c/devices/11-0050/power/control                auto
/sys/bus/i2c/devices/11-0050/power/runtime_status          unsupported
/sys/bus/i2c/devices/11-0050/power/runtime_active_time     0
/sys/bus/i2c/devices/11-0050/power/runtime_suspended_time  0
```

The reporter's verdict on it:

```
runtime_pm.i2c:11-0050=unavailable control=auto active_delta_us=unavailable suspended_delta_us=unavailable resume_latency_us=unavailable status=malformed
```

Aggregate from the production binary: **53 of 86 devices reported `malformed`,
33 `observed`.** Because `Malformed` is the highest severity in the merge order
(line 90), it also masks the readable `control`, `runtime_active_time` and
`runtime_suspended_time` values that came back fine, and suppresses their
deltas.

For a package titled *truthful* runtime-state observability, telling an operator
that 62% of their devices returned corrupt data — when the kernel returned a
documented value — is the specific failure the title forbids. It also poisons
the signal WP-N5 depends on: a device that legitimately has no runtime PM is
indistinguishable here from one whose sysfs is broken.

## Reproduction

```
git worktree add <dir> 54f6d68 && cd <dir>
cargo build --release -p optid --bin optid-observe
./target/release/optid-observe --config ./config/optid/policy.toml
```

No root required for either finding. Both appear in the first snapshot.

## Also observed, not a finding

`--samples N --interval-seconds N` produced an identical single snapshot with an
identical `reads=1171` count at N=2 and N=3, and no delta ever became available.
Given Finding 1 suppresses wakeup deltas and Finding 2 suppresses runtime-PM
deltas independently, I could not separate a multi-sample defect from those two
masking every delta on this host. Re-check after both are fixed rather than
treating it as a third defect now.

`pm_qos.cpu_latency_us=unavailable status=unsupported` is correct on this host —
nothing holds a `/dev/cpu_dma_latency` constraint, and `requestors=0` agrees.

## What the next builder needs to decide

1. Read `total_time_ms` and settle the unit: either convert ms→µs at the read
   and keep the `_us` field names, or rename the fields to `_ms`. Silence here
   is a 1000× error.
2. Accept `unsupported` as a sixth valid `runtime_status` and give it its own
   `ObservationStatus`, so it is neither `Observed` (it is not observing
   anything) nor `Malformed` (nothing is wrong).
3. Add a fixture built from a **captured real** `/sys/class/wakeup` and
   `power/` listing, not a hand-written one. Both defects exist because the
   fixture was written from the same assumption as the code.
