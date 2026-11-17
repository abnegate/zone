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
mod tests {
    use chrono::TimeZone;

    use super::*;

    const CREDENTIALS: Credentials<'static> = Credentials {
        access_key_id: "AKIAIOSFODNN7EXAMPLE",
        secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
    };
    const REGION: &str = "us-east-1";
    const EMPTY_HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const HOST: &str = "examplebucket.s3.amazonaws.com";

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap()
    }

    fn signs(request: &Request<'_>, canonical_request: &str, digest: &str, expected: &str) {
        let canonical = canonical(request);
        assert_eq!(canonical.request, canonical_request);
        assert_eq!(
            string_to_sign(
                &amz_date(at()),
                &scope(REGION, SERVICE, at()),
                &canonical.request
            ),
            format!("{ALGORITHM}\n20130524T000000Z\n20130524/us-east-1/s3/aws4_request\n{digest}")
        );
        assert_eq!(
            authorization(request, &CREDENTIALS, REGION, SERVICE, at()),
            expected
        );
    }

    #[test]
    fn reproduces_the_documented_get_object_example() {
        let headers = [
            ("Host", HOST),
            ("Range", "bytes=0-9 "),
            (PAYLOAD_HASH_HEADER, EMPTY_HASH),
            (DATE_HEADER, " 20130524T000000Z "),
        ];
        let request = Request {
            method: "GET",
            path: "/test.txt",
            query: &[],
            headers: &headers,
            payload_hash: &payload_hash(b""),
        };
        signs(
            &request,
            "GET\n/test.txt\n\nhost:examplebucket.s3.amazonaws.com\nrange:bytes=0-9\nx-amz-content-sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\nx-amz-date:20130524T000000Z\n\nhost;range;x-amz-content-sha256;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "7344ae5b7ee6c3e7e6b0fe0640412a37625d1fbfff95c48bbb2dc43964946972",
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,SignedHeaders=host;range;x-amz-content-sha256;x-amz-date,Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41",
        );
    }

    #[test]
    fn reproduces_the_documented_put_object_example() {
        let hash = payload_hash(b"Welcome to Amazon S3.");
        assert_eq!(
            hash,
            "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );
        let path = format!("/{}", encode_path("test$file.text"));
        let headers = [
            ("Host", HOST),
            ("Date", "Fri, 24 May 2013 00:00:00 GMT"),
            (DATE_HEADER, "20130524T000000Z "),
            ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
            (PAYLOAD_HASH_HEADER, &hash),
        ];
        let request = Request {
            method: "PUT",
            path: &path,
            query: &[],
            headers: &headers,
            payload_hash: &hash,
        };
        signs(
            &request,
            "PUT\n/test%24file.text\n\ndate:Fri, 24 May 2013 00:00:00 GMT\nhost:examplebucket.s3.amazonaws.com\nx-amz-content-sha256:44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072\nx-amz-date:20130524T000000Z\nx-amz-storage-class:REDUCED_REDUNDANCY\n\ndate;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class\n44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072",
            "9e0e90d9c76de8fa5b200d8c849cd5b8dc7a3be3951ddb7f6a76b4158342019d",
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,SignedHeaders=date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class,Signature=98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd",
        );
    }

    #[test]
    fn reproduces_the_documented_get_bucket_lifecycle_example() {
        let headers = [
            ("Host", HOST),
            (DATE_HEADER, "20130524T000000Z"),
            (PAYLOAD_HASH_HEADER, EMPTY_HASH),
        ];
        let request = Request {
            method: "GET",
            path: "",
            query: &[("lifecycle", "")],
            headers: &headers,
            payload_hash: EMPTY_HASH,
        };
        signs(
            &request,
            "GET\n/\nlifecycle=\nhost:examplebucket.s3.amazonaws.com\nx-amz-content-sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\nx-amz-date:20130524T000000Z\n\nhost;x-amz-content-sha256;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "9766c798316ff2757b517bc739a67f6213b4ab36dd5da2f94eaebf79c77395ca",
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,SignedHeaders=host;x-amz-content-sha256;x-amz-date,Signature=fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543",
        );
    }

    #[test]
    fn reproduces_the_documented_list_objects_example() {
        let headers = [
            ("Host", HOST),
            (DATE_HEADER, "20130524T000000Z"),
            (PAYLOAD_HASH_HEADER, EMPTY_HASH),
        ];
        let request = Request {
            method: "GET",
            path: "/",
            query: &[("prefix", "J"), ("max-keys", "2")],
            headers: &headers,
            payload_hash: EMPTY_HASH,
        };
        signs(
            &request,
            "GET\n/\nmax-keys=2&prefix=J\nhost:examplebucket.s3.amazonaws.com\nx-amz-content-sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\nx-amz-date:20130524T000000Z\n\nhost;x-amz-content-sha256;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "df57d21db20da04d7fa30298dd4488ba3a2b47ca3a489c74750e0f1e7df1b9b7",
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,SignedHeaders=host;x-amz-content-sha256;x-amz-date,Signature=34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7",
        );
    }

    #[test]
    fn header_order_and_case_do_not_change_the_signature() {
        let forward = [("host", HOST), (DATE_HEADER, "20130524T000000Z")];
        let backward = [("X-Amz-Date", "20130524T000000Z"), ("HOST", HOST)];
        let sign = |headers: &[(&str, &str)]| {
            let request = Request {
                method: "PUT",
                path: "/key",
                query: &[],
                headers,
                payload_hash: EMPTY_HASH,
            };
            authorization(&request, &CREDENTIALS, REGION, SERVICE, at())
        };
        assert_eq!(sign(&forward), sign(&backward));
    }

    #[test]
    fn collapses_whitespace_and_joins_repeated_headers() {
        let headers = [
            ("x-amz-meta-note", "  two   spaced\twords "),
            ("x-amz-meta-note", "second"),
            ("host", HOST),
        ];
        let request = Request {
            method: "PUT",
            path: "/key",
            query: &[],
            headers: &headers,
            payload_hash: EMPTY_HASH,
        };
        let canonical = canonical(&request);
        assert!(
            canonical
                .request
                .contains("\nx-amz-meta-note:two spaced words,second\n"),
            "{}",
            canonical.request
        );
        assert_eq!(canonical.signed_headers, "host;x-amz-meta-note");
    }

    #[test]
    fn encodes_query_names_and_values_including_slashes() {
        assert_eq!(
            canonical_query(&[
                ("prefix", "a/b c"),
                ("delimiter", "/"),
                ("a", "2"),
                ("a", "1")
            ]),
            "a=1&a=2&delimiter=%2F&prefix=a%2Fb%20c"
        );
    }

    #[test]
    fn encodes_a_path_keeping_slashes() {
        assert_eq!(
            encode_path("audit/2026/11/17/a b+c$é~_-.ndjson"),
            "audit/2026/11/17/a%20b%2Bc%24%C3%A9~_-.ndjson"
        );
    }

    #[test]
    fn leaves_an_object_key_unchanged() {
        let key = "audit/server/2026/11/17/143000Z-00000000-0000-0000-0000-000000000001.ndjson";
        assert_eq!(encode_path(key), key);
    }

    #[test]
    fn formats_the_request_timestamp() {
        let at = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(amz_date(at), "20260102T030405Z");
    }

    #[test]
    fn constants_are_the_wire_contract() {
        assert_eq!(ALGORITHM, "AWS4-HMAC-SHA256");
        assert_eq!(SERVICE, "s3");
        assert_eq!(DATE_HEADER, "x-amz-date");
        assert_eq!(PAYLOAD_HASH_HEADER, "x-amz-content-sha256");
    }

    #[test]
    fn debug_never_shows_the_secret_access_key() {
        let shown = format!("{CREDENTIALS:?}");
        assert!(shown.contains(CREDENTIALS.access_key_id), "{shown}");
        assert!(!shown.contains(CREDENTIALS.secret_access_key), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
    }
}
