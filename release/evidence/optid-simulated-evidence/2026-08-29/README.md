# optid simulated evidence — 2026-08-29

**Evidence class: deterministic software proof + model-conditional estimate.**
See `docs/research/0024-non-bare-metal-optid-validation-method.md`. Nothing in
this directory is a measured guest outcome or a physical measurement. It makes
no claim about laptop watts, battery life, temperature, fan behaviour, suspend
and resume, firmware compatibility, or support for any named machine.

## The question

> When optid is fully enabled, does it theoretically improve the modelled
> system compared with optid off, while remaining safe under faults and
> recovery?

## The answer

**Theoretically beneficial with named regressions**, inside the declared model.
Read [`report.md`](report.md) — it answers the eight questions the run was
commissioned to answer, names the actions responsible for each change, and
lists what is too assumption-sensitive to judge.

## How to reproduce

```sh
mkdir -p /tmp/optid-evidence
printf 'optid-simulation-evidence-root-v1\n' > /tmp/optid-evidence/.optid-evidence-root-v1
cargo run --release -p optid --features test-simulation --bin optid -- \
    --evidence-root /tmp/optid-evidence --evidence-repeats 3
```

The run writes three files under `<root>/out/`:

| File | Committed | Contents |
|---|---|---|
| `report.md` | yes | The human-readable answer |
| `evidence-summary.json` | yes | Every judgement, receipt and aggregate, first repeat only |
| `evidence-bundle.json` | no (~26 MB) | The full bundle: every write, every modelled cycle, every repeat |

The full bundle is not committed because it is 26 MB and because the run is
proven byte-reproducible: the harness reruns every arm and scenario three
times and refuses to report a result unless all repeats are identical. The
committed summary carries `reproduce_command` and `full_bundle_bytes`.

Absolute paths from the machine that produced this run were replaced with
`<simulation root>` in the committed copies. Nothing else was edited.

## What actually ran

The harness drives the unmodified production control loop — `crate::run` in
`crates/optid/src/main.rs` — against a machine materialised inside a verified
simulation root. Real optid code performs the sensing, workload classification,
mode selection, per-domain gating, hardware-allowlist checks, contract checks,
capability checks, transactional actuation, journalling, circuit-breaker
accounting, shutdown restoration and startup recovery. The real S3D
`optid-recover` executable runs as a subprocess before every supervised
restart, matching `optid-apply.service`'s `Requires=optid-recover.service`.

Only the machine underneath is modelled. The model
(`crates/optid/src/sim_evidence/model.rs`) computes latency, throughput,
completed work, stall pressure, energy and temperature from the control values
optid actually left behind. It has no knowledge of which configuration is
running and no branch on "optid is enabled", so a harmful action scores as
harmful for the same reason a helpful one scores as helpful.

## Containment

`total_host_write_attempts` is **0** and `containment_violations` is empty.
Four guards hold at once:

1. every path the daemon touches is either already inside the verified
   simulation root or is rewritten into `<run>/machine/<path>` before any
   syscall; a path that is neither is refused with `EPERM` and recorded;
2. `PATH` is an empty directory inside the simulation root, so no host binary
   can be executed by name;
3. `DBUS_SYSTEM_BUS_ADDRESS` and `DBUS_SESSION_BUS_ADDRESS` point at a
   non-existent socket inside the simulation root; and
4. the competing-daemon detector is pinned to a deterministic answer so it
   never spawns `systemctl`.

The one process the harness starts is the sibling `optid-recover` executable,
by absolute path, with `--machine-root` inside the simulation root.

## Related, and not fixed here

`cargo test -p optid` run as root writes the host's real
`/proc/sys/vm/swappiness`. Four tests
(`crash_after_first_mutation_restart_restores`,
`stale_incomplete_journal_deterministic_recovery`,
`test_t2_failed_real_sysctl_revert_keeps_journal`,
`phase6_runtime_pm_rollback_failure_retains_journal`) assume the revert write
to that path *fails* because "CI cannot write that path". As root it succeeds,
the journal is cleared, and the assertions fail — after the host value has
already been changed. This predates the work in this directory and is not
fixed by it. The fix is to route those tests through the F2 kernel seam
(`kernel_io::with_real_kernel_override`) as the newer tests already do.
