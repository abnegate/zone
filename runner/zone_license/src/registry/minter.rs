use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::registry::access::Access;
use crate::registry::claims::Claims;
use crate::registry::header::Header;
use crate::registry::token_key::TokenKey;

const SEGMENT_SEPARATOR: char = '.';

pub fn mint(
    key: &TokenKey,
    service: &str,
    subject: Uuid,
    access: &[Access],
    now: DateTime<Utc>,
    jti: Uuid,
) -> String {
    let mut token = encoded(&Header::new(key.kid()));
    token.push(SEGMENT_SEPARATOR);
    token.push_str(&encoded(&Claims::new(service, subject, access, now, jti)));
    let signature = key.sign(token.as_bytes());
    token.push(SEGMENT_SEPARATOR);
    token.push_str(&URL_SAFE_NO_PAD.encode(signature));
    token
}

fn encoded(part: &impl Serialize) -> String {
    URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(part).expect("a token part has only string keys, so it serializes"),
    )
}
