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
