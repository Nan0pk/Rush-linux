# Branch protection: required check

The `main` branch should require one ordinary pull-request status:

| check name | workflow | purpose |
|---|---|---|
| `Change checks / Relevant tests and safety rules` | `.github/workflows/ci.yml` | Runs only the tests relevant to the changed files and enforces the no-auto-merge rule |

Repository administrators configure this in **Settings -> Branches**. Require a
pull request and the check above. Do not enable automatic merging for agents or
repository automation.

Release, image-publishing, scheduled link, advisory, graph, and reassessment
workflows are intentionally not required PR gates. They create artifacts or
report maintenance state; they do not judge every ordinary change.
