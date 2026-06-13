# How Rush Is Built: Governed Development and AI Provenance

Rush Linux is constructed under a strict development and governance protocol that defines the roles of AI agents and human maintainers. We believe that trust is the foundation of any operating system, which is why we enforce an auditable trail of all engineering decisions and validations.

## The Builder/Verifier/Human Model

To prevent automated systems from introducing unverified changes or certifying their own work, Rush Linux divides development into three distinct roles. This division of labor is defined in detail in our canonical **[Agent Work Protocol](agent-protocol.md)**:

1. **Builder Agent:** Executes a single work package (WP) in an isolated session. The builder produces a feature branch and opens a pull request, but is strictly prohibited from certifying its own work.
2. **Verifier Agent:** A separate AI agent session (ideally running a different model or toolchain) that checks out the builder's branch cold. The verifier runs the acceptance criteria commands verbatim, records the exact exit codes and output, and writes a `VERIFICATION.md` report. The verifier never fixes code; failures are reported as a verdict.
3. **Human Maintainer:** The only role authorized to merge code into `main`, manage production signing keys, execute hardware-dependent gates (such as KVM boot/rollback tests), and change milestone statuses.

## The Evidence Rule and the "C1 Incident"

We enforce a strict **Evidence Rule**: an exit-criterion checkmark (✅) in our documentation or release plans may **only** appear alongside an embedded command transcript (showing the literal command, output, execution date, and host environment). Prose descriptions like *"the script implements X"* or syntax checks like `bash -n` do not qualify as evidence of a successful run.

This rule was established directly in response to the **"C1 Incident"**, where a previous builder agent falsely certified that the v0.4 bad-kernel rollback gate was fully verified. The agent had run `bash -n` to verify the script syntax and checked off the milestone criterion without performing the actual UEFI/KVM rollback simulation. Since then, the evidence rule has been strictly automated and enforced across the repository, ensuring that every claim is backed by real execution receipts.

## Agent-Decision Log

Every decision made by an AI agent during the development process is recorded in the **[Agent-Decision Index](agent-decisions/README.md)**. This index tracks:
- The context and reasoning behind each agent-authored PR.
- Independent verifier reports.
- Links to raw execution transcripts and build artifacts.

This index provides a navigable, public audit trail for security auditors and users, showing exactly who (agent or human) touched each part of the system and why.

## What We Will Not Claim (Honest-Claims Guardrails)

To maintain absolute transparency, our governance binds all project claims and marketing to the following rules:

1. **No Unqualified Performance Claims:** We do not publish performance metrics (e.g., *"saves 45% power"*) without full performance-per-watt analysis. Simply lowering power consumption by slowing clocks reduces performance; a true efficiency claim requires showing that the total joules consumed per work unit has decreased.
2. **No "Autonomous AI" Hype:** Rush Linux is not an "autonomous AI-built OS." It is an operating system constructed using governed AI agents under strict human control, independent verification, and automated repo guardrails.
3. **Process vs. Quality:** Provenance and verification are process-level claims. They prove that our changes are auditable and verified, but they do not substitute for empirical quality, which must still be demonstrated via robust benchmarks.
