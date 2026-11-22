use std::net::SocketAddr;
use std::num::NonZeroU32;

use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::header::{ALLOW, AUTHORIZATION, CONTENT_TYPE};
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chrono::{DateTime, TimeDelta, Utc};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use zone_enterprise::{Keyring, Plan, TrustedKey};

use super::*;
use crate::gate::{Gate, Limits, Revoked, Throttle, TrustedProxies};
use crate::http::tests::Recording;
use crate::http::{TOKEN_PATH, router};
use crate::logging;
use crate::polar::WebhookSecret;
use crate::private_key::PrivateKey;
use crate::products::Products;
use crate::registry::{RegistrySettings, Repositories, TokenKey};
use crate::signer::Signer;
use crate::terms::Terms;

const KEY_ID: &str = "test-token-1";
const SEED: [u8; 32] = [13; 32];
const SECRET: &str = "whsec_token-test-secret";
const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const SERVICE: &str = "registry.zone.test";
const CUSTOMER: &str = "Acme Token Ltd";
const LICENSE: Uuid = Uuid::from_u128(0x0193_a8e2_7c4b_7d10_8f00_4d2c_5a9b_2e01);
const PEER: [u8; 4] = [203, 0, 113, 7];
const SERVER_PULL: &str = "repository:zone/server:pull";

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-23T04:05:06.789Z")
        .unwrap()
        .to_utc()
}

fn signer() -> Signer {
    Signer::new(PrivateKey::new(SEED), KEY_ID)
}

fn keyring() -> Keyring {
    Keyring::new([TrustedKey::new(KEY_ID, signer().public()).unwrap()]).unwrap()
}

fn key(plan: Plan) -> String {
    signer()
        .sign(&Terms {
            id: LICENSE,
            customer: CUSTOMER.to_owned(),
            seats: 10,
            issued_at: now() - TimeDelta::days(30),
            expires_at: now() + TimeDelta::days(335),
            subscription: None,
            plan,
        })
        .unwrap()
}

fn generation() -> NonZeroU32 {
    NonZeroU32::MIN
}

fn registrar() -> Registrar {
    let gate = Gate::new(keyring(), Revoked::default(), TrustedProxies::default());
    let settings = RegistrySettings {
        service: SERVICE.to_owned(),
        repositories: Repositories::default(),
        generation: generation(),
    };
    Registrar::new(
        gate,
        TokenKey::derive(&signer(), generation()),
        settings,
        Throttle::new(Limits::default()),
    )
    .with_clock(now)
}

fn context(registrar: Option<Registrar>) -> Context {
    let context = Context::new(
        signer(),
        WebhookSecret::new(SECRET),
        Products::parse(PRODUCT).unwrap(),
        Arc::new(Recording::default()),
    );
    match registrar {
        Some(registrar) => context.with_registrar(registrar),
        None => context,
    }
}

fn app(registrar: Option<Registrar>) -> Router {
    router(context(registrar)).layer(MockConnectInfo(SocketAddr::from((PEER, 41_000))))
}

fn basic(user: &str, password: &str) -> String {
    format!("Basic {}", STANDARD.encode(format!("{user}:{password}")))
}

fn requesting(method: Method, query: &str, password: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(format!("{TOKEN_PATH}?{query}"))
        .header(AUTHORIZATION, basic(&LICENSE.to_string(), password))
        .body(Body::empty())
        .unwrap()
}

fn get(query: &str) -> Request<Body> {
    requesting(Method::GET, query, &key(Plan::Enterprise))
}

async fn send(app: Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

async fn issued(app: Router, query: &str) -> Value {
    let (status, body) = send(app, get(query)).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).unwrap()
}

fn claims(token: &str) -> Value {
    let payload = token.split('.').nth(1).unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap()
}

fn granted(answer: &Value) -> Vec<String> {
    claims(answer["token"].as_str().unwrap())["access"]
        .as_array()
        .unwrap()
        .iter()
        .map(|access| access["name"].as_str().unwrap().to_owned())
        .collect()
}

fn line<'a>(logged: &'a str, needle: &str) -> &'a str {
    logged
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle} in {logged}"))
}

#[tokio::test]
async fn without_a_peer_address_it_fails_closed_and_mints_nothing() {
    let app = router(context(Some(registrar())));

    let logged = logging::capture(tracing::Level::INFO, async {
        let (status, body) = send(app, get(&format!("service={SERVICE}"))).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "errors": [{ "code": "UNKNOWN", "message": "The client address can't be read." }] })
        );
    })
    .await;

    let refused = line(&logged, "outcome=unaddressable");
    assert!(refused.contains("ERROR"), "{refused}");
    assert!(!logged.contains("outcome=issued"), "{logged}");
    assert!(!logged.contains("Issued a registry token"), "{logged}");
}

#[tokio::test]
async fn issues_at_the_injected_clock_with_a_fresh_jti_each_time() {
    let query = format!("service={SERVICE}&scope={SERVER_PULL}");
    let first = issued(app(Some(registrar())), &query).await;
    let second = issued(app(Some(registrar())), &query).await;

    assert_eq!(first["issued_at"], "2026-11-23T04:05:06Z");
    assert_eq!(first["expires_in"], 300);
    assert_eq!(first["token"], first["access_token"]);
    let claims_first = claims(first["token"].as_str().unwrap());
    let claims_second = claims(second["token"].as_str().unwrap());
    assert_eq!(claims_first["iat"], now().timestamp());
    assert_eq!(claims_first["nbf"], now().timestamp());
    assert_eq!(claims_first["exp"], now().timestamp() + 300);
    assert_eq!(claims_first["aud"], SERVICE);
    assert_eq!(claims_first["sub"], LICENSE.to_string());
    let jti: Uuid = claims_first["jti"].as_str().unwrap().parse().unwrap();
    assert_eq!(jti.get_version_num(), 4);
    assert_ne!(claims_first["jti"], claims_second["jti"]);
}

#[tokio::test]
async fn the_issued_body_has_exactly_the_contracted_fields() {
    let answer = issued(app(Some(registrar())), &format!("service={SERVICE}")).await;

    let mut fields: Vec<&str> = answer
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(fields, ["access_token", "expires_in", "issued_at", "token"]);
}

#[tokio::test]
async fn ignores_the_parameters_docker_adds_and_reads_encoded_scopes() {
    let query = format!(
        "account={LICENSE}&client_id=docker&offline_token=true&service={SERVICE}\
         &scope=repository%3Azone%2Fserver%3Apull+repository:zone/manager:pull"
    );

    let answer = issued(app(Some(registrar())), &query).await;

    assert_eq!(granted(&answer), ["zone/server", "zone/manager"]);
}

#[tokio::test]
async fn the_service_must_appear_exactly_once_and_match() {
    for query in [
        String::new(),
        "service=".to_owned(),
        format!("service={SERVICE}.evil"),
        format!("service={SERVICE}&service={SERVICE}"),
        format!("service={SERVICE}&service=other"),
        format!("Service={SERVICE}"),
    ] {
        let (status, body) = send(app(Some(registrar())), get(&query)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(
            String::from_utf8(body).unwrap(),
            format!("Send service={SERVICE} once."),
            "{query}"
        );
    }
}

#[tokio::test]
async fn a_malformed_scope_is_unreadable_and_names_the_scope() {
    for scope in [
        "repository:zone/server",
        "repository::pull",
        "repository:zone/server:PULL",
    ] {
        let query = format!("service={SERVICE}&scope={scope}");
        let (status, body) = send(app(Some(registrar())), get(&query)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{scope}");
        assert!(String::from_utf8(body).unwrap().contains(scope), "{scope}");
    }
}

#[tokio::test]
async fn logs_each_outcome_with_its_fields() {
    let logged = logging::capture(tracing::Level::INFO, async {
        let query = format!(
            "service={SERVICE}&scope=repository:zone/server:pull,push&scope=repository:other/x:pull"
        );
        send(app(Some(registrar())), get(&query)).await;
        send(
            app(Some(registrar())),
            requesting(Method::GET, &query, "garbage"),
        )
        .await;
        send(
            app(Some(registrar())),
            requesting(Method::GET, &query, &key(Plan::Team)),
        )
        .await;
        send(app(Some(registrar())), get("service=other")).await;
        send(app(None), get(&query)).await;
    })
    .await;

    let issued = line(&logged, "outcome=issued");
    for field in [
        format!("license={LICENSE}"),
        "address=203.0.113.7".to_owned(),
        "granted=zone/server:pull".to_owned(),
        "dropped=2".to_owned(),
        "plan=enterprise".to_owned(),
        "status=active".to_owned(),
    ] {
        assert!(issued.contains(&field), "{field} missing from {issued}");
    }
    let unverified = line(&logged, "outcome=unverified");
    assert!(
        unverified.contains("error=\"license_malformed\""),
        "{unverified}"
    );
    assert!(unverified.contains("address=203.0.113.7"), "{unverified}");
    assert!(!unverified.contains("license="), "{unverified}");
    let ineligible = line(&logged, "outcome=ineligible");
    assert!(ineligible.contains("plan=team"), "{ineligible}");
    assert!(
        ineligible.contains(&format!("license={LICENSE}")),
        "{ineligible}"
    );
    let unreadable = line(&logged, "outcome=unreadable");
    assert!(
        unreadable.contains(&format!("license={LICENSE}")),
        "{unreadable}"
    );
    assert!(unreadable.contains("status=active"), "{unreadable}");
    assert!(line(&logged, "outcome=disabled").contains("INFO"));
    for secret in [key(Plan::Enterprise), key(Plan::Team), CUSTOMER.to_owned()] {
        assert!(!logged.contains(&secret), "{secret} logged in {logged}");
    }
}

#[tokio::test]
async fn every_method_but_get_is_not_allowed_whether_or_not_tokens_are_on() {
    for registrar in [Some(registrar()), None] {
        let enabled = registrar.is_some();
        let app = app(registrar);
        for method in [
            Method::POST,
            Method::HEAD,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
            Method::PATCH,
        ] {
            let request = requesting(
                method.clone(),
                &format!("service={SERVICE}"),
                &key(Plan::Enterprise),
            );
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} enabled={enabled}"
            );
            assert_eq!(response.headers().get(ALLOW).unwrap(), "GET", "{method}");
            assert!(response.headers().get(CONTENT_TYPE).is_none(), "{method}");
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert!(bytes.is_empty(), "{method}");
        }
    }
}

#[tokio::test]
async fn disabled_answers_not_found_with_an_empty_body() {
    let (status, body) = send(app(None), get(&format!("service={SERVICE}"))).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.is_empty());
}

#[test]
fn reads_repeated_and_unrelated_parameters() {
    let (services, scopes) =
        parameters("scope=a&service=one&account=x&scope=b%20c&service=two&scope");
    assert_eq!(services, ["one", "two"]);
    assert_eq!(scopes, ["a", "b c", ""]);
    assert_eq!(parameters(""), (Vec::new(), Vec::new()));
}
