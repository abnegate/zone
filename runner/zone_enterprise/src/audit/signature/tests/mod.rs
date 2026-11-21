use super::*;

const SECRET: &[u8] = b"whsec_test";
const TIMESTAMP: i64 = 1_700_000_000;
const BODY: &[u8] = br#"{"events":[]}"#;
const EXPECTED: &str = "v1=4f6f9b7a41398ad124e52a1320534f978e522923f3f2b2724e31d7a9dbf78a7f";

#[test]
fn headers_and_version_are_the_wire_contract() {
    assert_eq!(SIGNATURE_HEADER, "Zone-Signature");
    assert_eq!(TIMESTAMP_HEADER, "Zone-Timestamp");
    assert_eq!(VERSION, "v1");
}

#[test]
fn signs_the_timestamp_a_dot_and_the_body() {
    assert_eq!(sign(SECRET, TIMESTAMP, BODY), EXPECTED);
}

#[test]
fn a_signature_round_trips() {
    let header = sign(SECRET, TIMESTAMP, BODY);
    assert!(verify(SECRET, TIMESTAMP, BODY, &header));
}

#[test]
fn accepts_uppercase_hex() {
    let header = format!("{VERSION}={}", EXPECTED[3..].to_uppercase());
    assert!(verify(SECRET, TIMESTAMP, BODY, &header));
}

#[test]
fn accepts_an_empty_secret_and_body() {
    let header = sign(b"", 0, b"");
    assert!(verify(b"", 0, b"", &header));
}

#[test]
fn refuses_a_tampered_body() {
    assert!(!verify(SECRET, TIMESTAMP, br#"{"events":[1]}"#, EXPECTED));
}

#[test]
fn refuses_a_tampered_timestamp() {
    assert!(!verify(SECRET, TIMESTAMP + 1, BODY, EXPECTED));
}

#[test]
fn refuses_another_secret() {
    assert!(!verify(b"whsec_other", TIMESTAMP, BODY, EXPECTED));
}

#[test]
fn refuses_a_tampered_signature() {
    let mut tampered = EXPECTED.to_owned();
    tampered.replace_range(EXPECTED.len() - 1.., "e");
    assert!(!verify(SECRET, TIMESTAMP, BODY, &tampered));
}

#[test]
fn refuses_malformed_headers() {
    let truncated = &EXPECTED[..EXPECTED.len() - 2];
    let unversioned = &EXPECTED[3..];
    let wrong_version = EXPECTED.replacen("v1", "v2", 1);
    let without_separator = EXPECTED.replacen('=', "", 1);
    for header in [
        "",
        "v1=",
        "v1=zz",
        truncated,
        unversioned,
        &wrong_version,
        &without_separator,
    ] {
        assert!(!verify(SECRET, TIMESTAMP, BODY, header), "{header:?}");
    }
}
