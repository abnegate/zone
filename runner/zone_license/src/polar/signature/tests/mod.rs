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
