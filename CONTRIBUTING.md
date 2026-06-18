# Contributing to Rush Linux

Welcome! We're glad you're here.

Rush Linux is a source-built Linux distribution centered on `optid`, a runtime
optimizer that makes adaptive, explainable policy decisions for responsiveness,
battery life, thermals, and resource utilization.

This guide will help you make your first contribution — and many after that.

---

## Ways to Contribute

You don't have to write Rust to help. Here are all the ways you can contribute:

| Area | Examples |
|------|----------|
| **Code** | Fix bugs in `optid`/`optctl`, add sensors, improve policy engine |
| **Documentation** | Fix typos, write tutorials, improve architecture docs |
| **Testing** | Write tests, run benchmarks, test on real hardware |
| **Kernel config** | Tune config fragments, test on specific hardware |
| **Packaging** | Write new recipes, improve the build system |
| **Design review** | Comment on proposed ADRs, review architectural direction |
| **Bug reports** | File detailed issues with logs and reproduction steps |
| **Community** | Help others in Discussions, mentor new contributors |

Every one of these is a real contribution. We celebrate all of them.

---

## Quick Start

### 1. Get the code

```sh
git clone https://github.com/Nan0pk/Rush-linux.git
cd Rush-linux
```

### 2. Start a work session

```sh
bash tools/start-work.sh "fix typo in adaptive-engine.md"
```

This validates the repo is in a good state and sets a dirty flag so
others know you're working.

### 3. Build

You need a current Rust toolchain and a Linux environment (native or container).

```sh
cargo build --workspace
cargo test --workspace
```

### 4. Make a change

See **[Your First Contribution](docs/contributing/first-pr.md)** for a
step-by-step walkthrough.

### 5. Validate and finish

```sh
bash tools/finish-work.sh "docs: fix typo in adaptive-engine"
```

This runs all validators, syncs docs, removes the dirty flag, commits, and pushes.
See the manual validation commands below if you prefer to run checks individually.

If you need to leave mid-work, edit `DIRTY_STATE.md` to describe what's
done and what's left. The next contributor will see it.

---

## Your First Contribution

Looking for something to work on? Check issues labeled
[`good first issue`](https://github.com/Nan0pk/Rush-linux/labels/good%20first%20issue).
These are specifically chosen to be approachable for newcomers.

For a full walkthrough, see **[docs/contributing/first-pr.md](docs/contributing/first-pr.md)**.

**Stuck?** Open a [Discussion](https://github.com/Nan0pk/Rush-linux/discussions)
or comment on the issue — we're happy to help.

---

## Understanding the Project

Before making significant changes, read these to understand the architecture:

**Essential (read first):**
- [docs/PROJECT_BRIEF.md](docs/PROJECT_BRIEF.md) — what Rush Linux is and why it exists
- [docs/architecture.md](docs/architecture.md) — the four-layer system design

**For your area:**
- [docs/adaptive-engine.md](docs/adaptive-engine.md) — how `optid` works
- [docs/kernel-policy.md](docs/kernel-policy.md) — kernel config decisions
- [docs/packaging-and-builds.md](docs/packaging-and-builds.md) — build system
- [docs/decisions/](docs/decisions/) — Architecture Decision Records (ADRs)

**For context:**
- [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) — what's done and what's next
- [ROADMAP.md](ROADMAP.md) — the path to v1.0

---

## Pull Request Process

1. **Fork and branch.** Create a feature branch from `main`.
2. **Make focused changes.** One logical change per PR. Keep it scoped.
3. **Update documentation.** If your change affects behavior, defaults, policy,
   boot flow, kernel config, recipes, or services — update the relevant docs
   in the same PR. See [docs/documentation-policy.md](docs/documentation-policy.md).
4. **Run all checks.** `cargo fmt`, `cargo test`, `cargo clippy`,
   `validate-repo.ps1`, and `validate-doc-sync.py` must all pass.
5. **Open the PR.** Fill out the PR template completely.
6. **Respond to review.** We aim for initial review within 7 days.

### Documentation Is Required

Docs are part of acceptance criteria. Changes to behavior, defaults, policy,
boot/update flow, kernel fragments, recipes, or tests **must** update the
relevant docs in the same change. The doc registry (`docs/docmap.toml`) maps
every doc to its purpose and dependencies — check it to find which docs cover
the code you're changing. See [documentation-policy.md](docs/documentation-policy.md)
for the full list of what must be documented per change type.

### Review Criteria

We look for:

- Tests pass, clippy is clean, formatting is correct.
- Documentation is updated in the same PR.
- The change aligns with the project's modern-defaults direction.
- No guardrails are weakened without justification.
- Privileged actions have explainable reasons and are allowlisted.

---

## Design Rules

These are the project's architectural guardrails. Changes that violate them
will need a compelling justification:

- **One policy owner.** `optid` owns runtime optimization. Don't add competing
  daemons (TLP, power-profiles-daemon, TuneD) as active defaults.
- **Modern defaults only.** No X11, PulseAudio, iptables, cgroup v1, or SysV
  init as defaults. Wayland, PipeWire, nftables, cgroup v2, UKI.
- **Explainable behavior.** Every `optid` action must have a reason visible
  through `optctl explain`.
- **No undocumented changes.** Behavior changes without matching docs are
  incomplete, even if tests pass.
- **Deterministic before ML.** Prefer explicit policy over heuristic or ML
  tuning until deterministic policy has benchmarks and rollback.
- **No weakened guardrails.** Don't loosen safety constraints to make a
  benchmark look better.

---

## Communication

| Channel | Purpose |
|---------|---------|
| [GitHub Issues](https://github.com/Nan0pk/Rush-linux/issues) | Bug reports, feature requests, tracked work |
| [GitHub Discussions](https://github.com/Nan0pk/Rush-linux/discussions) | Questions, ideas, show & tell, announcements |
| [Security Advisories](https://github.com/Nan0pk/Rush-linux/security/advisories/new) | Private vulnerability reports |

## Code of Conduct

This project follows the [Contributor Covenant v2.1](CODE_OF_CONDUCT.md).
Please be respectful and inclusive in all interactions.

## Recognition

Contributors are acknowledged in the [AUTHORS](AUTHORS) file and in release
notes. We value all contributions — code, docs, testing, design, and community.

---

## Get Help

If you're ever unsure about anything:

- **Ask in [Discussions](https://github.com/Nan0pk/Rush-linux/discussions)**
  — no question is too basic.
- **Comment on the issue** you want to work on — we can scope it together.
- **Open a draft PR** early — we'll give feedback before you polish.

We'd rather help you succeed than have you struggle alone. Welcome aboard.

---

## Agent Work Protocol (v2)

This section codifies the evidence rule and builder/verifier separation introduced in work-plan-v2.

### Evidence Rule (non-negotiable)

An exit-criterion checkmark may **only** appear next to an **embedded command transcript**: the literal command, literal output (or attached log file), date, and host description.  
"The script implements X" is a description, not evidence.  
`bash -n` is a syntax check, not a test run.  
Any evidence README violating this rule is rejected at review without further reading.

### Roles

**Builder agent**  
- Executes exactly one WP per session under `tools/start-work.sh` / `finish-work.sh`.  
- Produces a branch and opens a PR.  
- May *claim* completion but **must never** *certify* it.

**Verifier agent**  
- A separate session (ideally a different model/tool than the builder).  
- Checks out the branch cold.  
- Runs the WP's acceptance block verbatim.  
- Writes a `VERIFICATION.md` report (see `docs/templates/VERIFICATION.md`) into the PR or as a comment.  
- Records each command, its exit code, and a one-line verdict per criterion.  
- Never fixes code — a failed check is a verdict, not a task.  
- Builder ≠ verifier for the same WP.

**Human (maintainer)**  
- The only role that can merge to `main`.  
- Runs hardware-dependent gates (KVM rollback test, physical benchmarks).  
- Holds production signing keys.  
- Changes milestone status.  
- Resolves disagreements between builder and verifier.

### Authority Matrix

| Action | Builder | Verifier | Human |
|--------|---------|----------|-------|
| Create branch / push commits | ✅ | ❌ | ✅ |
| Open PR | ✅ | ❌ | ✅ |
| Run acceptance commands | ✅ (self-check) | ✅ (authoritative) | ✅ |
| Mark WP criteria ✅ in evidence/docs | ❌ | ✅ (in VERIFICATION.md only) | ✅ |
| Merge to `main` | ❌ | ❌ | ✅ only |
| Edit `release/milestones.toml` status | ❌ | ❌ | ✅ only |
| Touch signing keys beyond test keys | ❌ | ❌ | ✅ only |
| Declare a gate "passed" without command transcript | ❌ | ❌ | ❌ — nobody |

### PR-only Merges

All merges to `main` happen via reviewed PRs. Direct pushes to `main` are not permitted except for emergency hotfixes by the human maintainer.

---

*This protocol was added as part of WP-P2 (work-plan-v2 recovery sprint).*
