# Yagni Ladder — Pre-Build Simplification Check

> Lift the 6-rung decision tree from [DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail) (MIT).
> Standalone, opt-in skill. Does **not** override the agent protocol or the WP evidence rule.

**Last synced with upstream:** 2026-06-15
**License:** MIT (© DietrichGebert). See `NOTICE.md`.
**Companion files:** [`ladder.md`](./ladder.md) · [`assessment-template.md`](./assessment-template.md) · [`NOTICE.md`](./NOTICE.md) · [`CHANGELOG.md`](./CHANGELOG.md)

---

## What this is

A focused extraction of ponytail's **decision tree** — the 6 rungs you stop at before writing code. We dropped the rest of ponytail's philosophy (lazy senior dev tone, benchmarking culture, plugin distribution) and kept only the ladder + the non-negotiable "not lazy about" section.

The skill produces a `YagniAssessment` — a recommendation, not a patch.

## When to invoke (tight trigger table)

| Trigger | Objectivity | Who invokes |
|---------|------------|-------------|
| New crate added to `Cargo.toml` in a PR | Objective (file diff) | Verifier or reviewer, on PR review |
| Verifier flags over-engineering in a prior verdict | Objective (verdict text contains the flag) | Verifier, in the next verdict |

Anything else: do not invoke. The skill is not for "any non-trivial change" — that trigger was too subjective and produced rubber-stamp approvals. If you think a change needs review, write a normal review comment instead.

## Output contract: `YagniAssessment`

See [`assessment-template.md`](./assessment-template.md) for the full schema. Every assessment MUST have all 8 fields. The skill **never writes code**; the assessment is informational only.

## Precedence

The WP evidence rule governs acceptance. The yagni-ladder is **informational** — it can recommend, it cannot gate. If a YagniAssessment disagrees with a WP verdict, the WP verdict wins; the agent must surface the disagreement in the next verdict for the human to resolve.

---

## Worked examples

### Example 1 — Rung 1 wins (skip)

**Proposal:** "Add a TTL cache class with `CacheEntry` dataclass, LRU eviction, and stats endpoint, for use by the `bench-optid-host.sh` host benchmark."

```json
{
  "proposal": "TTL cache class with CacheEntry, LRU, stats — for bench-optid-host",
  "rung_reached": 1,
  "decision": "skip",
  "evidence": "The host benchmark runs ≤ 5 cells in series. No cache is needed — re-running each cell once is < 2 s and the data fits in memory.",
  "alternative_considered": "TTLCache dataclass + thread-safety + invalidation + stats endpoint (~120 LOC, plus tests).",
  "why_not_chosen": "No demonstrated hot path. Spec by antonym. YAGNI.",
  "next_step": "Verifier follow-up: confirm no WP row requires a cache. If a future WP does, re-invoke.",
  "invoked_by": "verifier"
}
```

### Example 2 — Rung 2 wins (stdlib)

**Proposal:** "Add a custom JSON writer for the `rushbench` results so we can format `window_joules` as integers without `serde_json` overhead."

```json
{
  "proposal": "Custom JSON writer for rushbench results (drop serde_json)",
  "rung_reached": 2,
  "decision": "use_stdlib",
  "evidence": "Rush already depends on `serde` + `serde_json` (Cargo.toml line 34). The supposed overhead is < 1 ms per cell; not a measured bottleneck. Re-inventing JSON output adds a serialization-format audit surface we don't need.",
  "alternative_considered": "Hand-rolled `to_json()` writing field-by-field (~80 LOC, plus a test fixture for every schema change).",
  "why_not_chosen": "Stdlib-equivalent (serde_json) is already in `Cargo.toml` (rung 4 also wins). No measured perf gain. The custom writer would diverge from the schema over time.",
  "next_step": "WP row note: 'use serde_json::to_string_pretty; do not introduce a custom writer.'",
  "invoked_by": "verifier"
}
```

### Example 3 — Rung 6 wins (genuinely build)

**Proposal:** "Implement the workload-class detector for `optid` per WP-N1 row 4 (foreground / fullscreen / audio / video, selected by PSI signal)."

```json
{
  "proposal": "Workload-class detector for optid (foreground / fullscreen / audio / video)",
  "rung_reached": 6,
  "decision": "build_minimal",
  "evidence": "No stdlib/crate covers class detection from kernel PSI counters. Rung 2 checked: no Rust crate for 'classify linux workload from PSI'. Rung 3 checked: kernel exposes PSI via /proc/pressure/* but does not classify. Rung 4 checked: optid's Cargo.toml has no class-detection dep; this is the canonical place for it. Rung 5: more than one line — needs PSI sampling + state machine + D-Bus interface.",
  "alternative_considered": "Skip detection entirely; have the user set the class via a CLI flag.",
  "why_not_chosen": "WP-N1 row 4 explicitly requires automatic class detection. The CLI-only alternative would close the WP as REJECT.",
  "next_step": "BUILDER proceeds with the WP-N1 row 4 implementation per the existing spec. Verifier validates with real `rushbench matrix` on `victus`.",
  "invoked_by": "verifier"
}
```

---

## Anti-patterns (do NOT do these)

- **Do not invoke for typo fixes, comment cleanups, or whitespace changes.** The skill is for design decisions, not style.
- **Do not invoke to justify skipping a WP that was explicitly requested.** Rung 1 is for genuinely unneeded work, not for laziness disguised as YAGNI.
- **Do not auto-apply `alternative_considered`.** It is a description of what *would* have been built, not a patch. Apply requires a new WP.
- **Do not invoke silently.** The assessment must be recorded (commit message, PR comment, or WP verdict) so a human can review it.
- **Do not cite the skill as a substitute for evidence.** The WP evidence rule still applies; this skill does not produce evidence, only simplification recommendations.

## Failure modes

- **Skill invoked for a typo fix.** Reject — this is a style change, not a simplification decision. The author should just commit it.
- **Assessment produced but ignored.** Future WP may re-encounter the same issue. The assessment must be committed (PR comment or commit body) so it is searchable.
- **Agent uses rung 1 to refuse work.** This is the failure mode the skill was designed to enable. The verifier must check: was the work explicitly requested by a WP? If yes, rung 1 is not an option.
- **Stale upstream.** If ponytail updates its ladder text, this skill becomes a fork. Re-sync quarterly or when an upstream release notes a change to the ladder.

## Re-syncing with upstream

```
# Pull latest ladder text from ponytail's AGENTS.md:
curl -sL https://raw.githubusercontent.com/DietrichGebert/ponytail/main/AGENTS.md
# Update ladder.md if the rung text changed.
# Bump "Last synced with upstream" in this file.
# Add an entry to CHANGELOG.md.
```
