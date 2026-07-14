---
name: Bug Report
about: Report a bug in Rush Linux
title: "[bug] "
labels: type:bug, needs-triage
assignees: ""
---

## Describe the Bug

A clear and concise description of what the bug is.

## Steps to Reproduce

1. ...
2. ...
3. ...

## Expected Behavior

What you expected to happen.

## Actual Behavior

What actually happened.

## Environment

- **Rush Linux version** (check `VERSION` file or `optctl status`):
- **Kernel** (`uname -r`):
- **Edition** (desktop / laptop / server / realtime-audio):
- **Hardware** (CPU, laptop/desktop, battery present?):
- **optid mode** (`optctl mode`):

## Relevant Logs

<details>
<summary>optid status</summary>

```
Paste output of: optctl status
```

</details>

<details>
<summary>Decision log</summary>

```
Paste output of: optctl explain
```

</details>

<details>
<summary>Action log</summary>

```
Paste output of: optctl trace
```

</details>

## Additional Context

Add any other context about the problem here. If the bug is in a specific
subsystem, mention which one (optid, optctl, packaging, kernel config,
boot flow, etc.).
