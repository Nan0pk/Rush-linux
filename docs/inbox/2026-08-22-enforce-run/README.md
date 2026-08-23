# It actuates, and it reverts — plus the thing that was hiding it

2026-08-22, HP Victus 16-r0xxx laptop slot, root, `capability_sealing = "enforce"`
supplied through a run-local config copy (the shipped `config/optid/policy.toml`
was not modified).

## The result

```
optid: S4D seal enforced — capabilities=92 Landlock ABI=9 rights=0x7ff2
       new_write_open_denied=true state_write_allowed=true
[before] swappiness=60
[during] swappiness=100
[after]  swappiness=60
```

19 cycles, no crash. The seal took on this kernel, a real kernel write landed,
and it was put back on exit. Everything else reported `redundant_value` — the
values were already what policy wanted — and `platform_profile` was denied
because this board exposes no `platform_profile` file. All truthful.

This is the first demonstration in the repository that the actuate-and-revert
loop closes on physical hardware.

## What was hiding it, and it is worse than it looks

The two runs before this one looked *identical in the log* and changed nothing.
Same startup banner, same seal line, 20 clean cycles, no errors. The reason was
only visible by reading the per-target detail inside `control-cycles.jsonl`:

```
apply_armed: denied — S5D global observe-only circuit open:
  S5D unisolatable reconciliation failure; global circuit opened:
  StaleGeneration: systemd-unit:user.slice:property:CPUWeight belongs to
  generation ...18ce30b8..., not ...18ce31d7...
```

A global circuit latched during an earlier failure. It never closes by itself —
that is deliberate and documented. While it is open **every domain is denied at
the apply gate**, so the daemon runs, logs, reports, and does nothing at all.

Nothing surfaces it. The startup banner does not mention it. The log does not
mention it. `optid --clear-all-circuits` clears it, and nothing tells you that a
latch is why nothing is happening, or that this is the command.

So the failure mode is: one hiccup, then a permanently silent no-op. That is a
plausible explanation for why this project has never produced hardware numbers.
A run that changes nothing is indistinguishable from a run that had nothing to
change.

**Suggested follow-up (not done here):** name the latch where someone will see
it — a startup line when the breaker is open, and a line in `optctl status` —
together with the command that clears it.

## The root cause underneath all of it

The `StaleGeneration` above traced back to a record that could never be
restored, and the reason was a placeholder treated as a value:

```
systemctl set-property --runtime user.slice IOWeight=[not set]
  → Failed to parse IOWeight= value '[not set]': Invalid argument
```

`systemctl show` prints the literal string `[not set]` for a property with no
value. The capture path stored that string as the original value. Restoring it
then tried to *set* the property to `[not set]`, systemd rejected it, the record
stayed pending, and the next daemon start refused to touch the target — which
opened the global circuit, which silently disabled everything.

Three fixes landed, each with tests:

1. `[not set]` is recognized as the absence of a value on capture, and never
   sent back as one on restore.
2. Records written by earlier builds are canonicalized on load, so a record
   already carrying the placeholder compares equal to a freshly-read unset
   property instead of failing its readback forever.
3. `systemctl` failures now carry stderr. `systemctl exited with exit status: 1`
   is a dead end; the message above is what actually identified the bug, and it
   only existed because that call was changed to capture stderr.

Verified end to end afterwards: the stuck record reported `already_restored`
and cleared, the recovery directory drained, and `user.slice` weights returned
to unset.

## Machine state

Left as found: `swappiness=60`, `energy_performance_preference=balance_performance`,
`user.slice` `CPUWeight`/`IOWeight` unset, `tuned` active, no daemon running,
no circuit records, no pending transaction records. Records displaced during the
investigation are archived at `/var/lib/optid/recovery-archive-20260822/`
(outside the repository).
