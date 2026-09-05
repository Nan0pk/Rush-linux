# AI Interface Policy

> **Status:** proposed (binding on LiveDev work once ADR 0018 is ratified;
> descriptive for the rest of the project until then).
> **Owners:** maintainer of record.
> **See also:** `docs/automation-human-interface.md` (when to prompt a
> human), `docs/agent-protocol.md` (Builder/Verifier/Human split),
> `docs/decisions/0013-detection-and-ml-boundary.md` (the sequencing rule
> for ML in `optid`), ADR 0018 (LiveDev architecture contract),
> `docs/plans/livedev-transition-plan.md` (build sequence).

This document defines **how** AI is used in the Rush Linux project and
**where the ceiling is**. It exists because the LiveDev track
(`docs/decisions/0018-rush-livedev-architecture-contract.md`) will eventually
let automation call online AI providers for plan synthesis, code review, and
evidence summarization — and without an explicit interface policy, AI use
will silently expand into roles it must not hold (verifying evidence,
executing shell commands, merging PRs, changing release truth).

The policy is mechanical. Every AI surface in the LiveDev track (and, as far
as practical, every agent session in the rest of the project) must implement
these rules. The policy is not a recommendation; it is a contract.

---

## 1. AI interface is CLI/harness-based, not browser-chat-based

The system's interface to AI models is a **command-line harness**: a
deterministic Python or Rust program that constructs a prompt, calls a model
provider's API, parses the response, and exits with a structured result.
The harness is invoked from a shell; it is not a chat session; it is not a
browser tab.

This rule exists for four reasons:

1. **Reproducibility.** A harness reads its prompt from a file (or stdin)
   and writes its response to a file (or stdout). Two runs of the same
   harness with the same inputs produce comparable outputs (modulo model
   non-determinism, which is itself logged). A chat session is not
   reproducible: the prompt history, the model temperature, and the
   system prompt drift across sessions.
2. **Auditability.** A harness run is a single process with a start time,
   an end time, an exit code, and a log file. The log file can be attached
   to a PR as evidence. A chat session has no such artifact; the only
   record is whatever the human copy-pastes, which is partial and
   self-serving.
3. **Cost control.** A harness has a configured token budget and a
   configured model. Money actions (`docs/automation-human-interface.md`
   §2.3) apply. A chat session has no budget guardrail; the human can
   spend arbitrarily on a long conversation.
4. **Testability.** A harness can be run against a mock provider
   (§4) in CI. A chat session cannot be tested.

The harness's interface is:

```
rush-ai-harness \
  --provider <name> \
  --model <name> \
  --prompt-file <path> \
  --response-file <path> \
  --budget-tokens <n> \
  --budget-usd <n> \
  --mock <bool> \
  --log-file <path>
```

Every flag is required (no defaults that hide behavior). The harness exits
non-zero on budget exhaustion, provider error, or response-parse failure.
The harness does **not** retry on its own; retries are the caller's
responsibility (the caller is the autopilot planner, see
`docs/plans/livedev-transition-plan.md`).

### 1.1 Browser use is restricted to auth and PR viewing

A web browser is permitted only for:

- **Authentication flows** that cannot complete in a CLI (e.g., the GitHub
  OAuth device flow, which requires the user to visit a URL and enter a
  code; the AI provider's interactive login if no API key is configured).
- **PR viewing** — reading a PR's diff, comments, and CI status on
  github.com. The browser is read-only here; the harness reads the same
  data via the GitHub API for its own use.

The browser is **not** permitted for:

- Composing PR descriptions (the harness composes these from a template).
- Replying to review comments (the harness replies via the API).
- Editing files (the harness edits files via `git` and a text editor).
- Running AI chat sessions (the harness handles AI calls; the browser is
  not an AI surface).

If a LiveDev surface needs a browser for an auth flow, it opens the URL,
waits for the flow to complete (detected via the auth token landing at a
well-known path), and closes the browser. The browser is not kept open.

---

## 2. Online AI providers are used for serious model work

"Serious model work" is work where model quality materially affects the
outcome: plan synthesis (choosing the next phase of the transition plan),
code review (catching subtle bugs in a PR), evidence summarization
(condensing a 500-line transcript into a verdict), and architectural
reasoning (drafting an ADR). For these tasks, the system calls an online
provider (OpenAI, Anthropic, or a future ratified provider) via the
harness in §1.

"Non-serious" model work — parsing a known-format file, generating a
boilerplate commit message, formatting JSON — is done with **deterministic
code**, not an AI call. The rule is: if a regex, a parser, or a template
can do it, the regex/parser/template does it. AI is not a substitute for
deterministic code; it is a tool for tasks where deterministic code does
not exist.

### 2.1 Provider ratification

A provider is **ratified** when the maintainer has, in writing (an ADR or a
docmap-tracked policy update), approved its use. The default ratified
providers are:

- (none yet — the LiveDev track has not reached the "AI harness" phase of
  the transition plan)

When the AI harness phase lands, this section will list the ratified
providers, their model names, their budget caps, and the ADR that ratified
each. Until then, no provider is ratified and no AI call may be made by
any LiveDev surface.

### 2.2 Budget caps

Every ratified provider has a monthly budget cap (default: $50/month per
provider, override via maintainer-set environment variable). The harness
checks the cap before each call; if the call would exceed the cap, the
harness exits non-zero with a "budget exhausted" message and the caller
must prompt the human (money action, `docs/automation-human-interface.md`
§2.3).

Budget state is persisted at `~/.config/rush/ai-budget.json` and is
per-provider, per-month. The state file is not committed to the repo.

---

## 3. Tests must use a mock provider

The harness has a `--mock` flag. When `--mock true`, the harness does not
call the online provider; it reads a canned response from
`--mock-response-file` and writes it to `--response-file` as if the
provider had returned it. The harness's behavior is otherwise identical:
budget is not consumed, the log file is written, exit codes are the same.

Every test of an AI-using surface runs with `--mock true`. CI never calls a
real provider. This rule is non-negotiable: a test that calls a real
provider is flaky (the provider's responses are non-deterministic), slow
(network latency), and expensive (it spends budget). A test that calls a
real provider is also a money action (`docs/automation-human-interface.md`
§2.3) and requires a human confirmation per call — which is incompatible
with CI.

The mock response file is a fixture committed to the repo under
`tests/fixtures/ai-responses/`. Each fixture is a JSON file with the
provider's response shape. The fixture's filename is referenced from the
test; the test is deterministic because the fixture is fixed.

### 3.1 What the mock covers

The mock covers the **harness's contract with its caller**: given this
prompt file, the harness produces this response file and this exit code.
The mock does **not** cover the **provider's contract with the world**
(that the provider's response is actually a good plan / review / summary).
The latter is not testable in CI; it is evaluated by the Verifier role and
the Human on real PRs.

This split is important: tests verify that the system **uses** the AI
correctly (prompt is well-formed, response is parsed, budget is respected),
not that the AI is **correct**. AI correctness is the Verifier's job, not
the test suite's.

---

## 4. Heavy local LLM inference must not run during benchmarks

The system does **not** run a local LLM (e.g., llama.cpp, ollama) during a
benchmark run. This rule applies to LiveDev benchmark sessions, to testOS
benchmark sessions, and to any session that produces evidence committed to
`release/evidence/`.

This rule exists because local LLM inference is a workload: it loads the
GPU, it fills CPU caches, it consumes memory bandwidth, and it draws power.
A benchmark that runs alongside a local LLM is measuring the LLM's power
draw as if it were the workload's power draw — which corrupts the evidence.
The benchmark manifest (`benchmarks/manifest.toml`) declares the
workloads; an LLM is not on the manifest.

The rule is symmetric: if the system is running a benchmark, no local LLM
runs; if the system is running a local LLM, no benchmark runs. The two are
mutually exclusive on the same machine.

### 4.1 Online AI calls during benchmarks

Online AI calls (§2) during a benchmark are also forbidden, for the same
reason: the network round-trip and the JSON parsing perturb the workload.
The harness's `--budget-usd` flag is set to 0 during benchmark sessions;
the harness exits non-zero on any call attempt.

### 4.2 AI calls after benchmarks

After a benchmark session ends and the evidence is written, the system may
call an online AI provider to summarize the evidence (e.g., "the
mixed-load-001 transcript shows a 12% latency improvement on the laptop;
compose a one-paragraph summary for the PR description"). This is
post-hoc analysis, not measurement; the benchmark is over and the machine
is idle. The summary is logged and attached to the PR; it is not evidence
of the benchmark's outcome (the transcript is the evidence).

---

## 5. AI cannot verify evidence

The Evidence Rule (`docs/agent-protocol.md`) is unchanged: a checkmark may
only appear next to an **embedded command transcript** — the literal
command, the literal output, the date, the host description. AI-generated
summaries, AI-generated verdicts, and AI-generated "this looks correct"
statements are **not evidence**. They are commentary.

Concretely, the system may use AI to:

- **Summarize** a transcript ("the cyclictest p99 latency was 280µs, below
  the 500µs floor; this is a PASS for criterion 2").
- **Draft** a PR description that quotes the transcript.
- **Suggest** which transcript to attach to which criterion.

The system may **not** use AI to:

- **Mark** a criterion `verified = true` in `release/milestones.toml`.
  That is a final-approval action (`docs/automation-human-interface.md`
  §2.5).
- **Replace** a transcript with a summary. The transcript is the evidence;
  the summary is commentary.
- **Decide** that a transcript is "good enough" without the Verifier role
  running the acceptance block verbatim. AI is not the Verifier.

An AI-generated summary that says "this transcript passes criterion 2" is
a **claim**, not a verification. The Verifier must still run the acceptance
block and record the verdict. If the Verifier's verdict disagrees with the
AI summary, the Verifier wins.

---

## 6. AI cannot execute shell commands directly

The AI model never invokes a shell. The model produces text (a plan, a
review, a summary); the harness writes the text to a file and exits. If
the text describes a shell command ("run `cargo nextest run --workspace`"),
the **caller** (the autopilot planner, or a human reviewing the plan)
decides whether to execute it. The model does not execute it.

This rule exists for three reasons:

1. **Safety.** A model that can execute shell commands can do anything the
   caller can do: delete files, push to remote, modify `release/milestones.toml`.
   The damage radius of a hallucinated command is the entire project. A
   model that produces text has a damage radius of one bad plan, which the
   caller rejects.
2. **Auditability.** Every shell command the system executes has a caller
   (the planner, the harness, the human). The caller is logged. If the
   model executed commands directly, the caller would be "the model,"
   which is not a role in `docs/agent-protocol.md` and cannot be held
   accountable.
3. **Composability.** A model that produces text composes with any caller
   (the planner, a human, a future tool). A model that executes commands
   composes only with the shell it was built for.

### 6.1 What "execute shell commands directly" means

- The model may **suggest** a command in its text output. The suggestion is
  text; the caller decides.
- The model may **not** call `system()`, `exec()`, `subprocess.run()`, or
  any equivalent. The harness does not expose these to the model.
- The model may **not** write to a path that is later sourced as a shell
  script (e.g., writing to `~/.bashrc` or to a file in `PATH`). The
  harness writes only to `--response-file`, which is read by the caller,
  not executed.

If the caller decides to execute a command the model suggested, the
execution is the caller's action, logged under the caller's name (Builder,
Verifier, or Human per `docs/agent-protocol.md`), and subject to the
applicable policies (`docs/automation-human-interface.md` for destructive
or money actions).

---

## 7. Reviewed integration and the LiveDev boundary

[ADR 0027](decisions/0027-delegated-reviewed-merges.md) replaces the project-wide
human-only merge ban. A coordinating agent may integrate independently reviewed
changes under [the agent protocol](agent-protocol.md), using current CI and the
protected GitHub interface with an expected head SHA. The owner does not need
to approve each routine merge.

This does not give the LiveDev text-model harness, collector or submission
library a merge capability. They remain bounded builders. The coordinator must
actually obtain another agent's accuracy/completeness review; an arbitrary PR
comment, model response or green badge is not authorization. Record distinct
agent sessions honestly even when they share a GitHub identity.

No agent may bypass branch protection, force-push main, forge reviews or promote
release/hardware claims without their separate evidence and authority.

---

## 8. AI cannot change release truth

"Release truth" is the canonical state of the project's release: the
version, the milestone statuses, the evidence tree, the ADR ratification
state, and the Dragnet ledger. These live in:

- `VERSION`
- `Cargo.toml` `[workspace.package] version`
- `mkosi/mkosi.extra/etc/os-release`
- `RELEASES.md`
- `release/milestones.toml`
- `release/evidence/` (the entire tree)
- `release/evidence/dragnet/LEDGER.md`
- `docs/decisions/*.md` (the `Status:` and `Ratified-by:` lines)

AI may not modify any of these files. AI may **draft** proposed changes
(e.g., a draft milestone update, a draft ADR) as text in a PR description
or in a plan file, but the file modification is a final-approval action
(`docs/automation-human-interface.md` §2.5) executed by the Human.

### 8.1 The Evidence Rule restated

The Evidence Rule in `docs/agent-protocol.md` says: a checkmark may only
appear next to an embedded command transcript. This policy extends it: AI
may not add the checkmark, AI may not remove the checkmark, and AI may not
edit the transcript to which the checkmark refers. The checkmark is the
Verifier's (or the Human's) alone; the transcript is the Builder's (or the
benchmark harness's) alone.

---

## 9. Non-goals

This policy does **not**:

- Forbid AI use in the project. AI is permitted, within the harness, within
  the budget, within the ratification list, for serious model work.
- Forbid local LLM use in general. A developer may run a local LLM on
  their workstation for experimentation. The rule in §4 applies only to
  benchmark sessions that produce committed evidence.
- Forbid AI-assisted coding in a developer's editor (Copilot, Cursor, etc.).
  That is a developer-tooling choice, not a project surface. The code the
  developer commits is subject to the same review as any other code.
- Replace `docs/decisions/0013-detection-and-ml-boundary.md`. ADR 0013
  governs **ML inside `optid`** (the optimizer); this policy governs **AI
  outside `optid`** (the dev/CI/release tooling). The two are separate.
- Address AI licensing. Model output copyright and training-data licensing
  are out of scope for this document; they are addressed in the ADR that
  ratifies each provider (§2.1).

---

## 10. Acceptance criteria

A LiveDev AI surface is compliant with this policy if and only if:

- [ ] All AI calls go through the harness in §1 (no in-process model calls,
      no chat sessions, no browser-based AI).
- [ ] Browser use is restricted to §1.1 (auth and read-only PR viewing).
- [ ] The provider is on the ratified list (§2.1); if not, the call is
      refused.
- [ ] The call is within the monthly budget cap (§2.2); if not, the call
      is refused and the human is prompted (money action).
- [ ] Every test of the surface runs with `--mock true` (§3); no test
      calls a real provider.
- [ ] No local LLM inference runs during a benchmark session (§4).
- [ ] No online AI call runs during a benchmark session (§4.1).
- [ ] AI-generated text is treated as commentary, not evidence (§5). The
      Verifier's verdict wins over the AI summary.
- [ ] The AI model never executes shell commands (§6). The harness writes
      text; the caller decides whether to execute.
- [ ] The LiveDev harness cannot self-merge. Coordinating agents follow the
      separate-review procedure in §7.
- [ ] The AI does not modify release-truth files (§8). The Human modifies.

Non-compliance is a release blocker for the LiveDev track. A LiveDev PR
that violates this policy is rejected at review, regardless of whether its
code is otherwise correct.
