# Audit Archive

This directory holds superseded audit reports. The authoritative audit
report is `FINAL-AUDIT-REPORT.md` at the repository root.

## Archived reports

| File | Date | Status | Reason archived |
|------|------|--------|-----------------|
| `2026-07-19-pr317-audit.md` | 2026-07-19 | Superseded | Replaced by FINAL-AUDIT-REPORT.md (PR #318) which has more complete security/CI/forensics coverage |

## Convention

When a new authoritative audit report replaces an old one:
1. Move the old report from the repo root to `docs/audit-archive/<date>-<id>-audit.md`.
2. Update the table above.
3. Keep `FINAL-AUDIT-REPORT.md` at the repo root as the current authoritative report.
