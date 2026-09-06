# Runtime-state observability cold verification: changes requested

Package: Add truthful runtime-state observability (`O1`), implementation PR #450,
including the merged repair in PR #454.
Verified commit: `f3cbdb2a26177c26bf914386a5153dd8cdf8df75`.
Verifier: Codex independent agent `/root/runtime_cold_verifier`, separate from
the implementation and repair authors. Date: 2026-09-06.

## Project and architecture assessment

Rush pursues a responsive, efficient OS; the current strategy explicitly says
physical product advantage is unproven. This reporter contributes observations,
not an efficiency measurement or authority to actuate. I followed the
project-to-change sequence in `docs/agent-protocol.md`, reading the constitution,
Northstar, strategy compass, current selector, package packet and ledger before
tracing the production reporter and its shared `KernelRead` implementation.

The independent read-only executable respects the recorded maintainer separation
from the daemon control loop, F3 JSON and safety proof paths. It reads the policy
fragment, collects through `RealKernel`, retains only the previous sample and
prints the result. No hardware-write or daemon-state path is introduced. Device
and storage reconciliation depend on these observations, so error distinctions
and measured-versus-requested values matter even before CLI integration.

## Verdict

Do not issue a passing receipt or promote the package. The earlier wakeup
filename, millisecond conversion and `unsupported` runtime-status defects are
repaired, but an explicit acceptance requirement still fails on real hardware:
permission-denied input is reported as unsupported. The existing green suite
does not prove complete error handling.

The completion plan still names `optctl status`; the ledger records its deliberate
separation. That architectural direction supports the standalone report, but
does not itself establish that all end-state criteria are waived. Keep this
integration limitation explicit; it is not necessary to resolve it to repair the
confirmed reporting defects.

## Blocking finding: permission denial becomes unsupported

Physical reproduction, unprivileged uid 1000, kernel
`7.1.13-200.fc44.x86_64`:

```sh
ls -ld /sys/kernel/debug
stat /sys/kernel/debug/pm_qos/cpu_latency_constraints
target/debug/optid-observe --config config/optid/policy.toml --samples 2 --interval-seconds 1
```

The directory has mode `drwx------` and owner `nobody`. `stat` returns
`Permission denied`. Both production samples print:

```text
pm_qos.cpu_latency_us=unavailable requestors=0 status=unsupported
```

`collect_pm_qos` in `crates/optid/src/runtime_observability.rs` calls `exists`
before attempting the read. The shared real implementation in
`crates/optid/src/kernel_io_impl.rs` uses `Path::exists`, which returns false
when ancestor traversal is denied. The collector consequently never receives
the error needed to distinguish denial from absence. The number of requestors
is also unknown here; zero must not imply an observed absence of constraints.

The packet explicitly requires unsupported and permission-denied to be reported
separately. Attempt the read and classify its error, preserving the read-only
shared boundary. Cover inaccessible ancestors as well as denied leaf files.
The earlier repair record's statement that unsupported is correct for root-only
debugfs is contradicted by this observation.

## Additional implementation findings

These are established by source inspection, not claimed as physical reproductions:

- `collect_backlights` substitutes `brightness` for `actual_brightness` when
  the actual interface is absent, and renders that value as `actual=...` with
  `status=observed`. Requested brightness is not evidence of actual brightness.
  Preserve unavailable actual state and test the missing-interface case.
- `collect_runtime_pm` drops errors from an existing
  `power/pm_qos_resume_latency_us` using `.ok()`, leaving status observed when
  that value is malformed or denied. Propagate or independently report the
  affected field's status.
- Enumeration errors are discarded: wakeup root `read_dir` failure produces
  stale entries or an empty list; CPU root failure returns an empty list;
  backlight `read_dir` failure also returns an empty list. This hides denial
  and can erase previous observations instead of marking them stale. Report
  source-level failure separately from a successfully observed empty source.

## Checks actually run

```sh
git rev-parse HEAD
cargo test -p optid --bin optid-observe
cargo test -p optid --test o1_runtime_status
bash tools/start-work.sh "Record cold runtime observability findings"
```

All 13 module tests and all four production integration tests passed. The start
script's quick checks passed. The physical repeated-sample command above also
passed and reported 57 wakeup sources, 85 runtime-PM devices, 96 idle states,
11 storage entries and one backlight, with 1163 reads per sample. A PCI device
reported `active_delta_us=1011000` over the one-second interval, confirming the
repaired unit conversion reaches the real executable. First-sample deltas were
unavailable, as expected. No source repair, hardware actuation, receipt or ledger
promotion was performed by this verifier.

This assessment proves the listed software and read-only observations on one
host. It does not certify energy benefit, automatic actuation, other hardware,
package installation, or the unwired `optctl status` surface.
