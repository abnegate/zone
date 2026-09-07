//! AES-256-GCM envelope for credentials at rest.
//!
//! An encrypted credential is stored as `ENC[v1:<base64(nonce || ciphertext ||
//! tag)>]` with a fresh 12-byte nonce per value. Plaintext is still accepted so
//! values written before the envelope existed keep working; each one is
//! reported through `tracing::warn!` as it is read.

use std::fmt;

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, consts::U12},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use thiserror::Error;
use zeroize::Zeroize;

use super::redact::REDACTED;
use super::value::SecretValue;

const ENVELOPE_PREFIX: &str = "ENC[v1:";
const ENVELOPE_SUFFIX: &str = "]";
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const KEY_BYTES: usize = 32;

/// Failure to wrap or unwrap an [`ENVELOPE_PREFIX`] envelope.
#[derive(Debug, Error)]
pub enum SecretError {
    #[error("Encryption failed")]
    Encryption,
    #[error("Decryption failed: invalid key or corrupted envelope")]
    Decryption,
    #[error("Invalid base64 in encrypted value")]
    InvalidBase64,
    #[error("Encrypted value is shorter than a nonce and tag")]
    Truncated,
    #[error("Decrypted value is not valid UTF-8")]
    InvalidUtf8,
}

/// The AES-256 key every envelope in a deployment is sealed with.
pub struct MasterKey {
    key: [u8; KEY_BYTES],
}

impl MasterKey {
    pub fn new(key: [u8; KEY_BYTES]) -> Self {
        Self { key }
    }

    pub fn generate() -> Self {
        let mut key = [0u8; KEY_BYTES];
        rand::fill(&mut key);
        Self { key }
    }

    fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.key
    }
}

impl Drop for MasterKey {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl fmt::Debug for MasterKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "MasterKey({REDACTED})")
    }
}

/// Whether a stored value is already wrapped in an envelope.
pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(ENVELOPE_PREFIX) && value.ends_with(ENVELOPE_SUFFIX)
}

/// Wrap a credential in an envelope sealed with `key`.
pub fn encrypt(value: &SecretValue, key: &MasterKey) -> Result<String, SecretError> {
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).map_err(|_| SecretError::Encryption)?;

    let mut nonce_bytes = [0u8; NONCE_BYTES];
    rand::fill(&mut nonce_bytes);
    let nonce =
        Nonce::<U12>::try_from(nonce_bytes.as_slice()).map_err(|_| SecretError::Encryption)?;

    let ciphertext = cipher
        .encrypt(&nonce, value.expose().as_bytes())
        .map_err(|_| SecretError::Encryption)?;

    let mut sealed = Vec::with_capacity(NONCE_BYTES + ciphertext.len());
    sealed.extend_from_slice(&nonce_bytes);
    sealed.extend_from_slice(&ciphertext);

    Ok(format!(
        "{ENVELOPE_PREFIX}{}{ENVELOPE_SUFFIX}",
        BASE64.encode(&sealed)
    ))
}

/// Unwrap a stored value, passing plaintext straight through.
pub fn decrypt(value: &str, key: &MasterKey) -> Result<SecretValue, SecretError> {
    if !is_encrypted(value) {
        tracing::warn!(
            "Plaintext secret read from storage; re-save it to seal it in an ENC[v1:...] envelope"
        );
        return Ok(SecretValue::new(value));
    }

    let encoded = &value[ENVELOPE_PREFIX.len()..value.len() - ENVELOPE_SUFFIX.len()];
    let sealed = BASE64
        .decode(encoded)
        .map_err(|_| SecretError::InvalidBase64)?;

    if sealed.len() < NONCE_BYTES + TAG_BYTES {
        return Err(SecretError::Truncated);
    }

    let (nonce_bytes, ciphertext) = sealed.split_at(NONCE_BYTES);
    let nonce = Nonce::<U12>::try_from(nonce_bytes).map_err(|_| SecretError::Decryption)?;

    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).map_err(|_| SecretError::Decryption)?;
    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| SecretError::Decryption)?;

    match String::from_utf8(plaintext) {
        Ok(text) => Ok(SecretValue::new(text)),
        Err(error) => {
            let mut bytes = error.into_bytes();
            bytes.zeroize();
            Err(SecretError::InvalidUtf8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_credential() {
        let key = MasterKey::generate();
        let secret = SecretValue::new("sk-live-0123456789abcdef");

        let sealed = encrypt(&secret, &key).unwrap();
        assert!(is_encrypted(&sealed));
        assert!(sealed.starts_with(ENVELOPE_PREFIX));
        assert!(sealed.ends_with(ENVELOPE_SUFFIX));
        assert!(!sealed.contains(secret.expose()));

        assert_eq!(decrypt(&sealed, &key).unwrap(), secret);
    }

    #[test]
    fn round_trips_an_empty_value() {
        let key = MasterKey::generate();
        let sealed = encrypt(&SecretValue::new(""), &key).unwrap();
        assert!(decrypt(&sealed, &key).unwrap().is_empty());
    }

    #[test]
    fn round_trips_multibyte_text() {
        let key = MasterKey::generate();
        let secret = SecretValue::new("clé-secrète-🔐-中文");
        let sealed = encrypt(&secret, &key).unwrap();
        assert_eq!(decrypt(&sealed, &key).unwrap(), secret);
    }

    #[test]
    fn passes_plaintext_through() {
        let key = MasterKey::generate();
        let plaintext = "written-before-the-envelope-existed";
        assert_eq!(decrypt(plaintext, &key).unwrap().expose(), plaintext);
    }

    #[test]
    fn every_envelope_uses_a_fresh_nonce() {
        let key = MasterKey::generate();
        let secret = SecretValue::new("same-secret");

        let first = encrypt(&secret, &key).unwrap();
        let second = encrypt(&secret, &key).unwrap();

        assert_ne!(first, second);
        assert_eq!(decrypt(&first, &key).unwrap(), secret);
        assert_eq!(decrypt(&second, &key).unwrap(), secret);
    }

    #[test]
    fn rejects_the_wrong_key() {
        let sealed = encrypt(&SecretValue::new("secret"), &MasterKey::generate()).unwrap();
        assert!(matches!(
            decrypt(&sealed, &MasterKey::generate()),
            Err(SecretError::Decryption)
        ));
    }

    #[test]
    fn rejects_a_tampered_envelope() {
        let key = MasterKey::generate();
        let sealed = encrypt(&SecretValue::new("secret"), &key).unwrap();
        let mut tampered = sealed.into_bytes();
        let last = tampered.len() - 2;
        tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };

        let tampered = String::from_utf8(tampered).unwrap();
        assert!(decrypt(&tampered, &key).is_err());
    }

    #[test]
    fn rejects_invalid_base64() {
        let key = MasterKey::generate();
        assert!(matches!(
            decrypt("ENC[v1:not!valid@base64#]", &key),
            Err(SecretError::InvalidBase64)
        ));
    }

    #[test]
    fn rejects_a_truncated_envelope() {
        let key = MasterKey::generate();
        let short = format!(
            "{ENVELOPE_PREFIX}{}{ENVELOPE_SUFFIX}",
            BASE64.encode(b"short")
        );
        assert!(matches!(decrypt(&short, &key), Err(SecretError::Truncated)));
    }

    #[test]
    fn recognises_the_envelope_format() {
        assert!(is_encrypted("ENC[v1:YWJj]"));
        assert!(!is_encrypted("plaintext"));
        assert!(!is_encrypted("ENC[v1:missing-suffix"));
        assert!(!is_encrypted("WRONG[v1:YWJj]"));
    }

    #[test]
    fn master_key_debug_is_redacted() {
        let key = MasterKey::new([7u8; KEY_BYTES]);
        assert_eq!(format!("{key:?}"), "MasterKey([REDACTED])");
    }

    #[test]
    fn generated_keys_differ() {
        assert_ne!(
            MasterKey::generate().as_bytes(),
            MasterKey::generate().as_bytes()
        );
    }
}
