# Validation

Validation is split into three layers:

- Repository policy checks: ensure future-facing defaults are present and
  legacy defaults are not introduced.
- Documentation checks: ensure the first-class docs and ADRs exist and contain
  the required continuation/status/acceptance sections.
- Rust unit tests: ensure `optid` parsing and policy decisions remain stable.
- Hardware lab benchmarks: compare against Fedora, Ubuntu, Arch, and a minimal
  tuned baseline.

The first layer is implemented in `tools/validate-repo.ps1` so the project can
be checked even on the current Windows workspace without a Rust toolchain.

The validation script must fail when the core documentation layer is missing:

- `PROJECT_BRIEF.md`
- `AI_CONTINUATION.md`
- `IMPLEMENTATION_STATUS.md`
- `ROADMAP.md`
- `CONTRIBUTING.md`
- `SECURITY.md`
- `VERSION`
- `RELEASES.md`
- `docs/versioning.md`
- `docs/release-policy.md`
- `docs/release-checklist.md`
- `docs/release-plan-v1.md`
- `docs/documentation-policy.md`
- architecture docs under `docs/`
- ADRs under `docs/decisions/`
- release gates under `release/`

Docs are part of acceptance criteria, not cleanup. A change that alters code,
policy, packaging, boot, kernel fragments, benchmark expectations, or defaults
must update the relevant documentation in the same change.

The documentation policy requires each substantive change to describe purpose,
impact, safety implications, validation, and follow-up work.

Release governance is part of validation. Changes to version numbers, release
channels, milestone exit criteria, or test tiers must update `VERSION`,
`RELEASES.md`, `docs/versioning.md`, `docs/release-policy.md`, and
`release/milestones.toml` together.

Hardware lab scenarios:

- Foreground latency under compile, I/O, package update, swap, and browser load.
- Battery drain during idle, video playback, browser, video call, and suspend.
- Gaming p95/p99 frame times and shader/background interference.
- Realtime audio cyclictest/oslat and PipeWire underruns.
- Server PostgreSQL, nginx, containers, fio, iperf3, and kernel builds.
- Unsupported sensor and bad firmware knob regression tests.
