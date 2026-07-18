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
    let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes()).expect("encryption failed");
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    BASE64.encode(&combined)
}

/// Decrypt a string encrypted with `encrypt()`.
pub fn decrypt(encoded: &str, secret: &str) -> Result<String, String> {
    let key = derive_key(secret);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Invalid key: {}", e))?;
    let combined = BASE64.decode(encoded).map_err(|e| format!("Invalid base64: {}", e))?;
    if combined.len() < 12 {
        return Err("Ciphertext too short".to_string());
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|e| format!("Decryption failed: {}", e))?;
    String::from_utf8(plaintext).map_err(|e| format!("Invalid UTF-8: {}", e))
}

/// Prefix used to identify encrypted values.
const ENC_PREFIX: &str = "enc:";

/// Encrypt and prefix the result for identification.
pub fn encrypt_store(plaintext: &str, secret: &str) -> String {
    format!("{}{}", ENC_PREFIX, encrypt(plaintext, secret))
}

/// Decrypt a value that was stored with `encrypt_store()`.
/// Returns `None` if the value is not encrypted (plaintext fallback for backward compat).
pub fn decrypt_load(encoded: &str, secret: &str) -> String {
    if let Some(rest) = encoded.strip_prefix(ENC_PREFIX) {
        decrypt(rest, secret).unwrap_or_else(|_| encoded.to_string())
    } else {
        encoded.to_string()
    }
}
