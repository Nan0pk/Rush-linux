# Branch protection: required checks

This document lists the GitHub Actions checks that **must** be added to
the `main` branch's required status checks for the front-page sync
enforcement to be mandatory.

## Required checks

Add these as **required** status checks on `main`:

| check name | workflow | purpose |
|---|---|---|
| `frontpage-sync / README sync + docs impact` | `.github/workflows/frontpage-sync.yml` | Fails if README.md generated section is stale OR if user-facing changes lack a docs update (and no `docs-not-needed` label) |

## How to add

1. Go to the repo settings → Branches (repo admin only:
   `https://github.com/Nan0pk/Rush-linux/settings/branches` — paste into
   the browser address bar; do not click as a link, the URL is
   admin-gated and returns 404 for non-admins)
2. Click `main` (or add the rule if missing)
3. Under "Require status checks to pass before merging", search for and add:
   - `frontpage-sync`
4. Save.

## Bypass label

PRs with the `docs-not-needed` label skip the docs-impact portion of the
check. The `frontpage-sync` (README stale) portion always runs — the
generated README must always be in sync regardless of label.

## Without branch protection

If branch protection is not configured, the `frontpage-sync` workflow is
advisory only — it will run and report failures, but PRs can still be
merged. To make enforcement mandatory, configure branch protection as
described above.
