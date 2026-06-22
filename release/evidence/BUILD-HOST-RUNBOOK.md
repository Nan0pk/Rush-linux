# Build-Host Evidence Runbook

The criteria below cannot be evidenced in the CI container (no root/KVM; mkosi and
QEMU need syscalls the seccomp profile blocks). Run them on the build host
(root + KVM), then commit each `meta.txt` + `transcript.log` into the cited path so
`tools/validate-evidence.py` turns the ledger row green.

## `meta.txt` capture block (use for every transcript)

```bash
{
  echo "date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host=$(hostnamectl --static 2>/dev/null || hostname)"
  echo "kernel=$(uname -r)"
  echo "cpu=$(lscpu | sed -n 's/^Model name:[[:space:]]*//p' | head -1)"
  echo "git_commit=$(git rev-parse --short HEAD)"
  echo "project_version=$(cat VERSION)"
  echo "qemu_version=$(qemu-system-x86_64 --version | head -1)"
  echo "mkosi_version=$(mkosi --version 2>/dev/null | head -1)"
} > meta.txt
```

Capture each command's full stdout+stderr with `… 2>&1 | tee transcript.log`.

## Items (each maps to a ledger row)

| Ledger | Criterion | Acceptance command | Commit transcript to |
|--------|-----------|--------------------|----------------------|
| v0.3.1 | minimal VM boots to multi-user.target | `tools/validate-uefi-boot.sh build/rush-linux.raw` (confirm `multi-user.target` reached) | `release/evidence/v0.3.0-alpha.1/c1-multiuser/` |
| v0.3.2 | cgroup v2 and PSI are active | in the booted VM: `cat /sys/fs/cgroup/cgroup.controllers; ls /proc/pressure` | `release/evidence/v0.3.0-alpha.1/c2-cgroup-psi/` |
| v0.3.3 | optid.service starts | in the booted VM: `systemctl status optid.service` | `release/evidence/v0.3.0-alpha.1/c3-optid-service/` |
| v0.3.4 | nftables.conf loads | in the booted VM: `nft list ruleset; systemctl status nftables` | `release/evidence/v0.3.0-alpha.1/c4-nftables/` |
| v0.4.1 | VM boots through UKI | `tools/validate-uefi-boot.sh build/rush-linux.raw` | `release/evidence/v0.4.0-alpha.1/c1-uki-boot/` |
| v0.4.2 | three rollback entries retained | `tools/test-rollback.sh build/rush-linux.raw` (retention section) | `release/evidence/v0.4.0-alpha.1/c2-rollback-retain/` |
| v0.4.3 | simulated bad kernel rolls back | `tools/test-rollback.sh build/rush-linux.raw` (bad-kernel section) | `release/evidence/v0.4.0-alpha.1/c3-bad-kernel/` |
| v0.5.1 | fresh VM install succeeds | `sudo bash tools/test-install.sh build/rush-linux.raw` | `release/evidence/v0.5.0-beta.1/c1-fresh-install/` |
| v0.5.2 | installed system boots twice cleanly | `tools/test-double-boot.sh build/rush-linux.raw` | `release/evidence/v0.5.0-beta.1/c2-double-boot/` |
| v0.5.3 | update and rollback tests pass | `tools/test-rollback.sh build/rush-linux.raw` | `release/evidence/v0.5.0-beta.1/c3-update-rollback/` |
| v0.5.4 (confirm) | server has no desktop dependency (built image) | `mkosi -p server build` then `pacman -Qq` in the image, grep desktop pkgs | `release/evidence/v0.5.0-beta.1/c4-server-no-desktop/` (add `built-image.log`) |

## After committing a transcript

1. Set the matching `criteria_status.verified = true` and add
   `transcript = "<path>"` in `release/milestones.toml`.
2. When all of a milestone's criteria are `verified = true` with transcripts, set
   that milestone's `status = "complete"`.
3. Run `python3 tools/dragnet.py --observe`; when GREEN with zero pending v0.6 rows,
   the `0.6.0-beta.1` version bump is unlocked.
4. Follow the Authority Matrix: only the human maintainer flips `verified`/`status`.

Per-criterion landing directories carry a `meta.txt.template` to copy. Cross-ref the
Builder/Verifier flow in `docs/templates/VERIFICATION.md`.
