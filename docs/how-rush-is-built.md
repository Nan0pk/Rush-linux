# How Rush Is Built: Governed Development and AI Provenance

Rush Linux is constructed under a strict development and governance protocol that defines the roles of AI agents and human maintainers. We believe that trust is the foundation of any operating system, which is why we enforce an auditable trail of all engineering decisions and validations.

## The Builder/Verifier/Human Model

Rush separates ordinary engineering checks from independent certification. The
complete flow is in the **[project workflow](project-workflow.md)** and the
authority rules are in the **[Agent Work Protocol](agent-protocol.md)**:

1. **Builder Agent:** Builds the change, tests it, reports the results, and opens
   a draft pull request. Builders are not allowed to avoid normal testing by
   calling it "verification."
2. **Independent Verifier:** Checks high-risk hardware, security, boot,
   performance, milestone, and release claims cold. Every delegated merge receives focused accuracy/completeness review; a
   qualifying cold verification can satisfy it without a second review chain.
3. **Coordinating Agent:** Obtains a separate accuracy/completeness review,
   checks current CI and merges eligible PRs through the protected interface.
   It then continues authorized work without waiting for the owner to merge.
4. **Human Maintainer:** Gives direction, promotes trusted hardware, manages
   production signing authority, and declares milestones or releases complete.
   Existing authorization is not requested again for each implementation PR.

## The Evidence Rule and the "C1 Incident"

We enforce an **Evidence Rule**: an exit-criterion checkmark in release or
hardware records must point to evidence that actually proves that claim. Prose
like *"the script implements X"* or a syntax check such as `bash -n` does not
prove a real rollback or hardware run.

This rule came from the **C1 Incident**, where an agent used `bash -n` and then
marked a real rollback gate verified without running the UEFI/KVM rollback. The
rule now blocks the unsupported claim, not unrelated research or development.

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
