# v0.3 / v0.4 UEFI Boot Evidence

> **Status (corrected by Dragnet-001, 2026-06-22):** The transcript this directory
> was built around (`transcript-2026-06-21-qemu-tcg.log`) was **never committed**.
> The "Verified Markers" tables that previously appeared here cited that absent
> file, so they were unverified claims dressed as a verification record. They have
> been removed. The corresponding v0.3/v0.4 criteria in `release/milestones.toml`
> are now `verified = false` (evidence-pending) until a real build-host transcript
> lands here. See `release/evidence/dragnet/LEDGER.md` and
> `release/evidence/BUILD-HOST-RUNBOOK.md`.

## What this directory is for

The committable acceptance transcripts for the v0.3 and v0.4 UEFI/boot/rollback
exit criteria. It is currently a **placeholder** — no qualifying transcript is
present yet.

## What was actually observed (uncommitted, build host / container)

Prior sessions reported a QEMU-TCG run that demonstrated the **UKI boot chain**
(firmware → systemd-boot → UKI → kernel → initrd → root mount → systemd) but
**did not reach `multi-user.target`**, because the container's seccomp profile
blocks `mount()`/`mount_setattr()` and systemd drops to `emergency.target`. That
run's log was never committed, so none of it counts as evidence under the project
Evidence Rule (`docs/agent-protocol.md`). It is described here only as a pointer
for the build-host re-run, not as proof.

To produce qualifying evidence, run the acceptance commands on the build host
(root + KVM) per `release/evidence/BUILD-HOST-RUNBOOK.md` and commit the resulting
`meta.txt` + `transcript.log` into `release/evidence/v0.3.0-alpha.1/` and
`release/evidence/v0.4.0-alpha.1/`.

## Notes on issues this surfaced

- The `optid-boot-assess.service` `Confirms=` directive bug noted in earlier
  observations **has been fixed** (the directive was removed in both
  `packaging/systemd/optid-boot-assess.service` and the mkosi copy; see PR #163).
  No further action — this note is retained only to close the loop.
