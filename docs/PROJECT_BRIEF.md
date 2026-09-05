# Project brief

Rush Linux aims to be a dependable, responsive OS that completes useful work
efficiently and adapts automatically with little user configuration. The original
May 2026 intent covered responsiveness, battery life, thermal behavior and
resource utilization. Matching the quality of a Mac is a product ambition,
not an established benchmark result.

Optid coordinates adaptive policy. It is one component of the OS; installation,
updates, recovery, application compatibility, display/audio quality and everyday
usability have independent requirements.

## Success

- Responsive foreground work under concurrent CPU, memory and I/O demand.
- Competitive battery life and suspend behavior with equivalent service quality.
- Preserved useful throughput, correct results and explicit user preferences.
- Reliable installation, updates, rollback and native fallback if Optid stops.
- Few setup decisions for the user and clear explanations when a feature cannot
  operate on the machine.

Measure these outcomes together. Lower watts obtained by doing less work or
silently reducing brightness do not establish superior efficiency. A source
build, custom scheduler, edition count or completed package count is not itself
an outcome.

## Present foundation

The repository contains real Rust services, guarded control/recovery paths,
simulation, mkosi image composition, boot/install/rollback tooling and a
development/test environment. The build uses Arch packages plus source-built
Rush components; historical custom recipes do not constitute a complete source
distribution. The current package ledger and committed evidence define their
precise completion and verification scope.

The [source-build experiment](plans/source-build-experiment.md) preserves that
work while testing selected changes before choosing a different base. Its
[research](research/0025-os-goals-and-source-build-reassessment.md) identifies
what is sourced, what was observed, and what remains hypothetical. The Northstar
amendment is proposed for review on the experimental branch.

Prefer mature upstream components and one owner per controlled domain. Keep
privileged operations bounded, observable and recoverable. Build and test on a
narrow initial hardware envelope, then widen support from measurements. Deliver
built artifacts to users; development complexity belongs in automation.
