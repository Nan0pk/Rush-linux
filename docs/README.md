# 📚 Rush Linux Knowledge Hub

Welcome to the central documentation repository for Rush Linux. This hub is designed to take you from a high-level understanding of the project's vision to the deep technical details of its implementation.

## 🗺️ Documentation Map

### 🎯 Project Governance & Vision
*High-level goals, roadmaps, and the "why" behind the project.*
- [Project Brief](project/PROJECT_BRIEF.md) — The mission and success criteria.
- [Roadmap](project/ROADMAP.md) — Our path from MVP to a bootable distribution.
- [Implementation Status](project/IMPLEMENTATION_STATUS.md) — What's built, what's in progress.
- [Releases](project/RELEASES.md) — Version milestones and the release ledger.

- [The Ideas Pool](ideas-pool/README.md) — Brainstorming and future concepts.- [AI Continuation Guide](project/AI_CONTINUATION.md) — Instructions for LLMs helping with the project.

### 🏗️ System Architecture
*The technical design and decision-making process.*
- [System Architecture](architecture/architecture.md) — The big picture.
- [Adaptive Engine (`optid`)](architecture/adaptive-engine.md) — How the optimization loop works.
- [Kernel Policy](architecture/kernel-policy.md) — Adaptive vs. Realtime kernel strategies.
- [Architecture Decisions (ADRs)](decisions/) — A log of key design choices.

### ⚙️ Operations & Implementation
*Practical guides on how the system is built and operated.*
- [Boot & Updates](operations/boot-and-updates.md) — UKI boot flow and rollback strategies.
- [Packaging & Builds](operations/packaging-and-builds.md) — Source recipes and build system.
- [Hardware Support](operations/hardware-support.md) — Target hardware and policy allowlists.

### 🧪 Quality & Validation
*Ensuring the system is stable, performant, and focused.*

- [The Evidence Lab](quality/performance-lab.md) — Real-world PoC results and hardware benchmarks.- [Testing & Benchmarks](quality/testing-and-benchmarks.md) — How we prove our performance claims.
- [Non-Goals](quality/non-goals.md) — What Rush Linux is *not* trying to be.

### 📜 Policies & Standards
*The rules of the road for contributors.*
- [Versioning Policy](governance/versioning.md) — How we handle releases.
- [Release Policy](governance/release-policy.md) — Criteria for a stable release.
- [Release Checklist](governance/release-checklist.md) — Final gates before shipping.
- [Release Plan v1](governance/release-plan-v1.md) — The strategy for the first version.
- [Documentation Policy](governance/documentation-policy.md) — Standards for keeping docs in sync.

---

## 🛠️ Contributor's Corner

If you are looking to contribute to the code or the documentation:
- **Keeping Docs in Sync**: See [Contributing/Keeping Docs Synced](contributing/keeping-docs-synced.md).
- **Adding a Decision**: Create a new ADR in `docs/decisions/`.
- **Reporting an Issue**: Use the GitHub Issues tab with the appropriate label.

> **Note:** Documentation is considered a first-class citizen in Rush Linux. A feature is not "done" until its corresponding documentation is updated.
