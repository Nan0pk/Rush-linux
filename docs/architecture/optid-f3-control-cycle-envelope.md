# optid F3 control-cycle envelope

F3 makes the existing `optid` control loop produce one typed, versioned record for each completed cycle. It is a diagnostic projection of the decisions and writes that already occurred; it does not change policy selection, action order, values, write authorization, or restoration selection.

## Schema and compatibility

The current public schema is `schema_version = 2` and is defined in `crates/optid/src/envelope.rs`.

Compatibility rules:

- object readers must ignore unknown fields;
- omitted optional fields and collections use their documented defaults;
- enum values are closed: an unknown future enum value fails clearly rather than being silently reinterpreted;
- field meaning and enum semantics remain stable within a schema version;
- a schema-version increment is required for incompatible meaning or enum changes;
- domains are emitted in the canonical `Domain::all()` order, gate records in pipeline order, and per-target and restore outcomes by stable logical target ID.

The committed golden fixture is `crates/optid/tests/fixtures/f3-control-cycle-v2.json`.

## Correlation

The daemon creates one correlation ID per real control-loop iteration. The ID is a boot-scoped timestamp plus an in-process sequence:

```text
optid-<boot-scope-hex>-<cycle-sequence-hex>
```

The injected F2 clock makes the scheme deterministic in tests. The same top-level ID identifies the observation, resolved decision, desired actions, evaluated gates, apply outcomes, current inverse-restoration outcomes, human status, `status.json`, JSONL history, and relevant structured text logs for that cycle. Consecutive cycles receive distinct sequence values. F3 deliberately avoids a UUID dependency.

## State files and ownership

The daemon owns these files under its existing state directory:

- `status`: backward-compatible human-readable latest status, prefixed with the cycle correlation ID;
- `status.json`: the latest complete F3 envelope, replaced atomically;
- `control-cycles.jsonl`: append-only versioned cycle records;
- `decisions.log` and `actions.log`: existing text audit surfaces, now correlation-aware where the current path emits structured cycle or action messages.

`io.rushlinux.Optid1.StatusJson` and offline `optctl status --json` both return the daemon-generated `status.json`. `optctl` validates the schema version and correlation ID but does not reconstruct machine JSON from `status`, duplicate the Rust schema, or mutate hardware/state while inspecting status.

## Gate and outcome meaning

A domain-mode pass is only one gate result. It does not imply that apply, contract, allowlist, capability, journaling, write, or readback stages passed. Every reached gate has a typed stage, disposition, and reason.

Outcome terms are intentionally distinct:

- **unsupported**: the interface or target is absent or not supported;
- **not applicable**: the stage does not apply to this action or current inverse path;
- **not evaluated**: execution did not reach the stage;
- **denied**: a policy, boot, contract, allowlist, or capability gate blocked progress;
- **skipped**: the current implementation deliberately did not write for a runtime condition;
- **redundant**: the requested value was already present, so no write was attempted;
- **applied**: a write was attempted and accepted; readback is separately confirmed, unavailable, or mismatched;
- **failed**: a write was attempted and failed with a typed error kind;
- **drifted**: readback did not match the desired value;
- **restored**: the existing `active_keys + Actuator::revert_key_outcome` path restored a supported stale journal key;
- **restoration failed**: that actual restore path failed and retained pending ownership for retry;
- **ownership relinquished**: no supported current inverse-restoration path owns the target.

A readback is never claimed where the implementation performs none. Such cases are `not_performed` or `unavailable`.

## Public diagnostic boundary

The envelope uses stable logical component and target identities. It must not expose raw procfs/sysfs/device paths, Rust source locations, usernames, home directories, environment variables, command lines, credentials, tokens, arbitrary kernel dumps, or unrelated journal content. Detailed local text logs may retain operator-facing paths needed for on-host diagnosis; the public JSON is validated separately and rejects prohibited path markers.

## Deliberate limits

F3 records current behavior only. It does not implement the F4 desired-state reconciler cutover, Systemd property restoration, persistent write-ahead transactions, capability sealing, circuit breakers, event-reactor work, full diagnostics capture, journal bundles, telemetry repair, or new hardware controls. Unsupported restoration remains explicit instead of being reported as success. Package completion still requires an independent cold verifier and a separately committed verification receipt.
