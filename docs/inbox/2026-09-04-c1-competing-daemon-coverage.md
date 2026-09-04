# The shipped competing-daemon list misses three active policy owners on the default target

Found while gathering hardware evidence for **R2** (`docs/research/0022-platform-primitives-disposition.md`).
Not repaired here: R2's packet forbids bundling unrelated writes, and this is C1/F1 territory.

Reporter: Claude (Opus 5), Claude Code session. Read-only; nothing was written.
Host: HP Victus 16-r0086TX, 13th Gen Intel Core i7-13700HX, Fedora 44,
kernel `7.1.12-200.fc44.x86_64`, stock install.
Commit: `5072c48`

## What optid ships

`config/optid/policy.toml:7-11`:

```toml
competing_policy_daemons = [
  "tlp.service",
  "power-profiles-daemon.service",
  "tuned.service",
]
```

`crates/optid/src/main.rs:213` passes that list to
`shim::detect_conflicts`, which asks `systemctl is-active` for each **exact unit
name** (`crates/optid/src/shim/conflict.rs:86-155`).

## What is actually running

```
$ for u in tlp power-profiles-daemon tuned intel_lpmd thermald irqbalance; do
    printf '%-30s %s %s\n' "$u.service" \
      "$(systemctl is-enabled $u.service 2>&1)" "$(systemctl is-active $u.service)"; done
tlp.service                    not-found  inactive
power-profiles-daemon.service  not-found  inactive
tuned.service                  enabled    active
intel_lpmd.service             enabled    active
thermald.service               enabled    active
irqbalance.service             enabled    active
```

Four autonomous policy owners are enabled and active. **One of the four is on
optid's list.**

## Defect 1 — three active policy owners are invisible to the conflict check

| Daemon | Active | On optid's list | Domain it owns |
|---|---|---|---|
| `tuned.service` | yes | **yes** | cpufreq/EPP, sysctl, disk |
| `intel_lpmd.service` | yes | no | CPU online/offline, EPP, HFI/WLT consumption |
| `thermald.service` | yes | no | thermal policy, can drive `intel_powerclamp` |
| `irqbalance.service` | yes | no | IRQ placement |

`intel_lpmd` is the serious one. It can offline CPUs and rewrite EPP
autonomously — the same domain optid actuates — and optid neither detects nor
yields to it. The single-owner safety property the D2 amendment rests on is not
holding on a stock Fedora 44 install; it holds only because `tuned` happens to
be active and trips the check for unrelated reasons. Stop `tuned` alone and
optid would arm itself while three uncoordinated policy owners keep running.

`thermald` matters for the T-lane: it is a thermal policy owner, and
`intel_powerclamp` is present on this host as a 100-step cooling device, so a
future T-lane idle-injection or PL1 actuator would be entering an occupied
domain.

## Defect 2 — name-based detection produces a false negative for power-profiles

`power-profiles-daemon.service` reports `not-found`/`inactive`, yet the
power-profiles D-Bus interface is being served, by a differently named unit:

```
$ busctl --system list | grep PowerProfiles
net.hadess.PowerProfiles              76845 tuned-ppd root :1.281 tuned-ppd.service
org.freedesktop.UPower.PowerProfiles  76845 tuned-ppd root :1.281 tuned-ppd.service

$ busctl --system get-property net.hadess.PowerProfiles \
    /net/hadess/PowerProfiles net.hadess.PowerProfiles ActiveProfile
s "power-saver"
```

On Fedora, `tuned-ppd.service` provides the PPD API. So the interface has an
owner, that owner is currently holding a `power-saver` profile, and optid's
probe for it answers "nobody". A list of unit names cannot express "whoever owns
this D-Bus name", which is the property that actually matters.

This is not only a reporting gap. `config/optid/policy.toml:24-28` documents
what optid does with the answer:

```
# When the shim is active (no PPD conflict detected at startup), optid
# claims the net.hadess.PowerProfiles bus name.
```

The shim's activation condition is "no PPD conflict detected at startup". On
stock Fedora 44 that condition is **provably met** — the only PPD entry in the
list is `power-profiles-daemon.service`, which is `not-found` — while
`net.hadess.PowerProfiles` is in fact already owned by `tuned-ppd`. So optid
would proceed to claim a bus name that has an owner holding a live profile.

What actually happens on that collision was **not** tested here and is not
claimed; the daemon was never armed. Only the decision path is established, and
it is established from optid's own configuration. Testing the collision should
come before the shim is enabled on this distribution.

## Why C1's receipt is not stale

`config/optid/policy.toml` is not among C1's declared proof paths
(`runtime_entrypoints`, `integration_tests`, `completion_evidence` in the
ledger), and `detect_conflicts` correctly detects everything in the list it is
given. The code is not wrong; **the shipped list is incomplete and the
detection strategy is too weak for the default target.** So this is a
configuration and design-coverage finding, not a demotion trigger for C1. Which
package should own the repair is a maintainer call.

## Suggested repair, for whoever owns it

1. Add `intel_lpmd.service`, `thermald.service` and `irqbalance.service` to
   `competing_policy_daemons`, or explicitly document per domain why each is
   allowed to coexist. Silence is currently indistinguishable from a decision.
2. Detect the power-profiles owner by D-Bus name ownership rather than unit
   name, so `tuned-ppd` and any future provider are caught. Keep the unit-name
   list for daemons that expose no bus name.
3. Report the detected owner **per domain** rather than as one global
   conflict flag. `irqbalance` owning IRQ placement is no reason to downgrade a
   `vm.*` write, and today's single flag cannot express that.
4. Test what optid's PPD shim does when `net.hadess.PowerProfiles` is already
   owned.

## Reproduction

No root, no writes:

```
systemctl is-active intel_lpmd.service thermald.service irqbalance.service
busctl --system list | grep PowerProfiles
```
