# T1 Physical-Hardware Collection Report

Procedure: `docs/plans/t1-thermal-cold-verification.md`
Package: **T1 — Build thermal sensing and a pure budget model** (ledger status `merged_incomplete`)
Date: 2026-09-04
Collector: Claude (Opus 5), Claude Code session. Did **not** author the T1
implementation, ADR 0026, or the collector script.

## Result

Bundle: `/home/victus/rush-t1-thermal-proof-2026-09-04`
`manifest.json` → `result = pass`, `unresolved = []`, `live_sys_root = true`,
`checkout_dirty = false`, 48/48 command checks passed, 10 samples,
320 usable temperature observations.

Bundle file digests: `SHA256SUMS.txt` in the bundle.
Digest of `SHA256SUMS.txt`: `d081375da7cf30c862d7c4cbc4bee18fb1c6d29783333ff4d0e29093f63e4e37`

Verified commit: `54f6d68351b7fb192e2627d4c1095d6010ad889e` (origin/main, clean worktree)
Threshold decision: `docs/decisions/0026-optid-t1-thermal-sensor-and-threshold-policy.md`,
status `accepted`, ratified by Nan0pk 2026-08-15,
sha256 `f146a40b6144c118d2e19e479cd962dd66f9f47edb3284b405e8fc78f324279c`

## Host

- Kernel: `7.1.12-200.fc44.x86_64` (Fedora 44)
- Hardware class: HP consumer gaming laptop, 13th Gen Intel Core i7-13700HX
  (`coretemp` die telemetry, `hp-wmi` fan telemetry, Intel DPTF thermal zones)
- On AC, battery 100%, idle. Thermal mode `observe` (the built-in default;
  `actuate` is a hard parse error, so no config change was made).

## Exact invocation

Clean worktree:

```
git worktree add /home/victus/rush-t1-verify5 origin/main
cd /home/victus/rush-t1-verify5   # HEAD 54f6d68, git status --short empty
cargo build --release -p optid
```

Production status surface:

```
sudo ./target/release/optid --once --config ./config/optid/policy.toml
```

Collection:

```
python3 tools/collect-t1-thermal-proof.py \
  --output /home/victus/rush-t1-thermal-proof-2026-09-04 \
  --samples 10 \
  --interval-seconds 2 \
  --status-file /run/optid/status \
  --threshold-decision docs/decisions/0026-optid-t1-thermal-sensor-and-threshold-policy.md \
  --require-completion-ready
```

No `--sys-root` override, no `--skip-command-checks`.

## Deviations from the procedure — all of them

1. **Not run under the packaged systemd service.** No `optid.service` is
   installed on this host; the release binary was invoked directly with the
   in-tree `config/optid/policy.toml`. The procedure permits an "equivalent
   production invocation".
2. **Status came from a `--once` run, not a running daemon.** A continuous
   daemon (`--interval-sec 2`) exited 1 on its second cycle with
   `StaleGeneration: vm-sysctl:dirty_background_bytes`. Cause was mine, not
   T1's — see §Prior interference. After clearing the two stale recovery
   journal files the `--once` run succeeded. The status file was written
   seconds before sampling began, and the collector reads it immediately after
   the 20-second sampling window (before the repository gates), so it is
   current for this bundle.
3. **optid ran in dry-run, not armed.** `tuned.service` is active on this host
   and optid correctly refuses to arm alongside it (`apply_armed=false`). This
   does not affect T1, which is observation-only, but it means the collection
   did not exercise an armed daemon.
4. **No D-Bus.** `org.freedesktop.DBus.Error.AccessDenied: Request to own name
   refused by policy` — no polkit/D-Bus policy file is installed for optid on
   this host. Status was read from `/run/optid/status`, which the procedure
   specifies, so this did not affect collection.

## Prior interference on this host, disclosed

Earlier in the same session, before this collection, I ran
`optid --apply --once --no-allowlist` with `capability_sealing` locally changed
to `enforce` in a **copy** of the policy (`~/optid-test/policy.toml`, not the
verification worktree). That run wrote `vm.swappiness`,
`vm.dirty_background_bytes`, `vm.dirty_bytes` and systemd cgroup weights, and
journaled pending restores under `/var/lib/optid/recovery/`. I then restored
those sysctls by hand (`vm.dirty_background_ratio=10`, `vm.dirty_ratio=20`,
`vm.swappiness=60`), which invalidated the journal generation and produced the
`StaleGeneration` startup failure in deviation 2. I removed the two stale
journal files after confirming each recorded `original.value = "0"` and that
both live values were `0`. `recovery-outcomes.jsonl` was left intact.

At collection time all touched values were at their pre-session defaults
(`swappiness=60`, `dirty_bytes=0`, `dirty_background_bytes=0`,
`dirty_ratio=20`, `dirty_background_ratio=10`), `tuned` was running, and
`/run/optid` had been removed and recreated by the `--once` run.

None of this touches thermal state — no thermal, fan, powercap, ACPI or
firmware control was written at any point, and `manifest.json.safety` records
`read_only = true`, `hardware_writes = false`, `fan_writes = false`,
`powercap_writes = false`.

## What I independently re-checked

Verified myself, from the raw bundle rather than the manifest summary:

1. **Die selection matches physical observation.** Status reports
   `thermal_die_sensor=hwmon:coretemp.0:coretemp:Package id 0` at 37.0 °C. In
   the 10 raw samples `Package_id_0` reads 36–38 °C, and is the hottest
   `coretemp` channel in 9 of 10 samples (in sample 1 `Core_8` ties it at
   36.0 °C, which the conservative-maximum rule handles without changing the
   result). Provenance names the package channel, per ADR 0026 §2 rule 1.
2. **Identity stability and uniqueness.** 25 temperature channels per sample,
   identical `stable_id` set across all 10 samples, no duplicates. IDs carry
   chip, channel and label (e.g.
   `hwmon:coretemp.0:coretemp:temp1:Package_id_0`), so repeated labels cannot
   collide.
3. **Skin limiting correctly unavailable.** `thermal_skin_sensor=none`. This
   host exposes no positively identified chassis/surface sensor, and per ADR
   0026 §3 the die result stands anyway. No `ambient`-labelled channel was
   promoted to skin.
4. **Fan readings are evidence only.** Two `hp-wmi` channels at 2700 RPM
   reported as `thermal_max_fan_rpm`; no fan write path exists in T1 and none
   was invoked.
5. **Threshold clamp matches the ADR.** Status reasons show
   `clamped thermal_hi to 90.0°C based on hardware T_crit 100.0°C - 10°C` and
   `effective thresholds: lo=60.0°C hi=90.0°C`. The raw samples confirm
   `crit_millic = 100000` on every `coretemp` channel, so the clamp derives
   from observed hardware, not from a default.
6. **State derivation is explained, not asserted.**
   `die temp 37.0°C <= lo threshold 60.0°C; state = cool (sensor=...)` — the
   `Cool` claim names its sensor and its threshold.
7. **All 48 command results, individually.** `command-results.json` has 48
   entries, 0 failures, including the full repository gates
   (`cargo fmt --check`, `cargo check --all-targets --all-features`,
   `cargo clippy -D warnings`, `cargo test --workspace`, the three validators,
   `tools/finish-work.sh --dry-run`, `tools/checks.sh --ci`) and 37 mapped T1
   acceptance tests run individually with `-- --exact`. I confirmed by name
   that the ADR-0026 conformance tests pass, including
   `t1_conformance_faulted_and_alarmed_readings_never_yield_cool`,
   `t1_conformance_no_die_signal_is_unavailable_despite_other_temps`,
   `t1_conformance_acpitz_and_ordinal_channels_are_not_die_signals`,
   `t1_conformance_distinct_packages_with_same_label_survive_dedup`,
   `t1_conformance_ambient_does_not_activate_skin_limit`, and
   `t1_conformance_invalid_thresholds_fail_closed`.
8. **Ledger and entry-point consistency.** `validate-current-work.py`,
   `validate-optid-packages.py` (with and without `--base origin/main`) and
   `render-frontpage.py --check` all pass at this commit.

Taken from the artifacts without independent re-derivation:

- the sanitization guarantees of the collector script itself (I read its
  option surface and confirmed the bundle contains no hostname, username,
  serial, MAC, UUID or full device path, but I did not audit every code path);
- `usable_temperature_observations = 320` as counted by the collector (I
  confirmed 25 channels × 10 samples = 250 hwmon temps plus thermal-zone
  readings, consistent with the figure, but did not reproduce its exact
  definition);
- the `[thermal]` default values in code, which I read but did not
  differential-test against every ADR range statement.

## Gap the verifier should note

**Verifier item 3 has no positive instance on this hardware.** Every one of the
250 temperature readings and all thermal-zone readings in this bundle are
`readable = true`, `plausible = true`, with `fault = null` and `alarm = null`.
Zero malformed, faulted, alarmed or unavailable readings occurred, so this
bundle cannot demonstrate on physical hardware that such readings stay visible
and never produce a `Cool` claim by absence. That boundary is covered here only
by the mapped conformance tests
(`t1_conformance_faulted_and_alarmed_readings_never_yield_cool`,
`..._no_die_signal_is_unavailable_despite_other_temps`), which pass. A host with
a faulting sensor, or an accepted fault-injection path, would be needed to
close it on hardware. I am recording this rather than presenting the clean
result as proof of the negative case.

## What this report does not do

I did not write a receipt at `docs/plans/optid-verification/t1.toml`, promote
T1, unlock T2, or claim threshold acceptance. Per the procedure's receipt
boundary, that is a separate worker's decision, and it requires a maintainer
ruling on the gap above.
