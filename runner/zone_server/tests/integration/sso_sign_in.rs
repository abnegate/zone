//! Signing in through a configured identity provider: the public provider list, starting a
//! sign-in, and the callback that links, reclaims or provisions the account and starts a session.

use std::collections::HashMap;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{TimeDelta, Utc};
use reqwest::Url;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::sso::{
    Audience, Claims, DISCOVERY_FAILED, DOMAIN_NOT_ALLOWED, EMAIL_CONFLICT, EMAIL_MISSING,
    EMAIL_UNVERIFIED, ID_TOKEN_INVALID, PROVIDER_NOT_FOUND, SSO_REQUIRED, STATE_INVALID,
    TOKEN_EXCHANGE_FAILED, challenge, testing,
};
use zone_enterprise::{Tier, period_start};
use zone_server::auth::hash_password;
use zone_server::db::audit::{actions, resources};
use zone_server::db::devices::{self, Mode};
use zone_server::db::sso::identities;
use zone_server::db::{refresh_tokens, seats, sessions, users};
use zone_server::services::account::provision;
use zone_server::services::sso::console::CALLBACK_PATH;
use zone_server::utils::crypto::hash_token;

use crate::common::identity_provider::{AUTHORIZE_PATH, IdentityProvider, REDIRECT_URI};
use crate::common::sso::{Account, Fixture, lapsed};
use crate::common::{TestResponse, test_email, test_password};

const PREFIX: &str = "sso_sign_in";
const PUBLIC_PROVIDERS: &str = "/api/auth/sso/providers";
const CALLBACKS: &str = "/api/auth/sso/callbacks";
const ACCOUNT_DISABLED: &str = "account_disabled";
const DEVICE_BLOCKED: &str = "device_blocked";
const UNREACHABLE_ISSUER: &str = "http://127.0.0.1:9";
const TIME_ZONE: &str = "Europe/London";
const DEVICE_PENDING: &str = "device_pending";
const OTHER_CLIENT: &str = "zone-test-client-2";
const LOCK_WAIT: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(20);

type Adjustment<'a> = Box<dyn FnOnce(Claims) -> Claims + 'a>;

/// What the console keeps from starting a sign-in, plus what the identity provider reads from the
/// authorization URL.
struct Started {
    url: Url,
    state: String,
    nonce: String,
    challenge: String,
}

impl Started {
    fn parameters(&self) -> HashMap<String, String> {
        self.url.query_pairs().into_owned().collect()
    }
}

struct Session {
    user: Uuid,
    access: String,
    refresh: String,
}

struct Server {
    fixture: Fixture,
    admin: Account,
    provider: Uuid,
    identity_provider: IdentityProvider,
}

impl Server {
    /// A Team server with an admin and one enabled provider at the mock identity provider.
    async fn new() -> Self {
        Self::with(json!({})).await
    }

    /// As [`Server::new`], with `overrides` merged into the provider's configuration.
    async fn with(overrides: Value) -> Self {
        let fixture = Fixture::new(PREFIX).await;
        let admin = fixture.register().await;
        fixture.license(&admin, Tier::Team).await;
        let identity_provider = IdentityProvider::start().await;
        let mut body = identity_provider.provider();
        for (key, value) in overrides.as_object().unwrap() {
            body[key] = value.clone();
        }
        let provider = fixture.created(&admin.token, &body).await;
        Self {
            fixture,
            admin,
            provider,
            identity_provider,
        }
    }

    async fn authorize_at(&self, provider: impl std::fmt::Display) -> TestResponse {
        self.fixture
            .client
            .post_json(
                &format!("{PUBLIC_PROVIDERS}/{provider}/authorizations"),
                &json!({}),
            )
            .await
    }

    async fn start(&self) -> Started {
        self.start_at(self.provider).await
    }

    async fn start_at(&self, provider: Uuid) -> Started {
        let response = self.authorize_at(provider).await;
        response.assert_status(StatusCode::CREATED);
        let body = response.json_value();
        let url = Url::parse(body["url"].as_str().unwrap()).unwrap();
        let parameters: HashMap<String, String> = url.query_pairs().into_owned().collect();
        let state = body["state"].as_str().unwrap().to_owned();
        assert_eq!(parameters["state"], state);
        Started {
            nonce: parameters["nonce"].clone(),
            challenge: parameters["code_challenge"].clone(),
            url,
            state,
        }
    }

    /// The identity provider's valid claims for `started`, as `adjust` changes them.
    fn claims(&self, started: &Started, adjust: impl FnOnce(Claims) -> Claims) -> Claims {
        adjust(self.identity_provider.claims(&started.nonce))
    }

    /// Has the identity provider grant a fresh code for `claims`, as it would after the person
    /// signs in there, and returns the code.
    fn grant(&self, started: &Started, claims: Claims) -> String {
        let code = format!("code-{}", Uuid::new_v4().simple());
        self.identity_provider
            .grant(&code, &started.challenge, claims);
        code
    }

    /// Follows the authorization URL to the identity provider, which signs the test person in and
    /// redirects back to the console's callback with a code, and returns that code.
    async fn authenticate(&self, started: &Started) -> String {
        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
            .get(started.url.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = Url::parse(
            response.headers()[reqwest::header::LOCATION]
                .to_str()
                .unwrap(),
        )
        .unwrap();
        assert!(location.as_str().starts_with(REDIRECT_URI), "{location}");
        let parameters: HashMap<String, String> = location.query_pairs().into_owned().collect();
        assert_eq!(parameters["state"], started.state);
        parameters["code"].clone()
    }

    /// Waits until some statement in this server's database is queued behind a row or table
    /// lock, which is how a test knows a request reached the point it holds a transaction across.
    async fn queued_behind_a_lock(&self) {
        tokio::time::timeout(LOCK_WAIT, async {
            loop {
                let queued: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM pg_stat_activity \
                     WHERE datname = current_database() AND wait_event_type = 'Lock' \
                       AND pid <> pg_backend_pid())",
                )
                .fetch_one(self.fixture.pool())
                .await
                .unwrap();
                if queued {
                    return;
                }
                tokio::time::sleep(LOCK_POLL).await;
            }
        })
        .await
        .expect("the request queued behind the held lock");
    }

    async fn callback(&self, state: &str, code: &str) -> TestResponse {
        self.callback_from(state, code, None).await
    }

    async fn callback_from(&self, state: &str, code: &str, device: Option<Uuid>) -> TestResponse {
        let mut request = Request::builder()
            .method("POST")
            .uri(CALLBACKS)
            .header("Content-Type", "application/json")
            .header("x-zone-time-zone", TIME_ZONE);
        if let Some(device) = device {
            request = request
                .header("x-zone-device", device.to_string())
                .header("x-zone-device-name", "Laptop")
                .header("x-zone-device-platform", "browser");
        }
        self.fixture
            .client
            .send_request(
                request
                    .body(Body::from(
                        json!({ "state": state, "code": code }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
    }

    /// The whole flow, start to callback, for the provider's valid claims as `adjust` changes
    /// them.
    async fn sign_in(&self, adjust: impl FnOnce(Claims) -> Claims) -> TestResponse {
        self.sign_in_from(None, adjust).await
    }

    async fn sign_in_from(
        &self,
        device: Option<Uuid>,
        adjust: impl FnOnce(Claims) -> Claims,
    ) -> TestResponse {
        let started = self.start().await;
        let code = self.grant(&started, self.claims(&started, adjust));
        self.callback_from(&started.state, &code, device).await
    }

    async fn signed_in(&self, adjust: impl FnOnce(Claims) -> Claims) -> Session {
        let response = self.sign_in(adjust).await;
        response.assert_status(StatusCode::OK);
        session(&response)
    }

    async fn public_providers(&self) -> Value {
        let response = self.fixture.client.get(PUBLIC_PROVIDERS).await;
        response.assert_status(StatusCode::OK);
        response.json_value()
    }

    async fn users_with_email(&self, email: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE lower(email) = lower($1)")
            .bind(email)
            .fetch_one(self.fixture.pool())
            .await
            .unwrap()
    }

    async fn identities(&self) -> Vec<(Uuid, String, String)> {
        sqlx::query_as("SELECT user_id, subject, email FROM sso_identities ORDER BY created_at")
            .fetch_all(self.fixture.pool())
            .await
            .unwrap()
    }

    /// Whether the user's email is verified, whether they are an admin and whether they are
    /// active.
    async fn user(&self, id: Uuid) -> (bool, bool, bool) {
        sqlx::query_as(
            "SELECT email_verified, COALESCE(is_admin, false), COALESCE(is_active, true) \
             FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_one(self.fixture.pool())
        .await
        .unwrap()
    }

    async fn password_hash(&self, id: Uuid) -> String {
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(self.fixture.pool())
            .await
            .unwrap()
    }

    async fn live_sessions(&self, user: Uuid) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM sessions WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user)
        .fetch_one(self.fixture.pool())
        .await
        .unwrap()
    }

    async fn live_refresh_tokens(&self, user: Uuid) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM refresh_tokens WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user)
        .fetch_one(self.fixture.pool())
        .await
        .unwrap()
    }

    async fn verify_email(&self, user: Uuid) {
        sqlx::query("UPDATE users SET email_verified = true WHERE id = $1")
            .bind(user)
            .execute(self.fixture.pool())
            .await
            .unwrap();
    }

    async fn enforce(&self) {
        self.fixture
            .enforce(&self.admin.token, true)
            .await
            .assert_status(StatusCode::OK);
    }

    async fn cleanup(self) {
        self.fixture.cleanup().await;
    }
}

fn session(response: &TestResponse) -> Session {
    let body = response.json_value();
    Session {
        user: body["user"]["id"].as_str().unwrap().parse().unwrap(),
        access: body["access_token"].as_str().unwrap().to_owned(),
        refresh: body["refresh_token"].as_str().unwrap().to_owned(),
    }
}

fn refused(response: &TestResponse, status: StatusCode, code: &str) {
    response.assert_status(status);
    assert_eq!(
        response.json_value()["code"],
        json!(code),
        "{}",
        response.text()
    );
}

fn with_email(email: &str) -> impl FnOnce(Claims) -> Claims + '_ {
    move |claims| Claims {
        email: Some(email.to_owned()),
        ..claims
    }
}

fn unchanged(claims: Claims) -> Claims {
    claims
}

#[tokio::test]
async fn the_public_list_names_enabled_providers_the_requirement_and_the_callback_origin() {
    let server = Server::new().await;
    let disabled = server
        .fixture
        .created(
            &server.admin.token,
            &json!({
                "display_name": "Retired IdP",
                "issuer": server.identity_provider.issuer(),
                "client_id": "retired-client",
                "client_secret": "retired-secret",
                "enabled": false,
            }),
        )
        .await;

    let listed = server.public_providers().await;
    assert_eq!(
        listed,
        json!({
            "required": false,
            "callback_origin": "http://localhost:3000",
            "providers": [{ "id": server.provider, "display_name": "Test IdP" }],
        })
    );
    assert!(!listed.to_string().contains(&disabled.to_string()));

    server.enforce().await;
    assert_eq!(server.public_providers().await["required"], json!(true));
    server.cleanup().await;
}

#[tokio::test]
async fn starting_a_sign_in_sends_the_person_to_the_discovered_endpoint_with_every_parameter() {
    let server = Server::new().await;

    let started = server.start().await;

    assert_eq!(
        format!(
            "{}{}",
            started.url.origin().ascii_serialization(),
            started.url.path()
        ),
        format!("{}{AUTHORIZE_PATH}", server.identity_provider.issuer())
    );
    let parameters = started.parameters();
    assert_eq!(parameters["response_type"], "code");
    assert_eq!(
        parameters["client_id"],
        server.identity_provider.client_id()
    );
    assert_eq!(parameters["redirect_uri"], REDIRECT_URI);
    assert!(REDIRECT_URI.ends_with(CALLBACK_PATH));
    assert_eq!(parameters["scope"], "openid email profile");
    assert_eq!(parameters["code_challenge_method"], "S256");
    assert_eq!(parameters.len(), 8, "{parameters:?}");
    assert!(
        started
            .url
            .as_str()
            .contains("scope=openid%20email%20profile")
    );

    let (provider, nonce, verifier): (Uuid, String, String) = sqlx::query_as(
        "SELECT provider_id, nonce, code_verifier FROM sso_attempts WHERE state_hash = $1",
    )
    .bind(hash_token(&started.state))
    .fetch_one(server.fixture.pool())
    .await
    .unwrap();
    assert_eq!(provider, server.provider);
    assert_eq!(nonce, started.nonce);
    assert_eq!(challenge(&verifier), started.challenge);
    let stored_state: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sso_attempts WHERE state_hash = $1")
            .bind(&started.state)
            .fetch_one(server.fixture.pool())
            .await
            .unwrap();
    assert_eq!(stored_state, 0, "the state itself is never stored");
    server.cleanup().await;
}

#[tokio::test]
async fn an_unknown_or_disabled_provider_cannot_start_or_finish_a_sign_in() {
    let server = Server::new().await;
    refused(
        &server.authorize_at(Uuid::new_v4()).await,
        StatusCode::NOT_FOUND,
        PROVIDER_NOT_FOUND,
    );
    refused(
        &server.authorize_at("not-a-uuid").await,
        StatusCode::NOT_FOUND,
        PROVIDER_NOT_FOUND,
    );

    let started = server.start().await;
    let code = server.grant(&started, server.claims(&started, unchanged));
    server
        .fixture
        .update(
            &server.admin.token,
            server.provider,
            &json!({ "enabled": false }),
        )
        .await
        .assert_status(StatusCode::OK);

    refused(
        &server.callback(&started.state, &code).await,
        StatusCode::NOT_FOUND,
        PROVIDER_NOT_FOUND,
    );
    refused(
        &server.authorize_at(server.provider).await,
        StatusCode::NOT_FOUND,
        PROVIDER_NOT_FOUND,
    );
    assert_eq!(server.users_with_email(testing::EMAIL).await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn an_unreachable_provider_is_a_bad_gateway() {
    let server = Server::new().await;
    sqlx::query("UPDATE sso_providers SET issuer = $1 WHERE id = $2")
        .bind(UNREACHABLE_ISSUER)
        .bind(server.provider)
        .execute(server.fixture.pool())
        .await
        .unwrap();

    refused(
        &server.authorize_at(server.provider).await,
        StatusCode::BAD_GATEWAY,
        DISCOVERY_FAILED,
    );
    server.cleanup().await;
}

#[tokio::test]
async fn a_first_sign_in_provisions_a_verified_member_with_an_organization_of_their_own() {
    let server = Server::new().await;

    let session = server.signed_in(unchanged).await;

    assert_eq!(server.user(session.user).await, (true, false, true));
    let organizations = server
        .fixture
        .client
        .get_auth("/api/organizations", &session.access)
        .await;
    organizations.assert_status(StatusCode::OK);
    assert_eq!(
        organizations.json_value()["organizations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        server.identities().await,
        [(
            session.user,
            testing::SUBJECT.to_owned(),
            testing::EMAIL.to_owned()
        )]
    );
    let time_zone: Option<String> = sqlx::query_scalar("SELECT time_zone FROM users WHERE id = $1")
        .bind(session.user)
        .fetch_one(server.fixture.pool())
        .await
        .unwrap();
    assert_eq!(time_zone.as_deref(), Some(TIME_ZONE));
    server.cleanup().await;
}

#[tokio::test]
async fn the_response_is_the_password_sign_in_response() {
    let server = Server::new().await;
    let response = server.sign_in(unchanged).await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    let password = server
        .fixture
        .login(&server.admin.email, &test_password())
        .await;
    password.assert_status(StatusCode::OK);
    let password = password.json_value();

    let keys = |value: &Value| {
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(keys(&body), keys(&password));
    assert_eq!(keys(&body["user"]), keys(&password["user"]));
    assert_eq!(body["token_type"], json!("Bearer"));
    assert_eq!(body["user"]["email"], json!(testing::EMAIL));
    assert_eq!(body["user"]["display_name"], json!(testing::NAME));
    assert_eq!(body["user"]["email_verified"], json!(true));
    assert_eq!(body["user"]["is_admin"], json!(false));
    server.cleanup().await;
}

#[tokio::test]
async fn a_later_sign_in_matches_the_subject_even_after_the_email_changes() {
    let server = Server::new().await;
    let first = server.signed_in(unchanged).await;

    let second = server.signed_in(with_email("Jane.Doe@acme.com")).await;

    assert_eq!(second.user, first.user);
    assert_eq!(
        server.identities().await,
        [(
            first.user,
            testing::SUBJECT.to_owned(),
            "jane.doe@acme.com".to_owned()
        )]
    );
    assert_eq!(server.users_with_email("jane.doe@acme.com").await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn a_verified_password_account_is_linked_by_email_ignoring_case_and_keeps_its_password() {
    let server = Server::new().await;
    let local = server.fixture.register().await;
    server.verify_email(local.id).await;
    let hash = server.password_hash(local.id).await;
    let upper = local.email.to_uppercase();

    let session = server.signed_in(with_email(&upper)).await;

    assert_eq!(session.user, local.id);
    assert_eq!(server.users_with_email(&local.email).await, 1);
    assert_eq!(server.password_hash(local.id).await, hash);
    assert!(server.live_sessions(local.id).await >= 2);
    server
        .fixture
        .login(&local.email, &test_password())
        .await
        .assert_status(StatusCode::OK);
    let audits = server.fixture.audits(actions::USER_SSO_SIGNED_IN).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(
        audits[0].new_values,
        Some(json!({
            "provider_id": server.provider,
            "subject": testing::SUBJECT,
            "provisioned": false,
            "linked": true,
            "reclaimed": false,
        }))
    );
    server.cleanup().await;
}

#[tokio::test]
async fn an_unverified_squatter_loses_the_account_to_the_person_who_proves_the_email() {
    let server = Server::new().await;
    let registered = server.fixture.register_as(testing::EMAIL).await;
    registered.assert_status(StatusCode::CREATED);
    let squatter = session(&registered);
    server
        .fixture
        .client
        .get_auth("/api/organizations", &squatter.access)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(server.user(squatter.user).await, (false, false, true));

    let owner = server.signed_in(unchanged).await;

    assert_eq!(owner.user, squatter.user);
    assert_eq!(server.user(owner.user).await, (true, false, true));
    server
        .fixture
        .login(testing::EMAIL, &test_password())
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    server
        .fixture
        .client
        .post_json(
            "/api/auth/refresh",
            &json!({ "refresh_token": squatter.refresh }),
        )
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    server
        .fixture
        .client
        .get_auth("/api/organizations", &squatter.access)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    assert_eq!(server.live_sessions(owner.user).await, 1);
    server
        .fixture
        .client
        .get_auth("/api/organizations", &owner.access)
        .await
        .assert_status(StatusCode::OK);
    let audits = server.fixture.audits(actions::USER_SSO_SIGNED_IN).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(
        audits[0].new_values,
        Some(json!({
            "provider_id": server.provider,
            "subject": testing::SUBJECT,
            "provisioned": false,
            "linked": true,
            "reclaimed": true,
        }))
    );
    let linked = server.fixture.audits(actions::USER_SSO_LINKED).await;
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].actor_id, Some(owner.user));
    assert_eq!(linked[0].resource_id, Some(owner.user));
    assert_eq!(
        linked[0].new_values,
        Some(json!({
            "provider_id": server.provider,
            "subject": testing::SUBJECT,
            "reclaimed": true,
        }))
    );
    server.cleanup().await;
}

#[tokio::test]
async fn a_reclaim_is_audited_even_when_the_device_waits_for_approval() {
    let server = Server::new().await;
    let registered = server.fixture.register_as(testing::EMAIL).await;
    registered.assert_status(StatusCode::CREATED);
    let squatter = session(&registered);
    devices::set_policy(server.fixture.pool(), Mode::Allowed)
        .await
        .unwrap();

    refused(
        &server.sign_in_from(Some(Uuid::new_v4()), unchanged).await,
        StatusCode::FORBIDDEN,
        DEVICE_PENDING,
    );

    assert_eq!(server.user(squatter.user).await, (true, false, true));
    assert_eq!(server.live_sessions(squatter.user).await, 0);
    let linked = server.fixture.audits(actions::USER_SSO_LINKED).await;
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].actor_id, Some(squatter.user));
    assert_eq!(linked[0].organization_id, None);
    assert_eq!(linked[0].resource_type, resources::USER);
    assert_eq!(linked[0].resource_id, Some(squatter.user));
    assert_eq!(
        linked[0].new_values,
        Some(json!({
            "provider_id": server.provider,
            "subject": testing::SUBJECT,
            "reclaimed": true,
        }))
    );
    assert!(
        server
            .fixture
            .audits(actions::USER_SSO_SIGNED_IN)
            .await
            .is_empty()
    );
    server.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_password_sign_in_racing_a_reclaim_leaves_no_live_session() {
    let server = Server::new().await;
    let registered = server.fixture.register_as(testing::EMAIL).await;
    registered.assert_status(StatusCode::CREATED);
    let squatter = session(&registered);
    let replacement = hash_password("Unknown-horse-9").unwrap();
    let mut reclaim = server.fixture.pool().begin().await.unwrap();
    assert!(
        users::reclaim(&mut reclaim, squatter.user, &replacement)
            .await
            .unwrap()
    );
    refresh_tokens::revoke_all_user_tokens(&mut *reclaim, squatter.user)
        .await
        .unwrap();
    sessions::revoke_all_user_sessions(&mut *reclaim, squatter.user)
        .await
        .unwrap();

    let password = test_password();
    let (login, ()) = tokio::join!(server.fixture.login(testing::EMAIL, &password), async {
        server.queued_behind_a_lock().await;
        reclaim.commit().await.unwrap();
    });

    login.assert_status(StatusCode::UNAUTHORIZED);
    assert_eq!(server.live_sessions(squatter.user).await, 0);
    assert_eq!(server.live_refresh_tokens(squatter.user).await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn an_unverified_admin_is_linked_but_never_reclaimed() {
    let server = Server::new().await;
    assert_eq!(server.user(server.admin.id).await, (false, true, true));
    let hash = server.password_hash(server.admin.id).await;
    server.enforce().await;

    let session = server.signed_in(with_email(&server.admin.email)).await;

    assert_eq!(session.user, server.admin.id);
    assert_eq!(server.password_hash(server.admin.id).await, hash);
    assert_eq!(server.user(server.admin.id).await, (false, true, true));
    server
        .fixture
        .settings(&server.admin.token)
        .await
        .assert_status(StatusCode::OK);
    assert!(server.live_sessions(server.admin.id).await >= 2);
    server
        .fixture
        .login(&server.admin.email, &test_password())
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        server
            .fixture
            .audits(actions::USER_BREAK_GLASS_SIGNED_IN)
            .await
            .len(),
        1
    );
    let audits = server.fixture.audits(actions::USER_SSO_SIGNED_IN).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(
        audits[0].new_values,
        Some(json!({
            "provider_id": server.provider,
            "subject": testing::SUBJECT,
            "provisioned": false,
            "linked": true,
            "reclaimed": false,
        }))
    );
    server.cleanup().await;
}

#[tokio::test]
async fn trusting_unverified_emails_never_claims_an_existing_account() {
    let server = Server::with(json!({ "require_verified_email": false })).await;
    let member = server.fixture.register().await;
    let verified = server.fixture.register().await;
    server.verify_email(verified.id).await;
    let accounts = [&server.admin, &member, &verified];
    let mut hashes = Vec::new();
    for account in accounts {
        hashes.push(server.password_hash(account.id).await);
    }
    let mut sessions = Vec::new();
    for account in accounts {
        sessions.push(server.live_sessions(account.id).await);
    }

    for (index, account) in accounts.iter().enumerate() {
        let conflicted = server
            .sign_in(|claims| Claims {
                subject: format!("lookalike-{index}"),
                email: Some(account.email.to_uppercase()),
                email_verified: Some(false),
                ..claims
            })
            .await;
        refused(&conflicted, StatusCode::CONFLICT, EMAIL_CONFLICT);
    }

    for ((account, hash), live) in accounts.iter().zip(&hashes).zip(&sessions) {
        assert_eq!(&server.password_hash(account.id).await, hash);
        assert_eq!(&server.live_sessions(account.id).await, live);
    }
    assert!(server.identities().await.is_empty());
    server.cleanup().await;
}

#[tokio::test]
async fn an_account_linked_through_one_provider_cannot_be_claimed_by_email_through_another() {
    let server = Server::new().await;
    let first = server.signed_in(unchanged).await;
    let other = IdentityProvider::start().await;
    let second = server
        .fixture
        .created(&server.admin.token, &other.provider())
        .await;

    let started = server.start_at(second).await;
    let code = format!("code-{}", Uuid::new_v4().simple());
    other.grant(
        &code,
        &started.challenge,
        Claims {
            subject: "someone-else-at-the-second-provider".into(),
            ..other.claims(&started.nonce)
        },
    );
    let response = server.callback(&started.state, &code).await;

    refused(&response, StatusCode::CONFLICT, EMAIL_CONFLICT);
    assert_eq!(
        server.identities().await,
        [(
            first.user,
            testing::SUBJECT.to_owned(),
            testing::EMAIL.to_owned()
        )]
    );
    assert_eq!(server.users_with_email(testing::EMAIL).await, 1);
    server.cleanup().await;
}

#[tokio::test]
async fn a_single_sign_on_session_refreshes_while_sso_is_required() {
    let server = Server::new().await;
    let session = server.signed_in(unchanged).await;
    server.enforce().await;

    server
        .fixture
        .client
        .post_json(
            "/api/auth/refresh",
            &json!({ "refresh_token": session.refresh }),
        )
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn disabling_or_deleting_a_provider_ends_the_sessions_signed_in_through_it() {
    let server = Server::new().await;
    let other = IdentityProvider::start().await;
    let second = server
        .fixture
        .created(&server.admin.token, &other.provider())
        .await;
    let disabled = server.signed_in(unchanged).await;
    let deleted = {
        let started = server.start_at(second).await;
        let code = format!("code-{}", Uuid::new_v4().simple());
        other.grant(
            &code,
            &started.challenge,
            Claims {
                subject: "second-person".into(),
                email: Some(test_email()),
                ..other.claims(&started.nonce)
            },
        );
        let response = server.callback(&started.state, &code).await;
        response.assert_status(StatusCode::OK);
        session(&response)
    };

    server
        .fixture
        .update(
            &server.admin.token,
            server.provider,
            &json!({ "enabled": false }),
        )
        .await
        .assert_status(StatusCode::OK);
    server
        .fixture
        .delete(&server.admin.token, second)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    for ended in [&disabled, &deleted] {
        server
            .fixture
            .client
            .get_auth("/api/auth/sessions", &ended.access)
            .await
            .assert_status(StatusCode::UNAUTHORIZED);
        server
            .fixture
            .client
            .post_json(
                "/api/auth/refresh",
                &json!({ "refresh_token": ended.refresh }),
            )
            .await
            .assert_status(StatusCode::UNAUTHORIZED);
    }
    server
        .fixture
        .settings(&server.admin.token)
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn trusting_unverified_emails_provisions_a_verified_account() {
    let server = Server::with(json!({ "require_verified_email": false })).await;
    let session = server
        .signed_in(|claims| Claims {
            email_verified: Some(false),
            ..claims
        })
        .await;
    assert_eq!(server.user(session.user).await, (true, false, true));
    server.cleanup().await;
}

#[tokio::test]
async fn an_account_linked_to_another_subject_is_a_conflict() {
    let server = Server::new().await;
    let first = server.signed_in(unchanged).await;

    let other = server
        .sign_in(|claims| Claims {
            subject: "another-subject".into(),
            ..claims
        })
        .await;

    refused(&other, StatusCode::CONFLICT, EMAIL_CONFLICT);
    let identities = server.identities().await;
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].0, first.user);
    server.cleanup().await;
}

#[tokio::test]
async fn the_policy_refuses_what_the_provider_does_not_vouch_for() {
    let server = Server::with(json!({ "allowed_domains": ["acme.com"] })).await;
    server
        .identity_provider
        .serve_userinfo(Some(json!({ "sub": testing::SUBJECT })));
    let cases: [(&str, Adjustment<'_>, &str); 4] = [
        (
            "unverified",
            Box::new(|claims| Claims {
                email_verified: Some(false),
                ..claims
            }),
            EMAIL_UNVERIFIED,
        ),
        (
            "another domain",
            Box::new(with_email("jane@globex.com")),
            DOMAIN_NOT_ALLOWED,
        ),
        (
            "a hosted domain outside the allowed ones",
            Box::new(|claims| Claims {
                hosted_domain: Some("globex.com".into()),
                ..claims
            }),
            DOMAIN_NOT_ALLOWED,
        ),
        (
            "no email, here or at userinfo",
            Box::new(|claims| Claims {
                email: None,
                ..claims
            }),
            EMAIL_MISSING,
        ),
    ];

    for (name, adjust, code) in cases {
        let response = server.sign_in(adjust).await;
        response.assert_status(StatusCode::FORBIDDEN);
        assert_eq!(response.json_value()["code"], json!(code), "{name}");
    }
    assert_eq!(server.users_with_email(testing::EMAIL).await, 0);
    assert!(server.identities().await.is_empty());

    server
        .signed_in(|claims| Claims {
            hosted_domain: Some("acme.com".into()),
            ..claims
        })
        .await;
    server.cleanup().await;
}

#[tokio::test]
async fn an_id_token_that_does_not_verify_is_refused() {
    let server = Server::new().await;
    let now = Utc::now();
    let cases: [(&str, Adjustment<'_>); 3] = [
        (
            "another audience",
            Box::new(|claims| Claims {
                audience: Audience::One("another-client".into()),
                ..claims
            }),
        ),
        (
            "another nonce",
            Box::new(|claims| Claims {
                nonce: Some("replayed-nonce".into()),
                ..claims
            }),
        ),
        (
            "expired",
            Box::new(move |claims| Claims {
                issued_at: (now - TimeDelta::hours(2)).timestamp(),
                expires_at: (now - TimeDelta::hours(1)).timestamp(),
                ..claims
            }),
        ),
    ];
    for (name, adjust) in cases {
        let response = server.sign_in(adjust).await;
        response.assert_status(StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.json_value()["code"],
            json!(ID_TOKEN_INVALID),
            "{name}"
        );
    }

    server.identity_provider.forge_signature(true);
    refused(
        &server.sign_in(unchanged).await,
        StatusCode::UNAUTHORIZED,
        ID_TOKEN_INVALID,
    );
    assert_eq!(server.users_with_email(testing::EMAIL).await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn a_failing_token_endpoint_is_a_bad_gateway() {
    let server = Server::new().await;
    server.identity_provider.fail_token_endpoint(true);
    refused(
        &server.sign_in(unchanged).await,
        StatusCode::BAD_GATEWAY,
        TOKEN_EXCHANGE_FAILED,
    );
    assert!(
        server
            .fixture
            .audits(actions::USER_SSO_SIGNED_IN)
            .await
            .is_empty()
    );
    assert!(
        server
            .fixture
            .audits(actions::USER_CREATED)
            .await
            .is_empty()
    );
    server.cleanup().await;
}

#[tokio::test]
async fn a_state_is_redeemed_once_and_only_before_it_expires() {
    let server = Server::new().await;
    let started = server.start().await;
    let code = server.grant(&started, server.claims(&started, unchanged));
    server
        .callback(&started.state, &code)
        .await
        .assert_status(StatusCode::OK);

    let again = server.grant(&started, server.claims(&started, unchanged));
    refused(
        &server.callback(&started.state, &again).await,
        StatusCode::BAD_REQUEST,
        STATE_INVALID,
    );

    let expiring = server.start().await;
    let code = server.grant(&expiring, server.claims(&expiring, unchanged));
    sqlx::query("UPDATE sso_attempts SET expires_at = now() - interval '1 second'")
        .execute(server.fixture.pool())
        .await
        .unwrap();
    refused(
        &server.callback(&expiring.state, &code).await,
        StatusCode::BAD_REQUEST,
        STATE_INVALID,
    );
    refused(
        &server.callback("never-issued", "code").await,
        StatusCode::BAD_REQUEST,
        STATE_INVALID,
    );
    server.cleanup().await;
}

#[tokio::test]
async fn a_disabled_account_cannot_sign_in() {
    let server = Server::new().await;
    let session = server.signed_in(unchanged).await;
    sqlx::query("UPDATE users SET is_active = false WHERE id = $1")
        .bind(session.user)
        .execute(server.fixture.pool())
        .await
        .unwrap();

    refused(
        &server.sign_in(unchanged).await,
        StatusCode::FORBIDDEN,
        ACCOUNT_DISABLED,
    );
    server.cleanup().await;
}

#[tokio::test]
async fn a_blocked_device_cannot_sign_in() {
    let server = Server::new().await;
    let device = Uuid::new_v4();
    server
        .sign_in_from(Some(device), unchanged)
        .await
        .assert_status(StatusCode::OK);
    sqlx::query("UPDATE devices SET status = 'blocked' WHERE public_id = $1")
        .bind(device)
        .execute(server.fixture.pool())
        .await
        .unwrap();

    refused(
        &server.sign_in_from(Some(device), unchanged).await,
        StatusCode::FORBIDDEN,
        DEVICE_BLOCKED,
    );
    server.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_first_sign_in_that_loses_the_race_signs_in_to_the_account_the_winner_created() {
    let server = Server::new().await;
    let started = server.start().await;
    let code = server.grant(&started, server.claims(&started, unchanged));
    let mut winner = server.fixture.pool().begin().await.unwrap();
    let created = provision::insert(
        &mut winner,
        testing::EMAIL,
        &hash_password("Unknown-horse-9").unwrap(),
        Some(testing::NAME),
        false,
        true,
    )
    .await
    .unwrap();
    identities::link(
        &mut *winner,
        server.provider,
        created.id,
        testing::SUBJECT,
        testing::EMAIL,
    )
    .await
    .unwrap();

    let (response, ()) = tokio::join!(server.callback(&started.state, &code), async {
        server.queued_behind_a_lock().await;
        winner.commit().await.unwrap();
    });

    response.assert_status(StatusCode::OK);
    assert_eq!(session(&response).user, created.id);
    assert_eq!(server.users_with_email(testing::EMAIL).await, 1);
    assert_eq!(
        server.identities().await,
        [(
            created.id,
            testing::SUBJECT.to_owned(),
            testing::EMAIL.to_owned()
        )]
    );
    assert!(
        server
            .fixture
            .audits(actions::USER_CREATED)
            .await
            .is_empty()
    );
    server.cleanup().await;
}

#[tokio::test]
async fn a_new_client_id_unlinks_everyone_and_the_next_sign_in_relinks_by_verified_email() {
    let server = Server::new().await;
    let first = server.signed_in(unchanged).await;
    server.identity_provider.register(OTHER_CLIENT);

    server
        .fixture
        .update(
            &server.admin.token,
            server.provider,
            &json!({ "client_id": OTHER_CLIENT }),
        )
        .await
        .assert_status(StatusCode::OK);

    assert!(server.identities().await.is_empty());
    let updated = server.fixture.audits(actions::SSO_PROVIDER_UPDATED).await;
    assert_eq!(updated.len(), 1);
    assert_eq!(
        updated[0].new_values,
        Some(json!({ "client_id": OTHER_CLIENT, "identities_cleared": 1 }))
    );

    let started = server.start().await;
    assert_eq!(started.parameters()["client_id"], OTHER_CLIENT);
    let code = server.authenticate(&started).await;
    let response = server.callback(&started.state, &code).await;

    response.assert_status(StatusCode::OK);
    assert_eq!(session(&response).user, first.user);
    assert_eq!(
        server.identities().await,
        [(
            first.user,
            server.identity_provider.subject(OTHER_CLIENT),
            testing::EMAIL.to_owned()
        )]
    );
    server.cleanup().await;
}

#[tokio::test]
async fn editing_anything_but_the_issuer_or_client_id_keeps_the_links() {
    let server = Server::new().await;
    let first = server.signed_in(unchanged).await;

    server
        .fixture
        .update(
            &server.admin.token,
            server.provider,
            &json!({ "display_name": "Okta SSO", "client_secret": "rotated" }),
        )
        .await
        .assert_status(StatusCode::OK);

    assert_eq!(server.identities().await.len(), 1);
    let updated = server.fixture.audits(actions::SSO_PROVIDER_UPDATED).await;
    assert_eq!(
        updated[0].new_values,
        Some(json!({ "display_name": "Okta SSO", "client_secret_changed": true }))
    );
    assert_eq!(server.identities().await[0].0, first.user);
    server.cleanup().await;
}

#[tokio::test]
async fn sign_in_and_enforcement_outlive_the_license() {
    let server = Server::new().await;
    let member = server.fixture.register().await;
    server.enforce().await;

    server.fixture.store_license(&lapsed(Tier::Team)).await;
    server.signed_in(unchanged).await;
    refused(
        &server.fixture.login(&member.email, &test_password()).await,
        StatusCode::FORBIDDEN,
        SSO_REQUIRED,
    );

    server.fixture.remove_license(&server.admin).await;
    let listed = server.public_providers().await;
    assert_eq!(listed["required"], json!(true));
    assert_eq!(listed["providers"].as_array().unwrap().len(), 1);
    server
        .signed_in(|claims| Claims {
            subject: "second-person".into(),
            email: Some(test_email()),
            ..claims
        })
        .await;
    refused(
        &server.fixture.login(&member.email, &test_password()).await,
        StatusCode::FORBIDDEN,
        SSO_REQUIRED,
    );
    server.cleanup().await;
}

#[tokio::test]
async fn a_signed_in_person_takes_a_seat() {
    let server = Server::new().await;
    let since = period_start(Utc::now());
    let before = seats::used(server.fixture.pool(), since).await.unwrap();

    server.signed_in(unchanged).await;

    assert_eq!(
        seats::used(server.fixture.pool(), since).await.unwrap(),
        before + 1
    );
    server.cleanup().await;
}

#[tokio::test]
async fn provisioning_and_signing_in_are_audited() {
    let server = Server::new().await;

    let session = server.signed_in(unchanged).await;
    server.signed_in(unchanged).await;

    let created = server.fixture.audits(actions::USER_CREATED).await;
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].actor_id, Some(session.user));
    assert_eq!(created[0].organization_id, None);
    assert_eq!(created[0].resource_type, resources::USER);
    assert_eq!(created[0].resource_id, Some(session.user));
    assert_eq!(
        created[0].new_values,
        Some(json!({ "provider_id": server.provider }))
    );

    let signed_in = server.fixture.audits(actions::USER_SSO_SIGNED_IN).await;
    assert_eq!(signed_in.len(), 2);
    for (audit, provisioned) in signed_in.iter().zip([true, false]) {
        assert_eq!(audit.actor_id, Some(session.user));
        assert_eq!(audit.organization_id, None);
        assert_eq!(audit.resource_type, resources::USER);
        assert_eq!(audit.resource_id, Some(session.user));
        assert_eq!(audit.old_values, None);
        assert_eq!(
            audit.new_values,
            Some(json!({
                "provider_id": server.provider,
                "subject": testing::SUBJECT,
                "provisioned": provisioned,
                "linked": false,
                "reclaimed": false,
            }))
        );
    }
    assert!(
        !server
            .fixture
            .audit_text()
            .await
            .contains(server.identity_provider.client_secret())
    );
    server.cleanup().await;
}
