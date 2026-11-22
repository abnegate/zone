use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::registry::access::Access;

pub const ISSUER: &str = "zone-license";
pub const LIFETIME_SECONDS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Claims<'a> {
    pub iss: &'static str,
    pub sub: String,
    pub aud: &'a str,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    pub jti: String,
    pub access: &'a [Access],
}

impl<'a> Claims<'a> {
    pub fn new(
        service: &'a str,
        subject: Uuid,
        access: &'a [Access],
        now: DateTime<Utc>,
        jti: Uuid,
    ) -> Self {
        let issued_at = now.timestamp();
        Self {
            iss: ISSUER,
            sub: subject.to_string(),
            aud: service,
            iat: issued_at,
            nbf: issued_at,
            exp: issued_at + LIFETIME_SECONDS,
            jti: jti.to_string(),
            access,
        }
    }
}
