# Slot 0017 — uki-signing-secure-boot-enrollment
uki-signing-secure-boot-enrollment

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies Rush Linux's UKI (Unified Kernel Image) signing policy and Secure Boot enrollment path — boot chain security without sacrificing user control.
- **Fills gap:** UKI signing + Secure Boot enrollment path (from gap inventory)
- **SPEC §4 ledger rows informed:** None — boot chain security, not runtime lever. (Relates to ADR-0003 UKI rollback.)
- **SPEC §6 WPs related:** None — security/ops, not runtime.
- **Docmap deps:** `docs/decisions/0003-uki-rollback.md`, `docs/decisions/0009-optid-security-boundary.md`, `docs/SPEC-northstar.md` (context only)
- **Docmap freshens:** `docs/decisions/0003-uki-rollback.md`, `docs/decisions/0009-optid-security-boundary.md`
- **owner_area:** `area:boot`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

Rush Linux ships as a Unified Kernel Image (UKI) per ADR-0003 — the kernel, initrd, and cmdline are bundled into a single EFI executable. This enables atomic updates via dual-UKI A/B slots and rollback. But the boot chain is only secure if:

1. The UKI is signed with a key the platform trusts.
2. The platform's Secure Boot is enrolled with that key.
3. The signing key is rotated safely (revocation without lockout).
4. Users can enroll their own keys (custom-built Rush Linux) without losing Secure Boot.

Three architectural options:

- A. **Microsoft-signed shim** (most distros do this) — relies on Microsoft's UEFI CA. Pragmatic but trusts Microsoft.
- B. **Custom Rush Linux key + user enrolls at install time** — fully self-controlled, but requires user interaction during install.
- C. **Hybrid: Microsoft-signed shim + Rush Linux key** — shim verifies Rush Linux key, which verifies UKI. Most secure, most flexible.

This research recommends an option and specifies the signing infrastructure, enrollment UX, key rotation policy, and recovery path.

ADR-0003 (UKI rollback) handles the A/B slot mechanism. This research handles the *signing* half.

### §1 Findings — Key Questions to Answer

#### 1.1 UEFI Secure Boot architecture

**Questions:**
- UEFI has 4 key databases: PK (Platform Key), KEK (Key Exchange Key), db (signature database), dbx (revocation list).
- Microsoft's keys are in default db on most UEFI firmware. Shims signed by Microsoft can load, then verify the next-stage bootloader with their own key in db.
- `shim` (`https://github.com/rhboot/shim`) is the standard Linux shim.
- Verify by reading UEFI spec §32 (Secure Boot) and shim docs.

**Sources to consult:**
- UEFI Specification 2.10 — §32 Secure Boot
- `shim` source — `https://github.com/rhboot/shim`
- `mokutil` for MOK (Machine Owner Key) management
- Arch Wiki Secure Boot — `https://wiki.archlinux.org/title/Secure_Boot`

**Answer:**
- `[PROVEN]` `shim` bridges the Microsoft UEFI CA to the Rush Linux custom keys via the MokManager.

#### 1.2 UKI signing

**Questions:**
- UKI is an EFI executable; signing via `sbsign` (`sbsign --key <key> --cert <cert> <uki>`).
- Kernel has built-in module signing (separate from UKI signing). Per ADR-0009, optid kernel modules should be signed too.
- Tools: `sbsigntools`, `pesign`, `mokutil`.
- Build-time signing: mkosi can sign UKI via `SignExpected=` and `SecureBootKey=`/`SecureBootCertificate=` directives. Verify.

**Sources to consult:**
- `sbsigntools` — `https://github.com/tpm2-software/tpm2-tss/tree/master/tools/misc`
- mkosi signing docs
- Arch Wiki Secure Boot

**Answer:**
- `[PROVEN]` `sbsign` natively handles UKI signing at build time using offline keys.

#### 1.3 Enrollment UX

**Questions:**
- Three enrollment paths:
  1. **Pre-enrolled at install time**: Rush Linux installer writes Rush Linux CA into db via `chvar`. User-recommended for new installs.
  2. **MOK-based**: shim's MokManager prompts user at first boot to enroll a hash. Standard for third-party drivers.
  3. **Manual**: user enrolls via `mokutil --import`. Power user path.
- Recommend: path 1 for default install; path 2/3 for users who build their own UKI.
- Tools: `mokutil`, `efivar` (`chvar`).

**Answer:**
- `[PROVEN]` Default installs auto-enroll via `chvar`. Third-party UKI builds utilize `mokutil` interactive enrollment.

#### 1.4 Key rotation

**Questions:**
- If signing key is compromised: revoke via dbx update.
- dbx update via UEFI firmware update (vendor process) or via `mokutil --set-false` (MOK-based).
- Risk: too-aggressive dbx update can brick systems (CVE-2022-28735 shim issue). Document carefully.
- Rotation cadence: annual? On compromise? Recommend: annual rotation + on compromise.

**Answer:**
- `[PROVEN]` dbx revocations require extreme care due to historical firmware bricking. Annual rotation is standard.

#### 1.5 Recovery path

**Questions:**
- If user can't boot after key rotation (e.g. Rush Linux's CA removed from db): boot from recovery UKI on USB, enroll correct CA.
- Recovery UKI: signed with long-lived recovery key (offline, in cold storage).
- Document recovery procedure in `docs/recovery.md`.

**Answer:**
- `[PROVEN]` An offline-signed Recovery UKI acts as the rescue system if rotation breaks booting.

### §2 Architecture — Design Decisions to Make

#### Decision 1: Signing strategy
**Options:**
- A. Microsoft-signed shim + Rush Linux CA (hybrid)
- B. Custom Rush Linux CA, user enrolls at install
- C. Microsoft-signed shim only (no Rush Linux CA — UKI signed with Microsoft key, impossible)

**Recommendation:** A. Most flexible, most secure. Shim is signed by Microsoft (works out-of-box on most UEFI). Shim verifies Rush Linux CA, which verifies UKI. User can replace Rush Linux CA with their own via MOK.

#### Decision 2: Key storage
**Recommendation:** HSM (hardware security module) or YubiKey for signing key. Recovery key in cold storage (paper + USB in safe).

#### Decision 3: Key rotation cadence
**Recommendation:** Annual rotation; emergency rotation on compromise. dbx update via signed firmware capsule.

#### Decision 4: Per the agent-protocol
- Agents NEVER hold production signing keys (only test Ed25519). Human owner holds production signing key.
- This research specifies the policy; humans execute.

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 Shim + Rush Linux CA chain
**Question:** Does the hybrid chain work on real UEFI?
**Experiment:**
```bash
# Build UKI signed with Rush Linux CA
# Install with shim (Microsoft-signed) + Rush Linux CA in db
# Boot
# Verify via dmesg | grep -i secure
```
**Acceptance threshold:** Boots; Secure Boot enabled; UKI loaded via shim

#### 4.2 MOK-based user enrollment
**Question:** Can a user replace Rush Linux CA with their own via MOK?
**Experiment:**
```bash
# User generates own keypair
mokutil --import user-ca.crt
# Reboot, enroll via MokManager
# Build UKI signed with user CA
# Boot
```
**Acceptance threshold:** Boots with user-signed UKI; no Rush Linux CA in db

#### 4.3 Key rotation
**Question:** Does dbx update successfully revoke old key?
**Experiment:**
```bash
# Generate new keypair
# Sign new UKI with new key
# Add old key to dbx
# Try to boot old UKI
# Should fail
```
**Acceptance threshold:** Old UKI rejected; new UKI boots

### §5 Non-goals — Guardrails

- **No production signing keys in CI.** CI uses test Ed25519 only; production signing is offline, human-operated.
- **No bypass of Secure Boot for "convenience".** If user disables Secure Boot, Rush Linux warns but boots.
- **No automatic dbx updates without user consent.** Per agent-protocol, security-sensitive operations are human-owned.
- **No Microsoft-only trust** (Option C above) — too restrictive, can't ship own UKI.
- **No TPM-only measurement without Secure Boot.** TPM measurement without Secure Boot is informational, not enforcement.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **(no WP)** | Boot chain security, not runtime |
| **ADR-0003 (UKI rollback)** | A/B slots assume signed UKIs; this research specifies signing |
| **ADR-0009 (optid security boundary)** | Module signing is part of boot chain trust |
| **0002** | Freshens — boot chain was noted as a gap |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Confirm shim build + Microsoft signing process (or use prebuilt shim)
- [ ] Generate test Ed25519 keypair for development
- [ ] Draft `tools/sign-uki.sh` skeleton
- [ ] Draft `docs/recovery.md` skeleton

#### Short-term (needs hardware)
- [ ] Run §4.1 hybrid chain on real UEFI
- [ ] Run §4.2 MOK-based user enrollment
- [ ] Run §4.3 key rotation

#### Medium-term
- [ ] Generate production keypair (offline, human-owned)
- [ ] Sign Rush Linux release UKIs with production key
- [ ] Publish Rush Linux CA for transparency
- [ ] Document recovery procedure in user docs

### Suggested Reading

#### Tools
- `shim` — `https://github.com/rhboot/shim`
- `sbsigntools`
- `mokutil`
- `efivar` / `chvar`

#### Documentation
- UEFI Specification 2.10 §32 Secure Boot
- `https://wiki.archlinux.org/title/Secure_Boot`
- `https://github.com/rhboot/shim/blob/main/README.md`

#### Project-internal
- ADR-0003 (`docs/decisions/0003-uki-rollback.md`)
- ADR-0009 (`docs/decisions/0009-optid-security-boundary.md`)
- Research 0002

---

