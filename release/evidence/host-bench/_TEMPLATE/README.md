# Host-Bench Evidence Template (v0.6 Phase D)

> **This `_TEMPLATE/` directory is NOT evidence.** It is the empty,
> structured shape that each real reference-machine directory must follow. Copy
> it to `release/evidence/host-bench/<date>-<hostname>/` and fill it in with
> literal transcripts. Do **not** mark any v0.6 criterion `verified = true` in
> `release/milestones.toml` until a real (non-template) directory exists with a
> committed `VERDICT.md`.

## How to use (per reference machine)

In the commands below, `HB` is the host-bench root and `DIR` is this machine's
real (non-template) directory, named `<date>-<hostname>` (for example
`2026-07-15-thinkpad`):

```bash
HB="release/evidence/host-bench"
DIR="$HB/<date>-<hostname>"

# 1. Copy the template for this machine (D3 baseline + D4 optid live here)
cp -r "$HB/_TEMPLATE" "$DIR"

# 2. D3 + D4 — both arms, on battery. The script owns the service/daemon state
#    the two arms must differ in (baseline keeps the distro's default power
#    stack; the optid arm stops it and runs `optid --apply`), pins the energy
#    counter so the arms stay comparable, and writes meta.txt, transcript.log,
#    results.csv and the per-metric *.json straight into $DIR/{baseline,optid}/.
sudo bash tools/phase-d-capture.sh --arm both --dir "$DIR"

#    Or one arm at a time, for example after a recharge:
sudo bash tools/phase-d-capture.sh --arm baseline --dir "$DIR"
sudo bash tools/phase-d-capture.sh --arm optid    --dir "$DIR"

#    Underneath, each arm is one preset run (do not invoke this directly unless
#    you are managing the daemon and service state yourself):
# rushbench run preset=mixed-load-001 --tag=<lever>-<hostname> \
#     --cycles 5 --out "$DIR/<lever>"

# 4. D5 — comparison + verdict
rushbench report "$DIR/baseline" > "$DIR/baseline-report.md"
rushbench report "$DIR/optid"    > "$DIR/optid-report.md"
#   → fill in $DIR/VERDICT.md, then update milestones.toml v0.6 criteria_status
```

## Required files

```
<date>-<hostname>/
├── VERDICT.md            # D5 — PASS/FAIL per criterion; literal source for milestones.toml note
├── baseline/             # D3 — mainstream-default run
│   ├── meta.txt          #   host facts (Dragnet template; gate-checked)
│   ├── transcript.log    #   literal run transcript
│   ├── results.csv       #   per-phase/metric rows
│   └── *.json            #   RunRecord(s), schema_version 1
└── optid/                # D4 — optid --apply run
    ├── meta.txt
    ├── transcript.log
    ├── results.csv
    └── *.json
```

## Acceptance

- Both reference machines (per [`docs/strategy/reference-hardware.md`](../../../../docs/strategy/reference-hardware.md))
  have a committed, non-template directory.
- Every metric record has `n >= 5` (no `insufficient_n` anomaly) and no
  `phase_scale_shortened` anomaly (that stamp marks a shortened harness-validation
  run, which is never evidence).
- Both arms' `transcript.log` name the **same** energy counter. A baseline arm
  that fell back to the battery counter cannot be compared against an optid arm
  that read RAPL.
- `python3 tools/dragnet.py --observe` is green.
- Each `VERDICT.md` carries an explicit PASS/FAIL for Criterion 2, and
  PASS/FAIL/N-A for Criterion 3.
