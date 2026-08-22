# BLOCKED — Phase D phase 4 cannot certify `latency-critical`

Raised 2026-08-22 while building D2 (`mixed-load-001`) and validating it against
the nominated laptop slot. Follows the escalation format in
`OPTID-COMPLETION-PLAN.md` §6.3. **No capture has been run**; a capture with
this unresolved would spend a battery cycle certifying the wrong class for one
of the four phases.

```text
BLOCKED — NEW WORK PACKET REQUIRED

Package: D2/D3 — mixed-load-001 preset and the Phase D baseline/optid arms
Base SHA: 55221c7 (main)
Stopped before: running the D3/D4 captures on the nominated laptop slot
Trigger: a missing choice — three project documents disagree about what phase 4
         measures, and no source resolves it
Evidence:
  - docs/strategy/mixed-load-workload.md phase 4 expects class
    `latency-critical`, driven by `glmark2 --fullscreen`.
  - crates/optid/src/policy.rs:857 returns `LatencyCritical` only when
    `(1.5..4.0).contains(&load) && cpu_pressure >= cpu_pressure_perf_avg10
     && snapshot.on_ac == Some(true)`.
  - config/optid/policy.toml:68 sets `cpu_pressure_perf_avg10 = 12.0`.
  - Measured on the laptop slot: a *fully saturated* 48-job C++ build reaches
    `psi-cpu-avg10 = 60.31 %`, while `ninja`'s default `nproc + 2` reached only
    0.06 %. `glmark2` is GPU-bound and runs 1–2 threads; it produces neither the
    load band nor the 12 % CPU pressure.
  - release/milestones.toml Criterion 3 requires the full cycle to run **on
    battery**, and the `on_ac == Some(true)` term makes `LatencyCritical`
    unreachable there by construction.
Why guessing is unsafe: phase 4 would classify as `interactive` under every
  configuration, so its frametime numbers would be filed against a class the
  daemon never entered — a milestone claim about a class transition that did not
  happen.
Decision required: what drives phase 4 into `latency-critical`?
Options supported by sources:
  A. Phase 4 registers through the existing GameMode shim
     (`pin_class = "latency-critical"`, config/optid/policy.toml:56), the path
     Steam/Lutris already use. Consequence: that phase's class becomes an
     assertion rather than an observation, and the run stops proving the
     classifier can *infer* the class from load.
  B. Amend the `LatencyCritical` branch — drop the `on_ac` term, lower the
     pressure requirement, or admit a GPU-load signal. Consequence: a policy
     change needing its own ADR, and it touches every host, not just Phase D.
  C. Amend the workload so phase 4 carries CPU contention alongside the GPU
     load. Consequence: the phase stops being a clean latency-critical probe and
     starts overlapping phase 3.
Recommendation: A. The pin mechanism already works, it is the real-world path
  for this class, and it changes no policy. Record in VERDICT.md that phase 4's
  class was pinned, not inferred.
Files already changed: none for this decision. The D2 preset, the AC-aggregation
  fix, the optctl status-schema fix and the throughput-saturation fix are
  committed on their own branches and stand independently of it.
Tests already run: `cargo test -p rushbench` (34 passed);
  `cargo test --workspace` (one pre-existing failure, see below);
  two scaled harness-validation runs of the preset on the laptop slot.
Safe independent work remaining in this package: Criterion 1 ("unsupported knobs
  are skipped with reasons") needs only a short root `optid --apply` run with the
  allowlist denial log and audit JSONL captured — no full cycle, and it does not
  depend on this decision.
```

## Second, smaller conflict in the same area

Criterion 2 needs both a p99 latency metric **and** a joules-per-work-unit
no-regression check:

- the energy counter a non-root process can read here is the battery charge
  counter (`intel-rapl:0/energy_uj` is `0400 root`), which measures nothing
  while the charger holds the pack full — so energy metrics need an unplugged
  run;
- `input-latency-p95/p99-ms` has no probe at all, so the only p99 available is
  `frametime-p99-ms`, which comes from the phase that option A above is about.

If the harness is run as root the RAPL counter becomes readable and Criterion 2
can be captured on AC, with Criterion 3 captured separately on battery. That is
two runs per arm rather than one (~1.5 h of machine time), and both arms must
use the same counter either way.

## Unrelated red test on `main`

`reconciler::tests::s2d_production_daemon_run_uses_persistent_transaction_protocol`
fails on this host and has failed since the commit that introduced it
(`a0f3179`), so it is not a regression. `crate::run()` calls
`shim::detect_conflicts`, which spawns a real `systemctl is-active` rather than
going through the F2 kernel seam; `tuned.service` is in
`competing_policy_daemons` and is active on Fedora 44, so `--apply` is
downgraded to dry-run, no transaction is written, and the test's `state:` /
`rename:` assertions on `s2d-recovery` cannot hold. It passes only on hosts with
no competing power daemon — a CI container. Routing that check through the
injectable boundary is F2 work, not this package's.
