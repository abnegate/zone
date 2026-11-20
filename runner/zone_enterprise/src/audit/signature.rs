use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

pub const SIGNATURE_HEADER: &str = "Zone-Signature";
pub const TIMESTAMP_HEADER: &str = "Zone-Timestamp";
pub const VERSION: &str = "v1";

const VERSION_SEPARATOR: char = '=';
const TIMESTAMP_SEPARATOR: &[u8] = b".";

type HmacSha256 = Hmac<Sha256>;

pub fn sign(secret: &[u8], timestamp: i64, body: &[u8]) -> String {
    let tag = mac(secret, timestamp, body).finalize().into_bytes();
    format!("{VERSION}{VERSION_SEPARATOR}{}", hex::encode(tag))
}

pub fn verify(secret: &[u8], timestamp: i64, body: &[u8], header: &str) -> bool {
    let Some(encoded) = header
        .strip_prefix(VERSION)
        .and_then(|rest| rest.strip_prefix(VERSION_SEPARATOR))
    else {
        return false;
    };
    let Ok(tag) = hex::decode(encoded) else {
        return false;
    };
    mac(secret, timestamp, body).verify_slice(&tag).is_ok()
}

fn mac(secret: &[u8], timestamp: i64, body: &[u8]) -> HmacSha256 {
    let mut mac =
        HmacSha256::new_from_slice(secret).expect("HMAC-SHA256 accepts a key of any length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(TIMESTAMP_SEPARATOR);
    mac.update(body);
    mac
}

#[cfg(test)]
mod tests;
