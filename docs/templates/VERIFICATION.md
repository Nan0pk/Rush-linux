# Verification Report

| Field | Value |
|---|---|
| Target PR | #[PR Number] |
| Work Package (WP) | [WP ID] |
| Branch | `[Branch Name]` |
| Target Commit | `[Commit Hash]` |
| Verifier | [Agent/Human Name] |
| Date | YYYY-MM-DD |

## Environment Description
- **Host OS**: [e.g. Ubuntu 24.04 / Arch Linux]
- **Hardware Profile**: [e.g. CI Runner / KVM / Physical Laptop]
- **Missing Capabilities**: [State any acceptance criteria you CANNOT run, e.g. "Requires a real session bus" or "Requires KVM"]

## Acceptance Execution

*Note: You must provide literal command execution logs for a criterion to be marked PASS.*

### Criterion 1: [Name of Criterion]
**Verdict:** ⬜ PASS | ⬜ FAIL | ⬜ UNTESTABLE

**Command Transcript:**
```bash
# Literal command executed
$ [command]
# Literal output
[output]
```

### Criterion 2: [Name of Criterion]
**Verdict:** ⬜ PASS | ⬜ FAIL | ⬜ UNTESTABLE

**Command Transcript:**
```bash
# Literal command executed
$ [command]
# Literal output
[output]
```

## Final Verifier Verdict
- [ ] **REJECTED** (One or more criteria failed or failed to compile).
- [ ] **APPROVED** (All testable criteria passed with valid evidence).
- [ ] **PARTIAL** (Tested what was possible; requires Human to test hardware-specific gates).

