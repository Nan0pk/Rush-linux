//! Ed25519 payload signing.
//!
//! Signs compressed telemetry payloads for authenticity verification.
//! The signing key is derived from a machine-specific seed stored in
//! `/etc/rush/telemetry.key`. If no key exists, a throwaway keypair
//! is generated and the public key is embedded in the payload header.

use std::fs;
use std::io;
use std::path::Path;

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};

/// Default key path.
const DEFAULT_KEY_PATH: &str = "/etc/rush/telemetry.key";

/// Payload signer — holds the Ed25519 signing key.
pub struct PayloadSigner {
    signing_key: SigningKey,
}

impl PayloadSigner {
    /// Load or generate the signing key.
    ///
    /// If the key file exists, loads it. Otherwise generates a new
    /// random keypair and persists it.
    pub fn open_or_generate() -> io::Result<Self> {
        Self::from_path(DEFAULT_KEY_PATH)
    }

    /// Load or generate from a specific path.
    pub fn from_path(path: &str) -> io::Result<Self> {
        let key_path = Path::new(path);

        if key_path.exists() {
            let key_bytes = fs::read(key_path)?;
            if key_bytes.len() != 32 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "invalid key file length: expected 32 bytes, got {}",
                        key_bytes.len()
                    ),
                ));
            }
            let mut secret = [0u8; 32];
            secret.copy_from_slice(&key_bytes);
            let signing_key = SigningKey::from_bytes(&secret);
            log::info!("Loaded telemetry signing key from {path}");
            Ok(PayloadSigner { signing_key })
        } else {
            // Generate new keypair
            let mut csprng = rand::rngs::OsRng;
            let signing_key = SigningKey::generate(&mut csprng);

            // Ensure parent directory exists
            if let Some(parent) = key_path.parent() {
                fs::create_dir_all(parent)?;
            }

            // Write the secret key (with restrictive permissions)
            fs::write(key_path, signing_key.to_bytes())?;

            // Set file permissions to 0600 (owner read/write only)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(key_path, fs::Permissions::from_mode(0o600))?;
            }

            log::info!("Generated new telemetry signing key at {path}");
            Ok(PayloadSigner { signing_key })
        }
    }

    /// Sign a payload blob.
    ///
    /// Returns (signature_bytes, public_key_bytes).
    /// The signature is prepended to the payload for transport.
    pub fn sign(&self, payload: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let signature = self.signing_key.sign(payload);
        let public_key = self.signing_key.verifying_key().to_bytes().to_vec();
        (signature.to_bytes().to_vec(), public_key)
    }

    /// Get the verifying (public) key.
    pub fn public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Get the public key as bytes (for embedding in payload headers).
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }
}

/// Create a signed transport envelope.
///
/// Format: [32 bytes pubkey][64 bytes signature][remaining = compressed payload]
pub fn create_signed_envelope(
    signer: &PayloadSigner,
    compressed_payload: &[u8],
) -> Vec<u8> {
    let (signature, pubkey) = signer.sign(compressed_payload);

    let mut envelope = Vec::with_capacity(32 + 64 + compressed_payload.len());
    envelope.extend_from_slice(&pubkey);
    envelope.extend_from_slice(&signature);
    envelope.extend_from_slice(compressed_payload);
    envelope
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    #[test]
    fn test_sign_and_verify_roundtrip() {
        let signer = PayloadSigner::from_path("/tmp/rush_telemetry_test_key").unwrap();
        let payload = b"test telemetry payload for signing verification";

        let (signature_bytes, pubkey_bytes) = signer.sign(payload);

        // Verify
        let verifying_key = VerifyingKey::from_bytes(
            pubkey_bytes.as_slice().try_into().unwrap()
        ).unwrap();
        let signature = ed25519_dalek::Signature::from_bytes(
            signature_bytes.as_slice().try_into().unwrap()
        );
        assert!(verifying_key.verify(payload, &signature).is_ok());

        // Verify with tampered payload
        let mut tampered = payload.to_vec();
        tampered[0] ^= 0xFF;
        assert!(verifying_key.verify(&tampered, &signature).is_err());

        // Cleanup
        let _ = fs::remove_file("/tmp/rush_telemetry_test_key");
    }

    #[test]
    fn test_envelope_format() {
        let signer = PayloadSigner::from_path("/tmp/rush_telemetry_test_key2").unwrap();
        let payload = b"compressed payload data";

        let envelope = create_signed_envelope(&signer, payload);

        // Verify envelope structure
        assert_eq!(envelope.len(), 32 + 64 + payload.len());
        assert_eq!(&envelope[32 + 64..], payload);

        let _ = fs::remove_file("/tmp/rush_telemetry_test_key2");
    }
}
