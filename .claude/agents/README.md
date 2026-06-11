# Project subagents

Subagent definitions for this repo. Each `.md` file here is a Claude Code
subagent with YAML frontmatter (`name`, `description`, `tools`, `model`).

## Default model for subagents

Where supported, route subagents to a **cheaper** model. Set in frontmatter:

```yaml
---
name: explorer
description: Read-only search agent for locating code in this repo.
model: haiku
tools: Glob, Grep, Read, WebFetch
---
```

Valid `model` values: `haiku`, `sonnet`, `opus`, `fable` (lowest → highest cost).
Pick `haiku` by default; only escalate when a subagent's job requires deeper
reasoning (architecture review, multi-step refactor planning).

The main session model is set via `/model` or CLI flag, not here.
