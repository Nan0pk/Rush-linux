# Branch prune — 2026-07-25

Pruned 8 of 10 remote branches on `Nan0pk/Rush-linux`, leaving `main` and the
`graphify-data` data branch. Every deleted tip SHA is recorded below so any
branch can be restored.

## Restore any branch

```bash
git push origin <sha>:refs/heads/<branch-name>
```

GitHub keeps unreferenced objects reachable for a limited window; the six
merged branches are permanently recoverable regardless, because their commits
are ancestors of `main`.

## Deleted — merged, zero commits ahead of `main`

Each was merged through the PR listed and had no unique commits remaining.

| Branch | Tip SHA | Merged via | Behind `main` |
| --- | --- | --- | --- |
| `fix/testos-plan-provenance-units-timing` | `eec50937d3fb7c69f1500bd37dfca470492c1e3a` | #296 | 115 |
| `fix/testos-real-path-provenance-privacy-validation` | `75c33b10d24cb74e6c9301818c46796613f34df5` | #297 | 113 |
| `work/20260715-cloud-safe-livedev-run-intent-provenance-and-str` | `4f49c9cb9e471d0ada5dfda196932285b528bd34` | #298 | 108 |
| `fix/testos-evidence-submission-blockers` | `605f4f61e390de836a551e5d2630cebaff877ffb` | #299 | 106 |
| `testos-boot-corrective-2` | `29c86d1264805601381b9ba9d50e12c7fb968b75` | #303 | 97 |
| `fix/testos-corrected-build-version-sidecar` | `60181762494605364365e4b12b1d4a35cb1e6225` | #307 | 91 |

## Deleted — unmerged, content salvaged first

Both branches carried real work that existed nowhere else. The content was
cherry-picked onto `arena/019f9814-rush-linux` **before** deletion and verified
byte-identical to the branch tips (`git diff` against each source tip returned
empty for the affected paths).

| Branch | Tip SHA | Salvaged as | Content |
| --- | --- | --- | --- |
| `arena/019f93e2-rush-linux` | `ceafd3b18c946c72372a1e8cdbd9716796e15d43` | `cce212c` | `docs/plans/optid-verification/f1.toml` — F1 cold-verification receipt |
| `fix/critical-actuation-defects` | `7d75bcadfd60e20c8d3f79b2d837ee36ee70124e` | `91469e8`, `7a198e1` | `ProtectKernelTunables=yes` + dynamic capability path declarations + `ReadWritePaths` continuation parser |

Three commits from `fix/critical-actuation-defects` were deliberately **not**
carried over, because they are net-zero scaffolding that added a temporary
workflow and then removed it again:

- `8f95ac1` `chore: bootstrap critical actuation fixes`
- `bd1576c` `chore: trigger agent critical actuation workflow`
- `0716532` `chore: remove temporary bootstrap workflow`

The three-file diff between `main` and the branch tip confirms none of that
scaffolding survives in the final tree.

## Kept

| Branch | Tip SHA | Reason |
| --- | --- | --- |
| `main` | `001515bef45a0a5f9842d9831154ae9d0573d361` | Default branch |
| `graphify-data` | `a0e9b540cf551c4138f8c2cc3389ae314003d325` | Not stale — orphan data branch holding `graphify-out/` off `main`, required by WP-A2 (`docs/plans/agent-work-plan-v1.md`), whose acceptance check is `git ls-remote --heads origin graphify-data`. Shares no history with `main` by design. |
