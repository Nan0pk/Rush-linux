# Executor reports

Task reports from the executor agent (Antigravity), one file per task,
committed in the same PR as the task's changes. The verifier (a Claude/Fable
session) reads these cold, re-runs the `VERIFY` block, and records the verdict
in the PR review. Chat between owner and agents is ephemeral; this directory
is the durable record.

Naming: `YYYY-MM-DD-<task-id>.md`, e.g. `2026-06-11-T1.md` (T1's report ships
in the T2 evidence PR, since T1 itself produces no repo change).

## Required format (≤25 lines; overflow goes in committed evidence files)

```text
TASK: <id> | STATUS: done|blocked|partial
BRANCH/PR: <branch> / #<n> (draft)
ACCEPTANCE:
  <each acceptance-block command -> literal exit code or one-line literal
   result; "SKIPPED(<reason>)" if not runnable>
EVIDENCE: <committed file paths>
DEVIATIONS: <none, or ≤3 one-line bullets — anything done differently from
  the task pack, including anything a runner restored on failure>
FINDINGS: <≤3 one-line bullets of unexpected results worth the verifier's
  attention — a skipped benchmark phase or anomalous numbers belong here>
VERIFY: <exact commands/paths the verifier should run or inspect cold>
```

Rules:

- Any "passed/works/matches" claim must point to a committed transcript path;
  otherwise write "unverified" (repo rule: no claim without a transcript).
- Reports are append-only history: if a task is re-run, add a new dated file
  rather than rewriting the old one.
