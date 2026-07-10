<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/banner-dark.svg">
    <img src="docs/assets/banner-light.svg" alt="Rush Linux — adaptive power and performance for Arch" width="720">
  </picture>
</p>

<p align="center">
  <em>adaptive power &amp; performance for Arch</em>
</p>

<p align="center">
  <a href="https://github.com/Nan0pk/Rush-linux/actions"><img src="https://img.shields.io/github/actions/workflow/status/Nan0pk/Rush-linux/ci.yml?branch=main&style=flat-square" alt="CI"></a>
  <a href="https://github.com/Nan0pk/Rush-linux/releases"><img src="https://img.shields.io/github/v/release/Nan0pk/Rush-linux?include_prereleases&style=flat-square" alt="release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square" alt="license"></a>
  <a href="VERSION"><img src="https://img.shields.io/badge/version-0.7.0--beta.1-blue?style=flat-square" alt="version"></a>
</p>

<hr>

**Rush Linux is an Arch-based distribution that watches what you're doing and tunes the hardware to match.** Fast CPU when you're compiling. Tight latency when you're on a video call. Idle states when you're reading. No manual power profiles to flip between — every decision is logged, explained, and reversible.

It's early beta. The optimizer (`optid`) runs in safe dry-run mode, the boot path is verified end-to-end (UKI + systemd-boot + signed rollback), and a measurement harness (`rushbench`) is operational. The desktop and laptop editions are not yet buildable; a consumer-installable distribution is the long-term goal. What works today is real, verified, and committed.

---

## Rush LiveDev — one command

One command does everything. Paste this into a terminal:

**Linux/macOS:**

```sh
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.sh -o livedev-bootstrap.sh && bash livedev-bootstrap.sh
```

**Windows PowerShell:**

```powershell
curl.exe -L -o livedev-bootstrap.ps1 https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.ps1; powershell -ExecutionPolicy Bypass -File .\livedev-bootstrap.ps1
```

That's it. The script asks you what to do (or auto-picks if non-interactive):

```
  What would you like to do?

  [1] resume — Copy results from USB, validate, submit evidence PR
  [2] vm     — Run deterministic QEMU test cycle (no USB, no reboot)
  [3] usb    — Prepare a USB via testOS (for real-hardware testing)

  Pick [1-3] (or press Enter for default 1):
```

Options appear based on what's available: USB with results shows `resume`, QEMU installed shows `vm`, `usb` is always available. Non-interactive runs (CI, piped stdin) auto-pick.

You only approve: USB erase, boot from USB, physical AC/battery prompts, and GitHub auth. The script never auto-merges, never marks milestones verified, never edits release truth.

### GitHub auth (for `--submit`)

To open an evidence PR, the script needs GitHub auth. Three ways, best first:

1. **`gh` CLI (recommended, no token pasted):** install `gh` and run `gh auth login` once. After that, every `--submit` just works.
2. **Interactive prompt:** run `bash livedev-bootstrap.sh --resume --submit` and paste a token when prompted (not echoed, not stored).
3. **Env var:** `export GH_TOKEN=...` (typed in your terminal, not pasted from chat).

The script checks auth **before** doing any USB/copy/validate work, so you won't waste 30 seconds only to fail at the last step.

<details>
<summary><strong>What the PR looks like</strong> (click to expand)</summary>

The submission generates a rich PR body automatically:

- **Pass/fail badge** (green/red/yellow shield)
- **Summary table** (passed / failed / skipped counts)
- **Host table** (fingerprint, kernel, CPU, board, battery)
- **Per-benchmark results table** (bench id, status, value, unit, error)
- **Artifact bundle** (tar.gz with all JSON + system logs)
- **Validation checklist** (manifest parses, fingerprint present, results present)
- **Auto-labels**: `evidence`, `livedev`, `result-pass` / `result-fail` / `result-mixed`
- **Dedup**: if you re-run with the same host + date, it updates the existing PR instead of creating a duplicate
- **Deterministic branch**: `evidence/<date>/<host-fingerprint>`

Failed benchmarks are preserved as evidence (never deleted). No auto-merge — a maintainer reviews and merges.

</details>

<details>
<summary><strong>What the one command does</strong> (situation → action)</summary>

| situation | action |
|---|---|
| USB with `testos-results/` plugged in | resume: mount USB read-only, copy results, validate manifest, submit evidence PR (needs `GH_TOKEN` in env) |
| `qemu-system-x86_64` installed, no USB results | build livedev image (if missing, needs sudo), run `--run-vm` with deterministic marker-driven state machine, collect artifacts, submit |
| No QEMU, no USB results | prepare USB via testOS, print boot instructions; after reboot, re-run same command |

</details>

<details>
<summary><strong>Forcing a specific path</strong> (optional flags)</summary>

```sh
bash livedev-bootstrap.sh --vm        # force QEMU/--run-vm path
bash livedev-bootstrap.sh --auto      # force USB/testOS prepare path
bash livedev-bootstrap.sh --resume    # force resume path
bash livedev-bootstrap.sh --resume --submit  # resume + open real PR (needs GH_TOKEN)
bash livedev-bootstrap.sh --dry-run   # show what would run
```

</details>

Full runbook: [`docs/livedev/OPERATOR_RUNBOOK.md`](docs/livedev/OPERATOR_RUNBOOK.md)

---

## testOS — USB boot backend / manual fallback

testOS is the bootable USB image that `livedev-bootstrap.sh` and `livedev-bootstrap.ps1` invoke under the hood when preparing the USB for real-hardware testing. It is preserved as a manual fallback path for users who want to drive each step themselves. For QEMU-driven dev/CI testing, use `python3 tools/livedev-next --run-vm` with the LiveDev mkosi image instead.

> **Latest release: [v0.7.0-beta.2](https://github.com/Nan0pk/Rush-linux/releases/tag/v0.7.0-beta.2)**

<details>
<summary><strong>Linux</strong> — write the USB manually</summary>

```bash
wget https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh
less install.sh    # inspect it if you like
sudo bash install.sh /dev/sdX
```

Safety checks: refuses the host's root disk, refuses non-removable disks without `--force`, refuses mounted devices, warns if the target is much larger than the image, asks `yes` before writing.
</details>

<details>
<summary><strong>Windows</strong> — native PowerShell, no WSL</summary>

```powershell
curl.exe -L -o install.ps1 https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.ps1
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

If blocked: `Unblock-File .\install.ps1` first. The installer scans for USB disks automatically. Safety checks: refuses the system disk, refuses non-USB bus types without `-Force`, asks `yes` before writing.
</details>

<details>
<summary><strong>macOS</strong> — download-then-run</summary>

```bash
diskutil list
curl -fsSL -o install.sh https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh
sudo bash install.sh /dev/diskN
```

</details>

**After writing the USB:** plug it into the test machine, reboot, pick USB from the boot menu (disable Secure Boot if it refuses). testOS boots to a console menu — type `0` for all benchmarks, or pick specific numbers. Press Esc to abort early. Results are saved to the USB; testOS reboots back to the host OS when done.

**Pull results into the repo (manual fallback):** use `livedev-bootstrap.sh --resume` (or `.\livedev-bootstrap.ps1 -Resume`) which copies and validates the results, then opens an evidence PR for maintainer review. The manual collector scripts (`testos/collect-results.sh`, `testos/collect-results.ps1`) are also available. Full testOS docs: [`testos/README.md`](testos/README.md).

---

## How it works

`optid` polls sensors every 2 seconds, maps the current system state to one of five workload classes, looks up the PM QoS contract for that class, and either logs its intended actions (dry-run, the default) or applies them to the kernel. `optctl pin <app> <class>` lets applications claim a class directly. Default mode is always **dry-run**; kernel writes require explicit `--apply` on a supported host.

<details>
<summary><strong>Architecture diagram</strong> (how optid flows)</summary>

```
  +----------------- Sensors (every 2s) -----------------+
  |  /proc/pressure  /sys/power_supply  /sys/thermal     |
  +---------------------------+--------------------------+
                              |
                              v
  +------------- Classify into 5 classes ----------------+
  |  idle  light  interactive  latency-critical          |
  |  throughput                                           |
  +---------------------------+--------------------------+
                              |
                              v
  +----------- contracts.toml (PM QoS floors) -----------+
  |  per-class: governor, EPP, platform_profile,         |
  |  PM QoS CPU latency, cgroup slice weights            |
  +---------------------------+--------------------------+
                              |
                              v   (--apply to write)
  +----------------- Actuate (kernel knobs) -------------+
  |  energy_perf_preference  platform_profile            |
  |  /dev/cpu_dma_latency  cgroup slice weights         |
  +------------------------------------------------------+
```

</details>

<details>
<summary><strong>Workload class contracts</strong> (governor, EPP, latency floors)</summary>

| Workload class | CPU governor | EPP | Platform profile | PM QoS CPU latency | Use case |
|:---------------|:-------------|:----|:------------------|:-------------------|:---------|
| `idle` | powersave | power | low-power | 100 ms | Screen off, no foreground app |
| `light` | powersave | balance_performance | balanced | 50 ms | Reading, browsing, light editor |
| `interactive` | performance | performance | balanced | 1 ms | Typing, scrolling, UI interaction |
| `latency-critical` | performance | performance | performance | 10 µs | Video call, game, audio session |
| `throughput` | performance | performance | performance | 10 ms | Compile, render, batch job |

PM QoS CPU latency is the hard floor — the kernel will not let any CPU enter a C-state deeper than that floor allows. EPP is the hint the CPU scheduler uses to trade frequency vs. efficiency. Platform profile is the ACPI-level hint that drives fan curves, dGPU power, USB autosuspend, etc.

</details>

---

## What's built

- **`optid` daemon** — PSI + thermal + power-supply sensor polling, workload classification, PM QoS contract enforcement. Applies EPP, platform profile, and cgroup slices when run with `--apply`. Every decision is logged and explainable.
- **`optctl` CLI** — D-Bus client (`io.rushlinux.Optid1`): `status`, `explain`, `mode`, `pin`, plus the read-only `doctor` command for wakeup-source and runtime-PM diagnosis. Machine-readable output via `--json`.
- **`rushbench` harness** — measures battery drain (`energy_now` or RAPL) and latency (PSI avg10, cyclictest, foreground launch) per workload class.
- **Rush LiveDev** — automation foundation: planner, runner, capture, evidence validator, AI harness, PR submission. See [`docs/livedev/OPERATOR_RUNBOOK.md`](docs/livedev/OPERATOR_RUNBOOK.md).
- **testOS** — bootable USB image for real-hardware benchmarking. See [testOS README](testos/README.md).
- **Bootable VM** — Arch-based rootfs boots to `multi-user.target` via UKI through OVMF/systemd-boot with `optid.service` active. Verified.
- **Installable system** — `tools/rush-install.sh` stamps the mkosi-built image onto a blank disk via `systemd-repart`; installed system boots twice cleanly with `optid.service` active.
- **Rollback + signing** — systemd-sysupdate descriptors, ≥3 retained boot entries, boot assessment service, Ed25519 update metadata signing. Bad-kernel rollback verified.

<details>
<summary><strong>Technology choices</strong> (why each layer)</summary>

| Layer | Choice | Why |
|:------|:-------|:----|
| Optimizer | `optid` (Rust) | No GC pauses; direct sysfs access; auditable actuator path |
| Pressure sensing | PSI (`/proc/pressure`) | Kernel-native; quantifies actual CPU/IO/memory stall time |
| Latency enforcement | PM QoS (`/dev/cpu_dma_latency`) | Hard per-class latency floors, not soft hints |
| Image composition | mkosi + Arch Linux | Declarative, reproducible; no bespoke build scripts |
| Scheduling | sched_ext / scx_loader | BPF user-space scheduler; EEVDF as the verified fallback |
| Boot | UKI + systemd-boot | Atomic, signed, single-file boot entries |
| Updates | systemd-sysupdate | Structured, rollback-aware OTA |
| Firewall | nftables | Current kernel default; replaces iptables |
| Desktop | Wayland + PipeWire | Native compositor and audio stack *(planned, not yet built)* |

</details>

---

## Build from source

<details>
<summary><strong>Build commands</strong> (cargo + mkosi)</summary>

```bash
git clone https://github.com/Nan0pk/Rush-linux.git
cd Rush-linux
cargo build --workspace --release

# One sensor read + classify cycle (dry-run, exits after one pass)
./target/release/optid --once

# Check current status from the state directory
./target/release/optctl status

# Explain visible energy and sleep blockers without changing settings
./target/release/optctl doctor
./target/release/optctl doctor --json

# Build the testOS USB image from source (requires mkosi + archlinux-keyring;
# ~10 minutes the first time)
sudo bash testos/build-testos.sh

# Run the full test suite
cargo test --workspace
```

Or open in VS Code Dev Containers or GitHub Codespaces — the checked-in [dev container](.devcontainer/devcontainer.json) provisions Rust stable, Python 3.11+, and PowerShell Core, then builds the workspace automatically.

</details>

---

## Documentation

- [Architecture](docs/architecture.md) — how `optid`, `optctl`, and the systemd units fit together
- [Boot and updates](docs/boot-and-updates.md) — UKI, systemd-boot, signed rollback
- [Adaptive engine](docs/adaptive-engine.md) — workload classification, PM QoS contracts
- [LiveDev operator runbook](docs/livedev/OPERATOR_RUNBOOK.md) — how to run benchmarks, capture evidence, submit PRs
- [LiveDev developer guide](docs/livedev-developer-guide.md) — architecture boundaries, tool roles, data flow
- [Benchmark methodology](docs/decisions/0011-benchmark-methodology.md) — how claims are measured
- [Testing strategy](docs/testing-and-benchmarks.md) — release gates and tiers
- [testOS README](testos/README.md) — full design rationale for the USB benchmark environment
- [Roadmap](ROADMAP.md) — where the project is going
- [All docs](docs/SUMMARY.md)

---

## Status

| Milestone | State |
|:----------|:------|
| Phase 0 — repo, ADRs, CI | ✅ complete |
| v0.1 — compile-clean core, `optid --once` | ✅ complete |
| v0.2 — D-Bus control plane, `optctl` | ✅ complete |
| v0.3 — rootfs builder, VM boots | ✅ complete |
| v0.4 — UKI boot, rollback, update signing | ✅ complete |
| v0.5 — minimal installable system (mkosi/Arch) | ✅ complete |
| **v0.6 — hardware-aware optid, PPD/GameMode shims** | ⚙ in progress |
| v0.7 — desktop / laptop / realtime / server editions | planned |
| v0.8 — benchmark lab, published results | planned |
| v0.9 — release candidate hardening | planned |
| v1.0 — installable, benchmarked, stable | planned |

[Full roadmap](ROADMAP.md)

### The evidence rule

Rush Linux enforces a **Builder/Verifier separation**: no claim of correctness or performance is accepted without a literal command transcript. `✅ Verified` means a human or CI ran the command and the output is on record. Claims without transcripts are proposals, not facts. This applies to benchmarks, boot verification, and optimizer behavior equally. See [the testing doc](docs/testing-and-benchmarks.md) and the [agent protocol](docs/agent-protocol.md).

---

<!-- RUSH_FRONTPAGE:START -->
<details>
<summary><strong>Repository reference</strong> (editions, workflows, services, docs, tests — click to expand)</summary>

<details>
<summary><strong>Editions</strong></summary>

Rush Linux is built from a single mkosi base plus per-edition
profiles. Available editions:

| edition | image id | config |
|---|---|---|
| `desktop` | `rush-linux-desktop` | `mkosi/mkosi.profiles/desktop/mkosi.conf` |
| `livedev` | `rush-linux-livedev` | `mkosi/mkosi.profiles/livedev/mkosi.conf` |
| `server` | `rush-linux-server` | `mkosi/mkosi.profiles/server/mkosi.conf` |
| `testos` | `rush-linux-testos` | `mkosi/mkosi.profiles/testos/mkosi.conf` |

</details>

<details>
<summary><strong>Rush LiveDev</strong></summary>

Deterministic hardware-test and benchmark-campaign workflow with
two operator paths: `--run-vm` (QEMU-driven, for CI/dev) and
`--auto`/`--resume` (USB-based, for real hardware).

```sh
python3 tools/livedev-next --help    # livedev-next
python3 tools/rush-autopilot --help    # rush-autopilot
python3 tools/build-mkosi-image.sh --help    # build-mkosi-image.sh
```

Additional tools:

| tool | path |
|---|---|
| `livedev-bootstrap.ps1` | `tools/livedev-bootstrap.ps1` |
| `livedev-bootstrap.sh` | `tools/livedev-bootstrap.sh` |
| `livedev-e2e-dry-run.py` | `tools/livedev-e2e-dry-run.py` |
| `rush-agent` | `tools/rush-agent` |
| `rush-builder.py` | `tools/rush-builder.py` |
| `rush-capture` | `tools/rush-capture` |
| `rush-exec` | `tools/rush-exec` |
| `rush-install.sh` | `tools/rush-install.sh` |
| `rush-livedev-autostart` | `tools/rush-livedev-autostart` |
| `rush-livedev-orchestrator` | `tools/rush-livedev-orchestrator` |
| `rush-livedev-runner` | `tools/rush-livedev-runner` |
| `rush-submit-evidence` | `tools/rush-submit-evidence` |

</details>

<details>
<summary><strong>CI workflows</strong></summary>

GitHub Actions workflows that run on PRs and on main:

| workflow | name | path |
|---|---|---|
| `ci.yml` | CI | `.github/workflows/ci.yml` |
| `docker-publish.yml` | Docker Image CI | `.github/workflows/docker-publish.yml` |
| `dragnet.yml` | Dragnet Evidence Tripwire | `.github/workflows/dragnet.yml` |
| `frontpage-sync.yml` | frontpage-sync | `.github/workflows/frontpage-sync.yml` |
| `graphify.yml` | Graphify knowledge graph | `.github/workflows/graphify.yml` |
| `labeler.yml` | Pull Request Labeler | `.github/workflows/labeler.yml` |
| `livedev-validate.yml` | LiveDev validate | `.github/workflows/livedev-validate.yml` |
| `pages.yml` | Deploy to GitHub Pages | `.github/workflows/pages.yml` |
| `reassess.yml` | Strategic Reassessment | `.github/workflows/reassess.yml` |
| `release-drafter.yml` | Release Drafter | `.github/workflows/release-drafter.yml` |
| `release-testos.yml` | Release testOS image | `.github/workflows/release-testos.yml` |
| `rust-clippy.yml` | rust-clippy analyze | `.github/workflows/rust-clippy.yml` |
| `stale.yml` | Close stale issues and PRs | `.github/workflows/stale.yml` |
| `validate-install-ps1.yml` | Validate install.ps1 | `.github/workflows/validate-install-ps1.yml` |

</details>

<details>
<summary><strong>Systemd services</strong></summary>

Services shipped in the image (optid is the adaptive optimizer;
rush-* are the LiveDev tools):

| unit | description | path |
|---|---|---|
| `optid-apply.service` | Rush Linux optimization daemon (apply mode) | `packaging/systemd/optid-apply.service` |
| `optid-boot-assess.service` | Rush Linux boot assessment marker | `packaging/systemd/optid-boot-assess.service` |
| `optid.service` | Rush Linux optimization daemon (dry-run) | `packaging/systemd/optid.service` |
| `rush-autopilot.service` | Rush LiveDev autopilot planner/runner | `packaging/systemd/rush-autopilot.service` |
| `rush-capture.service` | Rush LiveDev capture session manager | `packaging/systemd/rush-capture.service` |
| `rush-livedev-autostart.service` | Rush LiveDev autostart (safe countdown before autopilot) | `packaging/systemd/rush-livedev-autostart.service` |
| `rush-livedev-failure.service` | Rush LiveDev failure handler (fail-closed, no root prompt) | `packaging/systemd/rush-livedev-failure.service` |
| `rush-livedev-test.service` | Rush LiveDev post-reboot test runner | `packaging/systemd/rush-livedev-test.service` |

</details>

<details>
<summary><strong>Operator commands</strong></summary>

Single entrypoint for LiveDev operations:

```sh
python3 tools/livedev-next --help    # livedev-next
python3 tools/rush-autopilot --help    # rush-autopilot
python3 tools/build-mkosi-image.sh --help    # build-mkosi-image.sh
```

Additional tools:

| tool | path |
|---|---|
| `livedev-bootstrap.ps1` | `tools/livedev-bootstrap.ps1` |
| `livedev-bootstrap.sh` | `tools/livedev-bootstrap.sh` |
| `livedev-e2e-dry-run.py` | `tools/livedev-e2e-dry-run.py` |
| `rush-agent` | `tools/rush-agent` |
| `rush-builder.py` | `tools/rush-builder.py` |
| `rush-capture` | `tools/rush-capture` |
| `rush-exec` | `tools/rush-exec` |
| `rush-install.sh` | `tools/rush-install.sh` |
| `rush-livedev-autostart` | `tools/rush-livedev-autostart` |
| `rush-livedev-orchestrator` | `tools/rush-livedev-orchestrator` |
| `rush-livedev-runner` | `tools/rush-livedev-runner` |
| `rush-submit-evidence` | `tools/rush-submit-evidence` |

</details>

<details>
<summary><strong>Documentation</strong></summary>

Key docs:

| doc | description |
|---|---|
| [`docs/livedev/OPERATOR_RUNBOOK.md`](docs/livedev/OPERATOR_RUNBOOK.md) | LiveDev operator runbook |
| [`docs/livedev-developer-guide.md`](docs/livedev-developer-guide.md) | LiveDev developer guide |
| [`docs/editions/livedev.md`](docs/editions/livedev.md) | LiveDev edition |
| [`docs/architecture.md`](docs/architecture.md) | Architecture |
| [`docs/build-system.md`](docs/build-system.md) | Build system |
| [`docs/boot-and-updates.md`](docs/boot-and-updates.md) | Boot & updates |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Contributing |
| [`docs/SUMMARY.md`](docs/SUMMARY.md) | Docs index |

</details>

<details>
<summary><strong>Tests & validation</strong></summary>

Run the test suite locally:

```sh
python3 -m pytest \
  tools/test-builder.py \
  tools/test-frontpage-sync.py \
  tools/test-livedev-hardening.py \
  tools/test-livedev-image.py \
  tools/test-livedev-next.py \
  tools/test-livedev-orchestrator.py \
  tools/test-livedev-smoke.py \
  tools/test-livedev-state.py \
  tools/test-rush-agent.py \
  tools/test-rush-autopilot.py \
  tools/test-rush-builder-unit.py \
  tools/test-rush-capture.py \
  tools/test-rush-pr.py \
  tools/test-rush-runner.py \
  tools/test-submit-evidence.py \
  tools/test-validate-hwtest-evidence.py
```

</details>

</details>

<!-- RUSH_FRONTPAGE:END -->

---

## Contributing

We are looking for kernel engineers, Rustaceans, and systems programmers who value verifiable claims over marketing copy.

- [How Rush is built](docs/how-rush-is-built.md)
- [Agent and contributor protocol](docs/agent-protocol.md)
- [Contributing guide](CONTRIBUTING.md)
- [Open a discussion](https://github.com/Nan0pk/Rush-linux/discussions)

---

## License

[Apache-2.0](LICENSE). Copyright the Rush Linux authors.
