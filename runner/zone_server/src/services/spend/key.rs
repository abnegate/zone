use abnegate_secret::{MasterKey, SecretValue};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const PREFIX: &str = "sk-zone-";

const PURPOSE: &str = "zone-spend-key:v1";
const AUTOMATION: &str = "automation";

type Signer = Hmac<Sha256>;

/// The LiteLLM virtual key a subject's spend is sent under. It is derived from the server's own
/// encryption key, which LiteLLM never sees, so it is never stored: only its token is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub secret: SecretValue,
    /// How LiteLLM names the key: the SHA-256 of `secret`, in hexadecimal.
    pub token: String,
}

impl Key {
    pub fn derive(master: &MasterKey, workspace: Uuid, user: Option<Uuid>) -> Self {
        let mut signer =
            Signer::new_from_slice(master.to_hexadecimal().as_bytes()).expect("HMAC takes any key");
        let spender = user.map_or_else(|| AUTOMATION.to_string(), |user| user.to_string());
        signer.update(format!("{PURPOSE}:{workspace}:{spender}").as_bytes());
        let secret = format!("{PREFIX}{}", hex::encode(signer.finalize().into_bytes()));
        let token = Self::hash(&secret);
        Self {
            secret: SecretValue::new(secret),
            token,
        }
    }

    pub fn hash(secret: &str) -> String {
        hex::encode(Sha256::digest(secret.as_bytes()))
    }
}
