# Rush Linux Project Workflow

This is the one path from an idea to a released feature. It keeps research
important without making every small change wait for a research project.

## The flow

```text
1. Intent
   What human problem are we solving?
        |
2. Understand
   What already exists, and what is actually unknown?
        |
3. Research — only for a real unknown
   Sources, measurements, assumptions, and a recommended experiment
        |
4. Decision — only when the choice constrains future work
   Accept, reject, or time-box an experimental direction
        |
5. Plan
   Small change, named risks, acceptance checks, rollback
        |
6. Implement
   Code + tests + affected docs, off by default when still experimental
        |
7. Self-check
   The builder runs the checks relevant to the changed files
        |
8. Independent review
   Accuracy and completeness; additional proof for high-risk claims
        |
9. Delegated merge
   Coordinator checks current review and CI, merges, then continues
        |
10. Observe
    Learn from real use and feed results back into research and decisions
```

## Choose the smallest honest path

| Change | Required path |
|---|---|
| Typo or wording correction | Understand -> edit -> documentation check |
| Ordinary bug with a reproducible failure | Understand -> reproduce -> fix -> test -> pull request |
| New internal code with no new policy | Understand -> plan -> implement -> tests -> pull request |
| New project direction | Intent -> research -> accepted decision -> plan |
| New hardware-specific write | Research -> accepted safety decision -> observe/dry-run prototype -> hardware evidence -> trusted allowlist -> rollout |
| Release or milestone claim | Existing implementation -> independent evidence -> human approval |

Research is not a ceremony. Use it when the answer is genuinely unknown or a
wrong answer would be expensive. A decision is not a research note. It records
which option the project chose after considering the research.

## Research states

Every research paper must use one of these states:

- **Question** — the problem is being framed.
- **Investigating** — sources or measurements are being gathered.
- **Candidate** — there is a proposed direction, but it is not trusted yet.
- **Validated** — the stated experiment or source review supports the finding.
- **Rejected** — evidence or a decision ruled it out.
- **Superseded** — a newer paper replaces it.

"Documented by the Linux kernel" and "tested successfully on Rush hardware"
are different claims. Record them separately.

## When proof blocks something

Proof blocks only the action that depends on the missing proof.

| Missing proof | It blocks | It does not block |
|---|---|---|
| Hardware safety result | Automatic writes and trusted allowlist promotion | Read-only detection, dry-run output, simulation, evidence collection |
| Performance benchmark | Claiming a performance or energy win | Correctness work, instrumentation, experiments |
| Independent review | Delegated merge and high-risk certification | Builder self-tests, opening a draft PR and independent work |
| Accepted direction | Shipping a new permanent project policy | Research, comparison, and an isolated off-by-default prototype |

Every failure must print the blocked action, the risk, the root of that risk,
the missing proof, and safe ways to continue.

## Check ledger

This table is the reason each blocking check exists. If a check cannot be
connected to a row, it should not block a pull request.

| Risk | Root | Blocking protection | Safe way to keep moving |
|---|---|---|---|
| R1: A claim is marked verified without real proof | C1 false rollback certification; Dragnet audits 001–015 | Validate evidence only when evidence or release truth changes | Keep the feature experimental; submit failing evidence honestly |
| R2: Unknown hardware is damaged or becomes unreliable | Decision 0009; Northstar clause 2; research 0006 | Unknown and unverified devices cannot receive automatic writes | Observe, dry-run, simulate, or run an owner-authorized one-time experiment |
| R3: A failed change cannot be undone | Northstar reversibility rule; power-control implementation | Revert unit tests and retention of the journal until restore succeeds | Disable actuation while diagnosis continues |
| R4: An agent silently changes project direction | Earlier unaccepted decisions were implemented | An accepted decision is required before permanent high-scope rollout | Research and isolated prototypes may continue |
| R5: A code change breaks existing behavior | Normal software regression risk | Tests, format, and lints for the code area that changed | Unrelated docs and research do not wait for a Rust toolchain |
| R6: Evidence leaks a token or private machine data | LiveDev/testOS threat model | Privacy and evidence checks when evidence tooling or bundles change | Keep the local bundle; redact and resubmit |
| R7: A dependency becomes unsafe or legally unsuitable | Supply-chain policy | Dependency policy on dependency changes and scheduled maintenance | Changes with no dependency impact are not blocked |
| R8: Public docs contradict the repo | Repeated stale README and version drift | Version/doc checks and generated front-page check | Internal experiments may be labeled experimental instead of advertised |
| R9: Package-shaped code is merged and treated as a finished dependency without runtime integration | F3/F4 dormant-module incident in PRs #326/#327; stale F1–D0 ledger after #324–#328 | One-package ledger update per optid code PR; candidate proof paths; cold-verification receipt before `completed`; reject new dead-code suppression | Merge useful partial code as `candidate` or `merged_incomplete`; repair and verify it before downstream work |

## Pull-request checks

There is one stable required status: **PR Gate**. It selects work by the files
changed and aggregates every selected lane:

- Repository integrity always: whitespace, generated artifacts, workflow
  safety, versions, docs, front page, evidence, and package truth.
- Rust changed: format, tests, Clippy, all targets/features, and dependency
  policy when relevant.
- Python changed: compile, Ruff, pytest, and evidence fixtures.
- Shell changed: parser and ShellCheck on shebang-discovered entry points.
- Windows or shared LiveDev changed: native PowerShell parsing and Windows
  parity tests.
- Image/build paths changed: image and boot contract tests plus a real product
  image build in a privileged Arch container.
- Optid code changed: exactly one package claim changes; candidates name
  production entry points, integration tests, and evidence; only a cold
  verifier may record `completed`.

A maintenance PR that changes only dependency references or other internal
workflow plumbing may carry the `docs-not-needed` label. PR Gate passes that
label only to the documentation-impact check; it does not bypass workflow
safety, Actionlint, dependency policy, tests, evidence checks, or independent review.

External-link scanning and newly published dependency advisories run on a
schedule. They remain visible, but an unreliable website does not block an
unrelated pull request.

Hardware tests cannot run honestly in ordinary GitHub CI. They block promotion
of the affected hardware claim, not merging a safe, disabled prototype.

The coordinating agent follows [the agent protocol](agent-protocol.md) for
review, protected merge and continued work. Do not wait for a routine human
merge or add duplicate verification of the same claim.

## Release rule

A green pull request means the changed repository behavior passed its relevant
automated checks. It does not mean Rush is proven on physical hardware or ready
for release. Release readiness is a separate human decision backed by the
specific evidence in `release/evidence/`.
