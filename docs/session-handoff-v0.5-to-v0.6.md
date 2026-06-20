# Rush Linux — Session Handoff Prompt

Copy-paste the block below into a new session to activate a fully-oriented workspace for the Rush Linux project.

---

## Handoff Prompt (copy from here)

```
You are continuing work on the Rush Linux project (https://github.com/Nan0pk/Rush-linux), an adaptive Linux distribution with a Rust-based optimization daemon (optid). The project follows a milestone-driven release process tracked in `release/milestones.toml`.

## Current State

- **Version:** 0.5.0-beta.1
- **Milestone status:** v0.5.0-beta.1 ("Minimal Installable System") — **CLOSED** (PR #163). All 4 exit criteria verified on build host with `criteria_status` entries committed.
- **Next milestone:** v0.6.0-beta.1 ("Hardware-Aware optid")
- **Repo:** main is at commit `58e3895` (includes PRs #157–#163 all merged)
- **Open issues:** #162 — v0.3/v0.4 exit criteria lack committed boot transcripts (Evidence Rule violation). Build-host validation needed.
- **PR #154** was closed after review — stale branch, redundant changes, broken benchmark data

## Workspace Setup

Run these steps first to orient:

```bash
cd /home/z/my-project
# Clone if not already present
[ -d Rush-linux ] || git clone https://github.com/Nan0pk/Rush-linux.git
cd Rush-linux
git checkout main && git pull --ff-only origin main

# Source the toolchain environment (adds QEMU, mkosi, mtools, sgdisk, etc.)
source tools/env-setup.sh
```

## Mandatory Orientation Reads

Read these files IN ORDER before doing any work:

1. `docs/AI_CONTINUATION.md` — Agent handoff conventions and next-task pointer
2. `docs/IMPLEMENTATION_STATUS.md` — What's implemented vs. not
3. `release/milestones.toml` — Milestone definitions and criterion verification state
4. `docs/docmap.toml` — Documentation dependency graph and freshness tracking
5. `docs/plans/v0.5-minimal-installable-system-proposal.md` — The 5-phase plan that was executed

## Environment Constraints

- This container has NO root/sudo — mkosi builds cannot run here
- Kubernetes seccomp blocks `mount()`, `mount_setattr()` — systemd mount units fail, so `multi-user.target` is unreachable in QEMU-TCG inside this container
- Rust toolchain IS available via `source tools/env-setup.sh`
- QEMU 10.0.8 TCG works (no KVM) — can boot UKI images in emulation
- OVMF firmware works with pflash drives (not `-bios` flag)
- `sgdisk`, `mtools`, `cpio` all work without root
- `mkfs.ext4 -d` works for building rootfs images without root
- The build host (`/home/victus/Rush-linux/`) has full root + KVM for real validation
- GitHub API access is embedded in the git remote URL (use `TOKEN=$(git remote get-url origin | sed -E 's|https://([^@]+)@.*/|\1|')` and `curl -H "Authorization: token $TOKEN"`)
- `gh` CLI is NOT installed — use GitHub REST API directly via curl
- Repo requires PRs to merge into main (direct push blocked by branch protection)
- CI checks required: Docker Image CI, rust-clippy analyze, CI

## QEMU Boot Command (verified working in this container)

```bash
source tools/env-setup.sh
# Copy OVMF vars (writable copy needed)
cp /home/z/my-project/tmp-debs/usr/share/OVMF/OVMF_VARS_4M.fd build/ovmf_vars.fd

# Boot with pflash (UEFI firmware)
timeout 120s qemu-system-x86_64 \
  -L /home/z/my-project/tmp-debs/usr/share/qemu/ \
  -drive if=pflash,format=raw,readonly=on,file=/home/z/my-project/tmp-debs/usr/share/OVMF/OVMF_CODE_4M.fd \
  -drive if=pflash,format=raw,file=build/ovmf_vars.fd \
  -drive file=build/disk.raw,format=raw,if=virtio \
  -m 1G -nographic -no-reboot \
  </dev/null 2>&1 | tee build/uefi-boot.log
```

Note: In this container, systemd drops to `emergency.target` because seccomp blocks mount syscalls. On the build host with KVM, `multi-user.target` is reached.

## Architecture Quick Reference

- **Image build:** mkosi (Arch-based) → `build/rush-linux.raw`; also `build-vm-unpriv.sh` (Ubuntu-based, no root needed)
- **Boot:** UKI in ESP, systemd-boot, no GRUB
- **Kernel cmdline:** `systemd.unified_cgroup_hierarchy=1 cgroup_no_v1=all psi=1 zswap.enabled=1 systemd.firstboot=no quiet loglevel=3`
- **Installer:** `tools/rush-install.sh` — systemd-repart partition layout + dd raw partition copy
- **Partition naming:** Loop/NVMe devices use `p1` suffix (`/dev/loop0p1`), SCSI/virtio use `1` (`/dev/sda1`)
- **Rollback:** UKI boot entry enumeration + bad-kernel fallback via systemd-boot
- **Edition profiles:** `mkosi/mkosi.profiles/{server,desktop}/mkosi.conf`
- **Repart definitions:** `mkosi/mkosi.repart/{00-ESP.conf, 10-root.conf}`

## Key Tools in `tools/`

| Tool | Purpose |
|------|---------|
| `build-mkosi-image.sh` | Primary image builder (mkosi, Arch-based). `--edition server|desktop` |
| `build-vm-unpriv.sh` | Unprivileged disk image builder using `mkfs.ext4 -d` (no root needed) |
| `rush-install.sh` | Install OS onto target block device (needs root) |
| `env-setup.sh` | Bootstrap full toolchain without root (Rust, QEMU, mkosi, etc.) |
| `validate-uefi-boot.sh` | Boot disk image through OVMF, check UKI boot markers |
| `test-rollback.sh` | Validate UKI boot, rollback entries, bad-kernel recovery |
| `test-install.sh` | End-to-end install + double-boot test (needs root) |
| `test-double-boot.sh` | Verify disk image boots twice cleanly |
| `start-work.sh` / `finish-work.sh` | Work session lifecycle (validation, doc sync) |
| `download-assets.py` | Download Debian/Ubuntu packages for build-vm-unpriv.sh |

## Priority Tasks (in order)

### 1. Produce full build-host boot transcripts (Issue #162)
- On the build host (has root + KVM), run `validate-uefi-boot.sh` and `test-rollback.sh`
- Commit the resulting logs to `release/evidence/`
- This satisfies the project's own Evidence Rule for v0.3/v0.4

### 2. Bump to v0.6.0-beta.1
- Update `VERSION`, `Cargo.toml`, `milestones.toml`, `os-release`, all version references
- Write `docs/plans/v0.6-hardware-aware-optid-proposal.md` for the 4 exit criteria:
  - "unsupported knobs are skipped with reasons"
  - "mixed-load responsiveness improves on two machines"
  - "battery behavior matches or improves mainstream defaults"
  - "no unsafe write occurs outside allowlisted paths"

### 3. Draft v0.5.0-beta.1 release notes
- Summary of what shipped: mkosi parity, edition profiles, installer flow, test harness, unpriv VM builder, 3 installer bug fixes, boot-assess Confirms= fix
- All 4 exit criteria verified
- Commit as a GitHub release or in `RELEASES.md`

### 4. Wire test scripts into GitHub Actions CI
- Add `test-rollback.sh` and `test-install.sh` as CI jobs (they need QEMU + OVMF, which GitHub Actions supports)
- This catches regressions on every PR

### 5. Validate desktop edition parity
- Only server edition has been tested; desktop profile needs build + boot validation

### 6. Harden `rush-install.sh`
- Add error handling / rollback for partial failures (e.g., if dd succeeds but bootctl install fails)
- Consider checksum verification of dd output

### 7. Test on real UEFI hardware
- Validate UEFI runtime variables work (currently `--no-variables` in bootctl install)
- Test Secure Boot with test keys

## Bugs Found This Session

| Bug | File | Fix | Status |
|-----|------|-----|--------|
| `Confirms=` invalid systemd directive | `optid-boot-assess.service` | Removed `Confirms=`, kept `After=` | Fixed in PR #163 |
| Partition suffix for loop/NVMe | `rush-install.sh` | Added regex check for `p1` vs `1` | Fixed in PR #161 |
| mkfs.vfat destroys ESP after dd | `rush-install.sh` | Removed mkfs.vfat call | Fixed in PR #161 |
| Missing `--partscan` on losetup | `test-install.sh` | Added `--partscan` flag | Fixed in PR #161 |

## Closed/Rejected Items

- **PR #154** (`claude/sleepy-goldberg-ga3z6h`) — CLOSED after review. Stale branch with merge conflicts. Its only salvageable change (`systemd.firstboot=no`) was already on main via PR #158. Review findings: class_mismatch in all benchmarks, resolved_floors:-1, cargo test failures, run.sh not production-ready, transient agent state files in repo root.

## Work Log

All agents share `/home/z/my-project/worklog.md`. Append after each task using the template:
```
---
Task ID: <id>
Agent: <name>
Task: <description>
Work Log:
- <step 1>
- <step 2>
Stage Summary:
- <results>
```
```

---

## End of Handoff Prompt
