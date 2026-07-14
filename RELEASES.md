# Releases

Current project version: `0.7.0-beta.5`

This version opens the editions milestone. The preceding `0.6.0-beta.1`
(Hardware-Aware optid) is **code-complete but not yet certified** — see the
status note below.

> **Status note (governance):** `0.5.0-beta.1` is **complete.** All four exit
criteria are certified in-repo with committed transcripts
(`release/evidence/v0.5.0-beta.1/{c1,c2,c3,c4}/`), closed by PR #174 on
2026-06-23. v0.3 and v0.4 are likewise complete (transcripts in
`release/evidence/v0.3.0-alpha.1/` and `release/evidence/v0.4.0-alpha.1/`).
The earlier Dragnet-001 finding G3 — that build-host assertions lacked
committed transcripts — is closed.
>
> **`0.6.0-beta.1` is code-complete, certification pending Phase D.** The four
in-container Work Packages are merged (PRs #183 PPD shim, #184 GameMode shim,
#185 `vm.guest` class, #186 foreground stub). The two code criteria
("unsupported knobs skipped with reasons", "no unsafe write outside allowlisted
paths") are satisfied. The two **quantitative, hardware-gated** criteria
("mixed-load responsiveness improves on two machines", "battery behavior matches
or improves mainstream defaults") require Phase D physical-hardware transcripts
and are **not yet verified** — host-bench evidence dirs are still templates
(`release/evidence/host-bench/_TEMPLATE/`). v0.6 therefore stays
`in-progress` in `release/milestones.toml` per the Evidence Rule, even though
the version pointer has advanced to `0.7.0-beta.1`.
>
> See `release/milestones.toml` for the canonical state and
`release/evidence/dragnet/LEDGER.md` for the audit history.

## Release Ledger

| Version | Channel | Status | Purpose |
| --- | --- | --- | --- |
| `0.1.0-alpha.0` | `unstable` | complete | Repository scaffold, release governance, docs, CI policy, optimizer MVP source. |
| `0.1.0-alpha.1` | `alpha` | complete | Compile-clean Rust core and fixture tests. |
| `0.2.0-alpha.1` | `alpha` | complete | D-Bus control plane and config parsing. |
| `0.3.0-alpha.1` | `alpha` | complete | Minimal rootfs and package builder MVP. All four exit criteria verified with committed transcripts (PR #174). |
| `0.4.0-alpha.1` | `alpha` | complete | UKI boot, rollback, update signing. All four exit criteria verified with committed transcripts (PR #174). |
| `0.5.0-beta.1` | `beta` | complete | Minimal installable system via mkosi/Arch pivot. All four exit criteria verified with committed transcripts (PR #174). |
| `0.6.0-beta.1` | `beta` | code-complete; certification pending Phase D | Hardware-aware optimizer policy. Shims + classes merged (PRs #183–#186); quantitative criteria gated on physical-hardware Phase D. |
| `0.7.0-beta.1` | `beta` | in progress | Desktop, laptop, realtime audio, and server editions. |
| `0.8.0-beta.1` | `beta` | planned | Benchmark lab and regression gates. |
| `0.9.0-rc.1` | `rc` | planned | v1 API/default freeze and release hardening. |
| `1.0.0` | `stable` | planned | First stable Rush Linux release. |

## Release Rule

Do not tag a planned version until every exit criterion in
`docs/release-plan-v1.md` and every gate in `release/milestones.toml` for that
version has passed.

