# Releases

Current project version: `0.5.0-beta.1`

This version targets the minimal installable system milestone via the mkosi/Arch image pivot.

> **Status note (governance):** `0.5.0-beta.1` is **evidence-pending.** Of its four
exit criteria, only *"server edition has no desktop dependency"* is certified in-repo
(static check). The other three — fresh install, double-boot, update/rollback — were
asserted on the build host but their acceptance transcripts were never committed and
are now `verified = false` pending those transcripts. This applies equally to v0.3
and v0.4 (see the Dragnet-001 audit, `release/evidence/dragnet/`). Per the Release
Rule below, none of these is a passed milestone until its transcript lands. See
`release/milestones.toml` and `release/evidence/dragnet/LEDGER.md`.

## Release Ledger

| Version | Channel | Status | Purpose |
| --- | --- | --- | --- |
| `0.1.0-alpha.0` | `unstable` | complete | Repository scaffold, release governance, docs, CI policy, optimizer MVP source. |
| `0.1.0-alpha.1` | `alpha` | complete | Compile-clean Rust core and fixture tests. |
| `0.2.0-alpha.1` | `alpha` | complete | D-Bus control plane and config parsing. |
| `0.3.0-alpha.1` | `alpha` | evidence-pending | Minimal rootfs and package builder MVP. Implemented; build-host transcripts owed (Dragnet-001). |
| `0.4.0-alpha.1` | `alpha` | evidence-pending | UKI boot, rollback, update signing. Signing certified in-repo; boot/rollback transcripts owed. |
| `0.5.0-beta.1` | `beta` | evidence-pending | Minimal installable system via mkosi/Arch pivot. Server-no-desktop certified; install/boot/rollback transcripts owed. |
| `0.6.0-beta.1` | `beta` | planned | Hardware-aware optimizer policy. |
| `0.7.0-beta.1` | `beta` | planned | Desktop, laptop, realtime audio, and server editions. |
| `0.8.0-beta.1` | `beta` | planned | Benchmark lab and regression gates. |
| `0.9.0-rc.1` | `rc` | planned | v1 API/default freeze and release hardening. |
| `1.0.0` | `stable` | planned | First stable Rush Linux release. |

## Release Rule

Do not tag a planned version until every exit criterion in
`docs/release-plan-v1.md` and every gate in `release/milestones.toml` for that
version has passed.

