# Antigravity Task Pack — 2026-06-11

| Field | Value |
|---|---|
| Doc | antigravity-tasks-2026-06-11, v1 |
| Produced by | Claude (Fable) planning/verification session |
| Intended consumer | Antigravity CLI running on the owner's machine |
| Verifier | a follow-up Claude session checks every acceptance block cold |
| Governs | task execution only; strategy stays with the handover doc and ADRs 0014/0015 |

Division of labor per the owner: **Fable plans and verifies; Antigravity
executes.** Each task below is a self-contained prompt. Rules that bind every
task: follow `AGENTS.md` (docs updated in the same change, docmap registered,
`tools/validate-doc-sync.py` green); no claim without a literal command
transcript; benchmark scripts keep the safety contract (capture every knob
before applying, restore + verify on every exit path, `--apply` required to
mutate); one task per branch/PR; draft PRs only; never push to `main`.

Execution order: T1 → T2 (same session is fine), T3 and T4 independently.
T1 requires PR #24 merged first (it contains the battery-detection fix that
made the battery phase reachable on the Victus).

---

## T1 — Matrix benchmark campaign on the Victus (hardware, local only)

Run on the HP Victus (Fedora 44), after merging PR #24:

```sh
sudo ./tools/bench-optid-matrix.sh --apply
sudo ./tools/bench-optid-matrix.sh --apply --levers baseline,epp-perf --iter 9
sudo ./tools/bench-optid-matrix.sh --apply --levers baseline,weight --iter 9
```

Purpose of each run:
1. Full matrix with the v2.1 fixes — the battery phase must actually execute
   this time (battery `status=Discharging` detection), and this produces the
   FIRST work-per-joule numbers (`rapl_watts_efficiency` cells). No public
   power-efficiency claim exists until these numbers do.
2. Attribute the p95 synergy: `optid-performance` improved p95 (0.059 vs
   0.068 ms) while `weight` alone and EPP=power alone did not. The new
   `epp-perf` cell tests whether EPP=performance alone explains it.
3. Settle the §5.2 open question: does `weight` alone clip p99 at iter=9, or
   was it noise?

Acceptance (verifier checks):
- `results.csv` contains battery-phase rows (power_source=battery) — if the
  battery phase skipped again, that is a FINDING to report, not a failure to
  hide.
- Every row carries a per-cell ambient sample.
- `meta.txt` + `transcript.log` captured for all three runs.
- The exit-trap restore block appears in each transcript (knobs verified
  restored).

## T2 — Fold T1 results into the evidence tree

Create `release/evidence/host-bench/2026-06-11-victus/` mirroring the
2026-06-10 package: raw `results.csv`, `meta.txt`, `transcript.log` per run
(suffix `-full`, `-epp-perf`, `-weight` if needed), plus a `README.md` with:
the tables, the lever-attribution conclusion (does epp-perf alone move p95?),
the p99 verdict at iter=9, the first joules-per-work-unit table, and explicit
caveats (n=1 machine, ambient load recorded, firmware ~25 W DC cap). Update
`docs/testing-and-benchmarks.md` if conclusions change, register nothing new
in docmap unless a new doc is added. Draft PR; transcripts in the PR body.

## T3 — Wave 0 completion: systemd-oomd policy drop-ins

`systemd-oomd.service` is enabled in the image build but has NO ManagedOOM*
policy, so it monitors nothing. Add (config-only, no Rust):

1. `distro/systemd/oomd.conf` — `[OOM]` section,
   `DefaultMemoryPressureDurationSec=20s`. Install in
   `tools/build-vm-final.sh` to `${ROOTFS}/usr/lib/systemd/oomd.conf`.
2. `distro/systemd/-.slice.d/10-oomd-swap.conf` — `[Slice]`
   `ManagedOOMSwap=kill`. Install to
   `${ROOTFS}/usr/lib/systemd/system/-.slice.d/`.
3. `distro/systemd/user@.service.d/10-oomd-memory-pressure.conf` —
   `[Service]` `ManagedOOMMemoryPressure=kill`,
   `ManagedOOMMemoryPressureLimit=50%`. Install to
   `${ROOTFS}/usr/lib/systemd/system/user@.service.d/`.

Constraints: optid stays the only OOM-policy owner at runtime — these are
static defaults oomd consumes; do NOT add a second OOM handler (nohang et al.
are rejected, handover §3.3). Update `docs/architecture.md` or
`docs/adaptive-engine.md` only if they describe OOM handling; update
`docs/docmap.toml` `last_verified` for touched docs.

Acceptance: `bash -n tools/build-vm-final.sh`; `python3
tools/validate-doc-sync.py` → 0 errors; on the next built image (or any
systemd ≥252 host with the drop-ins installed): `oomctl` transcript showing
the monitored paths under "swap" and "memory pressure".

## T4 — mkosi spike (ADR 0014, long pole, local machine)

Minimal Arch-base mkosi image: `mkosi/` directory with `mkosi.conf` pinned to
an Arch Archive snapshot date and a pinned mkosi version; UKI output;
systemd-boot; the packages needed to pass the existing validators and the
Rush units (`optid`, slices, zram-generator.conf, oomd drop-ins from T3).

Gate (run twice, transcripts both times, scripts UNMODIFIED):

```sh
./tools/validate-uefi-boot.sh
./tools/test-rollback.sh
```

Also run the image-policy assertions from ADR 0014 (cgroup v2 unified,
nftables loaded, PSI active, no tlp.service). Abort criterion: if parity is
not reached within ~3 sessions, stop and report — the fallback is hybrid
assembly (mkosi rootfs + current boot-layout scripts), not more spike time.

---

## Executor prompt (owner: paste this into Antigravity verbatim)

The repo is the shared channel between executor and verifier: tasks live in
this document, results live in `docs/plans/reports/`. The owner only needs to
paste the block below once, then say "Proceed with T1+T2" (or T3, or T4).

```text
ROLE: You are the executor for Rush-linux (Nan0pk/Rush-linux). Claude (Fable)
plans and verifies; you execute. Do not relitigate strategy, ADRs, or owner
decisions. If a task is ambiguous or blocked, ask the owner ONE short
question and stop — do not improvise around it.

SETUP:
  git fetch origin claude/practical-faraday-9hn0j8
  Read docs/plans/antigravity-tasks-2026-06-11.md from that branch (or from
  main once PR #24 has merged). It defines tasks T1–T4 with acceptance
  blocks. Also binding: AGENTS.md (docs updated in the same change,
  docmap.toml registered, tools/validate-doc-sync.py must pass), draft PRs
  only, never push main, one task per branch/PR. Benchmark safety contract
  is non-negotiable: capture every knob before applying, restore + verify
  on every exit path, --apply required to mutate.

ORDER: T1 then T2 (same session). T3 separate branch/PR. T4 only when told.
T1 uses the runner from claude/practical-faraday-9hn0j8 if PR #24 is not
yet merged (it contains the battery-detection fix you need).

OUTPUT DISCIPLINE (the repo is the channel; chat is for one-liners):
- Never paste raw logs, full diffs, or file contents into chat. Commit
  transcripts/CSVs under release/evidence/... and reference paths + commit
  SHAs.
- No plan restatement, no progress narration, no apologies.
- After each phase, ONE status line to the owner, e.g.:
  [T1.run2 done] battery phase: EXECUTED | results: <path> | anomalies: none
- At the end of each task, write your report to
  docs/plans/reports/2026-06-11-<task-id>.md in the task's own PR, using
  the exact format in docs/plans/reports/README.md, then say only:
  "<task-id> report committed: <path> @ <sha>, PR #<n>".
- Hard caps: status lines ≤120 chars; report ≤25 lines; overflow belongs
  in a committed evidence file, not the report or chat.
- Claims rule: any "passed/works/matches" claim must point to a committed
  transcript path. No transcript = write "unverified".
- A skipped battery phase or weird numbers are FINDINGS to report, never
  to hide or retry silently.
```

## Reporting protocol

Executor reports are committed files, not chat messages, so the verifier can
read them cold and they survive as an audit trail:

- Location: `docs/plans/reports/2026-06-11-<task-id>.md`, committed in the
  same PR as the task's changes (for T1, in the T2 evidence PR).
- Format: see `docs/plans/reports/README.md` (TASK / STATUS / BRANCH-PR /
  ACCEPTANCE / EVIDENCE / DEVIATIONS / FINDINGS / VERIFY).
- The verifier (Fable) reads the report, re-runs the VERIFY block cold, and
  records the verdict in the PR review — builder ≠ verifier.

## What Fable verifies on each returned PR

1. Acceptance block commands re-run or transcripts inspected cold (builder ≠
   verifier, `AGENTS.md`).
2. Diff audited against the constraints above (especially: no GPL code copied
   in — license quarantine; no second mechanism for a solved problem).
3. Docs/docmap moved in the same change; `validate-doc-sync.py` green.
4. Evidence claims trace to transcripts, with caveats preserved.
