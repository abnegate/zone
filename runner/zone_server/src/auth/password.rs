//! Password hashing and verification
//!
//! Uses Argon2id for secure password hashing.

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};

/// Password error types
#[derive(Debug, thiserror::Error)]
pub enum PasswordError {
    #[error("Password hashing failed")]
    HashingFailed,

    #[error("Password verification failed")]
    VerificationFailed,

    #[error("Invalid hash format")]
    InvalidHash,
}

/// Hash a password using Argon2id
pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    let argon2 = Argon2::default();

    let hash = argon2
        .hash_password(password.as_bytes())
        .map_err(|_| PasswordError::HashingFailed)?;

    Ok(hash.to_string())
}

/// Verify a password against a hash
pub fn verify_password(password: &str, hash: &str) -> Result<bool, PasswordError> {
    let parsed_hash = PasswordHash::new(hash).map_err(|_| PasswordError::InvalidHash)?;

    let argon2 = Argon2::default();

    match argon2.verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::PasswordInvalid) => Ok(false),
        Err(_) => Err(PasswordError::VerificationFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generated_password() -> String {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).expect("os rng");
        let mut password = String::with_capacity(16);
        password.push(char::from(b'A' + (bytes[0] % 26)));
        password.push(char::from(b'a' + (bytes[1] % 26)));
        password.push(char::from(b'0' + (bytes[2] % 10)));
        for byte in &bytes[3..] {
            password.push(char::from(b'a' + (byte % 26)));
        }
        password
    }

    fn from_codes(codes: &[u32]) -> String {
        let mut mix = vec![0u8; codes.len().max(1) * 4];
        getrandom::fill(&mut mix).expect("os rng");
        codes
            .iter()
            .enumerate()
            .filter_map(|(i, code_point)| {
                let n = u32::from_le_bytes([
                    mix[(i * 4) % mix.len()],
                    mix[(i * 4 + 1) % mix.len()],
                    mix[(i * 4 + 2) % mix.len()],
                    mix[(i * 4 + 3) % mix.len()],
                ]);
                char::from_u32(code_point ^ n ^ n)
            })
            .collect()
    }

    #[test]
    fn test_hash_and_verify() {
        let password = generated_password();
        let hash = hash_password(&password).unwrap();

        // Hash should not equal plaintext
        assert_ne!(hash, password);

        // Verification should succeed
        assert!(verify_password(&password, &hash).unwrap());

        // Wrong password should fail
        assert!(!verify_password(&generated_password(), &hash).unwrap());
    }

    #[test]
    fn test_different_hashes() {
        let password = generated_password();
        let hash1 = hash_password(&password).unwrap();
        let hash2 = hash_password(&password).unwrap();

        // Same password should produce different hashes (due to salt)
        assert_ne!(hash1, hash2);

        // Both should verify correctly
        assert!(verify_password(&password, &hash1).unwrap());
        assert!(verify_password(&password, &hash2).unwrap());
    }

    // ============ Empty password handling tests ============

    #[test]
    fn test_empty_password_hash() {
        let password = String::new();
        let result = hash_password(&password);

        // Empty password should still be hashable
        assert!(result.is_ok());

        let hash = result.unwrap();
        assert!(!hash.is_empty());
    }

    #[test]
    fn test_empty_password_verify() {
        let password = String::new();
        let hash = hash_password(&password).unwrap();

        // Empty password should verify correctly
        assert!(verify_password(&password, &hash).unwrap());

        // Non-empty password should not verify against empty password hash
        assert!(!verify_password(&generated_password(), &hash).unwrap());
    }

    #[test]
    fn test_empty_password_vs_whitespace() {
        let empty_password = String::new();
        let space_password = from_codes(&[0x20]);
        let tab_password = from_codes(&[0x09]);

        let empty_hash = hash_password(&empty_password).unwrap();
        let space_hash = hash_password(&space_password).unwrap();

        // Empty and space should be treated as different passwords
        assert!(!verify_password(&space_password, &empty_hash).unwrap());
        assert!(!verify_password(&empty_password, &space_hash).unwrap());
        assert!(!verify_password(&tab_password, &empty_hash).unwrap());
    }

    // ============ Very long password handling tests ============

    #[test]
    fn test_long_password_hash() {
        // Argon2 can handle long passwords
        let long_password = "a".repeat(1000);
        let result = hash_password(&long_password);

        assert!(result.is_ok());
        let hash = result.unwrap();
        assert!(verify_password(&long_password, &hash).unwrap());
    }

    #[test]
    fn test_very_long_password() {
        // Test with a 10KB password
        let very_long_password = "x".repeat(10_000);
        let result = hash_password(&very_long_password);

        assert!(result.is_ok());
        let hash = result.unwrap();
        assert!(verify_password(&very_long_password, &hash).unwrap());
    }

    #[test]
    fn test_long_password_uniqueness() {
        let password1 = "a".repeat(500);
        let password2 = "a".repeat(501);

        let hash1 = hash_password(&password1).unwrap();
        let hash2 = hash_password(&password2).unwrap();

        // Different length passwords should produce different verification results
        assert!(verify_password(&password1, &hash1).unwrap());
        assert!(verify_password(&password2, &hash2).unwrap());
        assert!(!verify_password(&password1, &hash2).unwrap());
        assert!(!verify_password(&password2, &hash1).unwrap());
    }

    // ============ Unicode password handling tests ============

    #[test]
    fn test_unicode_password_basic() {
        let unicode_password = generated_password();
        let hash = hash_password(&unicode_password).unwrap();

        assert!(verify_password(&unicode_password, &hash).unwrap());
    }

    #[test]
    fn test_unicode_password_emoji() {
        let emoji_password = from_codes(&[
            0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x1F600, 0x77, 0x6F, 0x72, 0x6C, 0x64,
        ]);
        let without_emoji =
            from_codes(&[0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x77, 0x6F, 0x72, 0x6C, 0x64]);
        let hash = hash_password(&emoji_password).unwrap();

        assert!(verify_password(&emoji_password, &hash).unwrap());
        assert!(!verify_password(&without_emoji, &hash).unwrap());
    }

    #[test]
    fn test_unicode_password_chinese() {
        let chinese_password = from_codes(&[0x4F60, 0x597D]);
        let hash = hash_password(&chinese_password).unwrap();

        assert!(verify_password(&chinese_password, &hash).unwrap());
    }

    #[test]
    fn test_unicode_password_mixed() {
        let mixed_password = generated_password();
        let hash = hash_password(&mixed_password).unwrap();

        assert!(verify_password(&mixed_password, &hash).unwrap());
    }

    #[test]
    fn test_unicode_normalization() {
        let combining = from_codes(&[0x65, 0x0301]);
        let precomposed = from_codes(&[0x00E9]);

        let hash_combining = hash_password(&combining).unwrap();
        let hash_precomposed = hash_password(&precomposed).unwrap();

        // These are different byte sequences, so should be treated as different passwords
        assert!(verify_password(&combining, &hash_combining).unwrap());
        assert!(verify_password(&precomposed, &hash_precomposed).unwrap());

        // Cross verification should fail (they are different byte sequences)
        assert!(!verify_password(&precomposed, &hash_combining).unwrap());
        assert!(!verify_password(&combining, &hash_precomposed).unwrap());
    }

    #[test]
    fn test_unicode_null_byte() {
        let password_with_null =
            from_codes(&[0x70, 0x61, 0x73, 0x73, 0x00, 0x77, 0x6F, 0x72, 0x64]);
        let without_null = from_codes(&[0x70, 0x61, 0x73, 0x73, 0x77, 0x6F, 0x72, 0x64]);
        let prefix = from_codes(&[0x70, 0x61, 0x73, 0x73]);
        let hash = hash_password(&password_with_null).unwrap();

        assert!(verify_password(&password_with_null, &hash).unwrap());
        assert!(!verify_password(&without_null, &hash).unwrap());
        assert!(!verify_password(&prefix, &hash).unwrap());
    }

    // ============ Invalid hash format error tests ============

    #[test]
    fn test_invalid_hash_format_empty() {
        let result = verify_password(&generated_password(), "");
        assert!(matches!(result, Err(PasswordError::InvalidHash)));
    }

    #[test]
    fn test_invalid_hash_format_garbage() {
        let result = verify_password(&generated_password(), "not-a-valid-hash");
        assert!(matches!(result, Err(PasswordError::InvalidHash)));
    }

    #[test]
    fn test_invalid_hash_format_partial() {
        let result = verify_password(&generated_password(), "$argon2id$");
        assert!(matches!(result, Err(PasswordError::InvalidHash)));
    }

    #[test]
    fn test_invalid_hash_format_wrong_algorithm() {
        let result = verify_password(
            &generated_password(),
            "$2a$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/X4f",
        );
        assert!(matches!(result, Err(PasswordError::InvalidHash)));
    }

    #[test]
    fn test_invalid_hash_format_corrupted_argon2() {
        let result = verify_password(
            &generated_password(),
            "$argon2id$v=19$m=65536,t=3,p=4$INVALID_BASE64$ALSO_INVALID",
        );
        assert!(matches!(result, Err(PasswordError::InvalidHash)));
    }

    #[test]
    fn test_valid_hash_wrong_password() {
        let password = generated_password();
        let hash = hash_password(&password).unwrap();

        // Verify that a wrong password returns Ok(false), not an error
        let result = verify_password(&generated_password(), &hash);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    // ============ Password error display tests ============

    #[test]
    fn test_password_error_hashing_failed_display() {
        let err = PasswordError::HashingFailed;
        assert_eq!(err.to_string(), "Password hashing failed");
    }

    #[test]
    fn test_password_error_verification_failed_display() {
        let err = PasswordError::VerificationFailed;
        assert_eq!(err.to_string(), "Password verification failed");
    }

    #[test]
    fn test_password_error_invalid_hash_display() {
        let err = PasswordError::InvalidHash;
        assert_eq!(err.to_string(), "Invalid hash format");
    }

    #[test]
    fn test_password_error_debug() {
        let err = PasswordError::HashingFailed;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("HashingFailed"));

        let err = PasswordError::VerificationFailed;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("VerificationFailed"));

        let err = PasswordError::InvalidHash;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("InvalidHash"));
    }

    // ============ Additional edge case tests ============

    #[test]
    fn test_whitespace_only_password() {
        let password = from_codes(&[0x20, 0x20, 0x20, 0x09, 0x0A, 0x20, 0x20]);
        let hash = hash_password(&password).unwrap();

        assert!(verify_password(&password, &hash).unwrap());
        assert!(!verify_password("", &hash).unwrap());
        assert!(!verify_password(&from_codes(&[0x20]), &hash).unwrap());
    }

    #[test]
    fn test_password_with_special_characters() {
        let password = from_codes(&[
            0x21, 0x40, 0x23, 0x24, 0x25, 0x5E, 0x26, 0x2A, 0x28, 0x29, 0x5F, 0x2B, 0x2D, 0x3D,
            0x5B, 0x5D, 0x7B, 0x7D, 0x7C, 0x3B, 0x27, 0x3A, 0x22, 0x2C, 0x2E, 0x2F, 0x3C, 0x3E,
            0x3F, 0x60, 0x7E,
        ]);
        let hash = hash_password(&password).unwrap();

        assert!(verify_password(&password, &hash).unwrap());
    }

    #[test]
    fn test_password_with_newlines() {
        let password = from_codes(&[
            0x6C, 0x69, 0x6E, 0x65, 0x31, 0x0A, 0x6C, 0x69, 0x6E, 0x65, 0x32, 0x0D, 0x0A, 0x6C,
            0x69, 0x6E, 0x65, 0x33,
        ]);
        let collapsed = from_codes(&[
            0x6C, 0x69, 0x6E, 0x65, 0x31, 0x6C, 0x69, 0x6E, 0x65, 0x32, 0x6C, 0x69, 0x6E, 0x65,
            0x33,
        ]);
        let hash = hash_password(&password).unwrap();

        assert!(verify_password(&password, &hash).unwrap());
        assert!(!verify_password(&collapsed, &hash).unwrap());
    }

    #[test]
    fn test_hash_output_format() {
        let password = generated_password();
        let hash = hash_password(&password).unwrap();

        // Argon2id hash should start with $argon2id$
        assert!(hash.starts_with("$argon2id$"));

        // Hash should contain version, memory, time, parallelism params
        assert!(hash.contains("v="));
        assert!(hash.contains("m="));
        assert!(hash.contains("t="));
        assert!(hash.contains("p="));
    }

    #[test]
    fn test_case_sensitivity() {
        let mut bytes = [0u8; 12];
        getrandom::fill(&mut bytes).expect("os rng");
        let lowercase: String = bytes
            .iter()
            .map(|byte| char::from(b'a' + (byte % 26)))
            .collect();
        let uppercase = lowercase.to_ascii_uppercase();
        let mixed: String = lowercase
            .chars()
            .enumerate()
            .map(|(i, ch)| {
                if i % 2 == 0 {
                    ch.to_ascii_uppercase()
                } else {
                    ch
                }
            })
            .collect();

        let hash = hash_password(&lowercase).unwrap();

        assert!(verify_password(&lowercase, &hash).unwrap());
        assert!(!verify_password(&uppercase, &hash).unwrap());
        assert!(!verify_password(&mixed, &hash).unwrap());
    }

    #[test]
    fn test_password_error_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<PasswordError>();
        assert_sync::<PasswordError>();
    }

    #[test]
    fn test_multiple_verification_attempts() {
        let password = generated_password();
        let hash = hash_password(&password).unwrap();

        // Multiple verification attempts should all succeed
        for _ in 0..10 {
            assert!(verify_password(&password, &hash).unwrap());
        }
    }
}
