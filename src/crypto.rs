use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use sha2::{Digest, Sha256};

/// Simple random nonce generator using a basic PRNG.
fn generate_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    // Use UUID v4 randomness as a simple source
    let u = uuid::Uuid::new_v4();
    let bytes = u.as_bytes();
    nonce.copy_from_slice(&bytes[..12]);
    nonce
}

/// Derive a 256-bit AES key from the JWT secret via SHA-256.
fn derive_key(secret: &str) -> [u8; 32] {
    let hash = Sha256::digest(secret.as_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(&hash);
    key
}

/// Encrypt a plaintext string using AES-256-GCM.
/// Returns base64(nonce || ciphertext).
pub fn encrypt(plaintext: &str, secret: &str) -> String {
    let key = derive_key(secret);
    let cipher = Aes256Gcm::new_from_slice(&key).expect("Valid AES-256 key");
    let nonce_bytes = generate_nonce();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .expect("encryption failed");
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    BASE64.encode(&combined)
}

/// Decrypt a string encrypted with `encrypt()`.
pub fn decrypt(encoded: &str, secret: &str) -> Result<String, String> {
    let key = derive_key(secret);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Invalid key: {}", e))?;
    let combined = BASE64
        .decode(encoded)
        .map_err(|e| format!("Invalid base64: {}", e))?;
    if combined.len() < 12 {
        return Err("Ciphertext too short".to_string());
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;
    String::from_utf8(plaintext).map_err(|e| format!("Invalid UTF-8: {}", e))
}

/// Versioned prefix for newly encrypted values.
const ENC_PREFIX: &str = "enc:v1:";
/// Historical format used before persistent encryption keys were separated
/// from the JWT signing secret.
const LEGACY_ENC_PREFIX: &str = "enc:";

/// Encrypt and prefix the result for identification.
pub fn encrypt_store(plaintext: &str, secret: &str) -> String {
    format!("{}{}", ENC_PREFIX, encrypt(plaintext, secret))
}

/// Decrypt a value that was stored with `encrypt_store()`.
///
/// Plaintext values are returned unchanged for migration compatibility.
/// Encrypted values fail closed: callers must never forward ciphertext as a
/// credential when the configured key is wrong.
pub fn decrypt_load(encoded: &str, secret: &str) -> Result<String, String> {
    if let Some(rest) = encoded.strip_prefix(ENC_PREFIX) {
        decrypt(rest, secret)
    } else if let Some(rest) = encoded.strip_prefix(LEGACY_ENC_PREFIX) {
        decrypt(rest, secret)
    } else {
        Ok(encoded.to_string())
    }
}

/// Decrypt a stored value with the primary encryption key, falling back to
/// the legacy JWT secret. The boolean indicates that the value must be
/// rewritten with the primary key.
pub fn decrypt_for_migration(
    encoded: &str,
    primary_key: &str,
    fallback_keys: &[&str],
) -> Result<(String, bool), String> {
    let (rest, current_format) = if let Some(rest) = encoded.strip_prefix(ENC_PREFIX) {
        (rest, true)
    } else if let Some(rest) = encoded.strip_prefix(LEGACY_ENC_PREFIX) {
        (rest, false)
    } else {
        return Ok((encoded.to_string(), !encoded.is_empty()));
    };

    if let Ok(plaintext) = decrypt(rest, primary_key) {
        return Ok((plaintext, !current_format));
    }
    for fallback_key in fallback_keys {
        if *fallback_key != primary_key {
            if let Ok(plaintext) = decrypt(rest, fallback_key) {
                return Ok((plaintext, true));
            }
        }
    }
    Err("value cannot be decrypted with any configured encryption key".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_values_fail_closed_with_wrong_key() {
        let stored = encrypt_store("upstream-secret", "correct-encryption-key");
        assert!(decrypt_load(&stored, "wrong-encryption-key").is_err());
    }

    #[test]
    fn legacy_ciphertext_is_marked_for_migration() {
        let stored = format!(
            "{}{}",
            LEGACY_ENC_PREFIX,
            encrypt("upstream-secret", "legacy-jwt-secret")
        );
        let (plaintext, needs_migration) = decrypt_for_migration(
            &stored,
            "new-independent-encryption-key",
            &["legacy-jwt-secret"],
        )
        .unwrap();
        assert_eq!(plaintext, "upstream-secret");
        assert!(needs_migration);
    }

    #[test]
    fn primary_ciphertext_does_not_need_migration() {
        let stored = encrypt_store("upstream-secret", "primary-encryption-key");
        let (plaintext, needs_migration) =
            decrypt_for_migration(&stored, "primary-encryption-key", &["legacy-jwt-secret"])
                .unwrap();
        assert_eq!(plaintext, "upstream-secret");
        assert!(!needs_migration);
    }

    #[test]
    fn plaintext_is_marked_for_migration() {
        let (plaintext, needs_migration) =
            decrypt_for_migration("legacy-plaintext", "primary-key", &["jwt-secret"]).unwrap();
        assert_eq!(plaintext, "legacy-plaintext");
        assert!(needs_migration);
    }

    #[test]
    fn previous_encryption_key_can_be_rotated() {
        let stored = encrypt_store("upstream-secret", "previous-encryption-key");
        let (plaintext, needs_migration) = decrypt_for_migration(
            &stored,
            "current-encryption-key",
            &["previous-encryption-key", "legacy-jwt-secret"],
        )
        .unwrap();
        assert_eq!(plaintext, "upstream-secret");
        assert!(needs_migration);
    }
}
