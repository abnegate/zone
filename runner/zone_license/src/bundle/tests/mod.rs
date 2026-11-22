use std::net::TcpListener;
use std::time::{Duration, Instant};

use rusty_s3::{Bucket, Credentials, UrlStyle};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::logging;
use crate::polar::Proxy;

const BUCKET: &str = "zone-distribution";
const REGION: &str = "ap-southeast-2";
const KEY: &str = "AKIAZONEBUNDLETEST01";
const SECRET: &str = "bundle-test-secret/never+logged";
const VERSION: &str = "1.4.3-lts";
const SHORT: Duration = Duration::from_millis(200);
const DELAY: Duration = Duration::from_secs(5);

fn version() -> BundleVersion {
    BundleVersion::parse(VERSION).unwrap()
}

fn marker() -> Object {
    Object::marker(version(), Architecture::Amd64)
}

fn marker_path() -> String {
    format!("/{BUCKET}/bundles/{VERSION}/zone-bundle-{VERSION}-amd64.tar.sha256.sigstore.json")
}

fn settings(endpoint: &str) -> BundleSettings {
    BundleSettings {
        bucket: Bucket::new(endpoint.parse().unwrap(), UrlStyle::Path, BUCKET, REGION).unwrap(),
        prefix: DEFAULT_PREFIX.to_owned(),
        credentials: Credentials::new(KEY, SECRET),
    }
}

fn presigner(endpoint: &str) -> Presigner {
    Presigner::new(settings(endpoint), Proxy::Direct).unwrap()
}

fn error(code: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>{code}</Code><Message>refused</Message><RequestId>4442587FB7D0A2F9</RequestId></Error>"
    )
}

async fn answering(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(marker_path()))
        .and(header("range", "bytes=0-0"))
        .and(query_param("X-Amz-Expires", "60"))
        .respond_with(response)
        .expect(1)
        .mount(&server)
        .await;
    server
}

async fn probe(response: ResponseTemplate) -> Result<Presence, StorageError> {
    let server = answering(response).await;
    presigner(&server.uri()).probe(&marker()).await
}

fn absent(code: Option<&str>) -> Result<Presence, StorageError> {
    Ok(Presence::Absent {
        code: code.map(str::to_owned),
    })
}

fn refused(code: &str) -> Result<Presence, StorageError> {
    Err(StorageError::Refused {
        code: code.to_owned(),
    })
}

#[test]
fn a_version_is_three_unpadded_numbers_and_the_lts_suffix() {
    for text in ["1.4.3-lts", "0.0.0-lts", "10.20.30-lts", "2026.11.0-lts"] {
        let version = BundleVersion::parse(text).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(version.to_string(), text);
    }
}

#[test]
fn refuses_anything_else_as_a_version() {
    for text in [
        "",
        "1.4.3",
        "1.4-lts",
        "01.4.3-lts",
        "1.04.3-lts",
        "1.4.03-lts",
        "1.4.3-lts/../x",
        "../1.4.3-lts",
        "1.4.3.5-lts",
        "1..3-lts",
        "v1.4.3-lts",
        "1.4.3-LTS",
        "1.4.3-lts ",
        " 1.4.3-lts",
        "+1.4.3-lts",
        "1.4.-3-lts",
        "1.4.3-lts-lts",
        "-lts",
        "1.4.99999999999999999999-lts",
    ] {
        assert_eq!(BundleVersion::parse(text), None, "{text:?}");
    }
}

#[test]
fn an_architecture_is_amd64_or_arm64() {
    assert_eq!(Architecture::parse("amd64"), Some(Architecture::Amd64));
    assert_eq!(Architecture::parse("arm64"), Some(Architecture::Arm64));
    for architecture in Architecture::ALL {
        assert_eq!(
            Architecture::parse(&architecture.to_string()),
            Some(architecture)
        );
    }
    for text in [
        "", "AMD64", "x86_64", "aarch64", "amd64/", "arm64 ", "../amd64",
    ] {
        assert_eq!(Architecture::parse(text), None, "{text:?}");
    }
}

#[test]
fn the_objects_have_the_contract_names_and_keys() {
    let files = Object::files(version(), Architecture::Arm64);
    assert_eq!(
        files.map(|object| object.name()),
        [
            "zone-bundle-1.4.3-lts-arm64.tar",
            "zone-bundle-1.4.3-lts-arm64.tar.sha256",
            "zone-bundle-1.4.3-lts-arm64.tar.sha256.sigstore.json",
        ]
    );
    assert_eq!(
        files.map(|object| object.key("bundles")),
        [
            "bundles/1.4.3-lts/zone-bundle-1.4.3-lts-arm64.tar",
            "bundles/1.4.3-lts/zone-bundle-1.4.3-lts-arm64.tar.sha256",
            "bundles/1.4.3-lts/zone-bundle-1.4.3-lts-arm64.tar.sha256.sigstore.json",
        ]
    );
    assert_eq!(files.map(|object| object.file()), File::ALL);
    assert!(files.iter().all(
        |object| object.version() == version() && object.architecture() == Architecture::Arm64
    ));
}

#[test]
fn the_marker_is_the_sigstore_bundle_written_last() {
    let marker = marker();
    assert_eq!(marker.file(), File::Marker);
    assert_eq!(
        marker.key("zone/bundles"),
        "zone/bundles/1.4.3-lts/zone-bundle-1.4.3-lts-amd64.tar.sha256.sigstore.json"
    );
    assert_eq!(Object::files(version(), Architecture::Amd64)[2], marker);
}

#[test]
fn a_presigned_url_is_a_path_style_get_for_the_lifetime() {
    let presigner = presigner(&format!("https://s3.{REGION}.amazonaws.com"));
    let archive = Object::new(version(), Architecture::Amd64, File::Archive);

    let url = presigner.presign(&archive, DOWNLOAD_LIFETIME);

    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("s3.ap-southeast-2.amazonaws.com"));
    assert_eq!(
        url.path(),
        format!("/{BUCKET}/bundles/{VERSION}/zone-bundle-{VERSION}-amd64.tar")
    );
    let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let value = |name: &str| {
        query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    assert_eq!(value("X-Amz-Expires"), Some("900"));
    assert_eq!(value("X-Amz-Algorithm"), Some("AWS4-HMAC-SHA256"));
    assert_eq!(value("X-Amz-SignedHeaders"), Some("host"));
    assert!(
        value("X-Amz-Credential").is_some_and(|credential| {
            credential.starts_with(&format!("{KEY}/"))
                && credential.ends_with(&format!("/{REGION}/s3/aws4_request"))
        }),
        "{url}"
    );
    assert!(
        value("X-Amz-Signature").is_some_and(|signature| signature.len() == 64),
        "{url}"
    );
    assert!(!url.as_str().contains("never"), "{url}");
}

#[test]
fn a_presigned_url_keeps_the_endpoint_path_and_prefix() {
    let mut settings = settings("http://127.0.0.1:9000/s3");
    settings.prefix = "zone/bundles".to_owned();
    let presigner = Presigner::new(settings, Proxy::Direct).unwrap();

    let url = presigner.presign(&marker(), Duration::from_secs(30));

    assert_eq!(
        url.path(),
        format!(
            "/s3/{BUCKET}/zone/bundles/{VERSION}/zone-bundle-{VERSION}-amd64.tar.sha256.sigstore.json"
        )
    );
    assert!(url.as_str().contains("X-Amz-Expires=30&"), "{url}");
}

#[tokio::test]
async fn a_partial_answer_means_present() {
    assert_eq!(
        probe(ResponseTemplate::new(206).set_body_string("{")).await,
        Ok(Presence::Present)
    );
}

#[tokio::test]
async fn a_whole_answer_means_present() {
    assert_eq!(
        probe(ResponseTemplate::new(200).set_body_string("{}")).await,
        Ok(Presence::Present)
    );
}

#[tokio::test]
async fn a_not_found_without_a_code_means_absent() {
    assert_eq!(probe(ResponseTemplate::new(404)).await, absent(None));
}

#[tokio::test]
async fn no_such_key_means_absent_with_the_code() {
    assert_eq!(
        probe(ResponseTemplate::new(404).set_body_string(error(NO_SUCH_KEY))).await,
        absent(Some("NoSuchKey"))
    );
}

#[tokio::test]
async fn access_denied_means_absent_with_the_code() {
    assert_eq!(
        probe(ResponseTemplate::new(403).set_body_string(error(ACCESS_DENIED))).await,
        absent(Some("AccessDenied"))
    );
}

#[tokio::test]
async fn a_bad_signature_is_refused() {
    assert_eq!(
        probe(ResponseTemplate::new(403).set_body_string(error("SignatureDoesNotMatch"))).await,
        refused("SignatureDoesNotMatch")
    );
}

#[tokio::test]
async fn an_unknown_access_key_is_refused() {
    assert_eq!(
        probe(ResponseTemplate::new(403).set_body_string(error("InvalidAccessKeyId"))).await,
        refused("InvalidAccessKeyId")
    );
}

#[tokio::test]
async fn an_expired_token_is_refused() {
    assert_eq!(
        probe(ResponseTemplate::new(400).set_body_string(error("ExpiredToken"))).await,
        refused("ExpiredToken")
    );
}

#[tokio::test]
async fn a_not_found_with_another_code_is_refused() {
    assert_eq!(
        probe(ResponseTemplate::new(404).set_body_string(error("NoSuchBucket"))).await,
        refused("NoSuchBucket")
    );
}

#[tokio::test]
async fn a_server_error_without_a_body_is_unexpected() {
    assert_eq!(
        probe(ResponseTemplate::new(500)).await,
        Err(StorageError::Unexpected { status: 500 })
    );
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    assert_eq!(
        probe(ResponseTemplate::new(307).insert_header("location", "http://127.0.0.1:1/elsewhere"))
            .await,
        Err(StorageError::Unexpected { status: 307 })
    );
}

#[tokio::test]
async fn a_code_that_is_not_alphanumeric_is_ignored() {
    for body in [
        "<Error><Code>Signature Does\nNotMatch</Code></Error>",
        "<Error><Code></Code></Error>",
        "<Error><Code>NoSuchKey</Error>",
    ] {
        assert_eq!(
            probe(ResponseTemplate::new(403).set_body_string(body)).await,
            Err(StorageError::Unexpected { status: 403 }),
            "{body:?}"
        );
    }
}

#[tokio::test]
async fn a_code_past_the_body_limit_is_never_read() {
    let body = format!("<Error>{}{}", " ".repeat(BODY_LIMIT), error(NO_SUCH_KEY));
    assert_eq!(
        probe(ResponseTemplate::new(403).set_body_string(body)).await,
        Err(StorageError::Unexpected { status: 403 })
    );
}

#[tokio::test]
async fn a_code_within_the_limit_of_a_huge_body_is_read() {
    let body = format!("{}{}", error("SlowDown"), " ".repeat(16 * 1024 * 1024));
    assert_eq!(
        probe(ResponseTemplate::new(503).set_body_string(body)).await,
        refused("SlowDown")
    );
}

#[tokio::test]
async fn a_code_straddling_the_limit_is_cut_off() {
    let padding = " ".repeat(BODY_LIMIT - "<Error><Code>NoSuch".len());
    let body = format!("{padding}<Error><Code>NoSuchKey</Code></Error>");
    assert_eq!(
        probe(ResponseTemplate::new(404).set_body_string(body)).await,
        absent(None)
    );
}

#[tokio::test]
async fn a_slow_answer_times_out_as_unreachable() {
    let server = answering(ResponseTemplate::new(206).set_delay(DELAY)).await;
    let presigner = Presigner::with_timeout(settings(&server.uri()), Proxy::Direct, SHORT).unwrap();

    let started = Instant::now();
    let probed = presigner.probe(&marker()).await;

    assert_eq!(probed, Err(StorageError::Unreachable));
    assert!(started.elapsed() < DELAY, "{:?}", started.elapsed());
}

#[tokio::test]
async fn unreachable_storage_logs_no_secret_or_signature() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let presigner = presigner(&format!("http://{address}"));
    let mut probed = None;

    let logged = logging::capture(tracing::Level::TRACE, async {
        probed = Some(presigner.probe(&marker()).await);
    })
    .await;

    assert_eq!(probed, Some(Err(StorageError::Unreachable)));
    assert!(logged.contains("could not be reached"), "{logged}");
    for hidden in ["X-Amz-Signature", "X-Amz-Credential", KEY, SECRET, "never"] {
        assert!(!logged.contains(hidden), "{hidden} in {logged}");
    }
}

#[test]
fn the_production_limits_match_the_contract() {
    assert_eq!(TIMEOUT, Duration::from_secs(10));
    assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(5));
    assert_eq!(BODY_LIMIT, 8 * 1024);
    assert_eq!(DOWNLOAD_LIFETIME, Duration::from_secs(900));
    assert_eq!(DEFAULT_PREFIX, "bundles");
    assert_eq!(
        BundleSettings::default_endpoint(REGION),
        "https://s3.ap-southeast-2.amazonaws.com"
    );
}

#[test]
fn debug_never_shows_the_secret() {
    let settings = settings(&format!("https://s3.{REGION}.amazonaws.com"));
    let presigner = Presigner::new(settings.clone(), Proxy::Direct).unwrap();
    let shown = format!("{settings:?} {settings:#?} {presigner:?} {presigner:#?}");
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(!shown.contains("never"), "{shown}");
    assert!(shown.contains("[redacted]"), "{shown}");
    assert!(shown.contains(KEY), "{shown}");
    assert!(shown.contains(BUCKET), "{shown}");
}

#[test]
fn a_storage_error_names_only_the_code_or_status() {
    assert_eq!(StorageError::Unreachable.code(), None);
    assert_eq!(StorageError::Unexpected { status: 500 }.code(), None);
    let refused = StorageError::Refused {
        code: "SignatureDoesNotMatch".to_owned(),
    };
    assert_eq!(refused.code(), Some("SignatureDoesNotMatch"));
    assert_eq!(
        refused.to_string(),
        "the bundle storage refused the request with code SignatureDoesNotMatch"
    );
}

#[test]
fn prefixes_are_trimmed_path_segments() {
    for (text, prefix) in [
        ("bundles", "bundles"),
        ("/bundles/", "bundles"),
        ("zone/bundles", "zone/bundles"),
        ("v1.0_x-y", "v1.0_x-y"),
    ] {
        assert_eq!(
            BundleSettings::parse_prefix(text).as_deref(),
            Some(prefix),
            "{text:?}"
        );
    }
    for text in [
        "/", "a//b", "a/../b", "./a", "..", "a b", "a?b", "a%2Fb", "ü",
    ] {
        assert_eq!(BundleSettings::parse_prefix(text), None, "{text:?}");
    }
}
