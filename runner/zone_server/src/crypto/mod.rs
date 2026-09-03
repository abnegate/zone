//! Cryptographic utilities for secure credential storage
//!
//! Uses AES-256-GCM for authenticated encryption

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, consts::U12},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Invalid key length: expected 32 bytes, got {0}")]
    InvalidKeyLength(usize),
    #[error("Encryption failed")]
    EncryptionFailed,
    #[error("Decryption failed: invalid ciphertext or wrong key")]
    DecryptionFailed,
    #[error("Invalid base64 encoding")]
    InvalidBase64,
    #[error("Key derivation failed")]
    KeyDerivationFailed,
}

pub type CryptoResult<T> = Result<T, CryptoError>;

/// Encrypt plaintext using AES-256-GCM
/// Returns base64-encoded: nonce (12 bytes) || ciphertext
pub fn encrypt(key: &[u8], plaintext: &str) -> CryptoResult<String> {
    if key.len() != 32 {
        return Err(CryptoError::InvalidKeyLength(key.len()));
    }

    // Initialize cipher
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::InvalidKeyLength(key.len()))?;

    // Generate random 12-byte nonce
    let mut nonce_bytes = [0u8; 12];
    rand::fill(&mut nonce_bytes);
    let nonce = Nonce::<U12>::try_from(nonce_bytes.as_slice())
        .map_err(|_| CryptoError::EncryptionFailed)?;

    // Encrypt plaintext
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| CryptoError::EncryptionFailed)?;

    // Concatenate nonce and ciphertext
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    // Base64 encode the result
    Ok(BASE64.encode(&result))
}

/// Decrypt base64-encoded ciphertext using AES-256-GCM
/// Expects format: base64(nonce || ciphertext)
pub fn decrypt(key: &[u8], ciphertext: &str) -> CryptoResult<String> {
    if key.len() != 32 {
        return Err(CryptoError::InvalidKeyLength(key.len()));
    }

    // Initialize cipher
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::InvalidKeyLength(key.len()))?;

    // Base64 decode
    let encrypted_data = BASE64
        .decode(ciphertext)
        .map_err(|_| CryptoError::InvalidBase64)?;

    // Must have at least 12 bytes for nonce
    if encrypted_data.len() < 12 {
        return Err(CryptoError::DecryptionFailed);
    }

    // Split nonce and ciphertext
    let (nonce_bytes, ciphertext_bytes) = encrypted_data.split_at(12);
    let nonce = Nonce::<U12>::try_from(nonce_bytes).map_err(|_| CryptoError::DecryptionFailed)?;

    // Decrypt
    let plaintext = cipher
        .decrypt(&nonce, ciphertext_bytes)
        .map_err(|_| CryptoError::DecryptionFailed)?;

    // Convert to UTF-8 string
    String::from_utf8(plaintext).map_err(|_| CryptoError::DecryptionFailed)
}

/// Derive a 32-byte encryption key using Argon2id
/// This is slow by design to resist brute-force attacks
///
/// Uses Argon2id with a deterministic salt derived from the key itself.
/// This is acceptable because the key is meant to be high-entropy (32+ bytes).
pub fn derive_key(config_key: &str) -> CryptoResult<[u8; 32]> {
    use argon2::Argon2;
    use sha2::{Digest, Sha256};

    let key_bytes = config_key.as_bytes();

    if key_bytes.len() < 32 {
        return Err(CryptoError::InvalidKeyLength(key_bytes.len()));
    }

    // Use a fixed salt derived from the key itself for deterministic derivation
    // This is acceptable because the key is meant to be high-entropy
    let mut hasher = Sha256::new();
    hasher.update(b"zone-encryption-salt-v1");
    hasher.update(key_bytes);
    let salt_bytes = hasher.finalize();

    let argon2 = Argon2::default();
    let mut output = [0u8; 32];

    argon2
        .hash_password_into(
            key_bytes,
            &salt_bytes[..16], // Use first 16 bytes as salt
            &mut output,
        )
        .map_err(|_| CryptoError::KeyDerivationFailed)?;

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_produces_different_output_each_time() {
        // Given: A 32-byte key and plaintext
        let key = b"12345678901234567890123456789012"; // 32 bytes
        let plaintext = "my secret password";

        // When: Encrypting the same plaintext twice
        let encrypted1 = encrypt(key, plaintext).unwrap();
        let encrypted2 = encrypt(key, plaintext).unwrap();

        // Then: The ciphertexts should be different (due to random nonce)
        assert_ne!(
            encrypted1, encrypted2,
            "Encryption should produce different output each time due to random IV"
        );
    }

    #[test]
    fn test_decryption_recovers_original_value() {
        // Given: A 32-byte key and plaintext
        let key = b"12345678901234567890123456789012"; // 32 bytes
        let plaintext = "my secret password";

        // When: Encrypting and then decrypting
        let encrypted = encrypt(key, plaintext).unwrap();
        let decrypted = decrypt(key, &encrypted).unwrap();

        // Then: The decrypted value should match the original plaintext
        assert_eq!(
            decrypted, plaintext,
            "Round-trip encryption/decryption should recover original value"
        );
    }

    #[test]
    fn test_invalid_key_fails_decryption() {
        // Given: Two different keys
        let key1 = b"12345678901234567890123456789012"; // 32 bytes
        let key2 = b"abcdefghijklmnopqrstuvwxyz123456"; // 32 bytes
        let plaintext = "my secret password";

        // When: Encrypting with key1 and decrypting with key2
        let encrypted = encrypt(key1, plaintext).unwrap();
        let result = decrypt(key2, &encrypted);

        // Then: Decryption should fail
        assert!(result.is_err(), "Decryption with wrong key should fail");
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn test_derive_key_accepts_valid_length() {
        // Given: A key that is exactly 32 bytes
        let config_key = "12345678901234567890123456789012";

        // When: Deriving the key
        let result = derive_key(config_key);

        // Then: Should succeed
        assert!(result.is_ok(), "32-byte key should be accepted");

        // Given: A key that is longer than 32 bytes
        let long_key = "123456789012345678901234567890123456789012345678901234567890";

        // When: Deriving the key
        let result = derive_key(long_key);

        // Then: Should succeed (hashed to 32 bytes)
        assert!(
            result.is_ok(),
            "Keys longer than 32 bytes should be accepted and hashed"
        );
    }

    #[test]
    fn test_derive_key_rejects_short_key() {
        // Given: A key that is shorter than 32 bytes
        let short_key = "short";

        // When: Deriving the key
        let result = derive_key(short_key);

        // Then: Should fail with InvalidKeyLength error
        assert!(
            result.is_err(),
            "Keys shorter than 32 bytes should be rejected"
        );
        assert!(matches!(result, Err(CryptoError::InvalidKeyLength(_))));
    }

    #[test]
    fn test_encrypt_with_invalid_key_length() {
        // Given: A key that is not 32 bytes
        let short_key = b"short";
        let plaintext = "test";

        // When: Attempting to encrypt
        let result = encrypt(short_key, plaintext);

        // Then: Should fail
        assert!(result.is_err());
        assert!(matches!(result, Err(CryptoError::InvalidKeyLength(_))));
    }

    #[test]
    fn test_decrypt_with_invalid_key_length() {
        // Given: A valid encrypted string but invalid key length
        let short_key = b"short";
        let encrypted = "dGVzdA=="; // some base64 string

        // When: Attempting to decrypt
        let result = decrypt(short_key, encrypted);

        // Then: Should fail
        assert!(result.is_err());
        assert!(matches!(result, Err(CryptoError::InvalidKeyLength(_))));
    }

    #[test]
    fn test_decrypt_with_invalid_base64() {
        // Given: Valid key but invalid base64 string
        let key = b"12345678901234567890123456789012";
        let invalid_base64 = "not!valid@base64#";

        // When: Attempting to decrypt
        let result = decrypt(key, invalid_base64);

        // Then: Should fail with InvalidBase64 error
        assert!(result.is_err());
        assert!(matches!(result, Err(CryptoError::InvalidBase64)));
    }

    #[test]
    fn test_decrypt_with_truncated_data() {
        // Given: Valid key but data too short (< 12 bytes for nonce)
        let key = b"12345678901234567890123456789012";
        let short_data = BASE64.encode(b"short"); // Less than 12 bytes

        // When: Attempting to decrypt
        let result = decrypt(key, &short_data);

        // Then: Should fail with DecryptionFailed error
        assert!(result.is_err());
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn test_long_plaintext() {
        // Given: A long plaintext
        let key = b"12345678901234567890123456789012";
        let plaintext = "a".repeat(10000); // 10KB of data

        // When: Encrypting and decrypting
        let encrypted = encrypt(key, &plaintext).unwrap();
        let decrypted = decrypt(key, &encrypted).unwrap();

        // Then: Should handle large data correctly
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_special_characters() {
        // Given: Plaintext with special characters
        let key = b"12345678901234567890123456789012";
        let plaintext = "P@ssw0rd! with émojis 🔐 and unicode 中文";

        // When: Encrypting and decrypting
        let encrypted = encrypt(key, plaintext).unwrap();
        let decrypted = decrypt(key, &encrypted).unwrap();

        // Then: Should handle special characters correctly
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_empty_plaintext() {
        // Given: Empty plaintext
        let key = b"12345678901234567890123456789012";
        let plaintext = "";

        // When: Encrypting and decrypting
        let encrypted = encrypt(key, plaintext).unwrap();
        let decrypted = decrypt(key, &encrypted).unwrap();

        // Then: Should handle empty string correctly
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_derive_key_consistency() {
        // Given: The same config key used twice
        let config_key = "my-super-secret-encryption-key-that-is-quite-long";

        // When: Deriving the key twice
        let key1 = derive_key(config_key).unwrap();
        let key2 = derive_key(config_key).unwrap();

        // Then: Should produce the same result
        assert_eq!(key1, key2, "derive_key should be deterministic");
    }

    #[test]
    fn test_error_display() {
        // Test that error messages are meaningful
        let err = CryptoError::InvalidKeyLength(16);
        assert_eq!(
            err.to_string(),
            "Invalid key length: expected 32 bytes, got 16"
        );

        let err = CryptoError::EncryptionFailed;
        assert_eq!(err.to_string(), "Encryption failed");

        let err = CryptoError::DecryptionFailed;
        assert_eq!(
            err.to_string(),
            "Decryption failed: invalid ciphertext or wrong key"
        );

        let err = CryptoError::InvalidBase64;
        assert_eq!(err.to_string(), "Invalid base64 encoding");

        let err = CryptoError::KeyDerivationFailed;
        assert_eq!(err.to_string(), "Key derivation failed");
    }
}
