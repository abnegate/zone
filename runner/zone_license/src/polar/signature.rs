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
mod tests {
    use chrono::TimeZone;

    use super::*;

    const KEY: &[u8] = b"whsec_unit";
    const ID: &str = "msg_unit";
    const TIMESTAMP: i64 = 1_700_000_000;
    const BODY: &[u8] = b"{\"type\":\"order.paid\"}";

    fn headers(signature: String) -> Headers {
        Headers {
            id: Some(ID.to_owned()),
            timestamp: Some(TIMESTAMP.to_string()),
            signature: Some(signature),
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.timestamp_opt(TIMESTAMP, 0).unwrap()
    }

    #[test]
    fn signature_round_trips() {
        let secret = WebhookSecret::new("whsec_unit");
        let headers = headers(signature(KEY, ID, TIMESTAMP, BODY));
        assert_eq!(verify(&secret, &headers, BODY, now()), Ok(()));
    }

    #[test]
    fn ignores_other_versions() {
        let secret = WebhookSecret::new("whsec_unit");
        let valid = signature(KEY, ID, TIMESTAMP, BODY);
        let other = valid.replacen(VERSION, "v1a", 1);
        assert_eq!(
            verify(&secret, &headers(other), BODY, now()),
            Err(SignatureError::Mismatch)
        );
    }

    #[test]
    fn skips_entries_that_are_not_base64() {
        let secret = WebhookSecret::new("whsec_unit");
        let valid = signature(KEY, ID, TIMESTAMP, BODY);
        let header = format!("v1,not*base64 {valid}");
        assert_eq!(verify(&secret, &headers(header), BODY, now()), Ok(()));
    }

    #[test]
    fn refuses_a_tag_of_the_wrong_length() {
        let secret = WebhookSecret::new("whsec_unit");
        let full = mac(KEY, ID, TIMESTAMP, BODY).finalize().into_bytes();
        let mut longer = full.to_vec();
        longer.push(0);
        for bytes in [&full[..16], &longer[..], &[][..]] {
            let header = format!("{VERSION}{VERSION_SEPARATOR}{}", STANDARD.encode(bytes));
            assert_eq!(
                verify(&secret, &headers(header), BODY, now()),
                Err(SignatureError::Mismatch),
                "{} bytes",
                bytes.len()
            );
        }
    }

    #[test]
    fn empty_secret_never_matches() {
        let secret = WebhookSecret::new("");
        let headers = headers(signature(b"", ID, TIMESTAMP, BODY));
        assert_eq!(
            verify(&secret, &headers, BODY, now()),
            Err(SignatureError::Mismatch)
        );
    }

    #[test]
    fn checks_the_window_before_the_signature() {
        let secret = WebhookSecret::new("whsec_unit");
        let headers = headers("v1,AAAA".to_owned());
        let late = now() + chrono::Duration::seconds(301);
        assert_eq!(
            verify(&secret, &headers, BODY, late),
            Err(SignatureError::Stale)
        );
    }
}
