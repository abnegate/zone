//! An OpenID Connect identity provider served in-process on loopback, for tests that configure a
//! provider and sign in through it.
//!
//! It serves discovery, the signing keys, the token endpoint and userinfo over plain http on
//! `127.0.0.1`, which Zone accepts because the host is loopback. The token endpoint checks what a
//! real provider checks (the grant type, the client's credentials, the redirect URI and the PKCE
//! verifier) against a code recorded beforehand with [`IdentityProvider::grant`], and answers
//! with an ID token signed by the key in `zone_enterprise::sso::testing`.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::Utc;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use uuid::Uuid;
use zone_enterprise::sso::{Claims, challenge, testing};

pub const CLIENT_ID: &str = "zone-test-client";
/// Carries the characters RFC 6749 makes clients form-encode inside HTTP basic credentials.
pub const CLIENT_SECRET: &str = "s3cret:with+plus %percent";
/// The console callback under `test_config()`'s `app_base_url`.
pub const REDIRECT_URI: &str = "http://localhost:3000/sso/callback";
pub const DISPLAY_NAME: &str = "Test IdP";
pub const MISMATCHED_ISSUER: &str = "https://impostor.example.test";

pub const DISCOVERY_PATH: &str = "/.well-known/openid-configuration";
pub const KEYS_PATH: &str = "/jwks";
pub const TOKEN_PATH: &str = "/token";
pub const USERINFO_PATH: &str = "/userinfo";
pub const AUTHORIZE_PATH: &str = "/authorize";

const GRANT_TYPE: &str = "authorization_code";
const BASIC: &str = "Basic ";
const BEARER: &str = "Bearer ";
const CLIENT_SECRET_BASIC: &str = "client_secret_basic";
const CLIENT_SECRET_POST: &str = "client_secret_post";
const INVALID_CLIENT: &str = "invalid_client";
const INVALID_GRANT: &str = "invalid_grant";
const INVALID_REQUEST: &str = "invalid_request";
const INVALID_TOKEN: &str = "invalid_token";
const SERVER_ERROR: &str = "server_error";
const UNSUPPORTED_GRANT_TYPE: &str = "unsupported_grant_type";

/// How the provider wants the client to authenticate at its token endpoint. Discovery advertises
/// only this method, and the token endpoint refuses the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authentication {
    Basic,
    Post,
}

impl Authentication {
    fn method(self) -> &'static str {
        match self {
            Self::Basic => CLIENT_SECRET_BASIC,
            Self::Post => CLIENT_SECRET_POST,
        }
    }
}

struct Grant {
    challenge: String,
    claims: Claims,
}

struct Inner {
    issuer: String,
    authentication: Authentication,
    grants: Mutex<HashMap<String, Grant>>,
    access_tokens: Mutex<HashSet<String>>,
    requests: Mutex<Vec<String>>,
    userinfo: Mutex<Option<Value>>,
    mismatched_issuer: AtomicBool,
    token_failure: AtomicBool,
}

impl Inner {
    fn record(&self, path: &str) {
        lock(&self.requests).push(path.to_owned());
    }
}

pub struct IdentityProvider {
    inner: Arc<Inner>,
    server: JoinHandle<()>,
}

impl IdentityProvider {
    pub async fn start() -> Self {
        Self::start_with(Authentication::Basic).await
    }

    pub async fn start_with(authentication: Authentication) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral loopback port");
        let address: SocketAddr = listener.local_addr().expect("the bound address");
        let inner = Arc::new(Inner {
            issuer: format!("http://{address}"),
            authentication,
            grants: Mutex::default(),
            access_tokens: Mutex::default(),
            requests: Mutex::default(),
            userinfo: Mutex::default(),
            mismatched_issuer: AtomicBool::new(false),
            token_failure: AtomicBool::new(false),
        });
        let router = Router::new()
            .route(DISCOVERY_PATH, get(discovery))
            .route(KEYS_PATH, get(keys))
            .route(TOKEN_PATH, post(token))
            .route(USERINFO_PATH, get(userinfo))
            .with_state(Arc::clone(&inner));
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Self { inner, server }
    }

    /// The issuer URL to configure a provider with, such as `http://127.0.0.1:49152`.
    pub fn issuer(&self) -> String {
        self.inner.issuer.clone()
    }

    pub fn client_id(&self) -> &'static str {
        CLIENT_ID
    }

    pub fn client_secret(&self) -> &'static str {
        CLIENT_SECRET
    }

    /// The body of `POST /api/sso/providers` that configures Zone to sign in through this
    /// provider.
    pub fn provider(&self) -> Value {
        json!({
            "display_name": DISPLAY_NAME,
            "issuer": self.issuer(),
            "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET,
        })
    }

    /// Valid ID token claims for this provider and client: the test subject, a verified email
    /// and a name, issued now and carrying `nonce`.
    pub fn claims(&self, nonce: &str) -> Claims {
        testing::claims(&self.inner.issuer, CLIENT_ID, nonce, Utc::now())
    }

    /// Records an authorization code as the provider would after the person signs in: the token
    /// endpoint redeems `code` once, for the PKCE verifier whose S256 challenge is `challenge`,
    /// and answers with an ID token carrying `claims`.
    pub fn grant(&self, code: &str, challenge: &str, claims: Claims) {
        lock(&self.inner.grants).insert(
            code.to_owned(),
            Grant {
                challenge: challenge.to_owned(),
                claims,
            },
        );
    }

    /// Makes discovery name [`MISMATCHED_ISSUER`] instead of this provider's own issuer.
    pub fn mismatch_issuer(&self, mismatched: bool) {
        self.inner
            .mismatched_issuer
            .store(mismatched, Ordering::SeqCst);
    }

    /// Makes the token endpoint answer every request with HTTP 500.
    pub fn fail_token_endpoint(&self, failing: bool) {
        self.inner.token_failure.store(failing, Ordering::SeqCst);
    }

    /// What userinfo answers for an access token the token endpoint issued. With `None`, the
    /// endpoint answers 404.
    pub fn serve_userinfo(&self, claims: Option<Value>) {
        *lock(&self.inner.userinfo) = claims;
    }

    /// How many requests reached `path`.
    pub fn requests(&self, path: &str) -> usize {
        lock(&self.inner.requests)
            .iter()
            .filter(|requested| requested.as_str() == path)
            .count()
    }
}

impl Drop for IdentityProvider {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn refuse(status: StatusCode, error: &str) -> Response {
    (status, Json(json!({ "error": error }))).into_response()
}

async fn discovery(State(inner): State<Arc<Inner>>) -> Json<Value> {
    inner.record(DISCOVERY_PATH);
    let mut document = testing::discovery(&inner.issuer);
    document["authorization_endpoint"] = json!(format!("{}{AUTHORIZE_PATH}", inner.issuer));
    document["userinfo_endpoint"] = json!(format!("{}{USERINFO_PATH}", inner.issuer));
    document["token_endpoint_auth_methods_supported"] = json!([inner.authentication.method()]);
    if inner.mismatched_issuer.load(Ordering::SeqCst) {
        document["issuer"] = json!(MISMATCHED_ISSUER);
    }
    Json(document)
}

async fn keys(State(inner): State<Arc<Inner>>) -> Json<Value> {
    inner.record(KEYS_PATH);
    Json(testing::key_set())
}

async fn token(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    inner.record(TOKEN_PATH);
    if inner.token_failure.load(Ordering::SeqCst) {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR);
    }
    if form.get("grant_type").map(String::as_str) != Some(GRANT_TYPE) {
        return refuse(StatusCode::BAD_REQUEST, UNSUPPORTED_GRANT_TYPE);
    }
    if !authenticated(inner.authentication, &headers, &form) {
        return refuse(StatusCode::UNAUTHORIZED, INVALID_CLIENT);
    }
    let (Some(code), Some(redirect_uri), Some(verifier)) = (
        form.get("code"),
        form.get("redirect_uri"),
        form.get("code_verifier"),
    ) else {
        return refuse(StatusCode::BAD_REQUEST, INVALID_REQUEST);
    };
    let Some(grant) = lock(&inner.grants).remove(code) else {
        return refuse(StatusCode::BAD_REQUEST, INVALID_GRANT);
    };
    if redirect_uri != REDIRECT_URI || challenge(verifier) != grant.challenge {
        return refuse(StatusCode::BAD_REQUEST, INVALID_GRANT);
    }
    let access_token = format!("access-{}", Uuid::new_v4().simple());
    lock(&inner.access_tokens).insert(access_token.clone());
    Json(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": 300,
        "id_token": testing::id_token(&grant.claims),
    }))
    .into_response()
}

fn authenticated(
    authentication: Authentication,
    headers: &HeaderMap,
    form: &HashMap<String, String>,
) -> bool {
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    match authentication {
        Authentication::Basic => {
            !form.contains_key("client_secret")
                && header
                    .and_then(|value| value.strip_prefix(BASIC))
                    .and_then(basic_credentials)
                    .is_some_and(|(id, secret)| id == CLIENT_ID && secret == CLIENT_SECRET)
        }
        Authentication::Post => {
            header.is_none()
                && form.get("client_id").map(String::as_str) == Some(CLIENT_ID)
                && form.get("client_secret").map(String::as_str) == Some(CLIENT_SECRET)
        }
    }
}

/// RFC 6749 section 2.3.1: each half of the basic credentials is form-urlencoded before the pair
/// is base64-encoded.
fn basic_credentials(encoded: &str) -> Option<(String, String)> {
    let decoded = String::from_utf8(STANDARD.decode(encoded).ok()?).ok()?;
    let (id, secret) = decoded.split_once(':')?;
    Some((form_decode(id)?, form_decode(secret)?))
}

fn form_decode(value: &str) -> Option<String> {
    urlencoding::decode(&value.replace('+', " "))
        .ok()
        .map(|decoded| decoded.into_owned())
}

async fn userinfo(State(inner): State<Arc<Inner>>, headers: HeaderMap) -> Response {
    inner.record(USERINFO_PATH);
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix(BEARER));
    if !bearer.is_some_and(|token| lock(&inner.access_tokens).contains(token)) {
        return refuse(StatusCode::UNAUTHORIZED, INVALID_TOKEN);
    }
    match lock(&inner.userinfo).clone() {
        Some(claims) => Json(claims).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use zone_enterprise::sso::{
        Error, Fields, Grant as Redemption, Identity, Oidc, Pkce, Protocol, Provider,
    };

    use super::*;

    const CODE: &str = "code-1";
    const NONCE: &str = "nonce-1";

    fn provider(identity_provider: &IdentityProvider) -> Provider {
        Provider::parse(&Fields {
            protocol: Protocol::Oidc,
            display_name: DISPLAY_NAME,
            issuer: &identity_provider.issuer(),
            client_id: CLIENT_ID,
            client_secret: CLIENT_SECRET,
            allowed_domains: &[],
            require_verified_email: true,
        })
        .unwrap()
    }

    async fn redeem(
        identity_provider: &IdentityProvider,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<Identity, Error> {
        Oidc::new()
            .exchange(
                &provider(identity_provider),
                &Redemption {
                    code: CODE,
                    verifier,
                    nonce: NONCE,
                    redirect_uri,
                },
            )
            .await
    }

    fn expected() -> Identity {
        Identity {
            subject: testing::SUBJECT.into(),
            email: testing::EMAIL.into(),
            name: Some(testing::NAME.into()),
        }
    }

    #[tokio::test]
    async fn a_granted_code_is_redeemed_once_with_either_client_authentication() {
        for authentication in [Authentication::Basic, Authentication::Post] {
            let identity_provider = IdentityProvider::start_with(authentication).await;
            let pkce = Pkce::generate();
            identity_provider.grant(CODE, &pkce.challenge, identity_provider.claims(NONCE));

            assert_eq!(
                redeem(&identity_provider, &pkce.verifier, REDIRECT_URI).await,
                Ok(expected()),
                "{authentication:?}"
            );
            assert!(matches!(
                redeem(&identity_provider, &pkce.verifier, REDIRECT_URI).await,
                Err(Error::TokenExchange(_))
            ));
            assert_eq!(identity_provider.requests(TOKEN_PATH), 2);
        }
    }

    #[tokio::test]
    async fn the_token_endpoint_checks_the_verifier_and_redirect_uri() {
        let identity_provider = IdentityProvider::start().await;
        let pkce = Pkce::generate();
        for (verifier, redirect_uri) in [
            ("another-verifier", REDIRECT_URI),
            (pkce.verifier.as_str(), "http://localhost:3000/elsewhere"),
        ] {
            identity_provider.grant(CODE, &pkce.challenge, identity_provider.claims(NONCE));
            let Err(Error::TokenExchange(message)) =
                redeem(&identity_provider, verifier, redirect_uri).await
            else {
                panic!("{verifier} at {redirect_uri} was redeemed");
            };
            assert!(message.contains(INVALID_GRANT), "{message}");
        }
    }

    #[tokio::test]
    async fn the_switches_break_discovery_and_the_token_endpoint() {
        let identity_provider = IdentityProvider::start().await;
        identity_provider.mismatch_issuer(true);
        let Err(Error::Discovery(message)) = Oidc::new()
            .discover(&provider(&identity_provider).issuer)
            .await
        else {
            panic!("a mismatched issuer was discovered");
        };
        assert!(message.contains(MISMATCHED_ISSUER), "{message}");

        identity_provider.mismatch_issuer(false);
        identity_provider.fail_token_endpoint(true);
        let pkce = Pkce::generate();
        identity_provider.grant(CODE, &pkce.challenge, identity_provider.claims(NONCE));
        let Err(Error::TokenExchange(message)) =
            redeem(&identity_provider, &pkce.verifier, REDIRECT_URI).await
        else {
            panic!("a failing token endpoint answered");
        };
        assert!(message.contains("500"), "{message}");
    }

    #[tokio::test]
    async fn userinfo_answers_for_an_issued_access_token() {
        let identity_provider = IdentityProvider::start().await;
        identity_provider.serve_userinfo(Some(json!({
            "sub": testing::SUBJECT, "email": testing::EMAIL, "email_verified": true
        })));
        let pkce = Pkce::generate();
        identity_provider.grant(
            CODE,
            &pkce.challenge,
            Claims {
                email: None,
                email_verified: None,
                ..identity_provider.claims(NONCE)
            },
        );

        assert_eq!(
            redeem(&identity_provider, &pkce.verifier, REDIRECT_URI).await,
            Ok(expected())
        );
        assert_eq!(identity_provider.requests(USERINFO_PATH), 1);
    }

    #[test]
    fn basic_credentials_are_form_decoded() {
        let encoded = STANDARD.encode("zone%3Aapp%2B1:s3c%25ret+value");
        assert_eq!(
            basic_credentials(&encoded),
            Some(("zone:app+1".to_owned(), "s3c%ret value".to_owned()))
        );
        assert_eq!(basic_credentials("!!"), None);
        assert_eq!(basic_credentials(&STANDARD.encode("no-separator")), None);
    }
}
