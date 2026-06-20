# Rush Linux — Session Handoff Prompt

**Project:** Rush Linux — a source-built Linux distribution centered on `optid`, a fast, explainable runtime optimizer.  
**Repository:** https://github.com/Nan0pk/Rush-linux  
**Current Version:** `0.5.0-beta.1` (Minimal Installable System)  
**Date:** 2026-06-20

---

## What Was Done This Session

### 1. Milestone Closure & Version Bump (PR #157 — MERGED)
- Closed `v0.4.0-alpha.1` ("UKI, Boot, Rollback, Updates") — all 4 exit criteria verified
- Bumped project to `0.5.0-beta.1` across **14 files**: VERSION, milestones.toml, Cargo.toml, allowlist.toml, ROADMAP.md, RELEASES.md, IMPLEMENTATION_STATUS.md, AI_CONTINUATION.md, docmap.toml, os-release (both mkosi and build-vm-final.sh), etc.
- CI initially failed on `Cargo.toml` workspace version — fixed in a follow-up commit

### 2. v0.5 Implementation Proposal (merged with PR #157)
- Wrote `docs/plans/v0.5-minimal-installable-system-proposal.md` — a 5-phase plan targeting all 4 exit criteria:
  - Phase A: mkosi/Arch image parity
  - Phase B: Edition profiles (server = no desktop deps)
  - Phase C: Installer flow via systemd-repart
  - Phase D: Double-boot and rollback tests
  - Phase E: Documentation and lifecycle

### 3. mkosi Parity, Edition Profiles, Installer, Test Scripts (PR #158 — MERGED)
- `mkosi/mkosi.conf` — refactored with UKI packages, multi-line kernel cmdline, reproducibility pinning
- `mkosi/mkosi.repart/00-ESP.conf` + `10-root.conf` — declarative partition layout
- `mkosi/mkosi.profiles/server/mkosi.conf` — headless profile, zero desktop packages (criterion 4)
- `mkosi/mkosi.profiles/desktop/mkosi.conf` — placeholder for v0.7
- `tools/build-mkosi-image.sh` — full rewrite with `--edition` flag, dynamic VERSION, proper staging
- `tools/rush-install.sh` — installs OS onto blank disk via systemd-repart + dd (criterion 1)
- `tools/test-install.sh` — end-to-end install + double-boot + desktop-check test (criteria 1, 2, 4)
- `tools/test-double-boot.sh` — standalone double-boot validation (criterion 2)
- `tools/test-rollback.sh` — fixed hardcoded ESP offset (now uses sgdisk dynamically), removed stale version strings

### 4. Unprivileged VM Image Builder (PR #159 — OPEN)
- `tools/build-vm-unpriv.sh` — builds a complete bootable disk image **without root access**
- Key innovation: uses `mkfs.ext4 -d` (directory population mode, e2fsprogs 1.47+) instead of mount/losetup/chroot
- systemd + deps extracted from .deb packages directly into rootfs
- **Boot test result:** UKI boots → initrd loads → root mounts → switch_root → systemd initializes → reaches getty.target
- **Known issue:** `systemd-udevd` fails because `kmod`/`libkmod2` packages are missing from rootfs; this cascades to `systemd-networkd` failure. Fixable by extracting more .deb packages.

### 5. Environment Bootstrapping
- `tools/env-setup.sh` — user-space toolchain bootstrap for containerized environments
- Installed in this session without root: Rust 1.96.0, QEMU 10.0.8, mkosi 25.3, systemd-repart 257, mtools 4.0.48, cpio 2.15, sgdisk 1.0.10, mkfs.ext4 1.47.2, mkfs.vfat 4.2, OVMF firmware, seabios
- Technique: `apt-get download` → `ar x` + `tar xf` into user-space prefix, then set PATH/LD_LIBRARY_PATH/PYTHONPATH

### 6. Container Environment Assessment
- Running inside a Kubernetes container (Debian trixie) with seccomp that blocks `mount_setattr()`
- This prevents `mkosi build` from running — mkosi's sandbox requires that syscall
- Root/sudo is not available (password required)
- `mkfs.ext4 -d`, `sgdisk`, `mtools`, `QEMU TCG mode` all work without root
- The unprivileged builder (`build-vm-unpriv.sh`) was created specifically to work around these constraints

---

## What Needs To Be Done

### Immediate: Close v0.5.0-beta.1 Milestone

The project owner is currently running **Option A** on their build host:
```bash
sudo bash tools/build-mkosi-image.sh --edition server
tools/validate-uefi-boot.sh build/rush-linux.raw
tools/test-rollback.sh build/rush-linux.raw
sudo bash tools/test-install.sh build/rush-linux.raw
```

**Once all 4 exit criteria pass**, the following updates are needed:

| # | Exit Criterion | Status | Action When Verified |
|---|---|---|---|
| 1 | Fresh VM install succeeds | ⏳ Awaiting build host results | Set verified=true in milestones.toml, add evidence note |
| 2 | Installed system boots twice cleanly | ⏳ Awaiting | Same |
| 3 | Update and rollback tests pass | ⏳ Awaiting | Same |
| 4 | Server edition has no desktop dependency | ⏳ Awaiting | Same |

Then: update VERSION → `0.6.0-beta.1`, update all cascading docs, commit, PR.

### Fix Unprivileged Builder (PR #159)

If the build host is not available and CI needs to run in containers:
1. Extract `kmod` + `libkmod2` .deb packages into rootfs in `build-vm-unpriv.sh`
2. Also extract: `libacl1`, `libblkid1`, `libcap-ng0`, `libpcre2-8-0`, `libssl3t64`, `libzstd1` (some already done)
3. Run `ldconfig` equivalent (create ld.so.cache) — may need proot or a static ldconfig
4. Re-test with QEMU TCG (allow 5+ minutes per boot)
5. Once `multi-user.target` + `optid.service` confirmed, merge PR #159

### Future Milestones

| Version | Name | Key Work |
|---|---|---|
| `0.6.0-beta.1` | Hardware-Aware optid | Hardware allowlist DB, compatibility D-Bus shims (PPD, GameMode), foreground detection, vm.* actuation |
| `0.7.0-beta.1` | Editions | Desktop profile (Plasma Wayland), laptop profile, realtime-audio profile, mkosi sysext architecture |
| `0.8.0-beta.1` | Benchmark Lab | Automated benchmark harness, regression gates, optctl explain correlation |
| `0.9.0-rc.1` | Release Candidate | v1 schema freeze, security review, signed metadata |
| `1.0.0` | Stable | First stable release |

---

## Build System Architecture

### Two Build Paths

| Path | Script | Needs Root? | Base Distro | Status |
|---|---|---|---|---|
| **mkosi (primary)** | `tools/build-mkosi-image.sh` | Yes (sudo) | Arch Linux | Scripts written, awaiting build host validation |
| **Unprivileged (CI)** | `tools/build-vm-unpriv.sh` | No | Ubuntu Base + Debian systemd | Boots to systemd, needs kmod fix |

### mkosi Pipeline (primary, for build host)
```
tools/build-mkosi-image.sh --edition server
  ├── cargo build --workspace --release       # Compile optid, optctl, rushbench
  ├── Stage mkosi.extra/ overlay              # Binaries, configs, systemd units, D-Bus, network
  ├── mkosi build --profile=server            # Arch rootfs + UKI + ESP + disk image
  └── Output: build/rush-linux-server.raw
```

### Unprivileged Pipeline (for containerized CI)
```
tools/build-vm-unpriv.sh
  ├── cargo build --workspace --release       # Compile Rust binaries
  ├── Extract Ubuntu Base rootfs              # tar -xzf
  ├── Install Rush components                # cp/install directly into rootfs
  ├── Extract systemd + deps from .debs       # ar x + tar xf (no chroot)
  ├── Build initrd (cpio + gzip)              # BusyBox + kernel modules
  ├── Build UKI (objcopy)                     # stub + vmlinuz + initrd + cmdline
  ├── Build ESP (mtools)                      # vfat image with UKI + systemd-boot
  ├── Build rootfs (mkfs.ext4 -d)             # Populate ext4 from directory, NO mount needed
  └── Assemble disk (sgdisk + dd)             # GPT partition table + dd partitions
```

### Key Architectural Decisions
- **ADR 0014:** Image composition via mkosi on Arch (ratified)
- **mkfs.ext4 -d:** Discovered this works without root — enables full CI builds in containers
- **mkosi profiles:** Server (no desktop) and Desktop (placeholder) — `--edition` flag selects profile
- **Installer:** Uses `systemd-repart` + `dd` to stamp image onto blank disks (not a GUI installer)

---

## Repository State

| Branch | Status | Content |
|---|---|---|
| `main` | Current at `0.5.0-beta.1` | All merged PRs below |
| `v0.5/unpriv-builder` | Open PR #159 | `tools/build-vm-unpriv.sh` |
| `v0.5/env-and-handoff` | Open PR #160 | `tools/env-setup.sh` update |

**Merged PRs:**
- PR #157: Milestone closure, version bump, implementation proposal
- PR #158: mkosi parity, edition profiles, installer, test scripts

**Open PRs:**
- PR #159: Unprivileged VM builder
- PR #160: env-setup.sh update

---

## Rules of Engagement (from AI_CONTINUATION.md)

1. **Evidence Rule:** Every claim must be backed by an authentic command transcript
2. **Session Lifecycle:** `bash tools/start-work.sh` → work → `bash tools/finish-work.sh`
3. **Forbidden Shortcuts:** No X11/PulseAudio/iptables/cgroup-v1, no bypassing allowlist, no derivative slop
4. **100% Documentation Synchronization:** Every code change must update all dependent docs
5. **No structural code changes without agreed plan**

---

## For The Next Agent

When you pick up this project, ask the user for a **GitHub Personal Access Token** with `repo` scope on `Nan0pk/Rush-linux` so you can push branches and open PRs. The current token from this session may have expired.

**First thing to do:** Check if the build host validation completed by reviewing the latest commits and PR comments on https://github.com/Nan0pk/Rush-linux — look for evidence of `validate-uefi-boot.sh` and `test-rollback.sh` passing on the mkosi-built image.
