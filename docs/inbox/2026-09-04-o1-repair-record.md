# O1 repair — three defects fixed, three decisions made. Needs a fresh cold verifier.

Package: **O1 — Add truthful runtime-state observability** (ledger stays `candidate`)
Repairs: `docs/inbox/2026-09-04-o1-cold-verification-findings.md` (PR #453)
Base commit: `8bbe10e`
Builder: Claude (Opus 5), Claude Code session. **Did not verify this repair** — the worker
who writes a fix may not sign its receipt.
Host used for hardware evidence: Fedora 44, kernel `7.1.12-200.fc44.x86_64`,
13th Gen Intel Core i7-13700HX laptop. No root, no writes, `tuned` untouched — the
reporter is read-only and there is no armed run in this work.

## One cause behind all three defects

`docs/research/0018-telemetry-runtime-state-observability.md` describes the debugfs
`/sys/kernel/debug/wakeup_sources` **table columns** and then, in the next section,
re-uses those column names as **sysfs file names**. It also lists the two runtime-PM
residency counters as microseconds. O1's code was written from that document, and its
fixture was written from the same assumption as the code — so the suite proved the
arithmetic and never the interface. `0018` is corrected in this change, because the next
builder reads it before reading the code.

## Decision 1 — units: convert ms to µs at the read, keep the `_us` field names

`total_time_ms`, `runtime_active_time` and `runtime_suspended_time` are all milliseconds.
They are now converted once, at the read, by `read_ms_as_us`.

Rejected the alternative (rename the fields to `_ms`) because D1 — "Complete runtime PM as
a reconciled domain", which depends on O1 — reconciles these values against
`cpuidle/state*/time` and `power/pm_qos_resume_latency_us`, both genuinely microseconds. A
struct holding `active_time_ms` beside `time_us` rebuilds the hazard this fix removes. One
canonical unit, converted at the boundary, with the kernel file and its unit named in a
comment at the conversion site.

Milliseconds is all the resolution the kernel has here, so a converted value is always a
whole multiple of 1000. That is a property of the source, not precision invented by the
conversion.

**No fallback to the old `total_time` path.** It never existed. A fallback would let the
wrong name keep working and would hide the unit, so a kernel exporting only `total_time`
now degrades — and a test asserts that it does.

## Decision 2 — `unsupported` gets its own `ObservationStatus::NotApplicable`

Neither `Observed` (nothing is being observed) nor `Malformed` (nothing is wrong), and not
the existing `Unsupported` either, which means *the interface is absent*. `NotApplicable`
ranks just above `Observed` in the merge order, so a not-applicable device still surfaces a
real fault found on any of its other files and never masks one.

The kernel's answer is **reported, not dropped**. The pre-repair code discarded the value
it rejected, so the operator saw `runtime_pm.i2c:11-0050=unavailable … status=malformed`
for a device whose every field was valid. It now reads:

```
runtime_pm.i2c:11-0050=unsupported control=auto active_delta_us=0 suspended_delta_us=0 resume_latency_us=unavailable status=not_applicable
```

## Decision 3 — the fixture is now pinned to a real capture

`tools/capture-o1-sysfs.sh` writes a flat, sorted, diffable record of the sysfs layout the
reporter reads, run unprivileged exactly as the reporter runs. The committed capture is
`crates/optid/tests/fixtures/o1-sysfs-capture-victus-2026-09-04.txt`: one instance of every
shape observed on the host — wakeup sources with zero counters, devices reporting `active`,
`suspended` and `unsupported` across four buses, cpu0's four cpuidle states, both PCIe ASPM
states, all eight SATA hosts, the NVMe controller and the panel backlight. `/sys/kernel/debug`
is root-only, so the capture records the PM QoS constraint list as absent rather than
inventing a readable one; the reporter correctly calls that `unsupported`.

The synthetic fixture is kept for the counter arithmetic and error paths, and is now
**fenced** by `o1_synthetic_fixture_uses_only_file_names_the_real_kernel_exports`: every
path it reads must appear in the real capture, compared as a (parent directory, file name)
pair rather than a bare file name — a bare-name check would accept a real name under the
wrong directory, which is the same class of mistake as the defect being fenced. An invented
path like `total_time`, or a real name in the wrong place like
`/sys/class/wakeup/wakeup0/runtime_status`, now fails there instead of shipping.

The allowlist is the one hole in that fence, so it is fenced too. Two names cannot appear in
an unprivileged capture on this host — `pm_qos_resume_latency_us` (no device here publishes
one) and `cpu_latency_constraints` (root-only) — and the test pins the allowlist to exactly
those two **and asserts each is absent from the capture**. A capture that grows one of them
invalidates its own exemption and the test fails until the entry is dropped, so the only way
to get a new file name past the fence is to capture it.

## Hardware evidence

Same command the verifier used, on the same host, no root:

```
cargo build --release -p optid --bin optid-observe
./target/release/optid-observe --config ./config/optid/policy.toml
```

| Surface | Before | After |
|---|---|---|
| wakeup sources | 57 of 57 `unsupported`, all deltas suppressed | 57 of 57 `observed` |
| runtime PM devices | 53 `malformed`, 33 `observed` | 53 `not_applicable`, 32 `observed`, **0 malformed** |
| pm_qos | `unsupported` | `unsupported` (correct — debugfs is root-only) |

The device count moved from 86 to 85 between the two runs because a USB device came or
went; it is not related to either defect.

### The unit fix, measured end to end

`--samples 3 --interval-seconds 2` against the live kernel:

```
runtime_pm.pci:0000:00:00.0=active control=on active_delta_us=2005000 suspended_delta_us=0 … status=observed
runtime_pm.pci:0000:00:08.0=suspended control=auto active_delta_us=0 suspended_delta_us=2006000 … status=observed
```

2.005 s of residency across a 2 s interval. Pre-repair the same line read
`active_delta_us=2005`.

### The deferred item from the findings file is resolved

The findings file deferred one observation rather than calling it a third defect: repeated
sampling appeared to produce an identical snapshot with no delta ever available. It was
Findings 1 and 2 masking every delta, not a sampling defect. With both fixed:

- sample 1: 0 numeric deltas — correct, there is no previous snapshot
- sample 2: 533 numeric deltas
- sample 3: 533 numeric deltas

`reads=1163` is identical across samples, which is expected: each sample reads the same
file set.

## Tests

Six new, and the pre-existing production test strengthened. Each new test was checked
against the defect it exists to catch by reintroducing that defect alone and confirming the
test fails.

| Test | Pins |
|---|---|
| `o1_wakeup_total_time_comes_from_the_kernel_ms_file_in_microseconds` | the real file name, the ×1000, and that the old name has no fallback |
| `o1_runtime_pm_residency_is_converted_from_kernel_milliseconds` | both residency counters, and that `pm_qos_resume_latency_us` is *not* converted |
| `o1_unsupported_runtime_status_is_not_applicable_not_malformed` | the value survives to the rendered line; a genuinely corrupt value still reports `malformed` |
| `o1_captured_real_sysfs_layout_reports_no_read_failure` | the whole captured host: no degraded wakeup source, no malformed device |
| `o1_synthetic_fixture_uses_only_file_names_the_real_kernel_exports` | the fixture cannot invent a path again |
| `o1_production_reports_real_kernel_surfaces_without_degrading_them` | the compiled binary against whatever live kernel runs the suite |

`o1_monotonic_counters_produce_real_deltas` changed one assertion: 800 ms → 900 ms of
suspended residency is `Some(100_000)`, not `Some(100)`.

The Finding-2 assertion in the production test is written as "no device is malformed"
rather than "unsupported devices are not applicable", because the pre-repair code dropped
the value it rejected — a test that only inspected lines already reporting `unsupported`
would have passed against the defect it exists to catch.

## What a cold verifier still owns

- O1 stays `candidate`. `docs/plans/optid-verification/o1.toml` is untouched.
- The `optctl status` surface is still deliberately unwired. That gap is unchanged by this
  repair and is recorded in the ledger as a maintainer call.
- The capture is from one host. A verifier on different hardware should re-run
  `tools/capture-o1-sysfs.sh` and check the layout matches; a second captured host would
  strengthen the fixture.
- `docs/research/0018` claims about surfaces O1 does not read (`/proc/acpi/wakeup`, the
  debugfs `wakeup_sources` nanosecond column, `runtime_usage`, `runtime_active_kids`) were
  not re-verified against hardware. Three of its **[PROVEN]** claims turned out to be
  unproven, so the rest of the document deserves the same suspicion.
- `docs/research/0009` is also corrected here. It had the units right ("cumulative ms") —
  which is how the third defect was caught, by noticing 0009 and 0018 disagree — but listed
  the same five-value `runtime_status` set as 0018. Its open follow-up "wire `runtime_status`,
  `runtime_suspended_time`, `runtime_active_time` into 0018 telemetry" is the exact provenance
  of these defects, and a builder reading 0009 instead of 0018 would otherwise have landed
  back where O1 started.
