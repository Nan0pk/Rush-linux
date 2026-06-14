# YagniAssessment — Template

Every invocation of the yagni-ladder skill produces one `YagniAssessment` JSON object with these 8 fields. **All 8 are required.** Omit nothing; the verifier will reject incomplete assessments.

## Schema

```json
{
  "proposal": "<string: the proposed change being assessed>",
  "rung_reached": <int: 1-6>,
  "decision": "<enum: skip | use_stdlib | use_native | use_dep | one_line | build_minimal>",
  "evidence": "<string: why this rung wins; cite files / lines / crate names / kernel interfaces>",
  "alternative_considered": "<string: what would have been built at rung 6 if rung N did not win>",
  "why_not_chosen": "<string: the specific reason rung N+1 was rejected>",
  "next_step": "<string: WP row note, verifier follow-up, or 'no action'>",
  "invoked_by": "<enum: verifier | reviewer>"
}
```

## Field rules

### `proposal` (required)
Plain prose. State what the change does, not how. One sentence preferred.

### `rung_reached` (required)
Integer 1–6. **Must equal the rung the assessment stopped at**, not the rung the proposal *could* have reached.

### `decision` (required)
Must match `rung_reached` per this table:

| rung_reached | decision |
|---|---|
| 1 | `skip` |
| 2 | `use_stdlib` |
| 3 | `use_native` |
| 4 | `use_dep` |
| 5 | `one_line` |
| 6 | `build_minimal` |

### `evidence` (required)
Cite real artifacts: file paths, line numbers, crate names, kernel interface paths. "I think stdlib has this" is **not** evidence; `grep 'fn.*HashMap' ~/.cargo/registry/src/*/rust-*/library/std/src/collections/hash/map.rs` is.

### `alternative_considered` (required)
Describe what would have been built if the assessment had reached rung 6. Be specific about LOC, files touched, and dependencies added.

### `why_not_chosen` (required)
Explain why the rung *above* `rung_reached` was rejected. This is the most important field — it is the load-bearing argument.

### `next_step` (required)
- If `decision` is `skip` or `use_*`: this is usually a verifier follow-up ("verify no WP requires this") or a WP row note ("use stdlib X; do not introduce Y").
- If `decision` is `build_minimal`: this is usually "BUILDER proceeds per WP-N row M; verifier validates with [evidence path]".
- If `decision` is `one_line`: cite the one line.

### `invoked_by` (required)
- `verifier` — invoked because a prior verdict flagged over-engineering
- `reviewer` — invoked because a new crate appeared in a PR diff

If invoked for any other reason, the skill should not have been invoked (see trigger table in `SKILL.md`).

## Recording the assessment

The assessment is **not** in-band with the PR diff. Record it via one of:

- **PR review comment** — copy/paste the JSON into a comment on the PR
- **Commit message body** — embed the JSON after the subject line
- **WP verdict** — embed in the verdict under a `## YagniAssessment` heading

Pick the most appropriate channel. The key is that the assessment is searchable by future agents.

## What this template is NOT

- **Not a patch.** The `alternative_considered` field is descriptive, not prescriptive.
- **Not a gate.** The skill does not block PRs or WPs.
- **Not a substitute for evidence.** The WP evidence rule still applies; this template produces simplification recommendations, not verifiability claims.
