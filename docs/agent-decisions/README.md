# Agent Decision Log

Rush Linux accepts contributions from AI agents as well as humans. Commit
authorship alone (e.g. "Antigravity", "Arena Agent") is not a sufficient audit
trail: it does not record *what an agent was asked to do*, *what it decided*, or
*who signed off*. When a security reviewer or future maintainer asks "why was
this architectural decision made?", the answer must live in the repository, not
in a chat transcript that no longer exists.

## When to add an entry

Add a short entry for any **significant agent-authored change**, in particular:

- new or amended ADRs, or anything that changes a documented decision;
- changes to public interfaces (D-Bus, recipe schema, release gates);
- security-relevant changes (privileges, sandboxing, allowlists);
- anything that resolves a contradiction or marks a milestone state.

Routine changes (typo fixes, formatting, dependency bumps) do not need an entry.

## Format

One file per entry: `NNNN-short-title.md`, using the template below. Keep it
brief — the goal is traceability, not prose.

```markdown
# AD-NNNN: <title>

- Date: YYYY-MM-DD
- Agent: <agent/model name>
- Human sign-off: <name, or "pending review">
- Prompt/intent: <one or two sentences on what was requested>

## Decisions
- <decision and the one-line rationale>

## Changes
- <files/areas touched>

## Follow-ups
- <anything deferred or left for a human>
```
