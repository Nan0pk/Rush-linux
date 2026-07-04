# LiveDev — Workspace Enablement Plan

> **Phase:** `workspace-enablement` (this PR)
> **Next phase:** `architecture-contract`
> **Source commit:** `2a10847d89d4413b9df305c48bb8c4545ce3017e` (tip of `main` when this branch was cut)
> **Working branch:** `chore/livedev-workspace-enablement`
> **Companion file:** `docs/plans/livedev-progress.json` (machine-readable handoff)
> **Captured at (UTC):** 2026-07-04T09:20:00Z

This document is a **map and contract** for the LiveDev track. It does not
implement LiveDev. It does not change release status. It does not modify
`optid`. It does not delete or deprecate `testOS`. It does not fabricate
evidence. Every path it cites was verified to exist on disk at the source
commit; nothing is invented.

The goal of this PR is to give the next agent (or the maintainer) a clean
handoff: a single document that explains what is in the repo today, what is
reusable for LiveDev, what is missing, and what is safety-sensitive — and a
JSON sidecar that a future `tools/validate-livedev.py` can read.

---

## 1. Repo map (verified, not invented)

The repository is Rush Linux — a source-built Arch-based distribution centered
on `optid`, a fast, explainable runtime optimizer. Current project version is
`0.7.0-beta.1` (Editions milestone; preceding `0.6.0-beta.1` is code-complete
but Phase D hardware-gated). The repo is **not** a consumer-installable distro
yet; what works today is the optimizer daemon, the boot/rollback/update-signing
chain, the mkosi image composition plane, the testOS USB benchmark
environment, and a measurement rig (`rushbench`).

The structure (file counts verified by `find <dir> -type f | wc -l`):

| Path | Files (≈) | Role |
|---|---|---|
| `crates/` | 76 | Rust workspace: `optid`, `optctl`, `rushbench`, `rush_collect`, `testos` (included); `rush_telemetry` (excluded) |
| `docs/` | 107 | Canon: SPEC-northstar, architecture, ADRs 0001–0017, research 0001–0020, strategy, plans, docmap |
| `release/` | 72 | Milestones, test tiers, evidence tree (`v0.3/v0.4/v0.5` transcripts, dragnet ledger, host-bench templates) |
| `tools/` | 45 | Build, install, test, validate, sign, benchmark, dragnet, session lifecycle |
| `.github/` | 25 | 13 workflows: ci, rust-clippy, dragnet, graphify, release-testos, reassess, etc. |
| `mkosi/` | 26 | mkosi.conf + 3 profiles (server/desktop/testos) + repart + extra |
| `benchmarks/` | 24 | manifest.toml + results/2026-06-14/fedora/ |
| `testos/` | 7 | README, bench-list.toml, build-testos.sh, install.sh, install.ps1, collect-results.{sh,ps1} |
| `distro/` | 17 | Kernel configs, edition descriptors, systemd units, sysupdate, nftables, UKI/boot fragments |
| `recipes/` | 7 | core/boot/desktop/server recipe skeletons |
| `packaging/` | 7 | D-Bus policy, systemd units, udev rule |
| `config/` | 4 | `optid/policy.toml`, `optid/contracts.toml`, `keys/testing.public.pem`, `keys/.gitkeep` |

Root-level canon files (verified present): `README.md`, `VERSION` (=
`0.7.0-beta.1`), `Cargo.toml` (workspace), `Cargo.lock`, `ROADMAP.md`,
`RELEASES.md`, `AGENTS.md`, `CLAUDE.md`, `HANDOFF.md`, `HANDOFF-2026-06-26.md`,
`CONTRIBUTING.md`, `SECURITY.md`, `SUPPORT.md`, `LICENSE` (Apache-2.0),
`AUTHORS`, `CODE_OF_CONDUCT.md`, `book.toml`, `deny.toml`.

`docs/AI_CONTINUATION.md` is the canonical orientation runbook for any agent
landing on this repo. `docs/agent-protocol.md` defines the Builder / Verifier /
Human role split and the Evidence Rule. `docs/SPEC-northstar.md` is the
single-objective canon — every other doc is derived from it.

The `crates/optid/` module map (line counts verified by `wc -l`):

```
main.rs         343   — event loop, signal hooks, flock, D-Bus wiring
policy.rs       943   — Policy::load, Policy::decide, auto_mode, hysteresis
actuator.rs     728   — guarded actuation (EPP, platform_profile, cgroups)
tests.rs       1931   — fixture tests (PSI, thermal, battery, workload classifier)
allowlist.rs    536   — hardware allowlist (WP-N4: seeded baseline + override dirs)
sensors.rs      453   — /proc and /sys readers (non-blocking)
io_util.rs      413   — guarded_write, atomic_write_state_file, path-traversal block
workload.rs     356   — 5-class classifier pure function (idle/light/interactive/latency-critical/throughput)
args.rs         260   — CLI; --allowlist defaults to true since v0.6 Phase A3
contracts.rs    250   — PM QoS latency-budget invariants
action.rs       228   — Action type
shim/ppd.rs    1054   — net.hadess.PowerProfiles D-Bus shim
shim/gamemode.rs 645  — com.feralinteractive.GameMode D-Bus shim
shim/conflict.rs 219  — competing-daemon detection (downgrades --apply to dry-run)
foreground/mod.rs 141 — foreground detection stub
actuators/display.rs     115 — display/backlight actuator
actuators/runtime_pm.rs  143 — per-device runtime PM actuator
actuators/storage.rs      67 — NVMe/APST/ASPM/ALPM actuator
```

Tests for the shim surface live alongside the crate as
`crates/optid/tests/{shim_ppd,shim_gamemode,shim_foreground,write_site_gating}.rs`
— `write_site_gating.rs` is the lexical drift-detection gate that counts
occurrences of `guarded_write(`, `pmqos_sink.write_*(`,
`atomic_write_state_file(`, and `Command::new("systemctl")` and fails
mechanically if a new write site is added without classification.

---

## 2. Existing assets to reuse

LiveDev is a fresh track; nothing under `crates/livedev*` exists yet. The
intent is to compose LiveDev from existing primitives rather than fork them.

### 2.1 Image build plane

- `mkosi/mkosi.conf` + `mkosi/mkosi.profiles/{server,desktop,testos}/mkosi.conf`
  — declarative image composition (ADR 0014). A future LiveDev edition should
  add `mkosi/mkosi.profiles/livedev/mkosi.conf` rather than a custom script.
- `tools/build-mkosi-image.sh --edition <server|desktop|testos>` — edition-aware
  image builder. LiveDev should add `--edition livedev` here, not in a new
  script.
- `tools/build-vm-unpriv.sh` — unprivileged VM image builder using
  `mkfs.ext4 -d` (no root, no loop device). Works around sandboxed environments;
  usable as a LiveDev CI fallback if mkosi sandbox is unavailable.
- `mkosi/mkosi.repart/` — declarative GPT partition layout (`00-ESP.conf`,
  `10-root.conf`). LiveDev disk layout should extend this, not replace it.

### 2.2 Boot and updates

- `distro/boot/uki.toml` + `distro/boot/cmdline.d/adaptive.conf` — UKI-first
  boot policy. LiveDev should reuse the UKI path; do not introduce a parallel
  boot scheme.
- `distro/sysupdate/{base,uki}.conf` — systemd-sysupdate descriptors for A/B
  updates. LiveDev updates should flow through the same descriptors.
- `tools/manage-boot-entries.sh` — boot-entry rotation/retention
  (`InstancesMax=3`).
- `tools/test-rollback.sh`, `tools/test-double-boot.sh`,
  `tools/test-sign-updates.sh` — verified boot/rollback/update-signing
  harnesses. LiveDev should reuse these for its own boot tests.
- `tools/validate-uefi-boot.sh` — OVMF+QEMU UEFI boot validator.
- `config/keys/testing.public.pem` — Ed25519 test verification key (private key
  gitignored). LiveDev update signing must reuse `tools/sign-updates.sh` /
  `tools/sign_updates.py` and the existing Ed25519 model.

### 2.3 Runtime optimization (the `optid` daemon)

- `crates/optid` — adaptive optimizer. LiveDev surfaces that need to observe
  or steer runtime policy should consume `optid` state via the existing
  `/run/optid/*` files and the `io.rushlinux.Optid1` D-Bus interface, not by
  re-implementing sensors or policy.
- `crates/optid/src/shim/{ppd,gamemode,conflict}.rs` — D-Bus compatibility
  shims. LiveDev must not register conflicting well-known names; if a new
  shim is needed, add it under `crates/optid/src/shim/` and follow the same
  registry/cookie model.
- `crates/optid/src/actuators/{display,runtime_pm,storage}.rs` — domain
  actuators. LiveDev features that touch these domains should extend the
  existing actuator, not create a parallel one.
- `crates/optid/src/io_util.rs` — `guarded_write`,
  `atomic_write_state_file`, path-traversal rejection. **Any new LiveDev
  write site must route through these primitives** (ADR 0009; audit #16).
- `crates/optctl` — D-Bus + file-based control client (status, explain, mode,
  pin, allow, deny, benchmark). LiveDev user-facing CLI work should extend
  `optctl`, not ship a separate binary.

### 2.4 Measurement and evidence

- `crates/rushbench` — measurement rig (energy via BAT/intel-rapl,
  responsiveness probes, report generation). LiveDev benchmarks should call
  `rushbench`, not re-invent energy measurement.
- `crates/rush_collect` — passive hardware profile + metric snapshot collector.
- `crates/testos` + `testos/` — bootable USB benchmark environment
  (`testos-launcher`, `testos-runner`, `testos-ingest`). Already wired into
  `.github/workflows/release-testos.yml`. **LiveDev must not delete, deprecate,
  rename, or shadow testOS**; any overlap (e.g., shared USB boot path) must be
  additive.
- `testos/bench-list.toml` — benchmark catalog re-usable for any future harness.
- `release/evidence/BUILD-HOST-RUNBOOK.md` — canonical runbook for build-host
  acceptance transcripts. LiveDev evidence work inherits this protocol.
- `release/evidence/host-bench/_TEMPLATE/` — Phase D transcript template.
- `tools/dragnet.py` + `tools/validate-evidence.py` — evidence-integrity gate.
- `benchmarks/manifest.toml` — 5-scenario benchmark manifest (mixed-load,
  server, battery, gaming, realtime audio).

### 2.5 Release governance

- `release/milestones.toml` — canonical milestone/criteria state machine.
  LiveDev must not modify release status fields without a maintainer signoff.
- `release/test-tiers.toml` — T0..T5 tier definitions and `required_for`
  channel mapping.
- `docs/agent-protocol.md` — Builder / Verifier / Human role separation and
  authority matrix.
- `docs/dragnet-protocol.md` — evidence integrity protocol.
- `tools/start-work.sh` + `tools/finish-work.sh` — session lifecycle (creates
  and removes `DIRTY_STATE.md`, runs validators). Every LiveDev work session
  must use these.
- `tools/validate-{repo,versions,doc-sync,evidence,dirty-state}.py` / `.ps1` —
  repo policy validators.
- `docs/docmap.toml` — single source of truth for doc ownership and code
  coverage. Any new LiveDev doc must add an entry here.

### 2.6 Config surface

- `config/optid/policy.toml` — optid policy (modes, shims,
  competing-daemon list). LiveDev policy additions go here, not in a new file.
- `config/optid/contracts.toml` — per-workload-class PM QoS latency budgets.
  LiveDev features that schedule work must respect this table.
- `crates/optid/data/allowlist.toml` — seeded hardware allowlist baseline
  (default-deny with override dirs). LiveDev hardware writes must respect the
  allowlist; the `--no-allowlist` flag is the emergency escape hatch only.
- `distro/editions/{server,desktop,laptop,realtime-audio}.toml` — edition
  descriptors. A future LiveDev edition should add
  `distro/editions/livedev.toml`.

---

## 3. Missing components

### 3.1 LiveDev-specific (none exist yet — this is a fresh track)

- No `crates/livedev*` crate. LiveDev is currently a phase name, not a code
  artifact. The next phase (`architecture-contract`) must draft the ADR before
  any code lands.
- No LiveDev ADR. `docs/decisions/` contains 0001–0017; none are LiveDev-scoped.
- No LiveDev research note. `docs/research/` contains 0001–0020; no LiveDev
  design note exists.
- No `docs/docmap.toml` entry for any LiveDev doc.
- No `docs/SUMMARY.md` entry for LiveDev.
- No LiveDev section in `ROADMAP.md`, `RELEASES.md`, or
  `docs/IMPLEMENTATION_STATUS.md`.
- No LiveDev plan file existed before this PR. The only LiveDev artifacts in
  the repo after this PR are `docs/plans/livedev-progress.json` and this file.

### 3.2 Toolchain gaps in this workspace container

The container this PR was prepared in does **not** have the build toolchain
installed. This is expected: CI runs everything on `ubuntu-latest` via
`dtolnay/rust-toolchain@stable` and apt-installs `libdbus-1-dev` +
`pkg-config`. The build-host runbook (`release/evidence/BUILD-HOST-RUNBOOK.md`)
documents the path for hardware-gated steps. The absent tools are:

- `rustc` / `cargo` — not installed locally; CI provides stable Rust + clippy +
  rustfmt via `dtolnay/rust-toolchain`. `cargo fmt --all -- --check`, `cargo
  nextest run --workspace`, and `cargo clippy --workspace --all-targets -- -D
  warnings` cannot be run in this container.
- `shellcheck` — not installed; audit #14 flags this as a project-hygiene gap.
- `gh` (GitHub CLI) — not installed; no PR creation or release-upload from
  this container.
- `mkosi` — not installed; image builds run on the build host.
- `qemu-system-x86_64` — not installed; UEFI boot validation runs on the build
  host.
- `libdbus-1-dev` / `pkg-config` — presence not verified; needed for
  `optid` / `optctl` zbus builds.

### 3.3 Known project-level gaps referenced in the repo (not introduced by LiveDev)

These are pre-existing gaps documented in the repo's own audits and plans.
LiveDev should be aware of them but must not try to fix them out of scope.

- **v0.6.0-beta.1 Phase D:** no physical-hardware benchmark transcripts yet
  (`release/evidence/host-bench/_TEMPLATE/` only). Hardware-gated, requires
  project-owner hands per `docs/strategy/reference-hardware.md`.
- **Reference hardware slots empty:** the Desktop and Laptop nomination slots
  in `docs/strategy/reference-hardware.md` are both unfilled.
- **`rush_telemetry` crate excluded** from the workspace; ADR 0017 is
  `proposed` (not ratified). Re-inclusion is a pending decision (Option A/B/C/D
  in the ADR).
- **`tools/build-vm-unpriv.sh` still broken at boot** (kmod/libkmod2 missing
  from rootfs) per `HANDOFF.md` and audit #7.
- **`tools/finish-work.sh` diverges from CI** per audit #15 — can pass locally
  while CI fails. LiveDev changes that depend on `finish-work.sh` should also
  be cross-checked against `.github/workflows/ci.yml`.
- **Foreground detection is a documented stub**; v0.7 gate undefined per audit
  #20.
- **Action value types still stringly-typed** per audit #19.
- **PSI/proc parsing duplicated 4× across crates** per audit #17 — no
  `rush_core` pattern yet.

---

## 4. Safety-sensitive areas

LiveDev work must respect these load-bearing invariants. Relaxing any of them
requires a Ratified-by ADR (per `docs/decisions/README.md`).

1. **`crates/optid/src/io_util.rs::guarded_write`** — path-traversal rejection
   (M1) and allowlist enforcement (M1 / WP-N4) are load-bearing. Any LiveDev
   surface that writes to sysfs/cgroup MUST route through `guarded_write` or a
   sibling primitive; introducing parallel write paths bypasses the security
   boundary (ADR 0009).

2. **`crates/optid/src/allowlist.rs` + `crates/optid/data/allowlist.toml` +
   `crates/optid/build.rs`** — hardware allowlist is default-enabled since
   v0.6 Phase A3 (`Args::allowlist: true`). LiveDev features that touch
   hardware MUST respect `--no-allowlist` as the emergency escape hatch and
   MUST NOT add writes outside the seeded baseline.

3. **`crates/optid/src/actuator.rs` + `crates/optid/src/io_util.rs`** — 29
   enumerated write sites (`crates/optid/tests/write_site_gating.rs`).
   Drift-detection assertions count occurrences of `guarded_write(`,
   `pmqos_sink.write_*(`, `atomic_write_state_file(`, and
   `Command::new("systemctl")`. Adding a new write site without classification
   breaks the lexical gate (audit #16).

4. **`crates/optid/src/main.rs` (signal hooks + flock + revert paths)** —
   `SIGTERM`/`SIGINT`/`SIGHUP` hooks (H2) and `optid.lock` exclusive flock (M4)
   guarantee deterministic reversion of sysctls and PM QoS floors on systemd
   stop. LiveDev surfaces that affect the daemon lifecycle must not weaken
   these.

5. **`crates/optid/src/policy.rs::Policy::load` +
   `config/optid/policy.toml`** — audit #1 (Critical): `Policy::load` fails
   open on parse error. LiveDev config additions must not relax fail-closed
   semantics; new sections must be optional with documented defaults.

6. **`crates/optid/src/shim/{ppd,gamemode,conflict}.rs` + D-Bus name
   ownership** — optid owns `net.hadess.PowerProfiles` and
   `com.feralinteractive.GameMode` bus names. LiveDev surfaces must not
   register conflicting well-known names or duplicate the cookie/registry
   semantics.

7. **`crates/optid/src/contracts.rs` + `config/optid/contracts.toml`** — PM
   QoS latency budgets are provisional pending WP-B1 validation. LiveDev
   features that schedule work or claim latency budgets must respect the
   contracts table or document an explicit override with a Ratified-by ADR.

8. **`release/evidence/*` and `release/milestones.toml`** — Evidence Rule: no
   claim without a committed transcript. LiveDev work must not mark any
   criterion verified without a real command transcript; must not bump release
   status fields; must not touch the Dragnet ledger except via
   `tools/dragnet.py`.

9. **`config/keys/` and `tools/sign-updates.sh`** — test signing keys are
   gitignored (private never committed). LiveDev update flows must reuse
   `tools/sign-updates.sh` / `tools/sign_updates.py` and the existing Ed25519
   key model; must not invent parallel signing paths.

10. **`testos/` and `crates/testos/`** — testOS is the canonical
    real-hardware benchmark environment. LiveDev must not delete, deprecate,
    rename, or shadow testOS. Any overlap (e.g., shared USB boot path) must be
    additive.

11. **`crates/rush_telemetry` (excluded)** — GPL-2.0-only inside an
    Apache-2.0 workspace; ADR 0017 is `proposed` (not ratified). LiveDev must
    not silently depend on `rush_telemetry` until ADR 0017 is ratified
    (Option A/B/C/D).

12. **`tools/finish-work.sh` vs `.github/workflows/ci.yml`** — audit #15:
    `finish-work.sh` diverges from CI; can pass locally while CI fails. LiveDev
    changes that depend on `finish-work.sh` should also be cross-checked
    against the CI workflow's actual commands (`cargo nextest run --workspace`
    rather than `cargo test --workspace`, etc.).

---

## 5. Existing testOS role

`testos/README.md` is explicit about what testOS is and is not. Verbatim from
that file:

> testOS is a temporary, self-contained Linux environment that boots from a USB
> stick, runs the Rush Linux benchmark suite on real hardware, and writes the
> results back to the USB. After it finishes, you reboot back into the host OS
> and pull the results into the repo.
>
> It exists because Rush Linux's benchmark manifest declares 5 scenarios
> (mixed-load, server throughput, laptop battery, gaming, realtime audio) but
> the project is currently blocked on **Phase D** — no real-hardware benchmark
> workflow exists. testOS is that workflow.

What testOS **is**:

- A single bootable USB image (`.raw`, ~500MB) built from the same `mkosi`
  config that builds Rush Linux itself.
- Contains the Rush Linux v0.5 server skeleton, the `optid` daemon, the
  `rushbench` measurement rig, plus benchmark tools (fio, iperf3, postgres,
  nginx, ab, cyclictest, jq).
- A Rust binary `testos-runner` that boots on tty1, shows a menu, runs the
  selected benchmarks, writes results to the USB, and reboots. Companion
  binaries `testos-launcher` (workstation side: build + write) and
  `testos-ingest` (workstation side: pull + format + commit) round out the
  flow.

What testOS **is not**:

- Not a new operating system — it is a thin wrapper around the existing Rush
  Linux image.
- Not a permanent install — nothing is written to the host machine's disk.
- Not a substitute for the `rushbench` crate — testOS calls `rushbench` for
  measurements it already knows how to make; testOS handles the
  boot/menu/results/ingest workflow that `rushbench` doesn't.

**LiveDev's relationship to testOS:** LiveDev must not delete, deprecate,
rename, or shadow testOS. testOS is the established path for real-hardware
benchmark evidence; any LiveDev work that touches hardware benchmarks must
either reuse testOS as-is or extend it additively (e.g., a new bench-list
entry, a new ingest format). A LiveDev edition is not a replacement for
testOS; the two serve different purposes (LiveDev = ongoing live
optimization, testOS = one-shot benchmark campaign).

The release pipeline is already wired:
`.github/workflows/release-testos.yml` builds the testOS `.raw` and the
`testos-launcher` / `testos-ingest` binaries and uploads them as GitHub
Release assets on every `v*` tag. `testos/install.sh` (Linux) and
`testos/install.ps1` (Windows) are the documented user-facing installers; the
README's "Try it on real hardware" section points users at them.

---

## 6. Existing evidence / release / version state

### 6.1 Version state (untouched by this PR)

- `VERSION` = `0.7.0-beta.1`
- `Cargo.toml` `[workspace.package] version` = `0.7.0-beta.1`
- `mkosi/mkosi.extra/etc/os-release` `VERSION` = `0.7.0-beta.1`
- `release/milestones.toml` `[project] current_version` = `0.7.0-beta.1`

No version field was modified by this PR.

### 6.2 Release ledger (per `RELEASES.md`)

| Version | Channel | Status |
|---|---|---|
| `0.1.0-alpha.0` | `unstable` | complete |
| `0.1.0-alpha.1` | `alpha` | complete |
| `0.2.0-alpha.1` | `alpha` | complete |
| `0.3.0-alpha.1` | `alpha` | complete (PR #174) |
| `0.4.0-alpha.1` | `alpha` | complete (PR #174) |
| `0.5.0-beta.1` | `beta` | complete (PR #174) |
| `0.6.0-beta.1` | `beta` | code-complete; certification pending Phase D |
| `0.7.0-beta.1` | `beta` | in progress (Editions) |
| `0.8.0-beta.1` | `beta` | planned (Benchmark Lab) |
| `0.9.0-rc.1` | `rc` | planned (RC Hardening) |
| `1.0.0` | `stable` | planned (Final Stable) |

This PR does not change any of these statuses.

### 6.3 v0.6.0-beta.1 specific state (code-complete, certification pending)

The `0.6.0-beta.1` milestone in `release/milestones.toml` carries four exit
criteria. Two are code-complete (PRs #183–#186 merged): "unsupported knobs are
skipped with reasons" and "no unsafe write occurs outside allowlisted paths".
Two are **hardware-gated and unverified**: "mixed-load responsiveness improves
on two machines" and "battery behavior matches or improves mainstream
defaults". All four `verified` flags for v0.6 are `false` in
`release/milestones.toml`. The host-bench evidence directories under
`release/evidence/host-bench/` are templates only. Phase D is the hard gate;
`docs/strategy/reference-hardware.md` shows both nomination slots (Desktop +
Laptop) unfilled.

### 6.4 Evidence rule (non-negotiable, per `docs/agent-protocol.md`)

> An exit-criterion checkmark may **only** appear next to an **embedded
> command transcript**: the literal command, literal output (or attached log
> file), date, and host description. "The script implements X" is a
> description, not evidence. `bash -n` is a syntax check, not a test run.

This PR adds **no new evidence**. It adds no `verified = true` flags. It adds
no transcript files. It does not modify `release/milestones.toml`,
`release/test-tiers.toml`, or anything under `release/evidence/`.

### 6.5 Dragnet state

`release/evidence/dragnet/LEDGER.md` is the audit history. `tools/dragnet.py
--observe` is the entry point any agent must run before relying on a
"verified" claim. This PR does not touch the Dragnet ledger and does not run
`dragnet.py` (it adds no claims that would need auditing).

---

## 7. Tool availability table

Captured at branch creation time in this workspace container. CI commands
reference `.github/workflows/ci.yml`.

| Tool | Local? | Version (local) | CI source |
|---|---|---|---|
| `git` | yes | 2.47.3 | `actions/checkout@v4` |
| `python3` | yes | 3.12.13 | `ubuntu-latest` default |
| `pytest` | yes | 9.0.2 | (not used in CI; project uses `tools/*.py` directly) |
| `jq` | yes | 1.7 | (not used in CI) |
| `rustc` | **no** | — | `dtolnay/rust-toolchain@stable` |
| `cargo` | **no** | — | `dtolnay/rust-toolchain@stable` |
| `cargo nextest` | **no** | — | `taiki-e/install-action@nextest` |
| `cargo audit` | **no** | — | `rustsec/audit-check@v2.0.0` |
| `cargo deny` | **no** | — | `embarkstudios/cargo-deny-action@v2` |
| `libdbus-1-dev` | unverified | — | `apt-get install -y libdbus-1-dev pkg-config` |
| `pkg-config` | unverified | — | `apt-get install -y libdbus-1-dev pkg-config` |
| `pwsh` (PowerShell) | no | — | `ubuntu-latest` default (used for `tools/validate-repo.ps1`) |
| `lychee` | no | — | `lycheeverse/lychee-action@v2` |
| `shellcheck` | **no** | — | (not installed in CI either; audit #14) |
| `gh` (GitHub CLI) | **no** | — | (not used in CI; releases via `release-drafter.yml` + `release-testos.yml`) |
| `mkosi` | **no** | — | `archlinux:latest` container in `release-testos.yml` |
| `qemu-system-x86_64` | **no** | — | (build-host only, per `release/evidence/BUILD-HOST-RUNBOOK.md`) |
| `graphify` | no | — | (Copilot/Codex only; see `AGENTS.md`) |

**Implication:** No LiveDev code change can be locally validated in this
container beyond `python3`/`jq`/`git`. Any Rust change must be validated in
CI (or on a build host with Rust installed). This is why this PR is
docs-only: it produces no Rust code, no shell script, no Python validator —
only the two markdown/JSON handoff files under `docs/plans/`.

---

## 8. Proposed build sequence (for the next phase, not this PR)

This sequence is **indicative**, not binding. It is recorded here so the next
agent has a starting point. Each step should be its own PR; each step must run
`tools/start-work.sh` before and `tools/finish-work.sh` after, and must be
cross-checked against `.github/workflows/ci.yml` (audit #15).

1. **`architecture-contract` (next phase, no code):** draft an ADR scoping
   LiveDev — its single-owner boundary, the surfaces it owns vs. reuses from
   `optid` / `testOS` / `rushbench`, the evidence rule it inherits, and the
   ADR-0009 write-site discipline it must follow. Do NOT write code in this
   phase. Exit condition: the ADR is `proposed` with acceptance criteria and
   a `docs/docmap.toml` entry exists for it.

2. **Design note:** add `docs/research/00NN-livedev-design.md` filling the
   SPEC §4.1 telemetry gaps LiveDev will need (compositor state, foreground
   app, etc.). Mark every row `[PROVEN]` or `[HYPOTHESIS]` per the 0018
   convention.

3. **Ratification:** maintainer moves the LiveDev ADR from `proposed` to
   `accepted` with a `Ratified-by:` line (per `docs/decisions/README.md`).
   Agent-authored ADRs 0008+ cannot self-ratify.

4. **Wiring-only PR:** add the LiveDev crate stub to the workspace
   (`Cargo.toml` `members`), add a `docs/docmap.toml` entry, add a
   `docs/SUMMARY.md` entry, add a `ROADMAP.md` line. No behavior; `cargo check
   --workspace` must stay green.

5. **Minimal implementation:** a single read-only LiveDev surface (e.g., a
   status reporter) that consumes existing `optid` state via `/run/optid/*` —
   no new write sites, no new D-Bus names, no new sysfs paths.

6. **Evidence scaffolding:** add `release/evidence/livedev/_TEMPLATE/`
   mirroring `release/evidence/host-bench/_TEMPLATE/`; do NOT mark any
   criterion verified.

7. **Incremental hardening:** only after steps 1–6 are merged and CI is
   green, add actuation paths — each new write site must be classified in
   `crates/optid/tests/write_site_gating.rs` and routed through
   `guarded_write`.

8. **Cross-check:** every step must run `tools/start-work.sh` before and
   `tools/finish-work.sh` after, and must be cross-checked against
   `.github/workflows/ci.yml` because `finish-work.sh` diverges from CI
   (audit #15).

---

## 9. What this PR does NOT do (explicit non-goals)

- Does not change release status. `release/milestones.toml`,
  `release/test-tiers.toml`, `RELEASES.md`, `VERSION`, `Cargo.toml`, and
  `mkosi/mkosi.extra/etc/os-release` are untouched.
- Does not implement LiveDev. No `crates/livedev*` is created.
- Does not modify `optid`. No file under `crates/optid/` or `config/optid/` is
  touched.
- Does not fabricate evidence. No `verified = true` flag is added. No
  transcript file is added or modified.
- Does not invent paths. Every path cited in this document and in
  `livedev-progress.json` was verified to exist on disk at the source commit.
- Does not delete or deprecate `testOS`. `testos/` and `crates/testos/` are
  untouched.
- Does not run `tools/start-work.sh` / `tools/finish-work.sh`. Those scripts
  pull from `origin` and run the full validator suite (which requires a Rust
  toolchain this container does not have). The branch is created directly from
  the cloned `main` and the two plan files are added as a single commit.

---

## 10. Handoff to the next phase

The next phase is `architecture-contract`. Its entry condition is: **this PR
merged to `main`**. Its exit condition is: **a LiveDev ADR exists, is
`proposed`, has acceptance criteria, and has a `docs/docmap.toml` entry. No
code yet.**

The machine-readable handoff is `docs/plans/livedev-progress.json`. Its
`next_phase` field is the canonical value the next agent should read:

```json
{
  "current_phase": "workspace-enablement",
  "next_phase": "architecture-contract"
}
```

When the next phase completes, the next agent should update
`docs/plans/livedev-progress.json`: set `current_phase` to
`architecture-contract`, set `next_phase` to the following phase (likely
`design-note` or `ratification`), and record the new `source_commit`. They
should NOT delete this document; it is the historical record of how the
LiveDev track was scoped.
