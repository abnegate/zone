use ed25519_dalek::Signer;
pub use ed25519_dalek::SigningKey;

use crate::encoding;
use crate::payload::Payload;

pub fn sign(payload: &Payload, key: &SigningKey) -> String {
    let bytes = serde_json::to_vec(payload)
        .expect("a payload has only string keys, so it always serializes");
    sign_bytes(&bytes, key)
}

pub fn sign_bytes(payload: &[u8], key: &SigningKey) -> String {
    encoding::encode(payload, &key.sign(payload).to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn signing_is_deterministic() {
        assert_eq!(
            sign(&fixtures::payload(), &fixtures::first()),
            sign(&fixtures::payload(), &fixtures::first())
        );
    }

    #[test]
    fn different_keys_sign_differently() {
        assert_ne!(
            sign(&fixtures::payload(), &fixtures::first()),
            sign(&fixtures::payload(), &fixtures::second())
        );
    }

    #[test]
    fn sign_matches_sign_bytes_over_the_serialized_payload() {
        let payload = fixtures::payload();
        let bytes = serde_json::to_vec(&payload).unwrap();
        assert_eq!(
            sign(&payload, &fixtures::first()),
            sign_bytes(&bytes, &fixtures::first())
        );
    }
}
