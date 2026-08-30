# optid simulated evidence — after the policy-reload fix, 2026-08-30

**Evidence class: deterministic software proof + model-conditional estimate.**
See `docs/research/0024-non-bare-metal-optid-validation-method.md`. Nothing
here is a measured guest outcome or a physical measurement, and nothing here
claims laptop watts, battery life, temperature, or support for any machine.

This is the same matrix as
[`../2026-08-29/`](../2026-08-29/README.md), re-run after the fixes for the
findings that run produced. Read the two together: the earlier directory is the
evidence that found the defects, this one is the evidence that they are gone.

The committed `report.md` and `evidence-summary.json` were regenerated at
`fabd364` (the merge of the device-removal repair), so they cover two fixes,
not one. The directory name records the first of them.

## What changed between the runs

The 2026-08-29 run reported two defects.

`policy_reload_fallback_escalates_domain_modes` (**high**): a `policy.toml`
that became unreadable while optid was running fell back to
`Policy::curated_baseline()`, whose per-domain default is `actuate`, while
`apply_armed` was computed once at startup and never re-evaluated. An arm with
every domain `mode = "off"` wrote eight kernel controls after the injected
reload failure, and the `observe` arm wrote four.

The run loop now keeps the last policy that loaded cleanly and disarms dynamic
writes for any cycle whose reload did not return `Ok`. Fixed in
[#437](https://github.com/Nan0pk/Rush-linux/pull/437).

`owned_target_hot_removal_aborts_the_control_loop` (**medium**): hot-removing a
device optid owned made the reconciler's transaction-target canonicalisation
fail, the error escaped the control loop, and the loop exited before its
shutdown handback. Two supervised restarts and one `optid-recover` pass were
needed to hand the machine back.

Handback now relinquishes a completely absent target without writing, durably
retires its transaction, and carries on restoring the targets that survive.
Fixed in [#439](https://github.com/Nan0pk/Rush-linux/pull/439).

| | 2026-08-29 | this run |
|---|---|---|
| `policy_reload_fallback_escalates_domain_modes` | reported, high | **absent** |
| Kernel writes by the all-`off` arm in the reload scenario | 8 | **0** |
| Kernel writes by the all-`observe` arm in the reload scenario | 4 | **0** |
| `owned_target_hot_removal_aborts_the_control_loop` | reported, medium | **absent** |
| Daemon outcome in the three hot-removal trials | `canonicalize transaction target: NotFound` | **`topologyrebuild`** (the loop finishes its cycle) |
| Supervised restarts needed to hand the machine back after hot removal | 2, plus a recovery pass | **1, and `optid-recover` exits 0 with nothing to repair** |
| Determinism | 225/225 across 3 repeats | 225/225 across 3 repeats |
| Write attempts outside the simulation root | 0 | 0 |
| No-change control / harmful control | held / detected | held / detected |
| Fully enabled verdict | 10 improved, 47 neutral, 1 worse, 41 uncertain | 10 improved, 47 neutral, 1 worse, 41 uncertain |

The performance comparison is unchanged in every cell, which is the point: both
fixes remove unsafe behaviour without moving any modelled result.

## Still open

Nothing in this matrix. The only finding this run still reports is
`controls_never_attempted_by_the_fully_enabled_arm`, which is informational: it
lists controls the simulated machine exposes that no arm ever tried to write.

The report's `Everything restored after recovery` line is `true` and the
`optid-recover` subprocess now exits `0` in every trial.

## How to reproduce

```sh
cargo build --release -p optid --features test-simulation --bins
mkdir -p /tmp/optid-evidence
printf 'optid-simulation-evidence-root-v1\n' > /tmp/optid-evidence/.optid-evidence-root-v1
cargo run --release -p optid --features test-simulation --bin optid -- \
    --evidence-root /tmp/optid-evidence --evidence-repeats 3
```

Build **every** binary of the crate, not just `optid`. The harness runs the
sibling `optid-recover` executable as a real subprocess before each supervised
restart; if that binary is missing it records the S3D step as not run and the
recovery results in the report are wrong.

`evidence-bundle.json` (~26 MB, every write and every modelled cycle for every
repeat) is not committed; `report.md` and `evidence-summary.json` are. The run
is proven byte-reproducible across all three repeats and across debug and
release builds. Absolute paths from the producing machine were replaced with
`<simulation root>`; nothing else was edited.

The regression guards are
`i2_a_failed_policy_reload_never_actuates_a_domain_that_is_switched_off` and
`removed_device_hands_back_before_one_clean_supervised_restart` in
`crates/optid/tests/i2_simulation_evidence_cli.rs`. The first was confirmed to
fail — naming all twelve escalated controls — with its fix temporarily
reverted.
