use jsonwebtoken::jwk::{Jwk, PublicKeyUse};
use jsonwebtoken::{Algorithm, AlgorithmFamily, DecodingKey};
use serde::Deserialize;

use crate::sso::error::Error;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Keys(Vec<Jwk>);

#[derive(Deserialize)]
struct Document {
    keys: Vec<serde_json::Value>,
}

impl Keys {
    pub(crate) fn parse(body: &[u8]) -> Result<Self, Error> {
        let document: Document = serde_json::from_slice(body).map_err(|error| {
            Error::Discovery(format!(
                "the identity provider's signing keys aren't a valid JWK set ({error})."
            ))
        })?;
        Ok(Self(
            document
                .keys
                .into_iter()
                .filter_map(|key| serde_json::from_value::<Jwk>(key).ok())
                .filter(usable)
                .collect(),
        ))
    }

    pub(crate) fn select(&self, key_id: Option<&str>, algorithm: Algorithm) -> Option<&Jwk> {
        match key_id {
            Some(key_id) => self
                .0
                .iter()
                .find(|key| key.common.key_id.as_deref() == Some(key_id)),
            None => {
                let mut candidates = self.0.iter().filter(|key| suits(key, algorithm));
                let key = candidates.next()?;
                candidates.next().is_none().then_some(key)
            }
        }
    }
}

fn usable(key: &Jwk) -> bool {
    key.common.public_key_use != Some(PublicKeyUse::Encryption)
        && DecodingKey::from_jwk(key)
            .is_ok_and(|decoding| decoding.family() != AlgorithmFamily::Hmac)
}

fn suits(key: &Jwk, algorithm: Algorithm) -> bool {
    let declared = key
        .common
        .key_algorithm
        .is_none_or(|declared| Algorithm::try_from(declared).ok() == Some(algorithm));
    declared
        && DecodingKey::from_jwk(key).is_ok_and(|decoding| decoding.family() == algorithm.family())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sso::testing;

    fn rsa(key_id: Option<&str>, algorithm: Option<&str>) -> serde_json::Value {
        let mut key = json!({"kty": "RSA", "n": testing::MODULUS, "e": testing::EXPONENT});
        if let Some(key_id) = key_id {
            key["kid"] = json!(key_id);
        }
        if let Some(algorithm) = algorithm {
            key["alg"] = json!(algorithm);
        }
        key
    }

    fn keys(keys: Vec<serde_json::Value>) -> Keys {
        Keys::parse(&serde_json::to_vec(&json!({ "keys": keys })).unwrap()).unwrap()
    }

    #[test]
    fn selects_by_key_id() {
        let keys = keys(vec![rsa(Some("a"), None), rsa(Some("b"), None)]);
        let selected = keys.select(Some("b"), Algorithm::RS256).unwrap();
        assert_eq!(selected.common.key_id.as_deref(), Some("b"));
        assert!(keys.select(Some("c"), Algorithm::RS256).is_none());
    }

    #[test]
    fn without_a_key_id_selects_the_only_suitable_key() {
        let keys = keys(vec![
            rsa(Some("a"), Some("RS256")),
            json!({"kty": "EC", "crv": "P-256", "kid": "e",
                   "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
                   "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"}),
        ]);
        let selected = keys.select(None, Algorithm::RS256).unwrap();
        assert_eq!(selected.common.key_id.as_deref(), Some("a"));
        assert!(keys.select(None, Algorithm::RS384).is_none());
    }

    #[test]
    fn without_a_key_id_refuses_to_guess_between_keys() {
        let keys = keys(vec![rsa(Some("a"), None), rsa(Some("b"), None)]);
        assert!(keys.select(None, Algorithm::RS256).is_none());
    }

    #[test]
    fn skips_encryption_symmetric_and_unreadable_keys() {
        let mut encryption = rsa(Some("enc"), None);
        encryption["use"] = json!("enc");
        let keys = keys(vec![
            encryption,
            json!({"kty": "unknown", "kid": "odd"}),
            json!({"kty": "oct", "kid": "shared", "k": "c2VjcmV0"}),
            rsa(Some("sig"), None),
        ]);
        assert!(keys.select(Some("enc"), Algorithm::RS256).is_none());
        assert!(keys.select(Some("odd"), Algorithm::RS256).is_none());
        assert!(keys.select(Some("shared"), Algorithm::RS256).is_none());
        assert!(keys.select(Some("sig"), Algorithm::RS256).is_some());
    }

    #[test]
    fn refuses_a_document_that_is_not_a_key_set() {
        assert!(matches!(Keys::parse(b"{}"), Err(Error::Discovery(_))));
        assert!(matches!(Keys::parse(b"nope"), Err(Error::Discovery(_))));
    }
}
