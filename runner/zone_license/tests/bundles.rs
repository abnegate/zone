mod support;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeDelta, Utc};
use reqwest::header::{ALLOW, AUTHORIZATION, CACHE_CONTROL, RETRY_AFTER, WWW_AUTHENTICATE};
use reqwest::{Method, StatusCode, Url};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::Level;
use uuid::Uuid;
use wiremock::matchers::{any, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_enterprise::{GRACE_DAYS, Keyring, Plan, TrustedKey};
use zone_license::gate::{CHALLENGE, FORWARDED_FOR, Refusal};
use zone_license::{Config, PrivateKey, ServeError, Signer, Terms, Variable, serve};

use crate::support::logs;

const SECRET: &str = "whsec_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
const KEY_ID: &str = "test-bundles-1";
const OTHER_KEY_ID: &str = "test-bundles-other";
const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const CUSTOMER: &str = "Acme Bundles Ltd";
const BUCKET: &str = "zone-distribution";
const REGION: &str = "ap-southeast-2";
const ACCESS_KEY: &str = "AKIAZONEBUNDLESERVE1";
const SECRET_KEY: &str = "bundle-serve-secret/never+logged";
const VERSION: &str = "1.4.3-lts";
const BUNDLE: &str = "/bundles/1.4.3-lts/amd64";
const LICENSE: Uuid = Uuid::from_u128(0x0193_a8e2_7c4b_7d10_8f00_4d2c_5a9b_3e01);
const OTHER: Uuid = Uuid::from_u128(0x0193_a8e2_7c4b_7d10_8f00_4d2c_5a9b_3e02);
const CLOSED: &str = "http://127.0.0.1:1";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

struct Keys {
    seed: [u8; 32],
    keyring: Keyring,
}

impl Keys {
    fn new() -> Self {
        let mut seed = [0; 32];
        getrandom::fill(&mut seed).unwrap();
        let public = PrivateKey::new(seed).public();
        let keyring = Keyring::new([TrustedKey::new(KEY_ID, public).unwrap()]).unwrap();
        Self { seed, keyring }
    }

    fn signer(&self) -> Signer {
        Signer::new(PrivateKey::new(self.seed), KEY_ID)
    }

    fn license(&self, plan: Plan, expires_at: DateTime<Utc>) -> String {
        sign(&self.signer(), plan, expires_at)
    }

    fn active(&self) -> String {
        self.license(Plan::Enterprise, Utc::now() + TimeDelta::days(335))
    }

    fn in_grace(&self) -> String {
        self.license(Plan::Enterprise, Utc::now() - TimeDelta::days(1))
    }

    fn lapsed(&self) -> String {
        self.license(
            Plan::Enterprise,
            Utc::now() - TimeDelta::days(GRACE_DAYS + 1),
        )
    }

    fn team(&self) -> String {
        self.license(Plan::Team, Utc::now() + TimeDelta::days(335))
    }
}

fn sign(signer: &Signer, plan: Plan, expires_at: DateTime<Utc>) -> String {
    signer
        .sign(&Terms {
            id: LICENSE,
            customer: CUSTOMER.to_owned(),
            seats: 10,
            issued_at: expires_at - TimeDelta::days(365),
            expires_at,
            subscription: None,
            plan,
        })
        .unwrap()
}

fn untrusted() -> String {
    let signer = Signer::new(PrivateKey::new([42; 32]), OTHER_KEY_ID);
    sign(&signer, Plan::Enterprise, Utc::now() + TimeDelta::days(335))
}

fn forged() -> String {
    let signer = Signer::new(PrivateKey::new([43; 32]), KEY_ID);
    sign(&signer, Plan::Enterprise, Utc::now() + TimeDelta::days(335))
}

fn basic(user: &str, password: &str) -> String {
    format!("Basic {}", STANDARD.encode(format!("{user}:{password}")))
}

fn presented(key: &str) -> String {
    basic(&LICENSE.to_string(), key)
}

fn config(keys: &Keys, storage: Option<&MockServer>, changes: &[(Variable, &str)]) -> Config {
    let mut variables = HashMap::from([
        (Variable::WebhookSecret, SECRET.to_owned()),
        (Variable::Products, PRODUCT.to_owned()),
        (Variable::PrivateKey, PrivateKey::new(keys.seed).encode()),
        (Variable::KeyId, KEY_ID.to_owned()),
        (Variable::Listen, "127.0.0.1:0".to_owned()),
        (Variable::SmtpHost, "127.0.0.1".to_owned()),
        (Variable::SmtpUser, "unused".to_owned()),
        (Variable::SmtpPassword, String::new()),
        (Variable::SmtpFrom, "licenses@zone.test".to_owned()),
    ]);
    if let Some(storage) = storage {
        variables.extend([
            (Variable::BundleBucket, BUCKET.to_owned()),
            (Variable::BundleRegion, REGION.to_owned()),
            (Variable::BundleEndpoint, storage.uri()),
            (Variable::BundleAccessKeyId, ACCESS_KEY.to_owned()),
            (Variable::BundleSecretAccessKey, SECRET_KEY.to_owned()),
        ]);
    }
    for (variable, value) in changes {
        variables.insert(*variable, (*value).to_owned());
    }
    Config::from_variables(|variable| variables.get(&variable).cloned(), &keys.keyring).unwrap()
}

struct Service {
    address: SocketAddr,
    client: reqwest::Client,
    stop: oneshot::Sender<()>,
    task: JoinHandle<Result<(), ServeError>>,
}

impl Service {
    async fn start(config: Config) -> Self {
        let server = serve::start(config).await.unwrap();
        let address = server.address().unwrap();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(server.run(async {
            let _ = stopped.await;
        }));
        Self {
            address,
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            stop,
            task,
        }
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        authorization: Option<&str>,
        forwarded: Option<&str>,
    ) -> reqwest::Response {
        let mut request = self
            .client
            .request(method, format!("http://{}{uri}", self.address));
        if let Some(authorization) = authorization {
            request = request.header(AUTHORIZATION, authorization);
        }
        if let Some(forwarded) = forwarded {
            request = request.header(FORWARDED_FOR.as_str(), forwarded);
        }
        request.send().await.unwrap()
    }

    async fn fetch(&self, uri: &str, authorization: Option<&str>) -> reqwest::Response {
        self.send(Method::GET, uri, authorization, None).await
    }

    async fn stop(self) {
        self.stop.send(()).unwrap();
        self.task.await.unwrap().unwrap();
    }
}

fn marker_path() -> String {
    format!("/{BUCKET}/bundles/{VERSION}/zone-bundle-{VERSION}-amd64.tar.sha256.sigstore.json")
}

async fn storage(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(marker_path()))
        .and(header("range", "bytes=0-0"))
        .and(query_param("X-Amz-Expires", "60"))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

async fn present() -> MockServer {
    storage(ResponseTemplate::new(206).set_body_string("{")).await
}

async fn untouched() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(206))
        .mount(&server)
        .await;
    server
}

async fn calls(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

fn s3_error(status: u16, code: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_string(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>{code}</Code><Message>refused</Message><RequestId>4442587FB7D0A2F9</RequestId></Error>"
    ))
}

fn line<'a>(logged: &'a str, outcome: &str) -> &'a str {
    logged
        .lines()
        .find(|line| line.contains(&format!("outcome={outcome}")))
        .unwrap_or_else(|| panic!("no {outcome} line in {logged}"))
}

async fn json(response: reqwest::Response) -> Value {
    response.json().await.unwrap()
}

fn query(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

#[tokio::test]
async fn bundles_are_not_found_without_a_bucket() {
    let keys = Keys::new();
    let service = Service::start(config(&keys, None, &[])).await;

    let logged = logs::capture(Level::INFO, async {
        let response = service
            .fetch(BUNDLE, Some(&presented(&keys.active())))
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    })
    .await;

    line(&logged, "disabled");
    service.stop().await;
}

#[tokio::test]
async fn post_and_head_answer_method_not_allowed_and_issue_nothing() {
    let keys = Keys::new();
    let storage = untouched().await;
    let service = Service::start(config(&keys, Some(&storage), &[])).await;
    let authorization = presented(&keys.active());

    let logged = logs::capture(Level::TRACE, async {
        for method in [Method::POST, Method::HEAD] {
            let response = service
                .send(method.clone(), BUNDLE, Some(&authorization), None)
                .await;
            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method}"
            );
            let allowed: Vec<_> = response.headers().get_all(ALLOW).iter().collect();
            assert_eq!(allowed, ["GET"], "{method}");
        }
    })
    .await;

    assert!(!logged.contains("outcome=issued"), "{logged}");
    assert_eq!(calls(&storage).await, 0);
    service.stop().await;
}

#[tokio::test]
async fn every_shared_refusal_answers_its_contracted_status() {
    let keys = Keys::new();
    let storage = untouched().await;
    let revoked = OTHER.to_string();
    let service = Service::start(config(
        &keys,
        Some(&storage),
        &[(Variable::Revoked, revoked.as_str())],
    ))
    .await;
    let revoked_key = {
        let signer = keys.signer();
        signer
            .sign(&Terms {
                id: OTHER,
                customer: CUSTOMER.to_owned(),
                seats: 10,
                issued_at: Utc::now() - TimeDelta::days(30),
                expires_at: Utc::now() + TimeDelta::days(335),
                subscription: None,
                plan: Plan::Enterprise,
            })
            .unwrap()
    };
    let cases = [
        (Refusal::Anonymous, None),
        (Refusal::Malformed, Some("Bearer not-basic".to_owned())),
        (Refusal::Malformed, Some("Basic !!!".to_owned())),
        (Refusal::Unverified, Some(presented("not a key"))),
        (Refusal::Unverified, Some(presented(&untrusted()))),
        (Refusal::Unverified, Some(presented(&forged()))),
        (
            Refusal::Mismatched,
            Some(basic(&OTHER.to_string(), &keys.active())),
        ),
        (Refusal::Revoked, Some(basic(&revoked, &revoked_key))),
        (Refusal::Ineligible, Some(presented(&keys.team()))),
        (Refusal::Lapsed, Some(presented(&keys.lapsed()))),
    ];

    for (refusal, authorization) in cases {
        let logged = logs::capture(Level::INFO, async {
            let response = service.fetch(BUNDLE, authorization.as_deref()).await;
            assert_eq!(response.status(), refusal.status(), "{refusal}");
            let challenge = response.headers().get(WWW_AUTHENTICATE).cloned();
            if refusal.status() == StatusCode::UNAUTHORIZED {
                assert_eq!(challenge.unwrap(), CHALLENGE, "{refusal}");
            } else {
                assert_eq!(challenge, None, "{refusal}");
            }
            let body = json(response).await;
            assert_eq!(
                body,
                json!({ "errors": [{ "code": refusal.code(), "message": refusal.message() }] }),
                "{refusal}"
            );
        })
        .await;

        let line = line(&logged, refusal.as_str());
        assert!(line.contains("address=127.0.0.1"), "{line}");
    }
    let statuses = Refusal::ALL.map(|refusal| refusal.status().as_u16());
    assert_eq!(statuses, [401, 401, 401, 401, 401, 403, 403]);
    assert_eq!(calls(&storage).await, 0);
    service.stop().await;
}

#[tokio::test]
async fn a_bad_version_or_architecture_is_unknown_without_calling_storage() {
    let keys = Keys::new();
    let storage = untouched().await;
    let service = Service::start(config(&keys, Some(&storage), &[])).await;
    let authorization = presented(&keys.active());

    for uri in [
        "/bundles/1.4.3/amd64",
        "/bundles/1.4-lts/amd64",
        "/bundles/01.4.3-lts/amd64",
        "/bundles/1.4.3-LTS/amd64",
        "/bundles/1.4.3-lts/x86_64",
        "/bundles/1.4.3-lts/AMD64",
        "/bundles/..%2F1.4.3-lts/amd64",
        "/bundles/1.4.3-lts%2F..%2Fx/amd64",
        "/bundles/%FF/amd64",
    ] {
        let logged = logs::capture(Level::INFO, async {
            let response = service.fetch(uri, Some(&authorization)).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        })
        .await;

        let line = line(&logged, "unknown");
        assert!(line.contains(&format!("license={LICENSE}")), "{line}");
    }
    assert_eq!(calls(&storage).await, 0);
    service.stop().await;
}

#[tokio::test]
async fn a_bundle_storage_says_is_missing_is_unknown_with_its_code() {
    let keys = Keys::new();
    for (response, code) in [
        (s3_error(404, "NoSuchKey"), Some("NoSuchKey")),
        (s3_error(403, "AccessDenied"), Some("AccessDenied")),
        (ResponseTemplate::new(404), None),
    ] {
        let storage = storage(response).await;
        let service = Service::start(config(&keys, Some(&storage), &[])).await;

        let logged = logs::capture(Level::INFO, async {
            let response = service
                .fetch(BUNDLE, Some(&presented(&keys.active())))
                .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{code:?}");
        })
        .await;

        let line = line(&logged, "unknown");
        assert!(line.contains(" INFO "), "{line}");
        assert!(line.contains("version=1.4.3-lts"), "{line}");
        assert!(line.contains("architecture=amd64"), "{line}");
        match code {
            Some(code) => assert!(line.contains(&format!("code=\"{code}\"")), "{line}"),
            None => assert!(!line.contains("code="), "{line}"),
        }
        assert_eq!(calls(&storage).await, 1);
        service.stop().await;
    }
}

#[tokio::test]
async fn a_storage_refusal_is_unavailable_and_logged_as_an_error_with_its_code() {
    let keys = Keys::new();
    for (response, code) in [
        (
            s3_error(403, "SignatureDoesNotMatch"),
            Some("SignatureDoesNotMatch"),
        ),
        (
            s3_error(403, "InvalidAccessKeyId"),
            Some("InvalidAccessKeyId"),
        ),
        (s3_error(400, "ExpiredToken"), Some("ExpiredToken")),
        (s3_error(503, "SlowDown"), Some("SlowDown")),
        (ResponseTemplate::new(500), None),
    ] {
        let storage = storage(response).await;
        let service = Service::start(config(&keys, Some(&storage), &[])).await;

        let logged = logs::capture(Level::INFO, async {
            let response = service
                .fetch(BUNDLE, Some(&presented(&keys.active())))
                .await;
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "{code:?}"
            );
        })
        .await;

        let line = line(&logged, "unavailable");
        assert!(line.contains(" ERROR "), "{line}");
        assert!(line.contains(&format!("license={LICENSE}")), "{line}");
        match code {
            Some(code) => assert!(line.contains(&format!("code=\"{code}\"")), "{line}"),
            None => assert!(line.contains("status 500"), "{line}"),
        }
        service.stop().await;
    }
}

#[tokio::test]
async fn storage_that_times_out_or_refuses_connections_is_unavailable() {
    let keys = Keys::new();
    let slow = storage(ResponseTemplate::new(206).set_delay(PROBE_TIMEOUT * 2)).await;
    let refused = config(&keys, Some(&slow), &[(Variable::BundleEndpoint, CLOSED)]);

    for config in [config(&keys, Some(&slow), &[]), refused] {
        let service = Service::start(config).await;

        let logged = logs::capture(Level::INFO, async {
            let response = service
                .fetch(BUNDLE, Some(&presented(&keys.active())))
                .await;
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        })
        .await;

        let line = line(&logged, "unavailable");
        assert!(line.contains(" WARN "), "{line}");
        assert!(!line.contains("code="), "{line}");
        service.stop().await;
    }
}

#[tokio::test]
async fn active_and_grace_licenses_get_the_three_contract_files_for_fifteen_minutes() {
    let keys = Keys::new();
    for key in [keys.active(), keys.in_grace()] {
        let storage = present().await;
        let service = Service::start(config(&keys, Some(&storage), &[])).await;

        let response = service.fetch(BUNDLE, Some(&presented(&key))).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        let body = json(response).await;
        assert_eq!(body["version"], VERSION);
        assert_eq!(body["architecture"], "amd64");
        assert_eq!(body["expires_in"], 900);
        let files = body["files"].as_array().unwrap();
        let names: Vec<&str> = files
            .iter()
            .map(|file| file["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "zone-bundle-1.4.3-lts-amd64.tar",
                "zone-bundle-1.4.3-lts-amd64.tar.sha256",
                "zone-bundle-1.4.3-lts-amd64.tar.sha256.sigstore.json",
            ]
        );
        for file in files {
            assert_eq!(file.as_object().unwrap().len(), 2, "{file}");
            let url: Url = file["url"].as_str().unwrap().parse().unwrap();
            assert_eq!(
                url.path(),
                format!(
                    "/{BUCKET}/bundles/{VERSION}/{}",
                    file["name"].as_str().unwrap()
                )
            );
            assert_eq!(
                query(&url, "X-Amz-Expires").as_deref(),
                Some("900"),
                "{url}"
            );
            assert_eq!(
                query(&url, "X-Amz-Algorithm").as_deref(),
                Some("AWS4-HMAC-SHA256"),
                "{url}"
            );
            assert!(query(&url, "X-Amz-Signature").is_some(), "{url}");
        }
        assert_eq!(body.as_object().unwrap().len(), 4, "{body}");
        assert_eq!(calls(&storage).await, 1);
        service.stop().await;
    }
}

#[tokio::test]
async fn the_address_limit_trips_before_the_key_is_checked() {
    let keys = Keys::new();
    let storage = untouched().await;
    let service = Service::start(config(
        &keys,
        Some(&storage),
        &[(Variable::AddressLimit, "2")],
    ))
    .await;

    assert_eq!(
        service.fetch(BUNDLE, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        service.fetch(BUNDLE, Some("Basic !!!")).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let logged = logs::capture(Level::INFO, async {
        let response = service
            .fetch(BUNDLE, Some(&presented(&keys.active())))
            .await;
        assert_limited(response).await;
    })
    .await;

    let line = line(&logged, "limited");
    assert!(line.contains("address=127.0.0.1"), "{line}");
    assert!(!line.contains("license="), "{line}");
    assert_eq!(calls(&storage).await, 0);
    service.stop().await;
}

#[tokio::test]
async fn the_license_limit_trips_per_license_once_it_is_admitted() {
    let keys = Keys::new();
    let storage = present().await;
    let service = Service::start(config(
        &keys,
        Some(&storage),
        &[(Variable::AddressLimit, "100"), (Variable::KeyLimit, "1")],
    ))
    .await;
    let authorization = presented(&keys.active());

    assert_eq!(
        service.fetch(BUNDLE, Some(&authorization)).await.status(),
        StatusCode::OK
    );
    let logged = logs::capture(Level::INFO, async {
        assert_limited(service.fetch(BUNDLE, Some(&authorization)).await).await;
    })
    .await;

    assert!(
        line(&logged, "limited").contains(&format!("license={LICENSE}")),
        "{logged}"
    );
    assert_eq!(calls(&storage).await, 1);
    service.stop().await;
}

#[tokio::test]
async fn clients_behind_a_trusted_proxy_are_limited_separately() {
    let keys = Keys::new();
    let storage = untouched().await;
    let service = Service::start(config(
        &keys,
        Some(&storage),
        &[
            (Variable::AddressLimit, "1"),
            (Variable::TrustedProxies, "127.0.0.1"),
        ],
    ))
    .await;
    let first = Some("198.51.100.7");
    let second = Some("203.0.113.40");

    let status = |response: reqwest::Response| response.status();
    assert_eq!(
        status(service.send(Method::GET, BUNDLE, None, first).await),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status(service.send(Method::GET, BUNDLE, None, second).await),
        StatusCode::UNAUTHORIZED
    );
    let logged = logs::capture(Level::INFO, async {
        assert_limited(service.send(Method::GET, BUNDLE, None, first).await).await;
    })
    .await;

    assert!(
        line(&logged, "limited").contains("address=198.51.100.7"),
        "{logged}"
    );
    service.stop().await;
}

async fn assert_limited(response: reqwest::Response) {
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after: u64 = response.headers()[RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(retry_after > 0);
    assert_eq!(json(response).await["errors"][0]["code"], "TOOMANYREQUESTS");
}

#[tokio::test]
async fn the_logs_hold_no_key_no_presigned_url_and_no_customer() {
    let keys = Keys::new();
    let storage = present().await;
    let key = keys.active();
    let mut urls = Vec::new();

    let logged = logs::capture(Level::TRACE, async {
        let service = Service::start(config(&keys, Some(&storage), &[])).await;
        let response = service.fetch(BUNDLE, Some(&presented(&key))).await;
        assert_eq!(response.status(), StatusCode::OK);
        for file in json(response).await["files"].as_array().unwrap() {
            urls.push(file["url"].as_str().unwrap().to_owned());
        }
        let refused = service.fetch(BUNDLE, Some(&presented(&forged()))).await;
        assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
        service.stop().await;
    })
    .await;

    assert!(logged.contains("Serving air-gapped bundles"), "{logged}");
    assert!(logged.contains(&format!("bucket=\"{BUCKET}\"")), "{logged}");
    assert!(logged.contains("prefix=\"bundles\""), "{logged}");
    line(&logged, "issued");
    let encoded = presented(&key);
    let mut secrets = vec![
        key.as_str(),
        encoded.as_str(),
        SECRET_KEY,
        CUSTOMER,
        "X-Amz-Signature",
        "X-Amz-Credential",
    ];
    secrets.extend(urls.iter().map(String::as_str));
    for secret in secrets {
        assert!(!logged.contains(secret), "{secret} logged in {logged}");
    }
}

#[tokio::test]
async fn the_existing_routes_still_answer_with_bundles_on() {
    let keys = Keys::new();
    let storage = untouched().await;
    let service = Service::start(config(&keys, Some(&storage), &[])).await;

    let health = service.fetch("/health", None).await;
    assert_eq!(health.status(), StatusCode::OK);
    assert_eq!(json(health).await, json!({ "status": "ok" }));
    let renew = service
        .send(Method::POST, "/licenses/renew", None, None)
        .await;
    assert_eq!(renew.status(), StatusCode::NOT_FOUND);
    assert_eq!(calls(&storage).await, 0);
    service.stop().await;
}
