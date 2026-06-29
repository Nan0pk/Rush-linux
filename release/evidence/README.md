# Evidence Index — Rush Linux

> **Canonical source of truth:** [`release/milestones.toml`](../../release/milestones.toml)
> (machine-readable) and [`dragnet/LEDGER.md`](dragnet/LEDGER.md) (per-row audit
> trail). This index is a human-readable rendering for GitHub visitors. If it
> disagrees with `milestones.toml`, `milestones.toml` wins.

Rush Linux enforces an **Evidence Rule**: no claim of correctness or performance
is accepted without a literal command transcript. `✅ Verified` means a human or
CI ran the command and the output is on record. Every milestone exit criterion
in `release/milestones.toml` may be marked `verified = true` only when it carries
a `transcript = "<path>"` that points to a committed, non-empty file in this
directory. The gate is enforced mechanically by
[`tools/validate-evidence.py`](../../tools/validate-evidence.py) on every push
and PR — there is no skip path.

## Current state (as of Dragnet-004, 2026-06-26)

| Milestone | Channel | Exit criteria | Transcripts committed | Status |
|---|---|---|---|---|
| `0.1.0-alpha.1` — Compile-Clean Core | alpha | 4 | 4 (CI history → [`core-tests/`](core-tests/)) | ✅ complete |
| `0.2.0-alpha.1` — Real Control Plane | alpha | 4 | 4 (CI history → [`core-tests/`](core-tests/)) | ✅ complete |
| `0.3.0-alpha.1` — Rootfs And Package Builder MVP | alpha | 4 | 4 ([`v0.3.0-alpha.1/`](v0.3.0-alpha.1/)) | ✅ complete |
| `0.4.0-alpha.1` — UKI, Boot, Rollback, Updates | alpha | 4 | 4 ([`v0.4.0-alpha.1/`](v0.4.0-alpha.1/)) | ✅ complete |
| `0.5.0-beta.1` — Minimal Installable System | beta | 4 | 4 ([`v0.5.0-beta.1/`](v0.5.0-beta.1/)) | ✅ complete |
| `0.6.0-beta.1` — Hardware-Aware optid | beta | 4 | 0 | ⚙ in progress — code-complete (PRs #183–#186); 2 quantitative criteria pending Phase D ([`host-bench/`](host-bench/)) |
| `0.7.0-beta.1` — Editions | beta | 4 | 0 | ⚙ in progress (current version) |
| `0.8.0-beta.1` — Benchmark Lab | beta | 3 | 0 | planned |
| `0.9.0-rc.1` — Release Candidate Hardening | rc | 4 | 0 | planned |
| `1.0.0` — Final Stable Release | stable | 5 | 0 | planned |

**Totals:** 12/12 v0.3-v0.5 exit criteria carry committed transcripts.
Dragnet-004 (2026-06-26) is the first GREEN report — see
[`dragnet/DRAGNET-004-2026-06-26.md`](dragnet/DRAGNET-004-2026-06-26.md).

---

## v0.3.0-alpha.1 — Rootfs And Package Builder MVP

Transcripts produced on the build host (root + KVM) per
[`BUILD-HOST-RUNBOOK.md`](BUILD-HOST-RUNBOOK.md) and committed by PR #174 on
2026-06-23. Dragnet-001 (2026-06-22) had flagged the earlier `verified = true`
flags as lacking committed transcripts; PR #174 closed that debt.

| # | Criterion | Transcript | `meta.txt` |
|---|---|---|---|
| 1 | minimal VM boots to `multi-user.target` | [`v0.3.0-alpha.1/c1-multiuser/transcript.log`](v0.3.0-alpha.1/c1-multiuser/transcript.log) | [`v0.3.0-alpha.1/c1-multiuser/meta.txt`](v0.3.0-alpha.1/c1-multiuser/meta.txt) |
| 2 | cgroup v2 and PSI are active | [`v0.3.0-alpha.1/c2-cgroup-psi/transcript.log`](v0.3.0-alpha.1/c2-cgroup-psi/transcript.log) | [`v0.3.0-alpha.1/c2-cgroup-psi/meta.txt`](v0.3.0-alpha.1/c2-cgroup-psi/meta.txt) |
| 3 | `optid.service` starts | [`v0.3.0-alpha.1/c3-optid-service/transcript.log`](v0.3.0-alpha.1/c3-optid-service/transcript.log) | [`v0.3.0-alpha.1/c3-optid-service/meta.txt`](v0.3.0-alpha.1/c3-optid-service/meta.txt) |
| 4 | `nftables.conf` loads | [`v0.3.0-alpha.1/c4-nftables/transcript.log`](v0.3.0-alpha.1/c4-nftables/transcript.log) | [`v0.3.0-alpha.1/c4-nftables/meta.txt`](v0.3.0-alpha.1/c4-nftables/meta.txt) |

## v0.4.0-alpha.1 — UKI, Boot, Rollback, Updates

| # | Criterion | Transcript | `meta.txt` |
|---|---|---|---|
| 1 | VM boots through UKI | [`v0.4.0-alpha.1/c1-uki-boot/transcript.log`](v0.4.0-alpha.1/c1-uki-boot/transcript.log) | [`v0.4.0-alpha.1/c1-uki-boot/meta.txt`](v0.4.0-alpha.1/c1-uki-boot/meta.txt) |
| 2 | three rollback entries are retained | [`v0.4.0-alpha.1/c2-rollback-retain/transcript.log`](v0.4.0-alpha.1/c2-rollback-retain/transcript.log) | [`v0.4.0-alpha.1/c2-rollback-retain/meta.txt`](v0.4.0-alpha.1/c2-rollback-retain/meta.txt) |
| 3 | simulated bad kernel rolls back | [`v0.4.0-alpha.1/c3-bad-kernel/transcript.log`](v0.4.0-alpha.1/c3-bad-kernel/transcript.log) | [`v0.4.0-alpha.1/c3-bad-kernel/meta.txt`](v0.4.0-alpha.1/c3-bad-kernel/meta.txt) |
| 4 | test update metadata is signed | [`v0.4.0-alpha.1/c4-update-signed/transcript.log`](v0.4.0-alpha.1/c4-update-signed/transcript.log) | [`v0.4.0-alpha.1/c4-update-signed/meta.txt`](v0.4.0-alpha.1/c4-update-signed/meta.txt) |

Criterion 4 was the first to close — done in-container by Dragnet-001 (PR #167,
2026-06-22) via `tools/test-sign-updates.sh` (Ed25519 keygen → sign → verify →
tamper-detect). Criteria 1-3 required build-host KVM and closed in PR #174.

## v0.5.0-beta.1 — Minimal Installable System

Per-milestone layout and producer/verifier protocol:
[`v0.5.0-beta.1/README.md`](v0.5.0-beta.1/README.md).

| # | Criterion | Transcript | `meta.txt` |
|---|---|---|---|
| 1 | fresh VM install succeeds | [`v0.5.0-beta.1/c1-fresh-install/transcript.log`](v0.5.0-beta.1/c1-fresh-install/transcript.log) | [`v0.5.0-beta.1/c1-fresh-install/meta.txt`](v0.5.0-beta.1/c1-fresh-install/meta.txt) |
| 2 | installed system boots twice cleanly | [`v0.5.0-beta.1/c2-double-boot/transcript.log`](v0.5.0-beta.1/c2-double-boot/transcript.log) | [`v0.5.0-beta.1/c2-double-boot/meta.txt`](v0.5.0-beta.1/c2-double-boot/meta.txt) |
| 3 | update and rollback tests pass | [`v0.5.0-beta.1/c3-update-rollback/transcript.log`](v0.5.0-beta.1/c3-update-rollback/transcript.log) | [`v0.5.0-beta.1/c3-update-rollback/meta.txt`](v0.5.0-beta.1/c3-update-rollback/meta.txt) |
| 4 | server edition has no desktop dependency | [`v0.5.0-beta.1/c4-server-no-desktop/transcript.log`](v0.5.0-beta.1/c4-server-no-desktop/transcript.log) | [`v0.5.0-beta.1/c4-server-no-desktop/meta.txt`](v0.5.0-beta.1/c4-server-no-desktop/meta.txt) |

Criterion 4 is closed by static analysis of the declared mkosi package set
(closed in PR #167); the other three required build-host KVM and closed in PR
#174.

---

## v0.1 and v0.2 — compile-clean + D-Bus control plane

These milestones have no `criteria_status` rows in `release/milestones.toml`;
their "complete" status rests on CI history (compile + clippy + test) captured
as a snapshot in [`core-tests/2026-06-22/`](core-tests/2026-06-22/). The
directory holds:

- [`core-tests/2026-06-22/cargo-test.log`](core-tests/2026-06-22/cargo-test.log) — full `cargo test --workspace` output
- [`core-tests/2026-06-22/cargo-clippy.log`](core-tests/2026-06-22/cargo-clippy.log) — `cargo clippy --workspace -- -D warnings` output
- [`core-tests/2026-06-22/meta.txt`](core-tests/2026-06-22/meta.txt) — date, host, kernel, cpu, git_commit, tool versions

---

## How this directory is structured

```
release/evidence/
├── README.md                         ← this file (human-readable index)
├── BUILD-HOST-RUNBOOK.md             ← how to produce build-host transcripts (root + KVM)
├── core-tests/<date>/                ← v0.1/v0.2 CI history snapshots
├── host-bench/<date>-<hostname>/     ← physical-machine benchmark runs (v0.6+)
├── v0.3-v0.4-uefi-boot/README.md     ← placeholder; superseded by per-milestone dirs
├── v0.3.0-alpha.1/<criterion-slug>/  ← one dir per exit criterion
│   ├── meta.txt                      ← date, host, kernel, cpu, git_commit, tool versions
│   └── transcript.log                ← literal stdout+stderr of the verification command
├── v0.4.0-alpha.1/<criterion-slug>/  ← same shape
├── v0.5.0-beta.1/<criterion-slug>/   ← same shape
└── dragnet/                          ← evidence-integrity sweep reports + audit ledger
    ├── LEDGER.md                     ← per-row evidence state (the live truth)
    ├── LESSONS.md                    ← lessons learned from past evidence failures
    ├── DRAGNET-001-2026-06-22.md     ← the audit that reset v0.3/v0.4/v0.5 to honest state
    ├── DRAGNET-002-2026-06-23.md
    ├── DRAGNET-003-2026-06-23.md
    └── DRAGNET-004-2026-06-26.md     ← first GREEN report (12/12 committed)
```

The `meta.txt` capture block and per-criterion acceptance commands live in
[`BUILD-HOST-RUNBOOK.md`](BUILD-HOST-RUNBOOK.md). Each `meta.txt` includes at
minimum: `date`, `host`, `kernel`, `cpu`, `git_commit`, `project_version`, and
the relevant tool versions (`qemu_version`, `mkosi_version`, etc.).

## The Builder/Verifier split

Per [`docs/agent-protocol.md`](../../docs/agent-protocol.md):

- A **Builder** agent may produce an image and run its self-checks, but may
  **not** declare a criterion verified.
- A **Verifier** agent checks out the branch cold, runs the acceptance block
  verbatim, and writes a `VERIFICATION.md`.
- Only the **human maintainer** flips `verified = true` in
  `release/milestones.toml`, with the relative path of the transcript in the
  `transcript = ` field.

This is why Dragnet-001 (2026-06-22) was necessary: earlier sessions had set
`verified = true` on v0.3/v0.4/v0.5 criteria without committing transcripts.
The Dragnet protocol catches that mechanically — `tools/validate-evidence.py`
refuses to pass if any `verified = true` row lacks a resolving transcript on
disk.

## Adding new evidence (for v0.6 and beyond)

1. Read [`BUILD-HOST-RUNBOOK.md`](BUILD-HOST-RUNBOOK.md) for the canonical
   acceptance commands and `meta.txt` capture block.
2. Run the acceptance command on the appropriate host (container for static
   checks; build host with root + KVM for boot/install/rollback tests).
3. Commit `meta.txt` + `transcript.log` to the criterion's directory under
   `release/evidence/<milestone-version>/<criterion-slug>/` — for example,
   `release/evidence/v0.6.0-beta.1/c1-allowlist-skip/`.
4. Update the `transcript = ` field for that criterion in
   `release/milestones.toml`.
5. Run `python3 tools/dragnet.py --observe` locally — it must return GREEN.
6. Open a PR; CI will run `tools/validate-evidence.py` as the
   "Evidence integrity (Dragnet)" check. It cannot be skipped.
7. After merge, this index should be updated in the same PR (or the
   immediately-following one) to add the new row to the relevant milestone
   table.

The next milestone that will populate this tree is `0.6.0-beta.1`
(Hardware-Aware optid). Its four exit criteria will land under
`release/evidence/v0.6.0-beta.1/` for the policy checks and under
`release/evidence/host-bench/` for the physical-machine benchmarks. See
[`docs/plans/v0.6-hardware-aware-optid-proposal.md`](../../docs/plans/v0.6-hardware-aware-optid-proposal.md)
Phase D for the benchmark plan.
