# optid simulated evidence — after the policy-reload fix, 2026-08-30

**Evidence class: deterministic software proof + model-conditional estimate.**
See `docs/research/0024-non-bare-metal-optid-validation-method.md`. Nothing
here is a measured guest outcome or a physical measurement, and nothing here
claims laptop watts, battery life, temperature, or support for any machine.

This is the same matrix as
[`../2026-08-29/`](../2026-08-29/README.md), re-run after the fix for the
high-severity finding that run produced. Read the two together: the earlier
directory is the evidence that found the defect, this one is the evidence that
it is gone.

## What changed between the two runs

The 2026-08-29 run reported
`policy_reload_fallback_escalates_domain_modes` (**high**): a `policy.toml`
that became unreadable while optid was running fell back to
`Policy::curated_baseline()`, whose per-domain default is `actuate`, while
`apply_armed` was computed once at startup and never re-evaluated. An arm with
every domain `mode = "off"` wrote eight kernel controls after the injected
reload failure, and the `observe` arm wrote four.

The run loop now keeps the last policy that loaded cleanly and disarms dynamic
writes for any cycle whose reload did not return `Ok`.

| | 2026-08-29 | 2026-08-30 |
|---|---|---|
| `policy_reload_fallback_escalates_domain_modes` | reported, high | **absent** |
| Kernel writes by the all-`off` arm in the reload scenario | 8 | **0** |
| Kernel writes by the all-`observe` arm in the reload scenario | 4 | **0** |
| Determinism | 225/225 across 3 repeats | 225/225 across 3 repeats |
| Write attempts outside the simulation root | 0 | 0 |
| No-change control / harmful control | held / detected | held / detected |
| Fully enabled verdict | 10 improved, 47 neutral, 1 worse, 41 uncertain | 10 improved, 47 neutral, 1 worse, 41 uncertain |

The performance comparison is unchanged in every cell, which is the point: the
fix removes an unsafe escalation without moving any modelled result.

## Still open

`owned_target_hot_removal_aborts_the_control_loop` (**medium**) is still
reported here and is **not** fixed by this change. Hot-removing a device optid
owns still aborts the control loop before its handback, and still needs two
supervised restarts and an `optid-recover` pass to hand the machine back.

## How to reproduce

```sh
mkdir -p /tmp/optid-evidence
printf 'optid-simulation-evidence-root-v1\n' > /tmp/optid-evidence/.optid-evidence-root-v1
cargo run --release -p optid --features test-simulation --bin optid -- \
    --evidence-root /tmp/optid-evidence --evidence-repeats 3
```

`evidence-bundle.json` (~26 MB, every write and every modelled cycle for every
repeat) is not committed; `report.md` and `evidence-summary.json` are. The run
is proven byte-reproducible across all three repeats and across debug and
release builds. Absolute paths from the producing machine were replaced with
`<simulation root>`; nothing else was edited.

The regression guard is
`i2_a_failed_policy_reload_never_actuates_a_domain_that_is_switched_off` in
`crates/optid/tests/i2_simulation_evidence_cli.rs`. It was confirmed to fail —
naming all twelve escalated controls — with the fix temporarily reverted.
