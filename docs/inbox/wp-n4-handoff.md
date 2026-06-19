# WP-N4 — Hardware Allowlist DB — Handoff Prompt

*Paste this into a fresh Claude Code session to continue implementation.*

---

## Objective

Implement WP-N4 from `docs/SPEC-northstar.md §6`: the HWID → {domain,state}
allow/deny database that gates every depth-enabler actuation per the §3
actuation rule. This is the critical-path unblock — WP-N5/N6/N7 cannot ship
without it.

**Verifier PASS criterion** (from SPEC §6, must be met exactly):
> "Default-deny for risky knobs; seeded safe baseline; denial logged with reason."

---

## Read first (in order)

1. `docs/SPEC-northstar.md` §3 (actuation rule), §6 (WP ledger) — the contract.
2. `docs/research/0006-hw-allowlist-db-design.md` — the design this implements.
   Treat `[PROVEN]` findings as the spec and `[HYPOTHESIS]` findings as decisions
   to confirm, not gospel.
3. `AGENTS.md` — Builder/Verifier protocol and Evidence Rule (you cannot
   certify your own work; success claims need a literal command transcript).
4. `CLAUDE.md` — fast-path conventions.
5. Existing code you are extending:
   - `crates/optid/src/actuator.rs` — the single write funnel; the gate goes here.
   - `crates/optid/src/io_util.rs` — existing **path** allowlist (ADR 0009).
     This is DIFFERENT from the HWID allowlist; do not conflate them.
   - `crates/optid/src/contracts.rs` — `fits_contract` is coded but unconsumed;
     N4 does not consume it either, but N5/N6 will once both exist.

---

## Scope (what to build)

Per `0006`'s "Hybrid E" decision:

- **`data/allowlist.toml`** — seeded safe baseline (compiled-in source of truth).
- **`crates/optid/build.rs`** — codegen that reads `data/allowlist.toml` into a
  compiled-in `const` table (libinput-quirks pattern). No `build.rs` exists yet;
  adding one changes the build — keep it hermetic and fast.
- **Runtime overrides** loaded from `/etc/optid/allowlist.d/*.toml` with override
  precedence from 0006: compiled-in < distro < admin < runtime (optctl) < `--unsafe-once`.
- **HWID canonical form** per 0006: PCI modalias `pci:vNNNNdNNNN...`, USB
  `usb:vNNNNpNNNN...`, ACPI `acpi:INTXXXX`. NVMe controllers resolve to the
  underlying PCI modalias via `/sys/class/nvme/nvme0/device/modalias`.
- **Lookup API**: given `(domain, hwid, requested_state)` → `Allow` or `Deny { reason }`.
- **Wire the gate** into `actuator.rs` so default-deny holds and every denial is
  logged with its reason string.

Do NOT remove or weaken the existing path-based `guarded_write` allowlist — this
is an additional, orthogonal gate.

---

## Out of scope (do not build)

- The N5/N6/N7 enablers that consume this allowlist (autosuspend, APST/ASPM,
  display depth). N4 only provides the gate and seeded baseline.
- Hardware measurement. Exit-latency reads (`nvme id-ctrl`, `PCI_EXP_LNKCAP`) are
  N5/N6 concerns; N4 just stores and serves allow/deny decisions.

---

## Definition of done

- `cargo fmt --all`, `cargo clippy --workspace` (clean), `cargo test -p optid` (green).
- New unit tests proving:
  - Default-deny on unknown HWID.
  - Allow on a seeded-baseline HWID.
  - Denial carries a reason string.
  - Override precedence resolves correctly.
- `docs/docmap.toml` updated in the **same commit** — `docs/research/0006-docmap-entry.toml`
  lists the `covers_code` paths; make them real, append the entry to `docmap.toml`,
  then run `python3 tools/validate-doc-sync.py`.
- Run `tools/finish-work.sh` (or the `rust-verifier` agent) and paste the literal
  transcript. Per the Evidence Rule you do not self-certify — the verifier verdict
  is the proof.

---

## Git / process

- Branch off latest `main` (PR #134 with research 0006–0018 is already merged).
- Use a fresh feature branch, e.g. `feat/wp-n4-allowlist-db`. Do NOT reuse
  `claude/analysis-research-3w2085` (that was the docs branch, now merged).
- Commit code + data + docmap together. Open a draft PR.
- Git author must be `Claude <noreply@anthropic.com>`.

---

## Guardrail

SPEC §7: agents implement leaves, humans own direction. Do not redefine the
objective or propose pivots. If `0006`'s design and the existing code conflict in
a way that needs a real architectural decision, stop and ask — don't guess.
