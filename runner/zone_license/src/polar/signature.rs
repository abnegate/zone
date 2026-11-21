use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use hmac::digest::{CtOutput, Output};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use super::headers::Headers;
use super::secret::WebhookSecret;
use super::signature_error::SignatureError;

pub const VERSION: &str = "v1";
pub const TOLERANCE_SECONDS: u64 = 300;

const VERSION_SEPARATOR: char = ',';
const CONTENT_SEPARATOR: &[u8] = b".";

type HmacSha256 = Hmac<Sha256>;

pub fn verify(
    secret: &WebhookSecret,
    headers: &Headers,
    body: &[u8],
    now: DateTime<Utc>,
) -> Result<(), SignatureError> {
    let id = headers.id()?;
    let timestamp = headers.timestamp()?;
    let header = headers.signature()?;
    if now.timestamp().abs_diff(timestamp) > TOLERANCE_SECONDS {
        return Err(SignatureError::Stale);
    }
    let tags: Vec<CtOutput<HmacSha256>> = header.split_whitespace().filter_map(tag).collect();
    let matches = secret.keys().iter().any(|key| {
        let expected = mac(key, id, timestamp, body).finalize();
        tags.contains(&expected)
    });
    if matches {
        Ok(())
    } else {
        Err(SignatureError::Mismatch)
    }
}

pub fn signature(key: &[u8], id: &str, timestamp: i64, body: &[u8]) -> String {
    let tag = mac(key, id, timestamp, body).finalize().into_bytes();
    format!("{VERSION}{VERSION_SEPARATOR}{}", STANDARD.encode(tag))
}

/// Compared as a `CtOutput`, whose equality is constant time.
fn tag(entry: &str) -> Option<CtOutput<HmacSha256>> {
    let (version, encoded) = entry.split_once(VERSION_SEPARATOR)?;
    if version != VERSION {
        return None;
    }
    let bytes = STANDARD.decode(encoded).ok()?;
    Output::<HmacSha256>::try_from(bytes.as_slice())
        .ok()
        .map(CtOutput::new)
}

fn mac(key: &[u8], id: &str, timestamp: i64, body: &[u8]) -> HmacSha256 {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts a key of any length");
    mac.update(id.as_bytes());
    mac.update(CONTENT_SEPARATOR);
    mac.update(timestamp.to_string().as_bytes());
    mac.update(CONTENT_SEPARATOR);
    mac.update(body);
    mac
}

#[cfg(test)]
mod tests;
