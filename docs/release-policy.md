# Release Policy

Rush Linux releases are gated by engineering evidence, not dates.

## Release Channels

- `unstable`: CI-passing main branch builds.
- `alpha`: subsystem milestones for developers.
- `beta`: installable images for testers.
- `rc`: feature-frozen release candidates.
- `stable`: supported final releases.
- `security`: urgent fixes for stable.

## Required Gates By Channel

| Channel | Required Gates |
| --- | --- |
| `unstable` | T0 repository policy, docs validation, basic CI. |
| `alpha` | T0-T1, plus VM smoke tests once rootfs exists. |
| `beta` | T0-T3, install/boot/update smoke tests. |
| `rc` | T0-T5, benchmark publication, security review, rollback tests. |
| `stable` | All RC gates and no release blockers for one RC cycle. |
| `security` | Affected tests, security review, signed metadata, rollback retained. |

## Test Tiers

- T0: repository policy, docs, ADRs, no obsolete defaults, recipe presence.
- T1: Rust unit, fixture, config, D-Bus, CLI, and policy tests.
- T2: VM boot, service start, cgroup v2, PSI, nftables, update, rollback.
- T3: hardware battery, thermals, suspend/resume, CPU/GPU/storage policy.
- T4: comparative benchmarks against Fedora, Ubuntu, Arch, and minimal tuned
  baseline.
- T5: security tests for privileged writes, service sandboxing, signatures,
  rollback, config fuzzing, and D-Bus input fuzzing.

## Release Blockers

A release must stop if any of these are present:

- data loss;
- boot failure without rollback;
- privilege escalation;
- unsigned install/update path for a release artifact;
- optimizer unsafe write outside allowlisted paths;
- obsolete default introduced against ADRs;
- regression beyond the benchmark threshold for the target channel;
- docs inconsistent with release behavior.

## Signing And Provenance

Before beta, test signing is acceptable. Before RC, package metadata, update
metadata, and release artifacts must use the real release signing process.

Each release must publish:

- source commit;
- version and channel;
- build inputs;
- package/artifact manifest;
- test tier results;
- known issues;
- rollback instructions.

