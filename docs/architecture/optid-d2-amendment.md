# Optid D2 Architecture Amendment — Fail-Passive Capability Sealing

**Status:** Accepted owner direction

**Accepted:** 2026-07-22

**Supersedes:** `OPTID-COMPLETION-PLAN.md` packages S1–S3 and every recommendation for a permanent actuation broker

**Does not supersede:** the plan's F1–F4, C1, E1, O1–O2, X1–X2, D1–D5, T1–T3, R1–R3, or I1–I3 packages

## Decision

Optid will be a fail-passive optimizer. Its safe state is native kernel, driver,
firmware, and userspace policy with optid no longer applying changes.

The steady-state architecture remains one optid daemon. It will not add a
permanent privileged broker or IPC hop to the hardware-write path. Instead it
will combine:

1. typed, semantically bounded operations;
2. exact pre-opened capability descriptors;
3. Landlock self-restriction before worker threads start;
4. a persistent write-ahead recovery journal;
5. verified write/readback/compensation transactions;
6. a correctly driven systemd watchdog;
7. a small independent `optid-recover` executable;
8. per-domain and per-device circuit breakers; and
9. supervisor-managed cold restart when hardware topology changes.

This is the accepted **D2** architecture. It is distinct from the existing
storage-depth package also named `D2`. D0 and S1D–S5D in the active plan
implement it. `docs/plans/optid-package-status.toml` records the current package
and dependency state.

## Why this replaces the broker proposal

The original S1–S3 lane correctly identified two defects: the packaged service
cannot reach dynamic device paths, and the current runtime journal is not a
durable crash-recovery protocol. It then treated a permanent broker as the
preferred solution.

D2 solves those defects without steady-state IPC:

- discovery opens only exact, validated kernel attributes;
- later writes reuse those descriptors instead of reopening arbitrary paths;
- Landlock blocks new write opens after initialization;
- the transaction log exists before a write and survives daemon failure;
- an independent one-shot recovery path does not share optid's policy,
  classifier, D-Bus, or async failure modes; and
- a failed domain is handed back and quarantined instead of repeatedly
  reapplied after restart.

The kernel and hardware remain the underlying protection layers, but an
interface accepting a value is not proof that the value is semantically safe
for a particular machine. D2 therefore retains the north-star mutation gates:
explicit apply mode, typed capability/path validation, responsiveness-contract
fit, and verified hardware authorization.

## Credible failure boundary

Optid does not flash firmware, change voltage tables, or disable silicon
thermal protection. Permanent physical damage from its planned interfaces is
therefore unlikely. The credible worst case is still substantial enough to
engineer explicitly:

| Failure | Credible consequence | Required D2 response |
|---|---|---|
| Bad but kernel-valid CPU or power limit | Severe throttling, heat, noise, battery or charger stress | Enforce the verified envelope; compensate; open the domain circuit |
| Runtime-PM or PCIe/storage depth bug | Lost input/network/audio/GPU, I/O timeout, kernel or driver hang | Hand back the affected device; quarantine its HWID/firmware; recover before restart |
| Backlight value accepted by the kernel | Visually unusable display | Restore the captured user value; use a verified visibility floor only as emergency stabilization |
| VM sysctl combination | OOM pressure, writeback stalls, poor responsiveness | Restore the boot snapshot; quarantine the memory domain |
| Crash between related writes | Inconsistent retained state | Recover the incomplete transaction from the persistent journal |
| Repeated startup failure | Crash/reapply/crash loop | Start observe-only, preserve the open circuit, and require a controlled canary before re-entry |
| Compromised privileged daemon | Abuse of every path it can open | Pre-open exact descriptors, then seal new write opens before untrusted runtime work begins |

Reboot remains a last recovery layer. It is not the rollback protocol and it
must not automatically re-enable a quarantined domain.

## Architectural invariants

Every D2 implementation must preserve these rules:

1. **No write without an undo record.** The authoritative recovery record is
   durable before the kernel write occurs.
2. **Rollback and stabilization are distinct.** Rollback restores the exact
   captured original. Stabilization uses a conservative state only when exact
   restoration is impossible.
3. **No arbitrary path/value interface.** Runtime code receives a typed handle
   such as `RuntimePmControl` or `RaplPl1`, never a free-form path and string.
4. **No new write opens after sealing.** The process discovers, validates, and
   opens its exact capability table before installing Landlock and starting
   worker threads.
5. **No false watchdog heartbeat.** Health is reported only after a complete
   control cycle, including transaction and journal health.
6. **No crash-loop reapplication.** Recovery and circuit state are evaluated
   before any domain returns to actuation.
7. **No hotplug escape hatch.** A landlocked process cannot fork or exec out of
   its restrictions. Topology rebuild uses a systemd-managed cold restart.
8. **No fan actuation in this architecture.** Firmware retains fan ownership
   unless a later accepted decision proves a separate hardware fail-safe.
9. **No promotion from mocks.** Fixtures can prove behavior; only matching
   HWID/firmware evidence can authorize automatic actuation.

## Per-lever handback contract

The exact original is the normal rollback target. Emergency stabilization is
used only when that original is missing, corrupt, unsafe to write, or cannot be
verified. A stabilizer must never be reported as “restored to default.”

| Lever | Normal rollback | Emergency stabilization | Verification |
|---|---|---|---|
| CPU DMA PM QoS | Close optid's request descriptor; the kernel removes that request | Close every optid-owned request descriptor and deny re-entry | Descriptor ownership plus effective constraint observation |
| Per-device PM QoS | Restore the captured request value or remove optid's request where the ABI supports ownership | Relax/remove only optid's request; otherwise force the device active | Readback and device identity match |
| CPU EPP | Restore each CPU policy's captured startup value | Use an advertised balanced preference only when the original is unavailable | Read every affected policy back |
| Platform profile | Restore the captured advertised profile | Use `balanced` only if the platform advertises it | Readback plus advertised-choice check |
| Runtime PM | Restore captured `power/control` and autosuspend delay | Write `power/control=on` to force the device active | Readback plus unchanged device identity |
| SATA ALPM | Restore the captured host policy | Use `max_performance` when supported | Readback on the same SCSI host identity |
| PCIe ASPM | Restore the captured link state | Disable only the optid-enabled deeper state | Readback plus PCI identity/topology match |
| Backlight | Restore the captured user-owned raw brightness | Use a hardware-verified visibility floor | Readback and selected-panel identity match |
| VM sysctls | Restore the captured boot/startup values | Use the distribution's recorded boot policy only when provenance is available | Readback of every member of the transaction |
| Powercap / PL1 | Restore the captured firmware/startup limit | Stop writing; use a reviewed conservative cap only when the original cannot be recovered | Constraint bounds, readback, package identity, and telemetry |
| dGPU runtime PM | Restore captured control/delay policy | Write `power/control=on`; MUX state remains untouched | Readback plus GPU/driver/firmware identity |

S1D must turn this table into typed contracts with credible worst case,
supported ABI, safe envelope, ownership/drift rule, rollback test, stabilization
test, and unsupported behavior for each lever.

## Transaction and recovery protocol

The authoritative journal lives under `/var/lib/optid/recovery/`, not the
ephemeral runtime directory. Status, sockets, and disposable diagnostics may
remain under `/run/optid/`.

For every operation, the normal path is:

1. Resolve a typed capability descriptor and verify device identity.
2. Read and validate the current value.
3. Compute a semantically bounded intended value.
4. Append a generation-owned record containing domain, operation, canonical
   identity, original value, intended value, rollback method, and stabilization
   method.
5. Flush the record and its directory metadata durably.
6. Apply through the pre-opened descriptor.
7. Read back through the same capability and verify the resulting state.
8. Mark the transaction committed only after verification.
9. On mismatch, timeout, or partial completion, compensate immediately.
10. Remove or compact the record only after verified rollback or explicit
    ownership relinquishment.

Recovery is idempotent. It refuses a stale path whose current hardware identity
does not match the journal. If the current value no longer equals optid's last
confirmed write, recovery reports drift and does not overwrite a newer owner
unless the lever's accepted emergency contract requires stabilization.

## Capability sealing

At startup, the daemon performs a short privileged initialization phase:

1. Load root-owned configuration and the verified hardware allowlist.
2. Discover supported kernel objects.
3. Canonicalize paths without following an untrusted replacement.
4. Validate each path against its typed operation and current hardware
   identity.
5. Open only the required files with the minimum access mode and `CLOEXEC`
   policy appropriate to supervised restart.
6. Store the descriptors in a typed capability table.
7. Install a Landlock ruleset that denies new write opens outside the approved
   non-hardware state paths.
8. Confirm sealing with a negative self-test.
9. Only then create worker threads, start D-Bus/runtime inputs, and permit
   policy evaluation.

Linux documents that filesystem permissions are checked when files are opened,
while later reads and writes use the rights associated with the existing file
descriptor. It also documents that Landlock restrictions are irreversible for
the restricted thread and inherited by descendants. D0 must prove the exact
sysfs behavior on Rush's supported kernels before S4D can ship. See the
[kernel Landlock documentation](https://docs.kernel.org/userspace-api/landlock.html).

If the required Landlock ABI or sysfs behavior is unavailable, optid remains
dry-run/observe-only for affected dynamic domains. D0 failure blocks S4D, not
F1–F4, read-only observation, simulation, diagnostics, or other independent
packages.

## Watchdog and independent recovery

`WatchdogSec=` is not useful until optid reports health. S3D must:

- use systemd notification semantics;
- emit `WATCHDOG=1` from the main control path only after sensors, policy,
  transactions, readback, and journal health complete;
- withhold the heartbeat on a stuck or inconsistent cycle;
- invoke a dedicated one-shot `optid-recover` path before automatic actuation
  resumes; and
- prove ordering for crash, watchdog kill, boot with an unresolved journal,
  repeated recovery, and failed recovery.

`optid-recover` contains no classifier, policy parser, D-Bus server, session
bridge, or general async runtime. It validates journal records and hardware
identity, performs rollback or named stabilization, records the outcome, and
exits. It is not a daemon and adds no steady-state latency.

## Hotplug is a cold restart

A restricted process and its descendants cannot remove Landlock restrictions.
No unrestricted sibling thread will be kept as an escape hatch. The topology
lifecycle is therefore:

1. Detect a topology change.
2. Treat newly discovered devices as observe-only.
3. Debounce rapid add/remove events.
4. Finish or cancel in-flight transactions.
5. Hand current levers back and flush the recovery journal.
6. Exit with the dedicated topology-rebuild status.
7. Let systemd run `optid-recover` and start a fresh process.
8. Rediscover devices, open exact descriptors, install Landlock, and only then
   start workers.
9. Evaluate the new topology under the normal allowlist and contract gates.

If recovery or restart fails, the machine remains under native kernel/driver
control and automatic actuation stays off.

## Circuit breakers and envelope expansion

Failures are isolated by domain and hardware identity. A repeated failure:

- hands the affected lever back;
- opens that domain/HWID/firmware circuit;
- persists the quarantine across process restart;
- allows unrelated healthy domains to continue;
- starts the failed domain in observe-only mode;
- permits one monitored canary only after cooldown and recovery success; and
- reopens the circuit immediately if the canary fails.

New capability promotion follows: observe, simulate, apply one reversible
value, verify readback and real state, test automatic handback, expand the
envelope gradually, then promote only the tested HWID/firmware combination.

## Execution order and stop rule

The safety lane is:

`D0 → S1D → S2D → S3D → S4D → S5D`

- D0 proves capability sealing and cold restart.
- S1D freezes per-lever handback and semantic envelopes.
- S2D builds persistent verified transactions.
- S3D builds independent recovery and supervision.
- S4D moves runtime writes to the sealed typed capability table.
- S5D adds circuit breakers and controlled re-entry.

D0 is the next safety-lane package. F1 remains the next general construction
package. Hardware nomination blocks release/promotion claims, not these build
packages.

If D0 cannot prove the required kernel behavior, stop S4D and record the exact
kernel, Landlock ABI, sysfs object, call sequence, and failure. Do not silently
fall back to a permanent broker, broad unsealed writes, or an unrestricted
discovery thread. The other lanes continue while a bounded alternative is
designed.
