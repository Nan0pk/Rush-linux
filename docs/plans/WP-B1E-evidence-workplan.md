# WP-B1E — Work Plan: First evidence dataset (clears the §6 gate)

**Assignee:** Human runs the matrix on a real laptop. Agent (Builder role,
Antigravity CLI / Claude Code, either) scaffolds the PR shell, post-processes
the rig output, validates schema conformance, and drafts the PR body. The
**numbers are not the agent's to author.**
**Authority:** Scoped by `docs/SPEC-northstar.md` (canon on `main`). This WP
produces the **dataset** that the §6 gate language requires. It is the second
half of WP-B1: PR #68 shipped the rig; this PR ships the evidence the rig was
built to produce.
**Version:** 1.0
**Depends on:** PR #68 merged to `main` (rushbench measurement rig +
`benchmarks/results/` scaffolding + schema v1 frozen).

---

## 1. Objective (one ledger role)

Commit a real, single-host, single-day `rushbench matrix` dataset under
`benchmarks/results/<UTC-date>/<host>/**`, plus the `rushbench report` markdown
rollup it produces, in a PR whose body honestly documents what the numbers say
— including any `budget_violation` flags, any `insufficient_n` cells, and any
gap between the contracts.toml's declared latency budgets and the hardware's
observed behaviour.

This is what clears SPEC §6's gate sentence: *"The gate is satisfied by the
benchmark results dataset (evidence) produced by the `rushbench` measurement
rig, not by the existence of the rig itself."* Until this PR lands, no WP-N5,
N6, or N7 enabler can claim it "works."

This PR does NOT tune contracts.toml. It does NOT add levers. It does NOT
re-interpret §1 class definitions if the data is uncomfortable. It commits
what the rig measured, full stop.

---

## 2. Hard constraints (do not violate)

1. **Zero code changes.** This PR touches `benchmarks/results/**`, the PR body
   itself, and at most one or two minor doc lines confirming the gate is now
   cleared. It does NOT modify any `.rs` file, any `Cargo.toml`, any
   `contracts.toml`, `SPEC-northstar.md` §1 / §3 / §4 (gate clearance note in
   §6 is the one acceptable exception). If you reach for a code edit because a
   number "looks wrong," STOP — that is a separate WP after a human reads
   this PR's data.

2. **One host. One day. One operator.** The dataset is the result of a single
   matrix run on a single physical laptop on a single calendar day. Do NOT
   blend results from multiple hosts, multiple sessions on different days, or
   "the better run of two attempts." If you re-run, the previous results
   directory is deleted whole and the new one replaces it. No cherry-picking.

3. **Battery-powered runs are authoritative.** Cells captured on AC are
   recorded if `--ac-ok` was used, but the PR body MUST flag them and MUST
   NOT use AC energy figures as part of any "battery drain" claim. Default
   matrix run is battery only.

4. **N ≥ 5 or the cell is excluded from the rollup.** Per workplan WP-B1 §2.7
   and T5: cells with `n < 5` carry the `insufficient_n` anomaly and are
   excluded from class-level medians in `rushbench report`. They MAY be
   committed (the JSONs are still data), but the report body MUST treat them
   as gaps, not data points.

5. **Class mismatch = discard, do not commit.** If any record has
   `class_requested != class_observed` (the `class_mismatch` anomaly), the
   underlying cause is investigated and fixed BEFORE the matrix is re-run.
   It is NOT shipped as part of the dataset. A `class_mismatch` in a
   committed record means either N1's pin path is broken or the rig's
   readback is broken — both are bugs to file, not numbers to publish.

6. **No hand-authored JSON, ever.** Every file under `benchmarks/results/`
   must have been emitted by `rushbench` and have intact provenance fields
   (host fingerprint, kernel, optid SHA, contracts.toml sha256, rig sha,
   timestamp, power source). A record where any provenance field is `null`,
   `""`, missing, or visibly edited (timestamps that don't move, identical
   `started_at` across cells, etc.) MUST be deleted and re-run. This is the
   §7 fabrication rule applied to evidence.

7. **`contracts.toml` content-hash must be consistent across all committed
   records in this PR.** Every JSON's `rush.contracts_sha256` must match,
   and must match the sha256 of `config/optid/contracts.toml` at the commit
   on which the matrix was run. If `contracts.toml` was edited mid-matrix,
   that invalidates the dataset — re-run from scratch.

8. **`rushbench report` output is committed verbatim, unedited.** The
   markdown rollup that goes in the PR body or under `benchmarks/results/
   <date>/<host>/report.md` is whatever the tool emitted. No prose
   improvements, no reordering of sections, no removing of inconvenient
   `budget_violation` lines. If the report is wrong, fix the rig in a
   separate PR; don't edit the artifact.

9. **The §6 gate is "satisfied" not "perfect."** This dataset's job is to
   *exist* and to be *honest*, not to be flattering. A dataset that surfaces
   a `budget_violation` on `latency-critical` STILL CLEARS THE GATE. The
   gate clears on producing real, provenanced evidence; it does not require
   the evidence to support the SPEC's current numbers.

10. **Doc-sync acceptance rule.** If the PR adds the gate-cleared note to
    SPEC §6, that doc change ships in the same PR.

---

## 3. Preconditions (idempotent — run first)

```bash
git checkout main && git pull
# PR #68 must be merged:
test -d crates/rushbench || { echo "rushbench crate missing — PR #68 not merged. STOP."; exit 1; }
test -d benchmarks/results || { echo "results dir missing. STOP."; exit 1; }
which optctl >/dev/null || { echo "optctl not on PATH — install Rush Linux build first. STOP."; exit 1; }
systemctl is-active optid >/dev/null || { echo "optid not running — start the daemon first. STOP."; exit 1; }
# Energy counter must exist on this host:
ls /sys/class/power_supply/BAT*/energy_now /sys/class/powercap/intel-rapl:0/energy_uj 2>/dev/null \
  | grep -q . || { echo "no energy counter on this host — rushbench will refuse. STOP."; exit 1; }
# cyclictest privilege check (mid-matrix sudo prompt = ruined run):
sudo -n cyclictest -l 1 -q >/dev/null 2>&1 \
  || setcap cap_sys_nice,cap_ipc_lock=eip "$(which cyclictest)" \
  || { echo "cyclictest needs caps or NOPASSWD sudo — fix before matrix. STOP."; exit 1; }
```

If any check fails, **stop**. Do not "work around" by editing the rig.

---

## 4. Discovery (no broad reads needed)

```bash
graphify query "what is the exact schema_version=1 layout rushbench emits, and what filename pattern does it write under benchmarks/results/?" --graph graphify-out/graph.json
graphify query "what command does rushbench report take and what does it emit to stdout?" --graph graphify-out/graph.json
```

Read only what they point to. This is an evidence PR; the agent should be
spending zero time reading source files.

---

## 5. Execution requirements (the matrix run itself)

### 5.1 Host preparation (human, one-time per run)

- Laptop unplugged from AC, battery ≥ 80%, screen on (not lid-closed sleep).
- Close every userspace application except a terminal. No browser, no IDE, no
  Slack, no system update notifications.
- Disable Wi-Fi if the workload doesn't need it (most cells don't). Note
  whether it was on or off in the PR body.
- Record host fingerprint independently as a sanity-check on the rig's
  provenance: `uname -r`, `cat /proc/cpuinfo | grep "model name" | head -1`,
  `cat /sys/class/dmi/id/board_name`. Save to a scratch file for cross-check
  against the JSON `host.*` fields.

### 5.2 The run

```bash
git rev-parse HEAD > /tmp/b1e-optid-sha
sha256sum config/optid/contracts.toml > /tmp/b1e-contracts-sha
rushbench matrix          # default: battery only, N=5 per cell, all classes × all supported workloads
```

- Do not multitask. Do not adjust the brightness. Do not unplug/replug AC.
- If the laptop crosses below 30% battery before the matrix finishes,
  **abort, charge, restart the whole matrix.** Do not stitch.
- If a cell errors mid-run (e.g. `optctl pin` returns non-zero), the matrix
  records the error in the JSON; do NOT manually retry that one cell in
  isolation.

### 5.3 Post-run validation (agent does this part)

The agent scaffolds a check script — under `tools/` or run in-place, not
committed if trivial — that confirms each emitted JSON in
`benchmarks/results/<date>/<host>/` satisfies:

- `schema_version == 1`
- All fields under `host`, `rush`, `class_requested`, `class_observed`,
  `resolved_floors`, `power_source`, `started_at` are non-empty
- `rush.contracts_sha256` is identical across every record
- `rush.optid_sha` is identical across every record
- `rush.optid_sha` matches `/tmp/b1e-optid-sha`
- `rush.contracts_sha256` matches `/tmp/b1e-contracts-sha`'s hash
- `host.kernel`, `host.cpu_model`, `host.dmi_board` identical across every
  record
- `started_at` timestamps are monotonic and clustered within one calendar day
- No record has `class_requested != class_observed` (if any does, see §2.5
  — discard, fix, re-run)

Any failure here → the dataset is not ready. The agent reports the failing
file path(s) and the human decides whether to re-run.

### 5.4 Report generation

```bash
rushbench report benchmarks/results/<date>/<host>/ > benchmarks/results/<date>/<host>/report.md
```

- The report.md is committed alongside the JSONs.
- It is NOT edited by hand. If it's structured badly, that's a follow-up
  rig improvement, separate PR.

---

## 6. Acceptance gates on the dataset (the "tests" of this PR)

Each gate has a stated kill condition. Green ≠ hollow.

- **G1 — Schema conformance.**
  Every JSON parses against schema v1, all required fields present.
  *Kill condition:* any JSON fails to parse or is missing a required field → FAIL. Re-run.

- **G2 — Provenance triple-match.**
  `rush.optid_sha`, `rush.contracts_sha256`, `host.*` identical across all
  committed records, and match the values captured to `/tmp/b1e-*` before
  the run.
  *Kill condition:* any drift across records, or mismatch against the
  scratch captures → FAIL. The dataset is from more than one snapshot or
  was tampered with.

- **G3 — Single host, single day.**
  All `started_at` timestamps fall within a 24h window; all `host.*`
  identical.
  *Kill condition:* records span >24h or come from >1 host fingerprint
  → FAIL.

- **G4 — N≥5 honesty.**
  Every cell included in `rushbench report`'s rollup has `n >= 5`. Cells
  with `n < 5` are present as JSONs but marked `insufficient_n` and
  excluded from medians.
  *Kill condition:* `report.md` cites a median for a cell whose source
  record has `n < 5` → FAIL.

- **G5 — No class mismatches in committed records.**
  Zero records have `class_requested != class_observed`.
  *Kill condition:* any `class_mismatch` in `benchmarks/results/` → FAIL.
  File a bug against N1 or the rig and re-run.

- **G6 — Battery primacy.**
  Every record's `power_source` is `battery`, OR if any are `ac` the PR
  body §5 explicitly identifies which cells and why and excludes those
  cells' energy numbers from headline claims.
  *Kill condition:* an `ac` record's `avg_watts` appears in the PR
  body's "battery drain" section without an AC asterisk → FAIL.

- **G7 — Report fidelity.**
  Committed `report.md` byte-equals `rushbench report
  benchmarks/results/<date>/<host>/` re-run on the committed JSONs.
  *Kill condition:* `diff` is non-empty → FAIL. Report was edited.

- **G8 — Honesty of headline.**
  If the dataset contains any `budget_violation` flag, the PR body's
  TL;DR section names it within the first 5 lines. If `idle` average
  watts is ≥ `interactive` average watts, the PR body names that within
  the first 5 lines.
  *Kill condition:* a `budget_violation` exists in the data but is
  not mentioned in the PR body's opening, or appears only buried in a
  later section → FAIL. This is the gate that prevents an agent from
  drafting a triumphant PR body over uncomfortable data.

- **G9 — contracts.toml untouched.**
  `git diff main -- config/optid/contracts.toml` is empty.
  *Kill condition:* any byte changed → FAIL. Tuning is a separate PR.

- **G10 — No code changes.**
  `git diff main -- '*.rs' 'Cargo.toml' 'Cargo.lock'` is empty.
  *Kill condition:* any byte changed → FAIL.

---

## 7. Docs to update in the SAME PR

- `docs/SPEC-northstar.md` §6: append a one-sentence note that the gate is
  **cleared by this dataset for the host listed in `benchmarks/results/<date>/
  <host>/`**, with a link/path. Phrasing: "Gate first cleared by [path]."
  Do NOT generalize beyond the one host.
- `IMPLEMENTATION_STATUS.md`: B1 row → "Rig shipped (PR #68); first evidence
  dataset committed [date, host]. Cross-distro PPD/TLP/baseline comparison
  remains open under same WP row."
- No other doc edits.

---

## 8. PR instructions

```bash
git checkout -b feat/wp-b1e-first-evidence-dataset
git add benchmarks/results/<UTC-date>/<host>/
git add docs/SPEC-northstar.md IMPLEMENTATION_STATUS.md
git commit -m "feat(benchmarks): first WP-B1 evidence dataset on <host>, <UTC-date>"
git push -u origin feat/wp-b1e-first-evidence-dataset
gh pr create --draft \
  --title "feat(benchmarks): WP-B1E first evidence dataset (<host>, <date>)" \
  --body-file /tmp/b1e-pr-body.md   # see §8.1
```

- Always `--draft` until the verifier verdict block is filled.
- Never `--fill`.

### 8.1 PR body template (the agent fills in `{placeholders}`, nothing else)

```
## TL;DR

First WP-B1 evidence dataset, captured on a single host on a single day.
This PR clears the SPEC §6 gate for that host only.

**Headline findings:**
- {one line per budget_violation, or "no budget violations observed"}
- {idle vs interactive avg_watts comparison, one line}
- {one line on any unsupported_here cells}
- {one line on any AC-only cells, if any}

**Host:** {host.cpu_model}, kernel {host.kernel}, board {host.dmi_board}
**Date:** {UTC date}
**Operator:** {human, single session}
**optid SHA:** {rush.optid_sha}
**contracts.toml sha256:** {rush.contracts_sha256}

## What this PR is

- A single-host, single-day `rushbench matrix` dataset under
  `benchmarks/results/{date}/{host}/`.
- The `rushbench report` markdown rollup, committed verbatim.
- A one-sentence note in SPEC §6 that the gate is first cleared by this
  dataset for this host.

## What this PR is NOT

- Not a cross-distro comparison (still open under same B1 row).
- Not a tuning of `contracts.toml` (separate PR after human review).
- Not a generalization to other hardware.
- Not a claim that any specific enabler "works" — N5/N6/N7 are still ahead.

## Honest gaps

{One line per cell that came back unsupported_here or insufficient_n, or
"all declared cells produced N>=5 samples".}

## Acceptance gates G1–G10

{paste the verifier verdict block from §9 here, with PASS/FAIL per line}
```

The agent fills `{placeholders}` from the dataset and the report.md. No prose
beyond that. If the agent finds itself writing argumentative text, STOP — the
honest report and the data are the argument.

---

## 9. Definition of done — post this verdict

```
WP-B1E VERDICT: PASS|FAIL
G1  schema conformance:               PASS|FAIL  <count of records / parser output>
G2  provenance triple-match:          PASS|FAIL  <sha values match scratch captures>
G3  single host, single day:          PASS|FAIL  <earliest/latest started_at>
G4  N≥5 honesty in report:            PASS|FAIL  <list of insufficient_n cells, confirmation excluded from medians>
G5  no class_mismatch records:        PASS|FAIL  <grep result>
G6  battery primacy:                  PASS|FAIL  <count of ac vs battery records>
G7  report fidelity (byte-equal):     PASS|FAIL  <diff output>
G8  honesty of headline:              PASS|FAIL  <quote first 5 lines of PR body>
G9  contracts.toml untouched:         PASS|FAIL  <git diff output>
G10 no code changes:                  PASS|FAIL  <git diff output>
Docs synced:                          YES|NO     <files>
Dataset path:                                    <benchmarks/results/<date>/<host>/>
Record count:                                    <N>
Cells with budget_violation:                     <list or "none">
Cells with insufficient_n:                       <list or "none">
Cells with unsupported_here:                     <list or "none">
```

FAIL any line → not ready. Human owns merge.

---

## 10. Out of scope (explicit — longer than usual on purpose)

This PR's center of gravity is *not adding things to itself.* The temptations
the data will create when it arrives are pre-listed here so the agent
recognises them as out of scope, not as "small extra wins."

- **Tuning `contracts.toml`** because `latency-critical` blew its budget.
  Separate PR. Human reads the dataset first.
- **Editing SPEC §1 class definitions** because `idle` doesn't draw less
  than `interactive` without N5/N6 enablers. Separate WP, human-owned.
- **Adding a new workload class or metric** because the data made one
  look useful. Separate WP.
- **Re-running cells that "look bad"** to get nicer numbers. The matrix
  runs once. The data is the data.
- **Averaging across runs** to smooth variance. Per-cell N≥5 inside one
  matrix run is the only averaging allowed. Multi-matrix averaging is
  not a thing.
- **Selectively committing JSONs** (omitting "noisy" cells). All cells
  from the matrix run go in, or none do — the dataset is the whole run.
- **Editing `rushbench report`'s output** for readability or to soften
  framing. The tool's output is the artifact.
- **Adding a CI job** to run `rushbench matrix` in CI. Out of scope here;
  this is the first-evidence PR, not a continuous-evidence PR.
- **Cross-distro / multi-host expansion.** Still under the same SPEC §6
  B1 row, but a separate PR after this one lands.
- **Code edits to the rig** because a probe was awkward. File a follow-up
  issue; do not edit the rig in this PR. Mid-flight changes invalidate
  the dataset's provenance.
- **Editing `SPEC-northstar.md`** beyond the one-line gate-cleared note
  in §6 — particularly §1, §3, §4 are off-limits in this PR.
- **Writing a longer PR body** than §8.1's template, no matter how
  interesting the data is. Numbers + flags + honest one-liners. Save
  analysis for the follow-up PRs that act on the data.
- **Promising "next steps"** in the PR body beyond "human reviews,
  decides whether to tune contracts.toml or ship N5/N6 first."

If the work seems to need any of the above, it doesn't — stop and report.
Every one of these is its own future PR after this one's data is read by
a human.

---

## 11. Notes specifically for the human (you, the operator)

The agent cannot run the matrix. You can. The split:

| Step | Who |
|---|---|
| §3 preconditions | you, on the laptop |
| §5.1 host prep | you |
| §5.2 the matrix run | you (`rushbench matrix`) |
| §5.3 post-run validation | agent (script over the JSONs) |
| §5.4 report generation | you OR agent (deterministic command) |
| §6 G1–G10 acceptance gates | agent reads, posts verdict |
| §7 docs | agent drafts, you eyeball |
| §8 PR open | agent opens as draft, you fill the operator name and any cell-by-cell notes |
| §9 verdict block | agent fills from data |
| Merge | you, after eyeballing G8 headline honesty |

If at any point the agent starts narrating analysis of the numbers ("this
suggests…", "we should…"), redirect: that is the next PR's job, not this
one's. This PR's job is to land the data, honestly framed, full stop.
