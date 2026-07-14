# optid crate

Adaptive optimization daemon. Loaded automatically when you read files here.

## Module map
- `sensors.rs` — reads `/proc`, `/sys`; must never block the event loop
- `policy.rs` — rule evaluation; pure functions, no I/O
- `decision.rs` — synthesises policy output into an `Action`
- `actuator.rs` — applies `Action` to the kernel; all side effects live here
- `contracts.rs` — invariant assertions; do not relax bounds without a failing test
- `dbus.rs` — D-Bus interface; keep separate from core logic

## Hard invariants
- Never `unwrap()` in sensors/actuator paths — use `?` and `tracing::error!`
- `contracts.rs` bounds are load-bearing; changing them requires a test that proves the old bound was wrong
- Sensor reads are non-blocking; any blocking call belongs in a `spawn_blocking`

## Test scope
`cargo test -p optid` — keep this green at all times.
