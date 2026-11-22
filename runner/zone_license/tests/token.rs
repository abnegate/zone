mod support;

use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::num::NonZeroU32;

use chrono::{DateTime, TimeDelta, Utc};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use reqwest::header::{ALLOW, CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER, WWW_AUTHENTICATE};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;
use zone_enterprise::{Keyring, Plan, TrustedKey};
use zone_license::gate::{CHALLENGE, FORWARDED_FOR, NONE};
use zone_license::http::{ALLOWED, HEALTH_PATH, RENEW_PATH, TOKEN_PATH, WEBHOOK_PATH};
use zone_license::registry::ISSUER;
use zone_license::{
    Config, PrivateKey, RegistryArguments, RegistryCommand, ServeError, Signer, Terms, Variable,
    serve,
};

use crate::support::logged;

const KEY_ID: &str = "test-token-1";
const SEED: [u8; 32] = [21; 32];
const OTHER_KEY_ID: &str = "test-token-unknown";
const OTHER_SEED: [u8; 32] = [22; 32];
const SECRET: &str = "whsec_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const SERVICE: &str = "registry.zone.test";
const CUSTOMER: &str = "Acme Registry Customer Ltd";
const LICENSE: Uuid = Uuid::from_u128(0x0193_a8e2_7c4b_7d10_8f00_4d2c_5a9b_3e01);
const OTHER: Uuid = Uuid::from_u128(0x0193_a8e2_7c4b_7d10_8f00_4d2c_5a9b_3e02);
const SUBSCRIPTION: &str = "b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c";
const PROXY: &str = "127.0.0.1/32";
const FIRST_CLIENT: &str = "198.51.100.10";
const SECOND_CLIENT: &str = "198.51.100.20";
const SERVER_PULL: &str = "repository:zone/server:pull";

struct Service {
    address: SocketAddr,
    client: reqwest::Client,
    stop: oneshot::Sender<()>,
    task: JoinHandle<Result<(), ServeError>>,
}

fn signer() -> Signer {
    Signer::new(PrivateKey::new(SEED), KEY_ID)
}

fn keyring() -> Keyring {
    Keyring::new([TrustedKey::new(KEY_ID, signer().public()).unwrap()]).unwrap()
}

/// The relay is never reached: nothing on these routes sends email.
fn config(extra: &[(Variable, &str)]) -> Config {
    let mut variables = HashMap::from([
        (Variable::WebhookSecret, SECRET.to_owned()),
        (Variable::Products, PRODUCT.to_owned()),
        (Variable::PrivateKey, PrivateKey::new(SEED).encode()),
        (Variable::KeyId, KEY_ID.to_owned()),
        (Variable::Listen, "127.0.0.1:0".to_owned()),
        (Variable::SmtpHost, "127.0.0.1".to_owned()),
        (Variable::SmtpUser, "unused".to_owned()),
        (Variable::SmtpPassword, String::new()),
        (Variable::SmtpFrom, "licenses@zone.test".to_owned()),
    ]);
    for (variable, value) in extra {
        variables.insert(*variable, (*value).to_owned());
    }
    Config::from_variables(|variable| variables.get(&variable).cloned(), &keyring()).unwrap()
}

impl Service {
    async fn start(extra: &[(Variable, &str)]) -> Self {
        let server = serve::start(config(extra)).await.unwrap();
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

    async fn enabled() -> Self {
        Self::start(&[
            (Variable::RegistryService, SERVICE),
            (Variable::TrustedProxies, NONE),
        ])
        .await
    }

    fn request(&self, method: Method, query: &str) -> RequestBuilder {
        self.client.request(
            method,
            format!("http://{}{TOKEN_PATH}?{query}", self.address),
        )
    }

    fn token(&self, query: &str, key: &str) -> RequestBuilder {
        self.request(Method::GET, query)
            .basic_auth(LICENSE, Some(key))
    }

    async fn stop(self) {
        self.stop.send(()).unwrap();
        self.task.await.unwrap().unwrap();
    }
}

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn signed_by(signer: &Signer, id: Uuid, plan: Plan, expires_at: DateTime<Utc>) -> String {
    signer
        .sign(&Terms {
            id,
            customer: CUSTOMER.to_owned(),
            seats: 10,
            issued_at: expires_at - TimeDelta::days(365),
            expires_at,
            subscription: None,
            plan,
        })
        .unwrap()
}

fn licensed(plan: Plan, expires_at: DateTime<Utc>) -> String {
    signed_by(&signer(), LICENSE, plan, expires_at)
}

fn enterprise() -> String {
    licensed(Plan::Enterprise, now() + TimeDelta::days(30))
}

fn tampered(key: &str) -> String {
    let mut bytes = key.as_bytes().to_vec();
    let index = bytes.len() - 8;
    bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
    String::from_utf8(bytes).unwrap()
}

fn query(scopes: &[&str]) -> String {
    let mut query = format!("service={SERVICE}");
    for scope in scopes {
        query.push_str("&scope=");
        query.push_str(&scope.replace(' ', "%20"));
    }
    query
}

async fn json(response: Response) -> Value {
    serde_json::from_str(&response.text().await.unwrap()).unwrap()
}

async fn issued(service: &Service, scopes: &[&str]) -> Value {
    let response = service
        .token(&query(scopes), &enterprise())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-store");
    json(response).await
}

/// Runs the `zone-license registry jwks` command code path, as an operator
/// does to give the registry its key set.
fn jwks() -> JwkSet {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("signing.key");
    fs::write(&path, PrivateKey::new(SEED).encode()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let arguments = RegistryArguments {
        generations: vec![NonZeroU32::MIN],
        key_id: Some(KEY_ID.to_owned()),
        private_key_file: Some(path),
    };
    let mut printed = Vec::new();
    RegistryCommand::Jwks(arguments)
        .run(&keyring(), &mut printed)
        .unwrap();
    serde_json::from_slice(&printed).unwrap()
}

fn verified(token: &str) -> Value {
    let kid = jsonwebtoken::decode_header(token).unwrap().kid.unwrap();
    let set = jwks();
    let jwk = set
        .find(&kid)
        .expect("the token's kid is in the printed key set");
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.set_audience(&[SERVICE]);
    validation.set_issuer(&[ISSUER]);
    validation.set_required_spec_claims(&["exp", "nbf", "iat", "aud", "iss", "sub"]);
    validation.validate_nbf = true;
    jsonwebtoken::decode::<Value>(token, &DecodingKey::from_jwk(jwk).unwrap(), &validation)
        .unwrap()
        .claims
}

fn access(answer: &Value) -> Value {
    verified(answer["token"].as_str().unwrap())["access"].clone()
}

fn pulls(names: &[&str]) -> Value {
    Value::Array(
        names
            .iter()
            .map(|name| json!({ "type": "repository", "name": name, "actions": ["pull"] }))
            .collect(),
    )
}

async fn assert_unauthorized(response: Response, case: &str) {
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{case}");
    assert_eq!(
        response.headers().get(CACHE_CONTROL).unwrap(),
        "no-store",
        "{case}"
    );
    assert_eq!(
        response.headers().get(WWW_AUTHENTICATE).unwrap(),
        CHALLENGE,
        "{case}"
    );
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/json",
        "{case}"
    );
    assert_eq!(
        json(response).await["errors"][0]["code"],
        "UNAUTHORIZED",
        "{case}"
    );
}

async fn assert_forbidden(response: Response, case: &str) {
    assert_eq!(response.status(), StatusCode::FORBIDDEN, "{case}");
    assert!(response.headers().get(WWW_AUTHENTICATE).is_none(), "{case}");
    assert_eq!(
        json(response).await["errors"][0]["code"],
        "DENIED",
        "{case}"
    );
}

async fn assert_limited(response: Response) {
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let wait: u64 = response
        .headers()
        .get(RETRY_AFTER)
        .expect("a Retry-After header")
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=60).contains(&wait), "{wait}");
    assert_eq!(json(response).await["errors"][0]["code"], "TOOMANYREQUESTS");
}

#[tokio::test]
async fn without_a_registry_service_the_token_route_is_not_found() {
    let service = Service::start(&[]).await;

    let response = service
        .token(&query(&[SERVER_PULL]), &enterprise())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(response.text().await.unwrap().is_empty());
    service.stop().await;
}

#[tokio::test]
async fn post_and_head_are_not_allowed_mint_nothing_and_log_no_issue() {
    let service = Service::enabled().await;
    assert_eq!(ALLOWED, "GET");

    let logged = logged::capture(tracing::Level::TRACE, async {
        for method in [Method::POST, Method::HEAD] {
            let response = service
                .request(method.clone(), &query(&[SERVER_PULL]))
                .basic_auth(LICENSE, Some(enterprise()))
                .form(&[("grant_type", "password"), ("service", SERVICE)])
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method}"
            );
            assert_eq!(response.headers().get(ALLOW).unwrap(), ALLOWED, "{method}");
            assert!(response.text().await.unwrap().is_empty(), "{method}");
        }
    })
    .await;

    assert!(!logged.contains("outcome=issued"), "{logged}");
    assert!(!logged.contains("Issued a registry token"), "{logged}");
    service.stop().await;
}

#[tokio::test]
async fn each_bad_credential_is_unauthorized_with_a_basic_challenge() {
    let revoked = LICENSE.to_string();
    let service = Service::start(&[
        (Variable::RegistryService, SERVICE),
        (Variable::TrustedProxies, NONE),
        (Variable::Revoked, revoked.as_str()),
    ])
    .await;
    let unrevoked = Service::enabled().await;
    let query = query(&[SERVER_PULL]);
    let unknown = signed_by(
        &Signer::new(PrivateKey::new(OTHER_SEED), OTHER_KEY_ID),
        OTHER,
        Plan::Enterprise,
        now() + TimeDelta::days(30),
    );

    let anonymous = unrevoked.request(Method::GET, &query).send().await.unwrap();
    assert_unauthorized(anonymous, "anonymous").await;
    let garbage = unrevoked
        .request(Method::GET, &query)
        .basic_auth(OTHER, Some("not a license key"))
        .send()
        .await
        .unwrap();
    assert_unauthorized(garbage, "garbage").await;
    let malformed = unrevoked
        .request(Method::GET, &query)
        .header("authorization", "Basic !!!")
        .send()
        .await
        .unwrap();
    assert_unauthorized(malformed, "malformed").await;
    let unknown = unrevoked
        .request(Method::GET, &query)
        .basic_auth(OTHER, Some(unknown))
        .send()
        .await
        .unwrap();
    assert_unauthorized(unknown, "unknown key id").await;
    let tampered = unrevoked
        .token(&query, &tampered(&enterprise()))
        .send()
        .await
        .unwrap();
    assert_unauthorized(tampered, "tampered").await;
    let mismatched = unrevoked
        .request(Method::GET, &query)
        .basic_auth(OTHER, Some(enterprise()))
        .send()
        .await
        .unwrap();
    assert_unauthorized(mismatched, "mismatched user name").await;
    let revoked = service.token(&query, &enterprise()).send().await.unwrap();
    assert_unauthorized(revoked, "revoked").await;
    let accepted = unrevoked.token(&query, &enterprise()).send().await.unwrap();
    assert_eq!(accepted.status(), StatusCode::OK, "the same key unrevoked");

    service.stop().await;
    unrevoked.stop().await;
}

#[tokio::test]
async fn a_revoked_subscription_refuses_a_renewed_key_with_a_new_id() {
    let service = Service::start(&[
        (Variable::RegistryService, SERVICE),
        (Variable::TrustedProxies, NONE),
        (Variable::RevokedSubscriptions, SUBSCRIPTION),
    ])
    .await;
    let renewed = signer()
        .sign(&Terms {
            id: OTHER,
            customer: CUSTOMER.to_owned(),
            seats: 10,
            issued_at: now() - TimeDelta::days(1),
            expires_at: now() + TimeDelta::days(364),
            subscription: Some(SUBSCRIPTION.to_owned()),
            plan: Plan::Enterprise,
        })
        .unwrap();

    let logged = logged::capture(tracing::Level::INFO, async {
        let response = service
            .request(Method::GET, &query(&[SERVER_PULL]))
            .basic_auth(OTHER, Some(&renewed))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            json(response).await,
            json!({ "errors": [{ "code": "UNAUTHORIZED", "message": "This license has been revoked." }] })
        );
    })
    .await;

    let revoked = logged
        .lines()
        .find(|line| line.contains("outcome=revoked"))
        .unwrap_or_else(|| panic!("no revoked line in {logged}"));
    assert!(revoked.contains(&format!("license={OTHER}")), "{revoked}");
    assert!(!logged.contains("outcome=issued"), "{logged}");
    service.stop().await;
}

#[tokio::test]
async fn team_and_lapsed_keys_are_forbidden_while_grace_is_issued() {
    let service = Service::enabled().await;
    let query = query(&[SERVER_PULL]);

    let team = licensed(Plan::Team, now() + TimeDelta::days(30));
    let response = service.token(&query, &team).send().await.unwrap();
    assert_forbidden(response, "team").await;
    let lapsed = licensed(Plan::Enterprise, now() - TimeDelta::days(31));
    let response = service.token(&query, &lapsed).send().await.unwrap();
    assert_forbidden(response, "lapsed beyond grace").await;
    let grace = licensed(Plan::Enterprise, now() - TimeDelta::days(1));
    let response = service.token(&query, &grace).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(access(&json(response).await), pulls(&["zone/server"]));

    service.stop().await;
}

#[tokio::test]
async fn the_user_name_is_the_license_id_in_any_case() {
    let service = Service::enabled().await;

    let response = service
        .request(Method::GET, &query(&[SERVER_PULL]))
        .basic_auth(
            LICENSE.to_string().to_uppercase(),
            Some(format!(" {} ", enterprise())),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    service.stop().await;
}

#[tokio::test]
async fn grants_only_pull_on_the_lts_repositories() {
    let service = Service::enabled().await;

    for (scopes, expected) in [
        (vec![SERVER_PULL], pulls(&["zone/server"])),
        (vec!["repository:zone/server:push"], pulls(&[])),
        (
            vec!["repository:zone/server:pull,push,delete"],
            pulls(&["zone/server"]),
        ),
        (vec!["repository:zone/server:*"], pulls(&[])),
        (
            vec!["repository:zone/private:pull", SERVER_PULL],
            pulls(&["zone/server"]),
        ),
        (vec!["registry:catalog:*"], pulls(&[])),
        (
            vec!["repository:zone/server:pull repository:zone/manager:pull"],
            pulls(&["zone/server", "zone/manager"]),
        ),
        (
            vec![SERVER_PULL, "repository:zone/litellm:pull"],
            pulls(&["zone/server", "zone/litellm"]),
        ),
        (vec![], pulls(&[])),
    ] {
        let answer = issued(&service, &scopes).await;
        assert_eq!(access(&answer), expected, "{scopes:?}");
    }

    service.stop().await;
}

#[tokio::test]
async fn a_missing_or_wrong_service_is_unreadable() {
    let service = Service::enabled().await;

    for query in [
        format!("scope={SERVER_PULL}"),
        format!("service=registry.other.test&scope={SERVER_PULL}"),
    ] {
        let response = service.token(&query, &enterprise()).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
    }
    let malformed = service
        .token(&query(&["repository:zone/server"]), &enterprise())
        .send()
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

    service.stop().await;
}

#[tokio::test]
async fn the_address_limit_answers_too_many_requests() {
    let service = Service::start(&[
        (Variable::RegistryService, SERVICE),
        (Variable::TrustedProxies, NONE),
        (Variable::AddressLimit, "2"),
    ])
    .await;
    let query = query(&[SERVER_PULL]);

    for _ in 0..2 {
        let response = service.request(Method::GET, &query).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    assert_limited(service.request(Method::GET, &query).send().await.unwrap()).await;
    assert_limited(service.token(&query, &enterprise()).send().await.unwrap()).await;

    service.stop().await;
}

#[tokio::test]
async fn the_key_limit_follows_the_license_across_addresses() {
    let service = Service::start(&[
        (Variable::RegistryService, SERVICE),
        (Variable::TrustedProxies, PROXY),
        (Variable::KeyLimit, "2"),
    ])
    .await;
    let query = query(&[SERVER_PULL]);

    for client in [FIRST_CLIENT, SECOND_CLIENT] {
        let response = service
            .token(&query, &enterprise())
            .header(FORWARDED_FOR.as_str(), client)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{client}");
    }
    let response = service
        .token(&query, &enterprise())
        .header(FORWARDED_FOR.as_str(), "198.51.100.30")
        .send()
        .await
        .unwrap();
    assert_limited(response).await;

    service.stop().await;
}

#[tokio::test]
async fn clients_behind_a_trusted_proxy_are_limited_separately() {
    let service = Service::start(&[
        (Variable::RegistryService, SERVICE),
        (Variable::TrustedProxies, PROXY),
        (Variable::AddressLimit, "2"),
    ])
    .await;
    let query = query(&[SERVER_PULL]);
    let from = |client: &str| {
        service
            .request(Method::GET, &query)
            .header(FORWARDED_FOR.as_str(), format!("203.0.113.99, {client}"))
    };

    let logged = logged::capture(tracing::Level::INFO, async {
        for _ in 0..2 {
            let response = from(FIRST_CLIENT).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        assert_limited(from(FIRST_CLIENT).send().await.unwrap()).await;
        let second = from(SECOND_CLIENT).send().await.unwrap();
        assert_eq!(second.status(), StatusCode::UNAUTHORIZED);
    })
    .await;

    assert!(
        logged.contains(&format!("address={FIRST_CLIENT}")),
        "{logged}"
    );
    assert!(
        logged.contains(&format!("address={SECOND_CLIENT}")),
        "{logged}"
    );
    assert!(!logged.contains("address=127.0.0.1"), "{logged}");
    assert!(!logged.contains("address=203.0.113.99"), "{logged}");
    service.stop().await;
}

#[tokio::test]
async fn without_a_trusted_proxy_forwarded_clients_share_the_peer_limit() {
    let service = Service::start(&[
        (Variable::RegistryService, SERVICE),
        (Variable::TrustedProxies, NONE),
        (Variable::AddressLimit, "2"),
    ])
    .await;
    let query = query(&[SERVER_PULL]);

    for client in [FIRST_CLIENT, FIRST_CLIENT] {
        let response = service
            .request(Method::GET, &query)
            .header(FORWARDED_FOR.as_str(), client)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let second = service
        .request(Method::GET, &query)
        .header(FORWARDED_FOR.as_str(), SECOND_CLIENT)
        .send()
        .await
        .unwrap();
    assert_limited(second).await;

    service.stop().await;
}

#[tokio::test]
async fn the_token_verifies_against_the_printed_key_set() {
    let service = Service::enabled().await;

    let before = now().timestamp();
    let answer = issued(&service, &[SERVER_PULL]).await;
    let after = now().timestamp();

    let token = answer["token"].as_str().unwrap();
    assert_eq!(answer["access_token"], token);
    assert_eq!(answer["expires_in"], 300);
    let header = jsonwebtoken::decode_header(token).unwrap();
    assert_eq!(header.alg, Algorithm::EdDSA);
    assert_eq!(header.typ.as_deref(), Some("JWT"));
    assert_eq!(header.kid.as_deref(), Some("registry-test-token-1-1"));
    let claims = verified(token);
    assert_eq!(claims["iss"], ISSUER);
    assert_eq!(claims["sub"], LICENSE.to_string());
    assert_eq!(claims["aud"], SERVICE);
    let issued_at = claims["iat"].as_i64().unwrap();
    assert!((before..=after).contains(&issued_at), "{claims}");
    assert_eq!(claims["nbf"], issued_at);
    assert_eq!(claims["exp"], issued_at + 300);
    let jti: Uuid = claims["jti"].as_str().unwrap().parse().unwrap();
    assert_eq!(jti.get_version_num(), 4);
    assert_eq!(claims["access"], pulls(&["zone/server"]));
    let stamped = DateTime::parse_from_rfc3339(answer["issued_at"].as_str().unwrap()).unwrap();
    assert_eq!(stamped.timestamp(), issued_at);

    service.stop().await;
}

#[tokio::test]
async fn the_logs_hold_no_key_token_or_customer_name() {
    let service = Service::start(&[
        (Variable::RegistryService, SERVICE),
        (Variable::TrustedProxies, PROXY),
        (Variable::KeyLimit, "1"),
    ])
    .await;
    let key = enterprise();
    let team = licensed(Plan::Team, now() + TimeDelta::days(30));
    let query = query(&[SERVER_PULL]);
    let mut tokens = Vec::new();

    let logged = logged::capture(tracing::Level::TRACE, async {
        let response = service
            .token(&query, &key)
            .header(FORWARDED_FOR.as_str(), FIRST_CLIENT)
            .send()
            .await
            .unwrap();
        tokens.push(json(response).await["token"].as_str().unwrap().to_owned());
        service.token(&query, &key).send().await.unwrap();
        service.token(&query, &team).send().await.unwrap();
        service.token(&query, &tampered(&key)).send().await.unwrap();
        service
            .request(Method::GET, &query)
            .basic_auth(OTHER, Some(&key))
            .send()
            .await
            .unwrap();
    })
    .await;

    let issued = logged
        .lines()
        .find(|line| line.contains("outcome=issued"))
        .unwrap_or_else(|| panic!("no issued line in {logged}"));
    for field in [
        format!("license={LICENSE}"),
        format!("address={FIRST_CLIENT}"),
        "granted=zone/server:pull".to_owned(),
        "dropped=0".to_owned(),
    ] {
        assert!(issued.contains(&field), "{field} missing from {issued}");
    }
    for outcome in ["limited", "ineligible", "unverified", "mismatched"] {
        assert!(
            logged.contains(&format!("outcome={outcome}")),
            "{outcome} in {logged}"
        );
    }
    for secret in [key.as_str(), team.as_str(), CUSTOMER, &tokens[0]] {
        assert!(!logged.contains(secret), "{secret} logged in {logged}");
    }
    service.stop().await;
}

#[tokio::test]
async fn the_existing_routes_answer_as_before_with_tokens_on() {
    let service = Service::enabled().await;
    let base = format!("http://{}", service.address);

    let health = service
        .client
        .get(format!("{base}{HEALTH_PATH}"))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    assert_eq!(json(health).await, json!({ "status": "ok" }));
    let renewal = service
        .client
        .post(format!("{base}{RENEW_PATH}"))
        .json(&json!({ "license": enterprise() }))
        .send()
        .await
        .unwrap();
    assert_eq!(renewal.status(), StatusCode::NOT_FOUND);
    let webhook = service
        .client
        .post(format!("{base}{WEBHOOK_PATH}"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(webhook.status(), StatusCode::UNAUTHORIZED);

    service.stop().await;
}
