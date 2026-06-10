# Verification Report — WP-{{WP_ID}}

**Branch:** {{BRANCH_NAME}}  
**Commit:** {{COMMIT_SHA}}  
**PR:** #{{PR_NUMBER}}  
**Verifier:** {{VERIFIER_ID}}  
**Date:** {{VERIFICATION_DATE}}  
**Environment:** {{ENV_DESCRIPTION}}

---

## Acceptance Criteria Results

| Criterion | Command | Exit Code | Verdict |
|-----------|---------|-----------|---------|
| {{CRITERION_1}} | `{{COMMAND_1}}` | {{EXIT_1}} | {{VERDICT_1}} |
| {{CRITERION_2}} | `{{COMMAND_2}}` | {{EXIT_2}} | {{VERDICT_2}} |
| {{CRITERION_3}} | `{{COMMAND_3}}` | {{EXIT_3}} | {{VERDICT_3}} |

---

## Notes

- Any criterion that could not be tested in this environment (e.g., requires KVM, real hardware, session bus) is explicitly marked above.
- Failures are recorded as verdicts only — no fixes were applied by the verifier.
- Builder and verifier were different sessions/models.

---

## Evidence Rule Compliance

All checkmarks in this report are backed by literal command transcripts (command + output + date + host) as required by §2 of the work plan.
