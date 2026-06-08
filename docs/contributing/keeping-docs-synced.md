# Keeping Documentation In Sync

Rush Linux has 40+ documentation files that must stay aligned with each
other and with the actual code. This guide explains the system that prevents
drift, contradictions, and staleness.

## The Doc Management System

### 1. Doc Registry: `docs/docmap.toml`

The docmap is the **single source of truth** for doc ownership and
relationships. Every documentation file has an entry that records:

| Field | Purpose |
|-------|---------|
| `purpose` | One-line description of what this doc covers |
| `owner_area` | GitHub label for the responsible area |
| `covers_code` | Code paths this doc describes (for drift detection) |
| `deps` | Other docs whose content this doc depends on |
| `freshens` | Other docs that must be reviewed when this doc changes |
| `last_verified` | Date a human/agent confirmed the doc matches reality |
| `validator` | Hint for the automated sync checker |

**When adding a new doc:** Add an entry to `docs/docmap.toml`.

**When changing a doc:** Update its `last_verified` date.

### 2. Automated Sync Validator: `tools/validate-doc-sync.py`

Runs in CI and checks:

- ✅ Every registered doc exists on disk
- ✅ All cross-references (`deps`) point to registered, existing docs
- ✅ Version strings in key docs match `VERSION` file
- ✅ ADR status values are valid (proposed/accepted/superseded/rejected)
- ✅ No known stale patterns (e.g., "next step" for completed features)
- ✅ Internal Markdown links resolve to real files
- ✅ optid features mentioned in docs actually exist in code
- ✅ `last_verified` dates are within the freshness window

Run locally:

```sh
python3 tools/validate-doc-sync.py              # default: warn at 90 days
python3 tools/validate-doc-sync.py --max-age 30  # stricter: warn at 30 days
```

### 3. CI Integration

The sync validator runs in CI alongside the existing checks. A PR that
changes code without updating the affected docs will fail CI.

## How To Update Docs For Common Changes

### Changing `optid` behavior

1. Edit `crates/optid/src/main.rs`
2. Update `docs/adaptive-engine.md` — add or change the feature description
3. Update `IMPLEMENTATION_STATUS.md` — move items between Implemented/Not Yet
4. Update `README.md` — if the "Current Implementation Status" section is affected
5. In `docs/docmap.toml` — bump `last_verified` for all four docs

### Changing optimizer policy

1. Edit `config/optid/policy.toml`
2. Update `docs/adaptive-engine.md` — thresholds, mode configs, guardrails
3. Check `docs/decisions/0004-adaptive-optid.md` — does the ADR still apply?
4. In `docs/docmap.toml` — bump `last_verified` for all changed docs

### Changing kernel config

1. Edit `distro/kernel/*.config`
2. Update `docs/kernel-policy.md` — explain the change
3. Check `docs/decisions/0010-realtime-edition-kernel-policy.md`
4. In `docs/docmap.toml` — bump `last_verified`

### Bumping the version

1. Update `VERSION`
2. Update `RELEASES.md` — add new entry
3. Update `ROADMAP.md` — change "Current project version" line
4. Update `IMPLEMENTATION_STATUS.md` — change version reference
5. Update `AI_CONTINUATION.md` — change version reference
6. In `docs/docmap.toml` — bump `last_verified` for all five files
7. Run `python3 tools/validate-doc-sync.py` to confirm

### Adding a new ADR

1. Create `docs/decisions/00XX-title.md` with `Status: proposed`
2. Update `docs/decisions/README.md` if it adds new rules
3. Add entry to `docs/docmap.toml`
4. Update any docs listed in the new ADR's consequences
5. Do NOT set `Status: accepted` without maintainer ratification (see ADR README)

### Adding a new doc

1. Write the doc
2. Add an entry to `docs/docmap.toml` with purpose, owner_area, and deps
3. If other docs should reference it, add it to their `deps` or `freshens`
4. Run `python3 tools/validate-doc-sync.py`

## For AI Agents

When an AI agent makes changes, it MUST:

1. **Before changing code or config:** Read the `covers_code` entries in
   `docs/docmap.toml` to find which docs describe the affected code.
2. **After changing code or config:** Update every affected doc, then update
   `last_verified` in `docs/docmap.toml` for each changed doc.
3. **After changing docs:** Check the `freshens` list for that doc and review
   whether the dependent docs also need updating.
4. **Before committing:** Run `python3 tools/validate-doc-sync.py` and confirm
   it passes. If it fails, fix the flagged issues before pushing.

### Quick Agent Workflow

```
1. Read docs/docmap.toml
2. Identify affected docs via covers_code and deps
3. Make code changes
4. Update affected docs
5. Bump last_verified in docmap.toml
6. Run validate-doc-sync.py
7. Commit code + docs + docmap changes together
```

## Troubleshooting

| Error | Fix |
|-------|-----|
| "Registered doc does not exist" | Create the file or remove the docmap entry |
| "does NOT contain current version" | Update the version string in that doc |
| "Broken link in X" | Fix or remove the link |
| "contains stale phrase" | Update the doc to reflect current reality |
| "last verified N days ago" | Review the doc, confirm it's still accurate, bump the date |
| "has dep which is not registered" | Add the missing doc to docmap.toml or fix the dep path |
