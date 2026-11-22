use std::fmt;
use std::num::NonZeroU32;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::Signer as _;
use ed25519_dalek::{SIGNATURE_LENGTH, SigningKey};

use crate::registry::jwk::Jwk;
use crate::signer::Signer;

pub const SALT: &[u8] = b"zone-license registry token";
pub const KID_PREFIX: &str = "registry";

pub struct TokenKey {
    key: SigningKey,
    kid: String,
}

impl TokenKey {
    pub fn derive(signer: &Signer, generation: NonZeroU32) -> Self {
        let key_id = signer.key_id();
        let seed = signer.derive(SALT, format!("{key_id}:{generation}").as_bytes());
        Self {
            key: SigningKey::from_bytes(&seed),
            kid: format!("{KID_PREFIX}-{key_id}-{generation}"),
        }
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }

    pub fn public(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    pub fn jwk(&self) -> Jwk {
        Jwk::new(self.kid.clone(), &self.public())
    }

    pub(crate) fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_LENGTH] {
        self.key.sign(message).to_bytes()
    }
}

impl fmt::Debug for TokenKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenKey")
            .field("kid", &self.kid)
            .field("public", &URL_SAFE_NO_PAD.encode(self.public()))
            .finish_non_exhaustive()
    }
}
