# WP-N2 — Work Plan: PM QoS contract enforcement (the spine)

**Assignee:** Gemini (Antigravity CLI), Builder role.
**Authority:** Scoped by `docs/SPEC-northstar.md` (canon on `main`). You implement
the CONTRACT-SETTER enforcement layer: turn the active workload class into
enforced PM QoS latency floors, and expose those floors so enablers can consume
them. You do NOT deepen any device. Output is code + a verifier verdict.
**Version:** 1.0
**Depends on:** WP-N0 merged (✓ vm.* enabler + revert-journal pattern),
WP-N1 merged (✓ class selection + `contracts.toml` declared-not-enforced).

---

## 1. Objective (one ledger role)

Make the class→latency-budget contract **real**. Read the committed workload
class (from N1), resolve its contract row to concrete floors
(`cpu_wakeup_latency`, `device_resume_latency`), and:

1. **Enforce** those floors via Linux PM QoS, behind the SPEC §3 actuation rule.
2. **Expose** the resolved floors as readable state + a `fits_contract()` helper,
   so WP-N5/N6 enablers can check device exit-latency against the floor.

This is the SPEC §4.2 PM QoS contract surface. It connects the setter (N1) to the
enablers (N0 and future). Before N2, N1's contracts.toml is inert data. After N2,
it governs the system.

---

## 2. Hard constraints (do not violate)

1. **Write PM QoS ONLY.** The only kernel interfaces N2 may write:
   - global CPU wakeup latency via `/dev/cpu_dma_latency` (hold-fd-open model),
   - per-device resume latency via `…/power/pm_qos_resume_latency_us`.
   N2 must NOT write EPP, `platform_profile`, systemd slices, `vm.*` (N0 owns it),
   runtime PM autosuspend, APST, ASPM, or any device power state. Those are
   enablers. N2 expresses floors; it deepens nothing. If you write a depth knob,
   STOP — that is N5/N6.
2. **Revert asymmetry (implement both correctly):**
   - `/dev/cpu_dma_latency` is asserted by holding the fd open; it auto-releases
     on close and on process death. Crash-safe for free. Do NOT build a disk
     journal for it — just hold/drop the fd.
   - `pm_qos_resume_latency_us` is a persistent sysfs write. It REQUIRES the N0
     disk-journal revert pattern: back up prior value before write, restore on
     startup + shutdown. Reuse N0's journal mechanism; do not reinvent it.
3. **Consume N1's class, don't re-derive it.** Read the committed class from the
   state/in-process surface N1 publishes. Do NOT re-implement classification, and
   do NOT add a second hysteresis layer — N1 already debounces. N2 inherits the
   committed class.
4. **No-thrash:** re-assert PM QoS only when the resolved floor actually changes,
   not every tick.
5. **§3 actuation rule applies in full:** `--apply` mutation gate (dry-run logs
   intended floors, writes nothing); revert (per §2.2); explainable reason per
   assertion = `(class, floor values, which contract row)`.
6. **Floor-query API is a deliverable, wired to zero devices.** Ship:
   - readable resolved floors (state dir + `optctl status`),
   - a pure helper `fits_contract(exit_latency_us, floor_us) -> bool`,
     unit-tested. NO device calls it yet. Mark "defined, consumed by WP-N5/N6; not
     yet wired" — same honesty rule as N1's contract table.
7. **Enforce declared values; invent none.** N2 enforces whatever `contracts.toml`
   declares. Do not add or tune budget numbers. Note in docs that the values
   remain **provisional pending WP-B1 validation** — N2 proves the mechanism, not
   the specific µs.
8. **Doc-sync acceptance rule.**

---

## 3. Preconditions (idempotent — run first)

```bash
git checkout main && git pull
test -f docs/SPEC-northstar.md || { echo "MISSING canon spec — STOP"; exit 1; }
test -f config/optid/contracts.toml || { echo "MISSING contracts.toml (WP-N1) — STOP"; exit 1; }
grep -qi "not yet enforced\|not-enforced\|defined, not" config/optid/contracts.toml || echo "review contracts.toml enforcement note"
```

If either file is absent, stop — the dependency chain is broken, do not proceed.

---

## 4. Discovery (repo tooling, no broad reads)

```bash
graphify query "where is the committed workload class published by N1, and where does optid hold long-lived handles / state?" --graph graphify-out/graph.json
graphify query "how does WP-N0 back up and revert sysctl values via the state dir journal?" --graph graphify-out/graph.json
```

Read only what they point to: N1's class publication, N0's journal helpers, the
state-dir handling, the daemon's main loop (where to re-assert on change), and the
contracts.toml schema.

---

## 5. Implementation requirements

### 5.1 Floor resolution (pure function)
- `class → ContractFloors { cpu_wakeup_latency_us, device_resume_latency_us, … }`
  by reading the matching `contracts.toml` row. Pure, fixture-testable.

### 5.2 CPU floor enforcement (fd model)
- On `--apply`, open `/dev/cpu_dma_latency` and write the active CPU floor; hold
  the fd for the daemon's lifetime; rewrite on floor change. Drop fd on
  stop/class-removal. No disk journal.

### 5.3 Per-device resume-latency enforcement (journal model)
- On `--apply`, for each targeted device, back up prior
  `pm_qos_resume_latency_us` to the state-dir journal (N0 pattern), then write the
  floor. Restore on startup + shutdown. Initial device set may be minimal/
  allowlist-driven; do NOT expand into runtime-PM policy (that's N5).
- If no device targeting exists yet, it is acceptable to enforce the CPU floor
  only and ship the per-device path behind the same gate with a small explicit
  device list — but say so honestly in the PR.

### 5.4 Floor exposure + helper
- Write resolved floors to state; surface via `optctl status` / `--json` and
  `optctl explain`.
- Implement and unit-test `fits_contract(exit_latency_us, floor_us) -> bool`.
- Wire it to NO device. It exists for N5/N6.

### 5.5 Re-assert on change only
- Track last-applied floors; act only on change (§2.4).

---

## 6. Tests required (verifier PASS criteria)

- **T1 — resolution:** each class resolves to the expected floors from a
  fixture `contracts.toml`. All five classes covered.
- **T2 — dry-run no-op:** without `--apply`, no `/dev/cpu_dma_latency` open and
  no `pm_qos_resume_latency_us` write; intended floors logged with reason.
- **T3 — apply CPU floor:** with `--apply`, the CPU floor is asserted; abstract
  the PM QoS sink behind a trait so this is testable without real hardware.
- **T4 — per-device revert:** prior `pm_qos_resume_latency_us` is journaled and
  restored on stop (reuse/verify N0 journal).
- **T5 — fd release:** dropping the CPU constraint releases the fd (constraint
  gone) — assert via the sink abstraction.
- **T6 — no-thrash:** unchanged resolved floor across ticks → no re-assert.
- **T7 — fits_contract helper:** boundary cases (exit == floor, exit < floor,
  exit > floor) return correct bool.
- **T8 — explainability:** every assertion emits `(class, floors, contract row)`.
- **T9 — workspace gates:** `cargo fmt`, `cargo test --workspace`,
  `cargo clippy -D warnings` green.

---

## 7. Docs to update in the SAME PR

- `IMPLEMENTATION_STATUS.md` — PM QoS enforcement implemented; contracts.toml now
  **enforced** (was defined-not-enforced); `fits_contract` API **defined, not yet
  wired to devices**; budget values **provisional pending WP-B1**.
- `docs/SPEC-northstar.md` — §4.2 PM QoS rows status `— → A` (setter). Note the
  enabler exit-latency check is available but unconsumed until N5/N6.
- adaptive-engine / policy doc: class → floor → PM QoS flow.

---

## 8. PR instructions

```bash
git checkout -b feat/wp-n2-pmqos-contracts
# ... commits ...
git push -u origin feat/wp-n2-pmqos-contracts
gh pr create \
  --title "feat(optid): WP-N2 PM QoS contract enforcement" \
  --body "Implements SPEC-northstar §4.2 PM QoS setter: resolves committed class to latency floors and enforces via /dev/cpu_dma_latency (fd-held, crash-safe) and pm_qos_resume_latency_us (N0-journal revert), behind the §3 gate. Exposes resolved floors + fits_contract() helper for N5/N6 (defined, not yet wired to devices). Consumes N1 class; no second hysteresis; re-asserts on change only. No depth-enabler writes. contracts.toml values provisional pending WP-B1. Tests T1-T9 green."
```

- Explicit `--title`/`--body`; never `--fill`. **Open as Draft** until verified.
- **Honest PR body.** fits_contract is unwired; budget values provisional. Say so.
  No "contracts fully active across all devices" language unless the per-device
  path actually targets a real device set.

---

## 9. Definition of done — post this verdict

```
WP-N2 VERDICT: PASS|FAIL
T1 resolution:           PASS|FAIL  <evidence>
T2 dry-run no-op:        PASS|FAIL  <evidence>
T3 apply CPU floor:      PASS|FAIL  <evidence>
T4 per-device revert:    PASS|FAIL  <evidence>
T5 fd release:           PASS|FAIL  <evidence>
T6 no-thrash:            PASS|FAIL  <evidence>
T7 fits_contract:        PASS|FAIL  <evidence>
T8 explainability:       PASS|FAIL  <evidence>
T9 fmt/test/clippy:      PASS|FAIL  <CI link>
Docs synced:             YES|NO     <files>
PM QoS writes only (scope): YES|NO  <confirm no EPP/slice/vm/device-depth writes>
fits_contract unwired + values provisional noted: YES|NO
```

FAIL any line → not ready. Human owns merge.

---

## 10. Out of scope (explicit)

Runtime PM autosuspend (N5), NVMe APST / PCIe ASPM / SATA ALPM (N6), wiring
`fits_contract` to real devices (N5/N6), PPD D-Bus shim (N1b), DTPM/powercap outer
loop (N8), tuning contract values (WP-B1), EPP/slice/vm.* changes, README/AGENTS
edits. The exit-latency-vs-floor *check* is shipped here; *applying* it to a device
is not. If the work seems to need any of these, it doesn't — stop and report.
