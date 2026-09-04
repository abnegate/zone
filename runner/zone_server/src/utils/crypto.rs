//! Cryptographic utilities for token generation and hashing
//!
//! This module provides shared functions for:
//! - Generating secure random tokens
//! - Hashing tokens for secure storage using SHA-256

use sha2::{Digest, Sha256};

/// Generate a secure random token
///
/// Generates a 32-byte random token and encodes it as a 64-character hex string.
/// Each call produces a unique, cryptographically secure random token.
///
/// # Returns
/// A 64-character hex-encoded string representing 32 random bytes
///
/// # Examples
/// ```
/// use zone_server::utils::crypto::generate_token;
/// let token = generate_token();
/// assert_eq!(token.len(), 64);
/// ```
pub fn generate_token() -> String {
    let mut random_bytes = [0u8; 32];
    rand::fill(&mut random_bytes);
    hex::encode(random_bytes)
}

/// Hash a token using SHA-256
///
/// Tokens are hashed before storage so that if the database is compromised,
/// the attacker cannot use the tokens directly. This function produces a
/// deterministic hash that can be used to verify tokens later.
///
/// # Arguments
/// * `token` - The plain token string to hash
///
/// # Returns
/// A 64-character hex-encoded SHA-256 hash of the token
///
/// # Examples
/// ```
/// use zone_server::utils::crypto::hash_token;
/// let token = "my-secret-token";
/// let hash = hash_token(token);
/// assert_eq!(hash.len(), 64);
/// ```
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token_is_unique() {
        let token1 = generate_token();
        let token2 = generate_token();
        assert_ne!(token1, token2, "Tokens should be unique");
    }

    #[test]
    fn test_generate_token_is_hex_encoded() {
        let token = generate_token();
        assert_eq!(
            token.len(),
            64,
            "Token should be 64 hex characters (32 bytes)"
        );
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit()),
            "Token should be hex"
        );
    }

    #[test]
    fn test_hash_token_is_deterministic() {
        let token = "test-token-123";
        let hash1 = hash_token(token);
        let hash2 = hash_token(token);
        assert_eq!(hash1, hash2, "Same token should produce same hash");
    }

    #[test]
    fn test_hash_token_is_different_from_input() {
        let token = "test-token-123";
        let hash = hash_token(token);
        assert_ne!(token, hash, "Hash should be different from input");
    }

    #[test]
    fn test_hash_token_produces_sha256_length() {
        let token = "test-token-123";
        let hash = hash_token(token);
        // SHA-256 produces 32 bytes = 64 hex characters
        assert_eq!(hash.len(), 64, "SHA-256 hash should be 64 hex characters");
    }
}
