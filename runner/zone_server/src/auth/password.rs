//! Password hashing and verification
//!
//! Uses Argon2id for secure password hashing.

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
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
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| PasswordError::HashingFailed)?;

    Ok(hash.to_string())
}

/// Verify a password against a hash
pub fn verify_password(password: &str, hash: &str) -> Result<bool, PasswordError> {
    let parsed_hash = PasswordHash::new(hash).map_err(|_| PasswordError::InvalidHash)?;

    let argon2 = Argon2::default();

    match argon2.verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(_) => Err(PasswordError::VerificationFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let password = "my-secure-password-123";
        let hash = hash_password(password).unwrap();

        // Hash should not equal plaintext
        assert_ne!(hash, password);

        // Verification should succeed
        assert!(verify_password(password, &hash).unwrap());

        // Wrong password should fail
        assert!(!verify_password("wrong-password", &hash).unwrap());
    }

    #[test]
    fn test_different_hashes() {
        let password = "same-password";
        let hash1 = hash_password(password).unwrap();
        let hash2 = hash_password(password).unwrap();

        // Same password should produce different hashes (due to salt)
        assert_ne!(hash1, hash2);

        // Both should verify correctly
        assert!(verify_password(password, &hash1).unwrap());
        assert!(verify_password(password, &hash2).unwrap());
    }

    // ============ Empty password handling tests ============

    #[test]
    fn test_empty_password_hash() {
        let password = "";
        let result = hash_password(password);

        // Empty password should still be hashable
        assert!(result.is_ok());

        let hash = result.unwrap();
        assert!(!hash.is_empty());
    }

    #[test]
    fn test_empty_password_verify() {
        let password = "";
        let hash = hash_password(password).unwrap();

        // Empty password should verify correctly
        assert!(verify_password(password, &hash).unwrap());

        // Non-empty password should not verify against empty password hash
        assert!(!verify_password("not-empty", &hash).unwrap());
    }

    #[test]
    fn test_empty_password_vs_whitespace() {
        let empty_password = "";
        let space_password = " ";
        let tab_password = "\t";

        let empty_hash = hash_password(empty_password).unwrap();
        let space_hash = hash_password(space_password).unwrap();

        // Empty and space should be treated as different passwords
        assert!(!verify_password(space_password, &empty_hash).unwrap());
        assert!(!verify_password(empty_password, &space_hash).unwrap());
        assert!(!verify_password(tab_password, &empty_hash).unwrap());
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
        let unicode_password = "password";
        let hash = hash_password(unicode_password).unwrap();

        assert!(verify_password(unicode_password, &hash).unwrap());
    }

    #[test]
    fn test_unicode_password_emoji() {
        let emoji_password = "hello\u{1F600}world";
        let hash = hash_password(emoji_password).unwrap();

        assert!(verify_password(emoji_password, &hash).unwrap());
        assert!(!verify_password("helloworld", &hash).unwrap());
    }

    #[test]
    fn test_unicode_password_chinese() {
        let chinese_password = "nihao";
        let hash = hash_password(chinese_password).unwrap();

        assert!(verify_password(chinese_password, &hash).unwrap());
    }

    #[test]
    fn test_unicode_password_mixed() {
        let mixed_password = "Pass123!";
        let hash = hash_password(mixed_password).unwrap();

        assert!(verify_password(mixed_password, &hash).unwrap());
    }

    #[test]
    fn test_unicode_normalization() {
        // Test with combining characters vs precomposed
        // "e" + combining acute accent vs precomposed "e"
        let combining = "e\u{0301}"; // e + combining acute accent
        let precomposed = "\u{00E9}"; // precomposed e

        let hash_combining = hash_password(combining).unwrap();
        let hash_precomposed = hash_password(precomposed).unwrap();

        // These are different byte sequences, so should be treated as different passwords
        assert!(verify_password(combining, &hash_combining).unwrap());
        assert!(verify_password(precomposed, &hash_precomposed).unwrap());

        // Cross verification should fail (they are different byte sequences)
        assert!(!verify_password(precomposed, &hash_combining).unwrap());
        assert!(!verify_password(combining, &hash_precomposed).unwrap());
    }

    #[test]
    fn test_unicode_null_byte() {
        // Password with embedded null byte
        let password_with_null = "pass\0word";
        let hash = hash_password(password_with_null).unwrap();

        assert!(verify_password(password_with_null, &hash).unwrap());
        assert!(!verify_password("password", &hash).unwrap());
        assert!(!verify_password("pass", &hash).unwrap());
    }

    // ============ Invalid hash format error tests ============

    #[test]
    fn test_invalid_hash_format_empty() {
        let result = verify_password("password", "");
        assert!(matches!(result, Err(PasswordError::InvalidHash)));
    }

    #[test]
    fn test_invalid_hash_format_garbage() {
        let result = verify_password("password", "not-a-valid-hash");
        assert!(matches!(result, Err(PasswordError::InvalidHash)));
    }

    #[test]
    fn test_invalid_hash_format_partial() {
        // Partial Argon2 hash format
        let result = verify_password("password", "$argon2id$");
        assert!(matches!(result, Err(PasswordError::InvalidHash)));
    }

    #[test]
    fn test_invalid_hash_format_wrong_algorithm() {
        // bcrypt-style hash (wrong algorithm)
        let result = verify_password("password", "$2a$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/X4f");
        assert!(matches!(result, Err(PasswordError::InvalidHash)));
    }

    #[test]
    fn test_invalid_hash_format_corrupted_argon2() {
        // Argon2-like but corrupted
        let result = verify_password(
            "password",
            "$argon2id$v=19$m=65536,t=3,p=4$INVALID_BASE64$ALSO_INVALID",
        );
        assert!(matches!(result, Err(PasswordError::InvalidHash)));
    }

    #[test]
    fn test_valid_hash_wrong_password() {
        let password = "correct-password";
        let hash = hash_password(password).unwrap();

        // Verify that a wrong password returns Ok(false), not an error
        let result = verify_password("wrong-password", &hash);
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
        let password = "   \t\n  ";
        let hash = hash_password(password).unwrap();

        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("", &hash).unwrap());
        assert!(!verify_password(" ", &hash).unwrap());
    }

    #[test]
    fn test_password_with_special_characters() {
        let password = r#"!@#$%^&*()_+-=[]{}|;':",./<>?`~"#;
        let hash = hash_password(password).unwrap();

        assert!(verify_password(password, &hash).unwrap());
    }

    #[test]
    fn test_password_with_newlines() {
        let password = "line1\nline2\r\nline3";
        let hash = hash_password(password).unwrap();

        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("line1line2line3", &hash).unwrap());
    }

    #[test]
    fn test_hash_output_format() {
        let password = "test-password";
        let hash = hash_password(password).unwrap();

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
        let lowercase = "password";
        let uppercase = "PASSWORD";
        let mixed = "PaSsWoRd";

        let hash = hash_password(lowercase).unwrap();

        assert!(verify_password(lowercase, &hash).unwrap());
        assert!(!verify_password(uppercase, &hash).unwrap());
        assert!(!verify_password(mixed, &hash).unwrap());
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
        let password = "my-password";
        let hash = hash_password(password).unwrap();

        // Multiple verification attempts should all succeed
        for _ in 0..10 {
            assert!(verify_password(password, &hash).unwrap());
        }
    }
}
