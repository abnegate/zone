use std::num::NonZeroU32;

use serde::Serialize;

use crate::registry::jwk::Jwk;
use crate::registry::token_key::TokenKey;
use crate::signer::Signer;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Jwks {
    keys: Vec<Jwk>,
}

impl Jwks {
    pub fn derive(signer: &Signer, generations: &[NonZeroU32]) -> Self {
        let mut keys: Vec<Jwk> = Vec::with_capacity(generations.len());
        for generation in generations {
            let jwk = TokenKey::derive(signer, *generation).jwk();
            if !keys.iter().any(|known| known.kid() == jwk.kid()) {
                keys.push(jwk);
            }
        }
        Self { keys }
    }

    pub fn keys(&self) -> &[Jwk] {
        &self.keys
    }
}
