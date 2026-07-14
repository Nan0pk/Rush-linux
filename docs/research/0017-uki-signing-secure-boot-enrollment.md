# 0017 — UKI Signing and Secure Boot Enrollment

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md, docs/research/0016-mkosi-ala-snapshot-pinning.md
**Code:** tools/sign-uki.sh, packaging/secureboot/, packaging/mkosi/

* * *

## 0. Motivation

Secure Boot ensures that only signed boot components are loaded by UEFI firmware. For Rush
Linux, which ships as a UKI (Unified Kernel Image — kernel + initrd + kernel cmdline + splash
combined into a single EFI PE binary), secure boot requires:

1. **Signing** the UKI with a private key whose public key is trusted by the UEFI firmware.
2. **Enrolling** the public key into UEFI `db` (Authorized Signatures Database) or via MOK
   (Machine Owner Key) on Shim-based systems.
3. A **trust chain**: either direct DB enrollment (requires physical access to UEFI setup) or
   Shim + MOK (user-enrollable, no UEFI firmware change needed).

Research questions: What is the UKI file format? How is it signed with `sbsign`/`pesign`?
What is the difference between DB enrollment and MOK enrollment? How does Rush Linux handle
both paths? What key hierarchy does Rush Linux use? What happens on key rotation?

* * *

## 1. Findings

### 1.1 UKI File Format

**Q: What is the UKI file format and how is it structured?**

A Unified Kernel Image (UKI) is a UEFI PE (Portable Executable) binary that combines
multiple components as PE sections [PROVEN — systemd UKI specification, `man 7 systemd-uki`]:

```
EFI PE binary (.efi)
├── .linux    — compressed kernel image (vmlinuz)
├── .initrd   — initramfs (cpio archive)
├── .cmdline  — kernel command line (fixed at build time)
├── .uname    — kernel version string (e.g., "6.9.7-arch1")
├── .splash   — optional boot splash (BMP image)
├── .sbat     — SBAT (Secure Boot Advanced Targeting) metadata
└── .osrel    — os-release file content
```

mkosi generates a UKI via `systemd-ukify` [PROVEN — `ukify(1)` man page]:
```bash
ukify build \
  --linux=/boot/vmlinuz-linux \
  --initrd=/boot/initramfs-linux.img \
  --cmdline="root=/dev/sda2 rw quiet" \
  --uname="$(uname -r)" \
  --output=rush-linux.efi
```

The resulting `.efi` file can be placed directly in the ESP (EFI System Partition) at
`/boot/EFI/rush-linux/rush-linux.efi` and added to the UEFI boot order [PROVEN].

### 1.2 Signing the UKI

**Q: How is the UKI signed and what tools does Rush Linux use?**

The UKI must be signed with a key that is in the UEFI `db` (or reachable via the
Shim/MOK chain). Signing is done with `sbsign` (from `sbsigntools` package) [PROVEN]:

```bash
sbsign \
  --key  packaging/secureboot/rush-linux-sb.key  \
  --cert packaging/secureboot/rush-linux-sb.crt  \
  --output rush-linux-signed.efi \
  rush-linux.efi
```

**Key format**: PKCS#8 PEM private key + X.509 certificate. The certificate must have:
- Extended Key Usage: `codeSigning` [PROVEN — required by Windows/UEFI signature validation]
- Subject: `CN=Rush Linux Secure Boot Key` (or similar)
- Validity: ≥ 10 years for a signing CA [HYPOTHESIS — long validity reduces key rotation
  frequency; EV certificates are not required for UEFI db signing]

**Build pipeline signing** [HYPOTHESIS — design]:
1. CI builds the unsigned UKI via mkosi
2. Signing happens in a separate, access-controlled step using a Hardware Security Module
   (HSM) or a signing service with the private key; the CI runner never holds the private key
3. The signed `.efi` is published as a CI artefact

For development builds, a local development key (not enrolled in production MOK) is used.

**Verification**:
```bash
sbverify --cert packaging/secureboot/rush-linux-sb.crt rush-linux-signed.efi
```

### 1.3 Trust Chain: DB vs. MOK

**Q: What is the difference between UEFI DB enrollment and Shim MOK, and which does Rush Linux use?**

**Path A: Direct UEFI DB enrollment** [PROVEN]:
- User enters UEFI setup, navigates to Secure Boot → Authorized Signatures → Enroll Certificate
- Imports `rush-linux-sb.der` (DER-encoded X.509 certificate)
- All UKIs signed with this certificate will boot without Shim
- Requires physical access to UEFI setup on first enrollment
- Advantage: no Shim dependency; cleaner boot chain
- Disadvantage: requires user to navigate OEM-specific UEFI menus; not automatable

**Path B: Shim + MOK enrollment** [PROVEN — Shim is the standard approach for Linux distros]:
- Boot chain: UEFI DB trusts Microsoft's certificate → Shim (signed by Microsoft) is loaded
  → Shim reads MOK (Machine Owner Key) database → loads `grub` or directly the UKI if
  Shim supports direct UKI loading
- User enrolls key via `mokutil --import rush-linux-sb.der` → reboot → MOK manager
  (MokManager.efi) prompts user to confirm enrollment with a password set by `mokutil`
- MOK keys are stored in NVRAM and trusted by Shim for all subsequent boots
- Advantage: no UEFI firmware UI navigation needed; scripted first-boot enrollment

**Rush Linux hybrid approach** [HYPOTHESIS — design]:
1. Installer ships a Shim binary (signed by Microsoft) and MokManager.efi in the ESP
2. At first boot after installation, the installer's post-install script runs:
   ```bash
   mokutil --import /usr/share/rush-linux/rush-linux-sb.der
   ```
3. A reboot + user MOK confirmation enrolls the Rush Linux key
4. Subsequently, `rush-linux-signed.efi` boots directly via Shim without prompts
5. Advanced users who want direct UEFI DB enrollment: `packaging/secureboot/enroll-db.sh`
   provides instructions and exports the DER certificate

### 1.4 Key Hierarchy

**Q: What is the Rush Linux secure boot key hierarchy?**

[HYPOTHESIS — recommended design for a small distro]:

```
Rush Linux Secure Boot Root CA (offline, airgapped)
├── Rush Linux SB Signing Key (online, used for UKI signing)
│   └── Issues certificate: CN=Rush Linux SB Signing 2026-2036
└── Rush Linux Development Key (per-developer, never enrolled in production)
```

**Key storage** [HYPOTHESIS]:
- Root CA: HSM or air-gapped laptop (cold storage); private key never on networked machine
- SB Signing Key: HSM in CI signing service; private key never exported
- Development keys: per-developer self-signed certificates; enrolled in personal MOK only

**Key format files** (checked into git — public certs only, never private keys):
```
packaging/secureboot/
├── rush-linux-ca.crt       # Root CA certificate (DER → .crt via: openssl x509 -inform DER -in *.der -out *.crt)
├── rush-linux-sb.crt       # SB Signing Key certificate (PEM)
├── rush-linux-sb.der       # SB Signing Key certificate (DER, for mokutil/UEFI import)
└── rush-linux-dev.crt      # Example dev key certificate (for documentation)
```

### 1.5 SBAT (Secure Boot Advanced Targeting)

**Q: What is SBAT and does Rush Linux need to manage it?**

SBAT is a revocation mechanism for Shim and GRUB that allows the UEFI consortium to
revoke vulnerable bootloader versions without replacing the DB certificate [PROVEN —
documented at github.com/rhboot/shim/SBAT.md]:

The `.sbat` section in a UKI declares the component's generation number:
```
sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md
rush-linux.uki,1,Rush Linux,rush-linux-uki,1,https://rush-linux.example/secureboot
```

If a security vulnerability is found in Rush Linux's bootloader component, the SBAT
generation number is incremented in an updated build, and any older UKI with the old
SBAT generation will be refused by a firmware that has received an updated SBAT policy
via Windows Update.

**Rush Linux requirement**: Include a correct `.sbat` section in every signed UKI
[PROVEN — required for `secure boot level 2` certification; also required for Fedora/
Ubuntu Shim re-signing if Rush Linux ever seeks Shim re-signing from a Shim signing
authority].

`ukify` supports `--sbat=packaging/secureboot/rush-linux.sbat` to embed the section
[PROVEN — `ukify(1)` `--sbat` option].

### 1.6 Key Rotation

**Q: What is the key rotation process when the SB signing key expires or is compromised?**

[HYPOTHESIS — process design]:

**Planned rotation** (certificate expiry):
1. Generate new SB Signing Key signed by Rush Linux Root CA
2. Publish `rush-linux-sb-new.der` in a software update
3. `mokutil --import rush-linux-sb-new.der` in the package post-install script
4. User reboots → MOK manager shows new key enrollment prompt
5. After enrollment confirmed across the install base (next release cycle), retire old key

**Emergency rotation** (key compromise):
1. Same as above, but also:
2. Publish SBAT generation increment to revoke all UKIs signed with the compromised key
3. Coordinate with Shim signing authorities if Shim-based trust chain is affected

* * *

## 2. Architecture Decisions

### Decision A: Shim vs. Direct UEFI DB

**Selected: Shim + MOK as the default path; direct UEFI DB as optional advanced path**
[HYPOTHESIS — Shim is the standard industry approach that works without UEFI UI navigation;
maximises compatibility with OEM firmware that may have reduced Secure Boot customisation UI].

### Decision B: CI Signing Architecture

**Selected: Unsigned UKI built by CI; signed in a separate signing step with HSM-protected key**
[PROVEN design — private key never on CI runner; this is the industry standard for code signing
security (analogous to Google's build infrastructure)].

### Decision C: SBAT Inclusion

**Selected: Always include `.sbat` section in every signed UKI** [PROVEN — required for
Shim compatibility and future supply-chain security; no downside].

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| Shim version compatibility | Rush Linux UKI boots via Shim 15.8+ on 5 target OEM firmware variants | Boot test on ThinkPad, Dell XPS, ASUS, HP, Framework laptops |
| MOK enrollment UX | Enrollment completed by a non-expert user in < 5 min | Usability test: 5 volunteers enroll MOK from scratch |
| SBAT revocation propagation | SBAT generation increment prevents old UKI boot within 30 days | Test environment: increment SBAT; verify old UKI rejected by firmware after policy update |
| Signing latency in CI | Sign step ≤ 60 s | Time `sbsign` on standard CI runner; HSM signing latency separate |
| UKI boot time vs. initrd+kernel | UKI boot ≤ 200ms additional overhead vs. separate kernel+initrd | `systemd-analyze` comparison |

* * *

## 5. Non-Goals

- optid (the daemon) has no role in Secure Boot — this brief covers build and packaging.
- Rush Linux does not pursue Microsoft UEFI CA signing (requires formal business relationship).
- Rush Linux does not implement TPM-based measured boot or PCR policy sealing in v0.1.
- Rush Linux does not manage UEFI `dbx` (revocation list) updates — those come via
  fwupd/LVFS from the OEM.
- This brief does not cover full disk encryption or LUKS integration with Secure Boot.

* * *

## 6. WP Relationship Map

| WP tag | How this brief addresses it |
|--------|-----------------------------|
| WP-N14 | UKI signing is the terminal step of the reproducible build pipeline (0016 feeds this) |
| WP-N15 | The signed UKI provides supply chain integrity guarantee for the Rush Linux boot path |

* * *

## 7. Next Steps

**Immediate**
- Generate Rush Linux development signing key pair for local testing:
  ```bash
  openssl req -newkey rsa:4096 -keyout packaging/secureboot/rush-linux-dev.key \
    -x509 -days 3650 -out packaging/secureboot/rush-linux-dev.crt \
    -subj "/CN=Rush Linux Dev Secure Boot Key/"
  ```
- Implement `tools/sign-uki.sh` that calls `sbsign` with the key from env/argument.
- Update `tools/build-image.sh` to call `sign-uki.sh` after mkosi build.

**Short-term**
- Test Shim + MOK enrollment on 3 target laptops.
- Add `.sbat` content to `packaging/secureboot/rush-linux.sbat` and wire into `ukify` call.

**Medium-term**
- Evaluate HSM options for CI signing (AWS CloudHSM, Azure Dedicated HSM, YubiHSM 2).
- Design the MOK auto-enrollment post-install script for the installer.
- Investigate `systemd-cryptenroll` + TPM2 for optional measured boot in v0.2.

* * *

## Appendix: Suggested Reading

- `sbsigntools` man pages: `sbsign(1)`, `sbverify(1)`
- `ukify(1)` man page — UKI generation and signing options
- Shim project: github.com/rhboot/shim — SBAT documentation
- `mokutil(1)` man page — MOK key management
- ArchWiki: "Unified Extensible Firmware Interface/Secure Boot" — practical guide
- Fedora Secure Boot overview: fedoraproject.org/wiki/Secure_Boot_Overview
- UEFI Specification §32 — Secure Boot (public download from uefi.org)
- SLSA Level 2 requirements for build integrity: slsa.dev
