use chrono::NaiveDateTime;
use uuid::Uuid;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_enterprise::audit::destination::{Addressing, S3Secrets, S3Settings};
use zone_enterprise::audit::sigv4::{self, Credentials, DATE_HEADER, PAYLOAD_HASH_HEADER, SERVICE};
use zone_enterprise::audit::{Event, FailureCode, Format, object_key};
use zone_server::db::audit::scope::Scope;
use zone_server::services::audit::transport::s3::{HOST_HEADER, S3};
use zone_server::services::audit::transport::{Batch, Failure, Transport, client};

use crate::common::audit::{batch, header};

const BUCKET: &str = "audit-bucket";
const REGION: &str = "eu-west-1";
const PREFIX: &str = "zone/audit/";
const ACCESS_KEY_ID: &str = "AKIDAUDITEXAMPLE";
const SECRET_ACCESS_KEY: &str = "audit/secret+access/key";
const VIRTUAL_DOMAIN: &str = "s3.test";
const SIGNED_HEADERS: &str = "SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date,";
const AMZ_DATE_FORMAT: &str = "%Y%m%dT%H%M%SZ";

fn settings(endpoint: &str, addressing: Addressing) -> S3Settings {
    S3Settings {
        bucket: BUCKET.to_owned(),
        region: REGION.to_owned(),
        endpoint: Some(endpoint.to_owned()),
        prefix: PREFIX.to_owned(),
        addressing,
    }
    .validated()
    .expect("the test settings are valid")
}

fn transport(endpoint: &str, addressing: Addressing, client: reqwest::Client) -> S3 {
    S3::new(
        &settings(endpoint, addressing),
        &S3Secrets {
            access_key_id: ACCESS_KEY_ID.to_owned(),
            secret_access_key: SECRET_ACCESS_KEY.to_owned(),
        },
        client,
    )
}

async fn store(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

fn key(batch: &Batch) -> String {
    let first = &batch.events[0];
    object_key(
        PREFIX,
        batch.scope.organization_id(),
        first.created_at,
        first.id,
    )
}

fn recomputed(request: &Request) -> String {
    let date = header(request, DATE_HEADER);
    let at = NaiveDateTime::parse_from_str(date, AMZ_DATE_FORMAT)
        .unwrap()
        .and_utc();
    let payload_hash = header(request, PAYLOAD_HASH_HEADER);
    let headers = [
        (HOST_HEADER, header(request, HOST_HEADER)),
        (PAYLOAD_HASH_HEADER, payload_hash),
        (DATE_HEADER, date),
        ("content-type", header(request, "content-type")),
    ];
    sigv4::authorization(
        &sigv4::Request {
            method: request.method.as_str(),
            path: request.url.path(),
            query: &[],
            headers: &headers,
            payload_hash,
        },
        &Credentials {
            access_key_id: ACCESS_KEY_ID,
            secret_access_key: SECRET_ACCESS_KEY,
        },
        REGION,
        SERVICE,
        at,
    )
}

fn assert_signed_object(request: &Request, sent: &Batch) {
    assert_eq!(
        header(request, "content-type"),
        Format::Ndjson.content_type()
    );
    assert_eq!(
        header(request, PAYLOAD_HASH_HEADER),
        sigv4::payload_hash(&request.body)
    );
    let authorization = header(request, "authorization");
    assert!(authorization.contains(SIGNED_HEADERS), "{authorization}");
    assert!(!authorization.contains(SECRET_ACCESS_KEY));
    assert_eq!(
        authorization,
        recomputed(request),
        "recomputing SigV4 from the captured request gives the sent Authorization"
    );

    let events: Vec<Event> = serde_json::Deserializer::from_slice(&request.body)
        .into_iter()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(events, sent.events);
    assert_eq!(request.body.last(), Some(&b'\n'));
    assert_eq!(
        request.body.iter().filter(|byte| **byte == b'\n').count(),
        sent.events.len()
    );
}

#[tokio::test]
async fn path_style_puts_a_signed_ndjson_object_under_the_bucket() {
    let server = store(ResponseTemplate::new(200)).await;
    let sent = batch(Scope::Organization(Uuid::new_v4()), 3);

    transport(&server.uri(), Addressing::Path, client::client())
        .deliver(&sent)
        .await
        .expect("a 2xx response stores the object");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.url.path(),
        format!("/{BUCKET}/{}", sigv4::encode_path(&key(&sent)))
    );
    assert_eq!(
        header(request, HOST_HEADER),
        format!("127.0.0.1:{}", server.address().port())
    );
    assert_signed_object(request, &sent);
}

#[tokio::test]
async fn virtual_host_puts_the_bucket_in_the_host() {
    let server = store(ResponseTemplate::new(200)).await;
    let port = server.address().port();
    let resolving = client::builder()
        .resolve(&format!("{BUCKET}.{VIRTUAL_DOMAIN}"), *server.address())
        .build()
        .unwrap();
    let sent = batch(Scope::Server, 2);

    transport(
        &format!("http://{VIRTUAL_DOMAIN}:{port}"),
        Addressing::VirtualHost,
        resolving,
    )
    .deliver(&sent)
    .await
    .expect("a 2xx response stores the object");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        header(request, HOST_HEADER),
        format!("{BUCKET}.{VIRTUAL_DOMAIN}:{port}")
    );
    assert_eq!(
        request.url.path(),
        format!("/{}", sigv4::encode_path(&key(&sent)))
    );
    assert!(request.url.path().contains("/server/"));
    assert_signed_object(request, &sent);
}

#[tokio::test]
async fn a_retried_batch_puts_the_same_key_and_body() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let s3 = transport(&server.uri(), Addressing::Path, client::client());
    let sent = batch(Scope::Organization(Uuid::new_v4()), 4);

    let first = s3.deliver(&sent).await.unwrap_err();
    assert_eq!(first.code, FailureCode::Status);
    s3.deliver(&sent)
        .await
        .expect("the retry stores the object");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].url.path(), requests[1].url.path());
    assert_eq!(requests[0].body, requests[1].body);
    for request in &requests {
        assert_signed_object(request, &sent);
    }
}

#[tokio::test]
async fn a_403_is_a_status_failure_without_the_response_body() {
    let server = store(ResponseTemplate::new(403).set_body_string(
        "<Error><Code>SignatureDoesNotMatch</Code><StringToSign>secret</StringToSign></Error>",
    ))
    .await;

    let failure = transport(&server.uri(), Addressing::Path, client::client())
        .deliver(&batch(Scope::Server, 1))
        .await
        .unwrap_err();

    assert_eq!(failure, Failure::new(FailureCode::Status, "HTTP 403"));
}

#[tokio::test]
async fn a_metadata_endpoint_is_a_target_failure() {
    for addressing in Addressing::ALL {
        let failure = transport("http://169.254.169.254", addressing, client::client())
            .deliver(&batch(Scope::Server, 1))
            .await
            .unwrap_err();

        assert_eq!(failure.code, FailureCode::Target, "{addressing:?}");
    }
}

#[tokio::test]
async fn path_style_on_an_endpoint_path_keeps_that_path() {
    let server = store(ResponseTemplate::new(200)).await;
    let sent = batch(Scope::Server, 1);

    transport(
        &format!("{}/storage/", server.uri()),
        Addressing::Path,
        client::client(),
    )
    .deliver(&sent)
    .await
    .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests[0].url.path(),
        format!("/storage/{BUCKET}/{}", sigv4::encode_path(&key(&sent)))
    );
    assert_signed_object(&requests[0], &sent);
}
