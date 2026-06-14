# Changelog — yagni-ladder skill

## 2026-06-15 — Initial extraction

Extracted the 6-rung decision tree + "Not lazy about" section from [DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail) `AGENTS.md` (MIT, © DietrichGebert).

Adapted for Rush-Linux:

- Renamed the trigger table to objective conditions only ("new crate" + "verifier flagged"); dropped the loose "non-trivial WP row" trigger.
- Renamed the output schema field `patch_suggestion` → `alternative_considered` to prevent auto-application.
- Added explicit precedence note: WP evidence rule governs acceptance; this skill is informational.
- Added 3 worked examples (rungs 1, 2, 6).
- Added anti-patterns and failure modes.
- One skill, no variants (no `-strict`/`-lite`/`-ultra` modes — ponytail's mode proliferation was not adopted).

Not adopted from ponytail:

- The "lazy senior dev" tone and culture (Rush's culture is evidence-first, not laziness-first).
- The plugin distribution model (`/.claude-plugin/`, `/.codex-plugin/`, etc.) — Rush's existing `.agents/skills/` convention is used instead.
- The benchmarking culture (ponytail's `benchmarks/` measuring 80-94% code reduction) — irrelevant to Rush's goals.
- The additional rules beyond the ladder (e.g. "Mark intentional simplifications with a `ponytail:` comment") — Rush's evidence rule already covers comment discipline.
