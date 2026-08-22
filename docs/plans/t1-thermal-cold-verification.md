# T1 Thermal Cold-Hardware Verification

This is the verifier-facing procedure for **T1 — Build thermal sensing and a pure budget model**. The implementation landed in pull request **#334** and was later repaired, but the canonical ledger remains `merged_incomplete`. The remaining boundary is not another builder assertion: it is physical-hardware observation plus explicit review of the threshold policy.

The procedure is read-only. It must not write fan controls, ACPI thermal controls, powercap constraints, firmware settings, or any other hardware state.

## Why T1 is not complete yet

The deterministic in-crate tests prove the pure functions and injected production path. They do not prove that a real machine exposes usable sensors, that optid's production status agrees with those sensors, or that the default threshold values are accepted policy rather than research hypotheses.

Completion requires all of these distinct results:

1. repository and mapped T1 tests pass at one exact commit;
2. a clean independent verifier captures real thermal observations on physical hardware;
3. the production optid status surface reports a corresponding thermal state and reasons;
4. sensor absence and malformed values remain explicit rather than being presented as cool/full headroom;
5. the threshold and zone-selection policy is reviewed and accepted separately; and
6. a cold verifier commits a receipt under `docs/plans/optid-verification/t1.toml` with no unresolved findings.

Passing simulated tests cannot substitute for item 2. Physical observations cannot substitute for item 5.

## Threshold decision that must exist first

The current implementation exposes these experimental values in the dedicated top-level `[thermal]` table:

- `thermal_lo_c`;
- `thermal_hi_c`;
- `hysteresis_c`;
- `skin_temp_limit_c`; and
- the dynamic upper clamp derived from `temp*_crit - 10°C`.

The completion plan also leaves zone selection and weighting open. The decision supplied to the collector must be a tracked file in the clean verification commit; a local or uncommitted note cannot satisfy this boundary. A reviewed decision must state at least:

- which die/package sensors are eligible;
- whether v1 uses the conservative maximum of eligible sensors or a weighted policy;
- how duplicate hwmon and ACPI views are recognized without collapsing distinct packages;
- acceptable ranges and ordering for low, high, hysteresis, and skin thresholds;
- behavior when a sensor is missing, malformed, faulted, alarmed, or unavailable between cycles;
- whether a skin limit is applied only to positively identified skin/chassis sensors; and
- why the chosen values are safe as an observation contract even though T1 performs no actuation.

The completion plan recommends the maximum of eligible zones for v1 and no fan writes. The research defaults remain hypotheses until that review is recorded. The collector can reference an accepted decision, but it cannot create or approve one.

## Maintainer ruling on evidence provenance (2026-08-17)

Verifiers have twice raised whether a locally produced proof bundle is
sufficient, given that every earlier receipt in `docs/plans/optid-verification/`
cites a CI artifact. The maintainer of record has ruled. It is settled; do not
re-open it as an unresolved item.

**Physical hardware is the objective, so hardware evidence is primary.** CI can
prove the algorithm and the sequence — what the levers and pulleys do. It cannot
prove what this package exists to prove, because a CI runner has no real thermal
topology to observe. CI evidence is acceptable for early versions; for a
completion receipt on a hardware-facing package, hardware is what counts.

**What replaces CI provenance is collector independence, not location.** The
weakness of a locally produced bundle is that the agent that wrote the code also
produced its evidence. That is fixed by having a *different* agent collect it on
real hardware, not by moving the collection to a machine with no sensors.

A completion-ready collection for this package must therefore:

- run on physical hardware with a real thermal topology;
- be collected by an agent that did not write the implementation under test;
- record in the collection report **how** the collection was performed — the
  exact invocation, the host state, and every deviation from this procedure; and
- record **what the collecting agent independently re-checked**, distinguishing
  what it verified for itself from what it took from the artifacts it was given.

A verifier may still record a provenance concern, but the bundle being locally
produced is not by itself grounds to withhold a receipt.

## Clean verification checkout

Use a fresh clone or worktree at the exact implementation commit being verified. Confirm the checkout is clean and has sufficient history for receipt-freshness validation.

```bash
git status --short
git rev-parse HEAD
python3 tools/validate-current-work.py
python3 tools/validate-optid-packages.py
python3 tools/render-frontpage.py --check
```

Then run the full repository gates required by `AGENTS.md` and the package workflow:

```bash
git diff --check
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 tools/validate-optid-packages.py --base origin/main
bash tools/finish-work.sh --dry-run
bash tools/checks.sh --ci --changed-base origin/main
```

The collector below also runs these commands and every mapped T1 acceptance test individually with `--exact`; the verifier must inspect its recorded command results rather than relying on a summary sentence.

## Physical-hardware collection

Start the exact optid build under the normal production service or equivalent production invocation, with thermal mode `observe`. Confirm `/run/optid/status` is current. Then run:

```bash
python3 tools/collect-t1-thermal-proof.py \
  --output /tmp/t1-thermal-proof \
  --samples 10 \
  --interval-seconds 2 \
  --status-file /run/optid/status \
  --threshold-decision docs/decisions/<accepted-threshold-decision>.md \
  --require-completion-ready
```

Completion-ready mode requires the live `/` sysfs root. The hidden `--sys-root` override exists only for deterministic developer tests; any non-root value is recorded as unresolved and cannot produce a completion-ready bundle.

The collector records only sanitized class, chip, channel, label, temperature, critical threshold, alarm/fault state, fan RPM, ACPI zone type and instance, trips, and thermal status fields. Hwmon channel names and ACPI thermal-zone instances remain in the sanitized identity so repeated labels or zone types cannot silently collide. It does not collect hostnames, usernames, serial numbers, MAC addresses, UUIDs, network details, home paths, full sysfs device paths, or unrelated optid status.

A passing collection must contain:

```text
manifest.json
thermal-observations.jsonl
optid-thermal-status.txt
threshold-decision-reference.json
command-results.json
```

The manifest must report `result = pass`, `live_sys_root = true`, a clean exact source commit, at least one plausible physical temperature observation, a complete production thermal status, all command checks passing, a digest of the accepted threshold decision, and an empty unresolved list.

## Verifier inspection

The independent verifier must inspect, not merely generate, the bundle:

1. compare the hottest plausible physical die/package observation with the selected die sensor and state in `optid-thermal-status.txt`;
2. confirm stable ordering and unique identities across all samples;
3. confirm malformed, implausible, faulted, or unavailable readings are visible and never create a `Cool` claim by absence;
4. confirm fan readings are observation only and no write path was invoked;
5. inspect every mapped test result and the full repository gates;
6. inspect the accepted threshold decision and verify the implementation matches it; and
7. record any mismatch as unresolved rather than repairing the implementation during verification.

If the implementation cannot represent an accepted stale-sensor or multi-zone rule, stop verification and open a separate T1 repair. Do not reinterpret missing behavior as hardware evidence.

## Receipt boundary

Only a different worker may propose `completed`. The receipt at `docs/plans/optid-verification/t1.toml` must name:

- package `T1`;
- implementation pull request `334` plus any later T1 repair pull request examined;
- the exact 40-character verified commit;
- the independent verifier;
- every command actually run;
- the physical hardware proof bundle and its digest;
- the accepted threshold decision and its digest;
- production-path runtime proofs;
- the observed kernel release and non-identifying hardware model class; and
- an empty unresolved list.

The builder of this procedure does not create that receipt, promote T1, unlock T2, or claim threshold acceptance.
