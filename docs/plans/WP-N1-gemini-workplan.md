# WP-N1 — Work Plan: Workload-class detector (contract-setter)

**Assignee:** Gemini (Antigravity CLI), Builder role.
**Authority:** Scoped by `docs/SPEC-northstar.md` (now canon on `main`). You
implement exactly one ledger role: the CONTRACT-SETTER that selects the active
workload class. You do not actuate anything. You do not propose direction. Output
is code + a verifier verdict, not a memo.
**Version:** 1.0
**Depends on:** SPEC-northstar on main (✓), WP-N0 merged (✓).
**Does NOT include:** the PPD D-Bus shim — that is WP-N1b, a separate PR.

---

## 1. Objective (one ledger role)

Implement a **workload-class detector** in `optid`: a pure function that maps the
signals optid already reads to exactly one of the five SPEC §1 classes, and
publishes the selected class to the state/explain surface.

Per SPEC §2 this is a **CONTRACT-SETTER**. It decides *how responsive the system
must stay*. It does **not** write a single sysctl, EPP value, or device state.
Selecting the class is the entire deliverable.

The five classes (SPEC §1, fixed — do not add or rename):
`idle`, `light`, `interactive`, `latency-critical`, `throughput`.

---

## 2. Hard constraints (do not violate)

1. **No actuation.** N1 emits a decision; it never mutates a knob. If you find
   yourself writing to `/proc/sys`, `energy_performance_preference`, a slice, or
   any device state, STOP — that is N2+, not N1.
2. **Inputs limited to signals optid already reads** plus explicit pins:
   - PSI (cpu/memory/io) — already ingested,
   - load average — already ingested,
   - AC/battery state — already ingested,
   - foreground app hint **only if already available**; if not present, do NOT
     build new detection for it — leave a typed `Option<...>` input and default
     its absence to the unclassified path. New telemetry is WP-N3, not N1.
   - `optctl pin <app> <class>` as a manual override (highest precedence).
3. **No new telemetry sources.** No GPU detection, no fullscreen/video-call
   heuristics, no wakeup-source reads, no D-Bus listening. Those are N3 / N1b.
   This is the scope-creep boundary; treat any temptation to cross it as a signal
   you are off-plan.
4. **Contract table is data, declared-not-enforced.** Ship a class→latency-budget
   table (see §5.3) as config. The classifier selects a row; the budget *values*
   are NOT consumed by any actuator in this WP. Mark them explicitly as
   "defined, not yet enforced" so this does not become a hidden lie (same honesty
   rule that flagged N0's parsed-not-applied vm keys).
5. **Explainability (SPEC §3).** Every class selection emits a reason:
   `(selected_class, deciding_signal(s), precedence_applied)`. Visible via the
   existing `optctl explain` surface.
6. **Determinism + hysteresis.** Class selection must be deterministic for a given
   input snapshot, and must resist flapping: a class change requires the new class
   to hold across a minimum dwell window (define a small, documented default).
   This prevents the classifier from oscillating on PSI jitter — a known failure
   mode the spec's enablers depend on not happening.
7. **Doc-sync acceptance rule.** Behavior change updates docs in the same PR.

---

## 3. Preconditions (idempotent — run first)

```bash
git checkout main && git pull
test -f docs/SPEC-northstar.md || { echo "MISSING canon spec — STOP"; exit 1; }
grep -q "Workload-class detector" docs/SPEC-northstar.md || echo "review §6 WP-N1 row"
```

---

## 4. Discovery (use repo tooling, do not broad-read)

```bash
graphify query "where are PSI, load average, and mode selection computed in optid, and where is class/state published?" --graph graphify-out/graph.json
```

Read only what it points to. Expect: the existing mode-resolution logic (battery/
balanced/performance/realtime), the PSI/load ingestion, the state-write path, and
the `optctl explain`/status surface. The classifier sits **upstream** of mode
selection conceptually but must not be wired into actuation in this WP — publish
the class to state and stop there.

---

## 5. Implementation requirements

### 5.1 Classifier (pure function)
- Signature: snapshot of inputs (§2.2) → exactly one class.
- Precedence: `optctl pin` override > signal-derived class.
- Deterministic; no I/O inside the decision function (read inputs first, decide
  on the snapshot). This is what makes it fixture-testable.

### 5.2 Hysteresis wrapper
- A class transition commits only after the candidate class persists across the
  dwell window. Document the default (e.g. N samples / M seconds) and make it a
  named constant or config key, not a magic number.

### 5.3 Contract table (data, not enforced)
- A `config/optid/contracts.toml` (or repo-consistent location) mapping each of
  the five classes to declared latency budgets: at minimum `cpu_wakeup_latency`
  and `device_resume_latency` placeholders.
- Values may be conservative first guesses. They are NOT read by any actuator in
  N1. Comment them as "consumed by WP-N2 (PM QoS); not enforced yet."

### 5.4 Publication
- Write the selected class + reason to the state dir and expose via
  `optctl status` (and `--json`) and `optctl explain`.

---

## 6. Tests required (verifier PASS criteria) — all fixture-based, no system state

- **T1 — class mapping:** for a table of input fixtures (PSI/load/AC
  combinations), the classifier returns the expected class. Cover all five
  classes at least once.
- **T2 — pin precedence:** an active `optctl pin` overrides the signal-derived
  class.
- **T3 — hysteresis:** a single-sample blip that does not persist the dwell window
  does NOT change the committed class; a sustained change does.
- **T4 — determinism:** identical input snapshot yields identical class across
  repeated calls.
- **T5 — explainability:** every selection produces a reason record with the
  deciding signal(s) and precedence applied.
- **T6 — absent foreground hint:** with the foreground input `None`, the
  classifier still returns a valid class via signal fallback (no panic, no
  default-to-performance).
- **T7 — workspace gates:** `cargo fmt`, `cargo test --workspace`,
  `cargo clippy -D warnings` all green.

---

## 7. Docs to update in the SAME PR

- `IMPLEMENTATION_STATUS.md` — add workload classifier as implemented; note the
  contract table is **defined, not yet enforced**.
- `docs/SPEC-northstar.md` — §4.2 ledger: "Workload-class detection" status `— → A`
  (setter). Leave the contract-*enforcement* rows untouched (that's N2).
- adaptive-engine / policy doc describing how the class is selected.

---

## 8. PR instructions

```bash
git checkout -b feat/wp-n1-workload-classifier
# ... commits ...
git push -u origin feat/wp-n1-workload-classifier
gh pr create \
  --title "feat(optid): WP-N1 workload-class detector" \
  --body "Implements SPEC-northstar §4.2 contract-setter: pure classifier mapping PSI/load/AC/pin to {idle,light,interactive,latency-critical,throughput}, with hysteresis and explainable reasons. Ships class->budget contract table as data (defined, NOT enforced; consumed by N2). No actuation. Tests T1-T7 green. Docs synced."
```

- Explicit `--title`/`--body`; never `--fill`.
- **Honest PR body.** State only what is implemented and tested. The contract
  table is declared-not-enforced — say so. No "contracts active" language.

---

## 9. Definition of done — post this verdict

```
WP-N1 VERDICT: PASS|FAIL
T1 class mapping:        PASS|FAIL  <evidence>
T2 pin precedence:       PASS|FAIL  <evidence>
T3 hysteresis:           PASS|FAIL  <evidence>
T4 determinism:          PASS|FAIL  <evidence>
T5 explainability:       PASS|FAIL  <evidence>
T6 absent foreground:    PASS|FAIL  <evidence>
T7 fmt/test/clippy:      PASS|FAIL  <CI link>
Docs synced:             YES|NO     <files>
No actuation (scope):    YES|NO     <confirm no knob writes added>
Contract table marked not-enforced: YES|NO
```

FAIL any line → not ready. Human owns merge.

---

## 10. Out of scope (explicit)

PPD D-Bus shim (= WP-N1b), PM QoS enforcement (= N2), any actuation, runtime PM,
new telemetry of any kind (GPU/fullscreen/video/wakeup = N3), APST/ASPM, README
edits, AGENTS.md edits. If the work seems to need any of these, it doesn't — stop
and report.
