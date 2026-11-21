use std::fmt;

use abnegate_secret::REDACTED;
use subtle::ConstantTimeEq;
use zone_enterprise::sso::{Error, Identity, Policy};

use crate::db::sso::Asserted;
use crate::utils::crypto::hash_token;

#[derive(Clone, PartialEq, Eq)]
pub struct Handoff {
    pub state: String,
    pub code: String,
}

impl fmt::Debug for Handoff {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Handoff")
            .field("state", &REDACTED)
            .field("code", &REDACTED)
            .finish()
    }
}

/// The domain is checked again because an admin may have narrowed the allowed domains since the
/// ACS stored this identity.
pub fn redeem(asserted: &Asserted, code: &str, policy: &Policy) -> Result<Identity, Error> {
    let presented = hash_token(code);
    if !bool::from(presented.as_bytes().ct_eq(asserted.code_hash.as_bytes())) {
        return Err(Error::StateInvalid);
    }
    if !policy.domains.allows(&asserted.email) {
        return Err(Error::DomainNotAllowed);
    }
    Ok(Identity {
        subject: asserted.subject.clone(),
        email: asserted.email.clone(),
        name: asserted.display_name.clone(),
    })
}
