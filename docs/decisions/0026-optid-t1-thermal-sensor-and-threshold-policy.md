# ADR 0026: optid T1 Thermal Sensor and Threshold Policy

Status: accepted
Ratified-by: Nan0pk, 2026-08-15

Date: 2026-08-07
Tags: optid, thermal, sensors, thresholds, safety, hardware, T1

## Context

T1 implements read-only thermal discovery and a pure derating model. Its code has
landed, but the package remains `merged_incomplete` because simulated tests do
not decide which platform readings may authorize a `Cool` claim, how duplicate
views are handled, or which thresholds are accepted policy. Completion also
requires independent physical-hardware proof and a cold-verification receipt.

Linux exposes overlapping thermal views through hwmon and the generic thermal
framework. A single physical junction may appear through more than one view,
while identically named channels may belong to different CPU packages or
unrelated devices. Conversely, a generic ACPI zone, board sensor, NVMe sensor,
or ambient sensor is not evidence of CPU junction temperature merely because it
is the only readable temperature.

The kernel `coretemp` documentation identifies package channels with labels such
as `Package id Y` and exposes `tempX_crit` as maximum junction temperature. The
kernel `k10temp` documentation distinguishes physical `Tdie` from the `Tctl`
control value on processors where both are exported. The generic thermal
framework also documents that ACPI thermal zones and their hwmon projections can
represent the same platform zone. These interfaces justify explicit provenance,
positive classification, and conservative duplicate handling; they do not
justify guessing from a generic channel name.

This ADR decides the v1 observation contract. It does not authorize fan writes,
ACPI mode changes, power-limit writes, hardware promotion, or T1 completion.

## Decision

### 1. Observation-only boundary

T1 remains read-only.

- Valid thermal modes are `off` and `observe`.
- optid does not write fan controls, PWM values, ACPI thermal-zone mode, trip
  points, platform profiles, or firmware/EC state.
- Kernel and firmware thermal protection remain authoritative.
- Fan RPM is reported as evidence only. It must not substitute for a valid CPU
  junction signal and must not independently create thermal headroom.

### 2. Eligible CPU die/package signals

A reading may participate in the primary die/package maximum only when its
source positively identifies it as CPU package, die, junction, or core
telemetry.

Eligible signals are:

1. Intel `coretemp` package channels labelled `Package id Y` or the equivalent
   package label exported by the running kernel.
2. Intel `coretemp` per-core channels. These may raise the conservative maximum
   but do not replace an available package channel in provenance reporting.
3. AMD `k10temp` or equivalent maintained kernel-driver channels labelled
   `Tdie`. `Tdie` is preferred when present.
4. AMD `Tctl` when `Tdie` is unavailable. It is an eligible conservative control
   signal, but status and evidence must identify it as `Tctl`; it must not be
   described as a physical chassis or case temperature.
5. AMD CCD channels such as `TccdN`. They may raise the conservative maximum but
   do not replace an available package/die channel in provenance reporting.
6. `x86_pkg_temp` or another platform thermal-zone type that positively names a
   CPU package, used as fallback when no positively identified hwmon CPU signal
   exists.
7. A model-specific CPU/SoC thermal zone only when a tracked platform mapping or
   driver contract positively establishes that identity.

The following are not sufficient by themselves:

- `acpitz`;
- `temp1`, `temp2`, or another ordinal channel;
- `ambient`, `board`, `system`, `VRM`, `NVMe`, `GPU`, `battery`, or wireless
  temperature;
- the fact that a reading is the hottest or the only readable temperature.

When no eligible CPU die/package signal exists, the result is `Unavailable`
with maximum derating. optid must never fall back to an unrelated usable
temperature and call it the die signal.

### 3. Skin/chassis signals

Skin limiting is secondary to the CPU die/package model and is applied only to a
positively identified user-accessible chassis or surface sensor.

Eligible skin signals require:

- a driver or model mapping identifying the channel as skin, chassis, surface,
  palm-rest, keyboard-deck, or another user-touch surface; and
- stable source/channel provenance in status and evidence.

A generic `ambient` label is not sufficient because ambient may describe air,
board, inlet, room, or another non-touch location. Generic ordinal channels are
also insufficient. If no positively identified skin sensor exists, skin
limiting is unavailable and must be reported as such; it does not invalidate an
otherwise valid CPU die/package result.

### 4. Aggregation and duplicate handling

For v1, use the conservative maximum rather than a weighted average.

- The selected die temperature is the maximum of all eligible, current,
  plausible CPU signals.
- The selected skin temperature is the maximum of all eligible, current,
  plausible skin signals.
- Weighting, averaging, smoothing that can reduce the observed maximum, and fan
  RPM inference are out of scope for T1.

Do not deduplicate solely by normalized label and class. Two sockets, packages,
CCDs, or model-specific zones may legitimately carry the same label.

Two views may be collapsed only when either:

1. their stable device topology establishes that they are projections of the
   same physical source; or
2. a tracked platform/driver alias rule identifies the pair as the same source.

When identity is uncertain, retain both readings. Maximum aggregation means a
true duplicate does not amplify the result, while retaining it avoids silently
losing a distinct hot package.

### 5. Telemetry validity and failure behavior

A temperature contributes only when it is finite, current for the present
collection cycle, successfully parsed, within the implementation's plausible
physical envelope, and not marked faulted or alarmed by an available kernel
attribute.

- Read failure, parse failure, disappearance, stale carry-forward, implausible
  value, fault, or alarm excludes that reading and records a reason.
- A critical alarm is evidence of an unsafe or out-of-contract state. It must
  never produce `Cool` or increased headroom.
- No eligible valid CPU signal means `Unavailable`, derating ratio `1.0`, and no
  selected die identity.
- `mode = off` remains `Disabled` with no headroom claim; it is not equivalent
  to a cool observation.
- Previous-cycle values may influence hysteresis state, but a previous
  temperature must not be reused as if it were a fresh observation.

### 6. Thresholds and ordering

The v1 product defaults are:

- `thermal_lo_c = 60.0`;
- `thermal_hi_c = 95.0`;
- `hysteresis_c = 2.0`;
- `skin_temp_limit_c = 43.0`.

These values are accepted as conservative initial product policy, not as a
claim that one universal regulatory or comfort limit applies to every material,
contact duration, processor, or chassis.

Configuration must satisfy all of the following:

- every value is finite;
- `40.0 <= thermal_lo_c <= 80.0`;
- `70.0 <= thermal_hi_c <= 120.0`;
- `0.0 <= hysteresis_c <= 10.0`;
- `35.0 <= skin_temp_limit_c <= 55.0`;
- `thermal_hi_c >= thermal_lo_c + 5.0`;
- `thermal_lo_c - hysteresis_c` remains within the plausible telemetry envelope.

Invalid configuration fails closed and must not be silently clamped into a
valid-looking policy.

When the selected CPU source exports a valid critical junction temperature, the
effective upper threshold is:

```text
effective_hi_c = min(thermal_hi_c, critical_temp_c - 10.0)
```

The effective upper threshold must still be at least `thermal_lo_c + 5.0`. If
that ordering cannot be satisfied, the result is `Unavailable` with maximum
derating rather than an invented interpolation range.

The lower threshold uses the configured hysteresis only when leaving a prior
`Derating` or `Constrained` state. Hysteresis must not hide a missing or invalid
current observation.

### 7. Derating model

With a valid selected die signal and valid thresholds:

- at or below the effective lower threshold: `Cool`, ratio `0.0`;
- at or above the effective upper threshold: `Constrained`, ratio `1.0`;
- between them: linear interpolation into `Derating`.

A valid skin signal above `skin_temp_limit_c` may only increase derating. It must
never reduce die-derived derating or create headroom.

Status must record the selected source identity, observed temperature, effective
thresholds, state, ratio, and reasons sufficient for an independent verifier to
reproduce the choice.

## Current implementation conformance findings

Inspection of `crates/optid/src/thermal.rs` at
`f3d785df064c9b2734509307bd1b33cf409ea9fb` found behavior that already matches
parts of this proposal:

- observation-only modes;
- linear derating and lower-threshold hysteresis;
- maximum selection among classified readings;
- dynamic `critical - 10°C` upper clamping;
- fail-closed handling when no usable temperature exists; and
- skin input only increasing derating.

The same inspection found blocking mismatches that must be repaired and tested
before this ADR can support completion-ready hardware proof:

1. When no reading is classified as die, the implementation falls back to the
   hottest usable temperature. This can turn an unrelated board, storage, GPU,
   battery, ambient, or generic ACPI reading into the CPU die signal.
2. Deduplication uses normalized label plus die/skin class without requiring
   stable physical-device identity. Identically labelled readings from distinct
   packages may be collapsed.
3. Discovery does not consume available temperature fault/alarm attributes as
   part of sensor validity.
4. Parsing does not enforce the threshold ranges and ordering decided above.
5. `ambient` is classified as skin without positive evidence that it represents
   a user-touch surface.

These findings are not waived by proposing or ratifying this ADR. They require a
separate T1 runtime repair with mapped deterministic tests. Until that repair,
physical collection may be diagnostic but cannot be treated as
completion-ready proof.

**Repair status (2026-08-15).** All five findings are repaired in the T1
conformance change merged as PR #406 (`main` at `0f44a2a`), with eleven mapped
deterministic tests added under `thermal::tests::t1_conformance_*` and recorded
in the ledger's T1 `acceptance_tests` map. The repair did not ratify this ADR;
ratification is the separate maintainer act recorded in the `Ratified-by:` line
above.

**Open findings at ratification (2026-08-15).** Ratification accepts the policy
in §2–§7 as written. It does not assert that the implementation matches it in
every respect. A review of the merged repair recorded four remaining
implementation-vs-policy gaps, none of which changes a decision above and none
of which is exercised on the first verification host (`coretemp` `crit` is
100.0 °C there, so the §6 clamp is inert):

1. §6's fail-closed rule for a low hardware `crit` is not implemented.
   `compute_thermal_budget` computes `(crit - 10.0).max(lo + 5.0)` and
   interpolates, where §6 requires `Unavailable` with maximum derating when the
   `hi >= lo + 5.0` ordering cannot be satisfied. Reachable only on a selected
   sensor exporting `crit < 75.0`.
2. §2's provenance rule — that core and `Tccd` channels may raise the maximum
   but do not replace an available package channel in reporting — is not
   honored: selection is a single maximum over temperature, so the hottest
   channel supplies both value and reported identity. The value stays correct
   and conservative; only the reported identity differs.
3. §3's positive-identification requirement is implemented as keyword matching
   against the hwmon chip *name* as well as the channel label, so a platform
   driver named for a touch surface classifies all of its channels as skin.
   The error can only raise derating, never lower it.
4. §5 does not state whether an alarm bit is authoritative when the alarmed
   channel's temperature is unreadable. The implementation drops such a channel
   before validity is assessed, so a second cool die channel can still yield
   `Cool`, and the alarm reason does not name the alarmed channel that §7 asks
   for.

These are recorded here so a cold verifier reads the accepted policy and the
known gaps together. They are scheduled as a follow-on repair after the T1
physical collection, not before it.

## Required conformance tests

The T1 repair must add deterministic tests proving at least:

1. no eligible die signal yields `Unavailable` even when unrelated valid
   temperatures exist;
2. generic `acpitz` and ordinal channels do not become die signals by fallback;
3. distinct packages with the same label survive deduplication;
4. positively mapped hwmon/thermal-zone aliases collapse deterministically;
5. `Tdie` is preferred in provenance while `Tctl` remains an explicitly named
   conservative fallback;
6. faulted and alarmed readings cannot produce `Cool`;
7. generic ambient temperature does not activate the skin limit;
8. invalid threshold ranges and ordering fail closed;
9. missing current-cycle telemetry is not replaced by a previous temperature;
10. maximum aggregation is stable regardless of discovery order.

Existing mapped T1 tests and full repository gates remain required in addition
to these cases.

## Consequences

- T1 obtains an explicit, reviewable policy instead of relying on research
  hypotheses or broad string matching.
- The model stays simple: positive classification, conservative maximum, linear
  derating, and fail-closed unknowns.
- Some machines will report `Unavailable` until a reliable platform mapping is
  added. That is preferable to false thermal headroom.
- Skin limiting remains optional unless the physical sensor identity is known.
- A code repair is required before completion-ready hardware proof.
- Ratification alone does not move T1 to `candidate` or `completed`, create a
  verification receipt, or unlock T2.

## Verification boundary

After human ratification and code conformance, an independent verifier must use
the read-only procedure in `docs/plans/t1-thermal-cold-verification.md` on
physical hardware. The proof must bind the exact source commit, this ADR and its
digest, mapped tests, production status, sanitized observations, and an empty
unresolved list. Only that independent receipt may propose T1 `completed`.

## References

- `docs/plans/t1-thermal-cold-verification.md`
- `docs/research/0013-thermal-fan-budget-coupling.md`
- `crates/optid/src/thermal.rs`
- Linux kernel `coretemp` documentation:
  https://docs.kernel.org/hwmon/coretemp.html
- Linux kernel `k10temp` documentation:
  https://docs.kernel.org/hwmon/k10temp.html
- Linux generic thermal sysfs documentation:
  https://docs.kernel.org/driver-api/thermal/sysfs-api.html
