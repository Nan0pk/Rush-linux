# S4D — Sealed Typed Capability Table

**Status:** Builder implementation for independent verification

**Package:** S4D — Move writes to a sealed typed capability table

## Runtime boundary

S4D keeps one optid daemon. It does not add a privileged broker, helper daemon,
or steady-state hardware-write IPC. During the single-threaded privileged
startup phase, optid:

1. loads root-owned policy and allowlist state;
2. collects one discovery snapshot;
3. maps every supported mutation target to a closed `CapabilityKind`;
4. validates the operation/path combination;
5. canonicalizes the exact attribute;
6. opens it read/write with `O_NOFOLLOW` and `O_CLOEXEC`;
7. records canonical path, device number, and inode identity;
8. pre-opens a bounded pool of CPU PM QoS request descriptors;
9. installs an ABI-aware Landlock ruleset that permits new writes only below
   the daemon state roots;
10. proves a new hardware write open is denied while a state write remains
    possible; and
11. only then starts D-Bus and optional foreground worker threads.

The descriptor table is shared by the production observation and actuation
boundaries. Reads and writes to registered attributes use the same descriptor.
Every access revalidates the current canonical path and device/inode identity;
a removed target, symlink replacement, or inode reuse fails closed.

## Configuration

```toml
[safety]
capability_sealing = "observe" # or "enforce"
```

`observe` is the migration-safe default. The daemon may discover and report the
capability inventory, but non-systemd actions are suppressed even when
`--apply` is present. `enforce` can actuate only after the complete table,
pre-opened PM QoS pool, Landlock installation, and both negative/positive
self-tests succeed. Any failure keeps the kernel-write surface observe-only.

The D0 receipt proves the required pre-opened-descriptor and Landlock semantics
on a supported hosted kernel. S4D does not promote any physical HWID or expand
an action envelope.

## Write path

Policy still emits typed `Action` variants. S2D still creates and flushes the
authoritative undo record before mutation, and S3D still owns independent
recovery. The final kernel mutation now resolves through the sealed table:

- CPU EPP → one descriptor per policy attribute;
- platform profile → exact firmware attribute;
- VM sysctls → three closed `/proc/sys/vm` attributes;
- device PM QoS → exact resume-latency attribute;
- runtime PM → distinct control and autosuspend-delay descriptor types;
- PCIe ASPM → exact `link/l1_aspm` attribute;
- SATA ALPM → exact host policy attribute;
- backlight → exact selected-panel brightness attribute; and
- CPU DMA PM QoS → a bounded set of descriptors opened before sealing.

A runtime path not present in the startup table cannot be written. There is no
fallback that reopens it by path.

## Topology lifecycle

Each cycle computes the same typed topology fingerprint used during startup. A
single changed observation is pending only; this prevents rapid add/remove
noise from causing a restart storm. While pending, new targets are inherently
observe-only because they have no table entry. If the same changed topology is
observed twice:

1. the check occurs at the cycle boundary before transaction preparation;
2. current owned targets are handed back through F4/S2D;
3. recovery and reconciliation state is flushed;
4. optid exits with status 75; and
5. systemd restarts through the existing required `optid-recover.service`
   dependency, constructing a fresh table in a fresh process.

The restricted process never keeps an unrestricted sibling or attempts to
remove Landlock.

## Fail-closed cases

The package tests pin these outcomes:

- operation/type mismatch is rejected;
- path or symlink replacement is rejected;
- stale device/inode identity is rejected;
- a removed target cannot be written through a stale table;
- every descriptor is `CLOEXEC`;
- new write opens are denied after sealing;
- restrictions are inherited by descendants through the D0 proof;
- state writes remain available only under explicit roots;
- topology changes are debounced;
- handback precedes the dedicated cold-rebuild exit; and
- a fresh process opens a fresh identity after replacement.

## Package boundary

S4D does not implement S5D circuit breakers, hardware promotion, powercap PL1,
dGPU runtime PM, a permanent broker, a session bridge, or a new policy/value
interface. The builder may record only `candidate`; completion requires a
separate post-merge cold verifier and committed receipt.
