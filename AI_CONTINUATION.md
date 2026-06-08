# AI Continuation

This file is for future AI agents or human maintainers continuing the project.
Read it before making changes.

## Mission

Continue building Rush Linux: a future-aligned, source-built Linux
distribution centered on `optid`, a fast and explainable runtime optimizer for
responsiveness, battery life, thermals, and resource utilization.

The project goal is serious OS engineering, not random tweak accumulation.

## Forbidden Shortcuts

Do not:

- Replace the distro architecture with a derivative distro script.
- Add X11, PulseAudio, iptables, cgroup v1, SysV init, OpenRC, runit, TLP,
  power-profiles-daemon, TuneD, laptop-mode-tools, pm-utils, or old network
  scripts as defaults.
- Make PREEMPT_RT the universal kernel default.
- Make sched_ext production-critical while its upstream ABI is unstable.
- Add shell scripts that fight `optid` over CPU, power, cgroup, or I/O knobs.
- Add opaque AI/ML tuning before deterministic policy has benchmarks and
  rollback.
- Touch privileged sysfs paths without an allowlist and an explanation path.
- Treat docs as optional cleanup. Docs must stay aligned with code and config.
- Make undocumented changes. Every future code, config, service, recipe,
  release, benchmark, or safety change must update the relevant docs in the
  same commit.

## Session Lifecycle

Every work session must follow this cycle:

1. **Start:** `bash tools/start-work.sh "what you are about to do"`
   - Validates repo state, checks for leftover DIRTY_STATE.md, creates dirty flag.
2. **Work:** Make changes, update docs per docmap.toml, run validators.
3. **Finish:** `bash tools/finish-work.sh "commit message"`
   - Runs all validators, updates docmap dates, removes dirty flag, commits, pushes.

If you must leave mid-work, edit DIRTY_STATE.md to note what is done and what
remains. The next agent will pick up from there.

## Current Status

Implemented:

- Release governance exists with `VERSION`, `RELEASES.md`,
  `docs/versioning.md`, `docs/release-policy.md`,
  `docs/release-checklist.md`, `docs/release-plan-v1.md`, and
  `release/milestones.toml`.
- Documentation governance exists in `docs/documentation-policy.md`; future
  changes must document purpose, impact, validation, and follow-up work.
- Graphify continuation support exists in `graphify-out/`, `AGENTS.md`,
  `.agents/skills/graphify/`, `.codex/hooks.json`,
  `docs/graphify-knowledge-graph.md`, and `tools/graphify-refresh.sh`.
- Rust workspace with `crates/optid` and `crates/optctl`.
- `optid` D-Bus server/client integration implemented, supporting both system bus calls and file-based fallback.
- `optid` MVP reads PSI, AC/battery, thermal, and load signals.
- `optid` emits explainable decisions and applies guarded actions only with
  `--apply`.
- `optctl` supports status, explain, mode, trace, benchmark, and has `--json` output option for telemetry.
- Package builder (`tools/rush-builder.py`) implemented, supporting TOML recipe builds, dependency resolution, local metadata DB initialization with signatures, UKI/initrd assembly with required virtio/ext4 modules for the cached VM kernel, systemd-boot loader entry staging, and partition image formatting using `systemd-repart`.
- UEFI UKI VM boot path validated through OVMF/systemd-boot using `tools/validate-uefi-boot.sh`; verified 2026-06-08 to reach `multi-user.target` and start `optid.service` without QEMU `-kernel`/`-initrd` direct-boot arguments.
- Boot entry manager (`tools/manage-boot-entries.sh`) rotates the current UKI into a versioned rollback entry, retains at least `INSTANCES_MAX` (default 3) rollback entries, and prunes oldest entries beyond the limit.
- Boot assessment service (`optid-boot-assess.service`) marks boots as good after reaching `multi-user.target` via `tools/optid-boot-assess`.
- Boot assessment tool (`tools/optid-boot-assess`) provides `mark-good`, `check`, `count-failed`, and `reset` commands for boot-good/bad tracking on the ESP.
- Rollback integration test (`tools/test-rollback.sh`) validates all three v0.4 exit criteria: UKI boot, rollback entry retention (≥3), and simulated bad-kernel rollback recovery.
- Update metadata signing with Ed25519 test keys via `tools/sign_updates.py` (Python) and `tools/sign-updates.sh` (shell). Signing test (`tools/test-sign-updates.sh`) validates key generation, signing, verification, and tamper detection. `rush-builder.py repo-init` uses real signatures when keys are present.
- Pre-compiled base assets downloaded and unpacked locally into `build/tmp_downloads/` for offline/no-root compilation of VM image:
  - Debian `systemd-boot-efi` package (`systemd-boot-efi_252.39-1~deb12u2_amd64.deb`) -> extracts `linuxx64.efi.stub` and `systemd-bootx64.efi`.
  - Debian kernel package (`linux-image-6.1.0-49-amd64_6.1.174-1_amd64.deb`) -> extracts `vmlinuz-6.1.0-49-amd64` and kernel modules.
  - Debian static rescue shell (`busybox-static_1.35.0-4+deb12u1+b1_amd64.deb`) -> extracts static busybox.
  - Ubuntu Base rootfs tarball (`ubuntu-base-24.04.4-base-amd64.tar.gz`) cached in `build/tmp_downloads/`.

Not implemented yet:

- Real UKI signing keys, Secure Boot enrollment path, and measured boot policy.
- Minimal ISO installer.
- Hardware benchmark harness execution.
- eBPF probes.

## Safe Assumptions

- Mainstream x86_64 and ARM64 upstream-supported hardware is the initial target.
- Proprietary firmware may be optional where needed for practical hardware
  support.
- systemd with unified cgroup v2 is the resource-control foundation.
- Wayland, PipeWire, nftables, UKI, PSI, zswap, and cgroup v2 are default
  direction.
- The default kernel is adaptive low-latency with PREEMPT_DYNAMIC; RT is a
  specialist package.
- `optid` is the only default runtime policy owner.

## Repo Layout

```text
crates/optid/             Optimization daemon
crates/optctl/            CLI (status, explain, mode, pin, trace, benchmark)
config/optid/             Default optimizer policy (policy.toml)
distro/boot/              UKI-first boot defaults and kernel command line
distro/editions/          Install-time role profiles (desktop, laptop, server, RT)
distro/kernel/            Kernel config fragments (adaptive, RT, sched_ext)
distro/network/           nftables baseline
distro/systemd/           cgroup and slice defaults
distro/sysupdate/         systemd-sysupdate descriptors
docs/                     Architecture docs, ADRs, contributor guides
docs/docmap.toml          Doc registry (read this before changing docs)
docs/contributing/        Onboarding and doc-sync guides
docs/decisions/           Architecture Decision Records (ADRs)
graphify-out/             Knowledge graph for codebase navigation
packaging/dbus/           D-Bus API contract (io.rushlinux.Optid1)
packaging/systemd/        optid unit and tmpfiles
recipes/                  Source package recipe skeletons
release/                  Version milestones and test-tier gates
tools/                    Build, validate, test, and publish scripts
```

## Graphify Continuation Workflow

Use the committed knowledge graph before broad searches:

```sh
graphify query "what connects optid policy to systemd packaging?" --graph graphify-out/graph.json
```

After code or supported config changes, refresh the AST/local graph without API
or LLM tokens:

```sh
./tools/graphify-refresh.sh code
```

If Markdown/design-document changes need semantic graph updates, run full mode
with an available backend, for example:

```sh
GEMINI_API_KEY=... ./tools/graphify-refresh.sh full --backend gemini
```

If hooks are not installed in the current clone, install them once:

```sh
./tools/graphify-refresh.sh install-hooks
```

## Commands And Checks

Canonical environment is Linux (native or container). The rootfs builder, UKI
generation, `systemd-repart`, and QEMU boot require Linux, and CI is Linux-only:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pwsh ./tools/validate-repo.ps1   # cross-platform repository-policy check
./tools/build-rootfs.sh
git status --short
```

On Windows the policy check also runs via
`powershell -ExecutionPolicy Bypass -File .\tools\validate-repo.ps1`, but build
and test on Linux.

Documentation sync check (run before committing):

```sh
python3 tools/validate-doc-sync.py
```

This validates doc consistency, version matching, link resolution, and
freshness. See `docs/docmap.toml` for the doc registry and
`docs/contributing/keeping-docs-synced.md` for the update guide.

Publishing target:

```text
https://github.com/Nan0pk/Rush-linux
```

## Next Task

Current project version is `0.4.0-alpha.1` (in progress). The UKI boot, rollback entry management, boot assessment, and update signing infrastructure are implemented. The remaining v0.4 gate is end-to-end validation:

1. Run `tools/test-rollback.sh build/disk.raw` to validate:
   - VM boots through UKI
   - Three rollback entries are retained
   - Simulated bad kernel rolls back

2. Run `tools/test-sign-updates.sh` to validate:
   - Test signing keys can be generated
   - repodata.json can be signed with Ed25519
   - Signature verification works (valid = pass, tampered = fail)

3. If both pass, mark v0.4.0-alpha.1 as complete in `release/milestones.toml` and advance to `v0.5.0-beta.1` (Minimal Installable System).

Key new tools/files in this milestone:
- `tools/manage-boot-entries.sh` — Boot entry rotation and retention
- `tools/optid-boot-assess` — Boot assessment marker (mark-good/check/reset)
- `tools/test-rollback.sh` — End-to-end rollback test suite
- `tools/sign_updates.py` — Ed25519 signing/verification (Python)
- `tools/sign-updates.sh` — Ed25519 signing/verification (shell, OpenSSL)
- `tools/test-sign-updates.sh` — Signing test suite
- `packaging/systemd/optid-boot-assess.service` — Boot-good marker service
- `config/keys/` — Test Ed25519 key pair (git-ignored)

To resume work on a Linux machine (native or container):

1. **Download build assets:**
   Run `python3 tools/download-assets.py` to fetch the kernel, BusyBox,
   systemd-boot EFI stub, and Ubuntu Base rootfs into `build/tmp_downloads/`.
2. **Build and Stage `optid`:**
   Run `python3 tools/rush-builder.py build recipes/core/optid.toml` and
   populate `build/rootfs` using `rootfs-create`.
3. **Assemble Initrd, UKI, and Loader Entry:**
   Run `python3 tools/rush-builder.py build-uki build/rootfs` to construct
   the initrd, compile the Unified Kernel Image, and stage `loader/loader.conf`
   plus `loader/entries/rush-linux.conf` for systemd-boot.
4. **Generate GPT VM Image:**
   Run `python3 tools/rush-builder.py vm-image build/rootfs build/disk.raw`
   to generate a bootable raw disk image using `systemd-repart`.
5. **Boot in QEMU via UEFI:**
   Validate boot to login prompt with `optid` running, without `-kernel` or
   `-initrd` direct-boot arguments:
   `tools/validate-uefi-boot.sh build/disk.raw`
   On Debian hosts the manual equivalent uses
   `qemu-system-x86_64 -bios /usr/share/ovmf/OVMF.fd -drive file=build/disk.raw,format=raw,if=virtio -m 1G -nographic`.

### Before making any changes

Read `docs/docmap.toml` to find which docs cover the code you are about to
change. After changes, update affected docs and run
`python3 tools/validate-doc-sync.py` before committing. See
`docs/contributing/keeping-docs-synced.md` for the full guide.
