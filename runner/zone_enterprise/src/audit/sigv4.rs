use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write;

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use crate::audit::destination::REDACTED;

pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";
pub const SERVICE: &str = "s3";
pub const DATE_HEADER: &str = "x-amz-date";
pub const PAYLOAD_HASH_HEADER: &str = "x-amz-content-sha256";

const KEY_PREFIX: &str = "AWS4";
const TERMINATOR: &str = "aws4_request";
const DATE_FORMAT: &str = "%Y%m%d";
const TIMESTAMP_FORMAT: &str = "%Y%m%dT%H%M%SZ";
const ROOT: &str = "/";
const SLASH: u8 = b'/';
const VALUE_SEPARATOR: &str = ",";
const SIGNED_HEADER_SEPARATOR: &str = ";";
const QUERY_SEPARATOR: &str = "&";

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Copy)]
pub struct Credentials<'a> {
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
}

impl fmt::Debug for Credentials<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credentials")
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &REDACTED)
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a [(&'a str, &'a str)],
    pub headers: &'a [(&'a str, &'a str)],
    pub payload_hash: &'a str,
}

struct Canonical {
    request: String,
    signed_headers: String,
}

pub fn authorization(
    request: &Request<'_>,
    credentials: &Credentials<'_>,
    region: &str,
    service: &str,
    at: DateTime<Utc>,
) -> String {
    let scope = scope(region, service, at);
    let canonical = canonical(request);
    let string_to_sign = string_to_sign(&amz_date(at), &scope, &canonical.request);
    let key = signing_key(credentials.secret_access_key, region, service, at);
    let signature = hex::encode(hmac(&key, string_to_sign.as_bytes()));
    format!(
        "{ALGORITHM} Credential={}/{scope},SignedHeaders={},Signature={signature}",
        credentials.access_key_id, canonical.signed_headers
    )
}

pub fn amz_date(at: DateTime<Utc>) -> String {
    at.format(TIMESTAMP_FORMAT).to_string()
}

pub fn payload_hash(payload: &[u8]) -> String {
    hex::encode(Sha256::digest(payload))
}

pub fn encode_path(key: &str) -> String {
    encode(key, |byte| byte == SLASH)
}

fn encode_component(value: &str) -> String {
    encode(value, |_| false)
}

fn encode(value: &str, kept: impl Fn(u8) -> bool) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if unreserved(byte) || kept(byte) {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    encoded
}

fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn scope(region: &str, service: &str, at: DateTime<Utc>) -> String {
    format!("{}/{region}/{service}/{TERMINATOR}", at.format(DATE_FORMAT))
}

fn canonical(request: &Request<'_>) -> Canonical {
    let headers = canonical_headers(request.headers);
    let signed_headers = headers
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(SIGNED_HEADER_SEPARATOR);
    let mut lines = String::new();
    for (name, value) in &headers {
        writeln!(lines, "{name}:{value}").expect("writing to a String cannot fail");
    }
    let path = if request.path.is_empty() {
        ROOT
    } else {
        request.path
    };
    let text = format!(
        "{}\n{path}\n{}\n{lines}\n{signed_headers}\n{}",
        request.method,
        canonical_query(request.query),
        request.payload_hash
    );
    Canonical {
        request: text,
        signed_headers,
    }
}

fn canonical_query(query: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(name, value)| (encode_component(name), encode_component(value)))
        .collect();
    pairs.sort_unstable();
    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(QUERY_SEPARATOR)
}

fn canonical_headers(headers: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut canonical: BTreeMap<String, String> = BTreeMap::new();
    for (name, value) in headers {
        let value = collapse(value);
        canonical
            .entry(name.to_ascii_lowercase())
            .and_modify(|existing| {
                existing.push_str(VALUE_SEPARATOR);
                existing.push_str(&value);
            })
            .or_insert(value);
    }
    canonical
}

fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn string_to_sign(timestamp: &str, scope: &str, canonical_request: &str) -> String {
    format!(
        "{ALGORITHM}\n{timestamp}\n{scope}\n{}",
        payload_hash(canonical_request.as_bytes())
    )
}

fn signing_key(
    secret_access_key: &str,
    region: &str,
    service: &str,
    at: DateTime<Utc>,
) -> [u8; 32] {
    let secret = format!("{KEY_PREFIX}{secret_access_key}");
    let date = hmac(
        secret.as_bytes(),
        at.format(DATE_FORMAT).to_string().as_bytes(),
    );
    let region = hmac(&date, region.as_bytes());
    let service = hmac(&region, service.as_bytes());
    hmac(&service, TERMINATOR.as_bytes())
}

fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts a key of any length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests;
