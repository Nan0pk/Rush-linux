# 💡 Policy Integrity Verification

**Status:** 🟢 Seed
**Proposed by:** @Nan0pk
**Date:** 2026-06-12

### 🎯 The Vision
Implement cryptographic verification for `policy.toml` and any compiled policy artifacts to prevent silent drift, corruption, or tampering.

### ❓ The "Why" (Motivation)
A corrupted policy file could lead to unstable system behavior or security vulnerabilities. Ensuring the integrity of the "Brain" is critical for a production-grade OS.

### 🛠️ Potential Implementation
- Use a simple checksum (`policy.toml.sha256`).
- Verify the hash against a constant baked into the binary at build time.
- For advanced versions, sign the policy using the same Ed25519 key used for update metadata.

### ⚠️ Trade-offs & Risks
- **UX Friction:** A failed hash check prevents the daemon from starting, requiring manual intervention.

### ⏳ Why not now?
Simple checksums can be added quickly, but a full signed-policy chain requires the update infrastructure to be fully stable.
