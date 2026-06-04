# Releases

Current project version: `0.3.0-alpha.1`

This version implements the minimal rootfs builder and package repository system.

## Release Ledger

| Version | Channel | Status | Purpose |
| --- | --- | --- | --- |
| `0.1.0-alpha.0` | `unstable` | complete | Repository scaffold, release governance, docs, CI policy, optimizer MVP source. |
| `0.1.0-alpha.1` | `alpha` | complete | Compile-clean Rust core and fixture tests. |
| `0.2.0-alpha.1` | `alpha` | complete | D-Bus control plane and config parsing. |
| `0.3.0-alpha.1` | `alpha` | current | Minimal rootfs and package builder MVP. |
| `0.4.0-alpha.1` | `alpha` | planned | UKI boot, rollback, and local update flow. |
| `0.5.0-beta.1` | `beta` | planned | First minimal installable server image. |
| `0.6.0-beta.1` | `beta` | planned | Hardware-aware optimizer policy. |
| `0.7.0-beta.1` | `beta` | planned | Desktop, laptop, realtime audio, and server editions. |
| `0.8.0-beta.1` | `beta` | planned | Benchmark lab and regression gates. |
| `0.9.0-rc.1` | `rc` | planned | v1 API/default freeze and release hardening. |
| `1.0.0` | `stable` | planned | First stable Rush Linux release. |

## Release Rule

Do not tag a planned version until every exit criterion in
`docs/release-plan-v1.md` and every gate in `release/milestones.toml` for that
version has passed.

