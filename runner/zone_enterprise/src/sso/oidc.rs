use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeDelta, Utc};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use jsonwebtoken::jwk::Jwk;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::redirect;
use url::Url;
use url::form_urlencoded::byte_serialize;

use crate::sso::authorization::Authorization;
use crate::sso::body;
use crate::sso::cached::Cached;
use crate::sso::claims::Claims;
use crate::sso::client_authentication::ClientAuthentication;
use crate::sso::clock::{self, Clock};
use crate::sso::discovery::Discovery;
use crate::sso::error::Error;
use crate::sso::grant::Grant;
use crate::sso::header::Header;
use crate::sso::identity::Identity;
use crate::sso::issuer::Issuer;
use crate::sso::keys::Keys;
use crate::sso::provider::Provider;
use crate::sso::rejection::Rejection;
use crate::sso::token_refusal::TokenRefusal;
use crate::sso::token_response::TokenResponse;
use crate::sso::userinfo::Userinfo;
use crate::sso::verifier::Verifier;

pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const REFETCH_COOLDOWN: TimeDelta = TimeDelta::seconds(60);

const JSON: &str = "application/json";
const BEARER: &str = "Bearer";
const GRANT_TYPE: &str = "authorization_code";

#[derive(Clone)]
pub struct Oidc {
    client: reqwest::Client,
    clock: Clock,
    documents: Arc<DashMap<String, Cached<Discovery>>>,
    keys: Arc<DashMap<String, Cached<Keys>>>,
    refetched: Arc<DashMap<String, DateTime<Utc>>>,
}

impl Oidc {
    pub fn new() -> Self {
        Self::with_clock(clock::system())
    }

    pub fn with_clock(clock: Clock) -> Self {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .redirect(redirect::Policy::none())
            .build()
            .expect("an HTTP client with a timeout and no redirects always builds");
        Self {
            client,
            clock,
            documents: Arc::default(),
            keys: Arc::default(),
            refetched: Arc::default(),
        }
    }

    pub async fn discover(&self, issuer: &Issuer) -> Result<Arc<Discovery>, Error> {
        let now = (self.clock)();
        let cached = self
            .documents
            .get(issuer.as_str())
            .and_then(|entry| entry.fresh(now));
        if let Some(discovery) = cached {
            return Ok(discovery);
        }
        let body = self
            .get(&issuer.discovery_url())
            .await
            .map_err(Error::Discovery)?;
        let discovery = Arc::new(Discovery::parse(&body, issuer)?);
        self.documents.insert(
            issuer.as_str().to_owned(),
            Cached {
                value: Arc::clone(&discovery),
                fetched_at: now,
            },
        );
        Ok(discovery)
    }

    pub async fn authorize(
        &self,
        provider: &Provider,
        redirect_uri: &str,
    ) -> Result<Authorization, Error> {
        let discovery = self.discover(&provider.issuer).await?;
        Ok(Authorization::new(
            &discovery.authorization_endpoint,
            &provider.client_id,
            redirect_uri,
        ))
    }

    pub async fn exchange(
        &self,
        provider: &Provider,
        grant: &Grant<'_>,
    ) -> Result<Identity, Error> {
        let discovery = self.discover(&provider.issuer).await?;
        let tokens = self.token(provider, &discovery, grant).await?;
        let verifier = Verifier {
            issuer: &provider.issuer,
            client_id: &provider.client_id,
            nonce: grant.nonce,
            now: (self.clock)(),
            advertised: discovery.signing_algorithms.as_deref(),
        };
        let header = verifier.header(&tokens.id_token)?;
        let key = self.key(&provider.issuer, &discovery, &header).await?;
        let claims = verifier.verify(&tokens.id_token, &header, &key)?;
        let claims = self.complete(provider, &discovery, &tokens, claims).await?;
        provider.policy.admit(&provider.issuer, &claims)
    }

    async fn complete(
        &self,
        provider: &Provider,
        discovery: &Discovery,
        tokens: &TokenResponse,
        claims: Claims,
    ) -> Result<Claims, Error> {
        let Some(endpoint) = &discovery.userinfo_endpoint else {
            return Ok(claims);
        };
        if !provider.policy.incomplete(&claims) {
            return Ok(claims);
        }
        let access_token = tokens.access_token.as_deref().ok_or_else(|| {
            Error::TokenExchange(
                "the token endpoint's response had no access token for the userinfo endpoint."
                    .to_owned(),
            )
        })?;
        self.userinfo(endpoint, access_token)
            .await?
            .complete(claims)
    }

    async fn userinfo(&self, endpoint: &Url, access_token: &str) -> Result<Userinfo, Error> {
        let response = self
            .client
            .get(endpoint.clone())
            .header(ACCEPT, JSON)
            .header(AUTHORIZATION, format!("{BEARER} {access_token}"))
            .send()
            .await
            .map_err(|error| {
                Error::TokenExchange(format!(
                    "the userinfo endpoint couldn't be reached ({}).",
                    error.without_url()
                ))
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::TokenExchange(format!(
                "the userinfo endpoint answered HTTP {}.",
                status.as_u16()
            )));
        }
        let body = body::read(response).await.map_err(|unread| {
            Error::TokenExchange(format!(
                "the userinfo endpoint's response couldn't be read ({unread})."
            ))
        })?;
        serde_json::from_slice(&body).map_err(|_| {
            Error::TokenExchange(
                "the userinfo endpoint's response isn't valid userinfo.".to_owned(),
            )
        })
    }

    async fn token(
        &self,
        provider: &Provider,
        discovery: &Discovery,
        grant: &Grant<'_>,
    ) -> Result<TokenResponse, Error> {
        let mut form = vec![
            ("grant_type", GRANT_TYPE),
            ("code", grant.code),
            ("redirect_uri", grant.redirect_uri),
            ("code_verifier", grant.verifier),
        ];
        let mut request = self
            .client
            .post(discovery.token_endpoint.clone())
            .header(ACCEPT, JSON);
        match discovery.authentication() {
            ClientAuthentication::Basic => {
                request = request.header(
                    AUTHORIZATION,
                    basic(provider.client_id.as_str(), provider.client_secret.expose()),
                );
            }
            ClientAuthentication::Post => {
                form.push(("client_id", provider.client_id.as_str()));
                form.push(("client_secret", provider.client_secret.expose()));
            }
        }
        let response = request.form(&form).send().await.map_err(|error| {
            Error::TokenExchange(format!(
                "the token endpoint couldn't be reached ({}).",
                error.without_url()
            ))
        })?;
        let status = response.status();
        let body = body::read(response).await.map_err(|unread| {
            Error::TokenExchange(format!(
                "the token endpoint's response couldn't be read ({unread})."
            ))
        })?;
        if !status.is_success() {
            return Err(Error::TokenExchange(
                match serde_json::from_slice::<TokenRefusal>(&body) {
                    Ok(refusal) => format!(
                        "the token endpoint answered HTTP {} ({}).",
                        status.as_u16(),
                        refusal.error
                    ),
                    Err(_) => format!("the token endpoint answered HTTP {}.", status.as_u16()),
                },
            ));
        }
        serde_json::from_slice::<TokenResponse>(&body).map_err(|_| {
            Error::TokenExchange("the token endpoint's response had no ID token.".to_owned())
        })
    }

    async fn key(
        &self,
        issuer: &Issuer,
        discovery: &Discovery,
        header: &Header,
    ) -> Result<Jwk, Error> {
        let now = (self.clock)();
        let cached = self
            .keys
            .get(issuer.as_str())
            .and_then(|entry| entry.fresh(now));
        let keys = match cached {
            Some(keys) => keys,
            None => self.fetch_keys(issuer, discovery, now).await?,
        };
        if let Some(key) = keys.select(header.key_id.as_deref(), header.algorithm) {
            return Ok(key.clone());
        }
        if !self.may_refetch(issuer, now) {
            return Err(Error::IdTokenInvalid(Rejection::UnknownKey));
        }
        self.fetch_keys(issuer, discovery, now)
            .await?
            .select(header.key_id.as_deref(), header.algorithm)
            .cloned()
            .ok_or(Error::IdTokenInvalid(Rejection::UnknownKey))
    }

    async fn fetch_keys(
        &self,
        issuer: &Issuer,
        discovery: &Discovery,
        now: DateTime<Utc>,
    ) -> Result<Arc<Keys>, Error> {
        let body = self
            .get(&discovery.keys_endpoint)
            .await
            .map_err(Error::Discovery)?;
        let keys = Arc::new(Keys::parse(&body)?);
        self.keys.insert(
            issuer.as_str().to_owned(),
            Cached {
                value: Arc::clone(&keys),
                fetched_at: now,
            },
        );
        Ok(keys)
    }

    pub fn forget(&self, issuer: &str) {
        self.documents.remove(issuer);
        self.keys.remove(issuer);
        self.refetched.remove(issuer);
    }

    fn may_refetch(&self, issuer: &Issuer, now: DateTime<Utc>) -> bool {
        match self.refetched.entry(issuer.as_str().to_owned()) {
            Entry::Occupied(entry) if now - *entry.get() < REFETCH_COOLDOWN => false,
            Entry::Occupied(mut entry) => {
                entry.insert(now);
                true
            }
            Entry::Vacant(entry) => {
                entry.insert(now);
                true
            }
        }
    }

    async fn get(&self, url: &Url) -> Result<Vec<u8>, String> {
        let response = self
            .client
            .get(url.clone())
            .header(ACCEPT, JSON)
            .send()
            .await
            .map_err(|error| format!("{url} couldn't be fetched ({}).", error.without_url()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("{url} answered HTTP {}.", status.as_u16()));
        }
        body::read(response)
            .await
            .map_err(|unread| format!("{url} couldn't be read ({unread})."))
    }
}

impl Default for Oidc {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Oidc {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Oidc")
            .field("documents", &self.documents.len())
            .field("keys", &self.keys.len())
            .finish_non_exhaustive()
    }
}

fn basic(client_id: &str, client_secret: &str) -> String {
    let encode = |value: &str| byte_serialize(value.as_bytes()).collect::<String>();
    format!(
        "Basic {}",
        STANDARD.encode(format!("{}:{}", encode(client_id), encode(client_secret)))
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    use serde_json::{Value, json};
    use url::form_urlencoded;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    use super::*;
    use crate::sso::fields::Fields;
    use crate::sso::protocol::Protocol;
    use crate::sso::testing;

    const CLIENT_ID: &str = "zone";
    const CLIENT_SECRET: &str = "s3cret";
    const CODE: &str = "code-1";
    const VERIFIER: &str = "verifier-1";
    static NONCE: LazyLock<String> = LazyLock::new(testing::nonce);
    const REDIRECT: &str = "https://zone.example.com/sso/callback";
    const DISCOVERY_PATH: &str = "/.well-known/openid-configuration";

    struct Harness {
        server: MockServer,
        oidc: Oidc,
        time: Arc<Mutex<DateTime<Utc>>>,
    }

    impl Harness {
        async fn new() -> Self {
            let time = Arc::new(Mutex::new(Utc::now()));
            let reader = Arc::clone(&time);
            Self {
                server: MockServer::start().await,
                oidc: Oidc::with_clock(Arc::new(move || *reader.lock().unwrap())),
                time,
            }
        }

        fn issuer(&self) -> String {
            self.server.uri()
        }

        fn now(&self) -> DateTime<Utc> {
            *self.time.lock().unwrap()
        }

        fn advance(&self, delta: TimeDelta) {
            *self.time.lock().unwrap() += delta;
        }

        fn provider(&self, client_id: &str, client_secret: &str, domains: &[String]) -> Provider {
            Provider::parse(&Fields {
                protocol: Protocol::Oidc,
                display_name: "Test IdP",
                issuer: &self.issuer(),
                client_id,
                client_secret,
                allowed_domains: domains,
                require_verified_email: true,
            })
            .unwrap()
        }

        fn default_provider(&self) -> Provider {
            self.provider(CLIENT_ID, CLIENT_SECRET, &[])
        }

        fn claims(&self) -> Claims {
            testing::claims(&self.issuer(), CLIENT_ID, &NONCE, self.now())
        }

        async fn serve_discovery(&self, document: Value) {
            Mock::given(method("GET"))
                .and(path(DISCOVERY_PATH))
                .respond_with(ResponseTemplate::new(200).set_body_json(document))
                .mount(&self.server)
                .await;
        }

        async fn serve_standard_discovery(&self) {
            self.serve_discovery(testing::discovery(&self.issuer()))
                .await;
        }

        async fn serve_keys(&self) {
            Mock::given(method("GET"))
                .and(path("/jwks"))
                .respond_with(ResponseTemplate::new(200).set_body_json(testing::key_set()))
                .mount(&self.server)
                .await;
        }

        async fn serve_token(&self, token: String) {
            Mock::given(method("POST"))
                .and(path("/token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "access_token": "access", "token_type": "Bearer", "id_token": token
                })))
                .mount(&self.server)
                .await;
        }

        async fn exchange(&self, provider: &Provider) -> Result<Identity, Error> {
            self.oidc
                .exchange(
                    provider,
                    &Grant {
                        code: CODE,
                        verifier: VERIFIER,
                        nonce: &NONCE,
                        redirect_uri: REDIRECT,
                    },
                )
                .await
        }

        async fn requests(&self, route: &str) -> usize {
            self.server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|request| request.url.path() == route)
                .count()
        }
    }

    fn form(request: &Request) -> HashMap<String, String> {
        form_urlencoded::parse(&request.body).into_owned().collect()
    }

    fn form_is(expected: &[(&str, &str)]) -> impl Fn(&Request) -> bool + Send + Sync + 'static {
        let expected: HashMap<String, String> = expected
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        move |request: &Request| form(request) == expected
    }

    fn without_authorization(request: &Request) -> bool {
        !request.headers.contains_key("authorization")
    }

    fn code_grant() -> Vec<(&'static str, &'static str)> {
        vec![
            ("grant_type", "authorization_code"),
            ("code", CODE),
            ("redirect_uri", REDIRECT),
            ("code_verifier", VERIFIER),
        ]
    }

    #[tokio::test]
    async fn discovery_refuses_an_issuer_mismatch() {
        let harness = Harness::new().await;
        harness
            .serve_discovery(testing::discovery("https://evil.test"))
            .await;
        let Err(Error::Discovery(message)) = harness
            .oidc
            .discover(&harness.default_provider().issuer)
            .await
        else {
            panic!("a mismatched issuer was accepted");
        };
        assert!(message.contains("https://evil.test"), "{message}");
    }

    #[tokio::test]
    async fn discovery_is_cached_for_an_hour() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        let issuer = harness.default_provider().issuer;
        let first = harness.oidc.discover(&issuer).await.unwrap();
        let second = harness.oidc.discover(&issuer).await.unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(harness.requests(DISCOVERY_PATH).await, 1);
        harness.advance(TimeDelta::hours(1));
        harness.oidc.discover(&issuer).await.unwrap();
        assert_eq!(harness.requests(DISCOVERY_PATH).await, 2);
    }

    #[tokio::test]
    async fn clones_share_the_cache() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        let issuer = harness.default_provider().issuer;
        harness.oidc.discover(&issuer).await.unwrap();
        harness.oidc.clone().discover(&issuer).await.unwrap();
        assert_eq!(harness.requests(DISCOVERY_PATH).await, 1);
    }

    #[tokio::test]
    async fn discovery_failures_are_reported_and_not_cached() {
        let harness = Harness::new().await;
        Mock::given(method("GET"))
            .and(path(DISCOVERY_PATH))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&harness.server)
            .await;
        harness.serve_standard_discovery().await;
        let issuer = harness.default_provider().issuer;
        let Err(Error::Discovery(message)) = harness.oidc.discover(&issuer).await else {
            panic!("a 503 was accepted");
        };
        assert!(message.contains("503"), "{message}");
        assert!(harness.oidc.discover(&issuer).await.is_ok());
    }

    #[tokio::test]
    async fn an_oversized_discovery_document_is_refused() {
        let harness = Harness::new().await;
        Mock::given(method("GET"))
            .and(path(DISCOVERY_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b' '; 2 * 1024 * 1024]))
            .mount(&harness.server)
            .await;
        let Err(Error::Discovery(message)) = harness
            .oidc
            .discover(&harness.default_provider().issuer)
            .await
        else {
            panic!("an oversized document was accepted");
        };
        assert!(message.contains("larger than"), "{message}");
    }

    #[tokio::test]
    async fn a_forgotten_issuer_is_discovered_again() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        let issuer = harness.default_provider().issuer;
        harness.oidc.discover(&issuer).await.unwrap();
        harness.oidc.discover(&issuer).await.unwrap();
        assert_eq!(harness.requests(DISCOVERY_PATH).await, 1);

        harness.oidc.forget(issuer.as_str());

        harness.oidc.discover(&issuer).await.unwrap();
        assert_eq!(harness.requests(DISCOVERY_PATH).await, 2);
    }

    #[tokio::test]
    async fn discovery_refuses_a_plain_http_endpoint_off_loopback() {
        let harness = Harness::new().await;
        let mut document = testing::discovery(&harness.issuer());
        document["token_endpoint"] = json!("http://idp.acme.test/token");
        harness.serve_discovery(document).await;
        assert!(matches!(
            harness
                .oidc
                .discover(&harness.default_provider().issuer)
                .await,
            Err(Error::Discovery(_))
        ));
    }

    #[tokio::test]
    async fn authorize_uses_the_discovered_endpoint() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        let authorization = harness
            .oidc
            .authorize(&harness.default_provider(), REDIRECT)
            .await
            .unwrap();
        let expected = format!(
            "{}/authorize?response_type=code&client_id=zone&",
            harness.issuer()
        );
        assert!(
            authorization.url.as_str().starts_with(&expected),
            "{}",
            authorization.url
        );
        assert!(!authorization.nonce.is_empty() && !authorization.verifier.is_empty());
    }

    #[tokio::test]
    async fn exchange_uses_basic_authentication_by_default() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        harness.serve_keys().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(header("authorization", "Basic em9uZTpzM2NyZXQ="))
            .and(header("accept", "application/json"))
            .and(form_is(&code_grant()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id_token": testing::id_token(&harness.claims())
            })))
            .expect(1)
            .mount(&harness.server)
            .await;
        let identity = harness.exchange(&harness.default_provider()).await.unwrap();
        assert_eq!(
            identity,
            Identity {
                subject: testing::SUBJECT.into(),
                email: testing::EMAIL.into(),
                name: Some(testing::NAME.into()),
            }
        );
    }

    #[tokio::test]
    async fn basic_authentication_form_encodes_the_credentials() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        harness.serve_keys().await;
        let claims = testing::claims(&harness.issuer(), "zone:app+1", &NONCE, harness.now());
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(header(
                "authorization",
                "Basic em9uZSUzQWFwcCUyQjE6czNjJTI1cmV0K3ZhbHVl",
            ))
            .and(form_is(&code_grant()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "id_token": testing::id_token(&claims) })),
            )
            .expect(1)
            .mount(&harness.server)
            .await;
        let provider = harness.provider("zone:app+1", "s3c%ret value", &[]);
        assert!(harness.exchange(&provider).await.is_ok());
    }

    #[tokio::test]
    async fn exchange_posts_credentials_when_only_post_is_supported() {
        let harness = Harness::new().await;
        let mut document = testing::discovery(&harness.issuer());
        document["token_endpoint_auth_methods_supported"] = json!(["client_secret_post"]);
        harness.serve_discovery(document).await;
        harness.serve_keys().await;
        let mut expected = code_grant();
        expected.push(("client_id", CLIENT_ID));
        expected.push(("client_secret", CLIENT_SECRET));
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(without_authorization)
            .and(form_is(&expected))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id_token": testing::id_token(&harness.claims())
            })))
            .expect(1)
            .mount(&harness.server)
            .await;
        assert!(harness.exchange(&harness.default_provider()).await.is_ok());
    }

    #[tokio::test]
    async fn a_refused_code_is_a_token_exchange_failure() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": "invalid_grant", "error_description": "The code expired."
            })))
            .mount(&harness.server)
            .await;
        let error = harness
            .exchange(&harness.default_provider())
            .await
            .unwrap_err();
        assert_eq!(error.code(), crate::sso::error::TOKEN_EXCHANGE_FAILED);
        assert!(error.to_string().contains("400 (invalid_grant)"), "{error}");
    }

    #[tokio::test]
    async fn a_server_error_without_a_body_is_a_token_exchange_failure() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&harness.server)
            .await;
        assert_eq!(
            harness.exchange(&harness.default_provider()).await,
            Err(Error::TokenExchange(
                "the token endpoint answered HTTP 500.".into()
            ))
        );
    }

    #[tokio::test]
    async fn the_token_endpoint_is_not_followed_through_redirects() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        harness.serve_keys().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/elsewhere", harness.issuer())),
            )
            .mount(&harness.server)
            .await;
        Mock::given(method("POST"))
            .and(path("/elsewhere"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id_token": testing::id_token(&harness.claims())
            })))
            .expect(0)
            .mount(&harness.server)
            .await;
        assert!(matches!(
            harness.exchange(&harness.default_provider()).await,
            Err(Error::TokenExchange(_))
        ));
    }

    #[tokio::test]
    async fn a_response_without_an_id_token_is_a_token_exchange_failure() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "a"})))
            .mount(&harness.server)
            .await;
        assert!(matches!(
            harness.exchange(&harness.default_provider()).await,
            Err(Error::TokenExchange(_))
        ));
    }

    #[tokio::test]
    async fn exchange_applies_the_provider_policy() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        harness.serve_keys().await;
        harness
            .serve_token(testing::id_token(&harness.claims()))
            .await;
        let provider = harness.provider(CLIENT_ID, CLIENT_SECRET, &["example.com".to_owned()]);
        assert_eq!(
            harness.exchange(&provider).await,
            Err(Error::DomainNotAllowed)
        );
    }

    #[tokio::test]
    async fn exchange_verifies_the_nonce() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        harness.serve_keys().await;
        let claims = Claims {
            nonce: Some(testing::nonce()),
            ..harness.claims()
        };
        harness.serve_token(testing::id_token(&claims)).await;
        assert_eq!(
            harness.exchange(&harness.default_provider()).await,
            Err(Error::IdTokenInvalid(Rejection::Nonce))
        );
    }

    #[tokio::test]
    async fn an_algorithm_the_provider_does_not_advertise_is_refused() {
        let harness = Harness::new().await;
        let mut document = testing::discovery(&harness.issuer());
        document["id_token_signing_alg_values_supported"] = json!(["ES256"]);
        harness.serve_discovery(document).await;
        harness.serve_keys().await;
        harness
            .serve_token(testing::id_token(&harness.claims()))
            .await;
        assert_eq!(
            harness.exchange(&harness.default_provider()).await,
            Err(Error::IdTokenInvalid(Rejection::Algorithm))
        );
    }

    #[tokio::test]
    async fn signing_keys_are_cached_between_sign_ins() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        harness.serve_keys().await;
        harness
            .serve_token(testing::id_token(&harness.claims()))
            .await;
        let provider = harness.default_provider();
        assert!(harness.exchange(&provider).await.is_ok());
        assert!(harness.exchange(&provider).await.is_ok());
        assert_eq!(harness.requests("/jwks").await, 1);
    }

    #[tokio::test]
    async fn an_unknown_key_refetches_the_keys_after_rotation() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(testing::key_set_with("old")))
            .up_to_n_times(1)
            .mount(&harness.server)
            .await;
        harness.serve_keys().await;
        harness
            .serve_token(testing::id_token(&harness.claims()))
            .await;
        assert!(harness.exchange(&harness.default_provider()).await.is_ok());
        assert_eq!(harness.requests("/jwks").await, 2);
    }

    #[tokio::test]
    async fn unknown_keys_refetch_at_most_once_a_minute() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        harness
            .serve_token(testing::id_token(&harness.claims()))
            .await;
        let provider = harness.default_provider();
        let old_keys = || {
            Mock::given(method("GET")).and(path("/jwks")).respond_with(
                ResponseTemplate::new(200).set_body_json(testing::key_set_with("old")),
            )
        };
        {
            let _guard = old_keys().expect(2).mount_as_scoped(&harness.server).await;
            for _ in 0..2 {
                assert_eq!(
                    harness.exchange(&provider).await,
                    Err(Error::IdTokenInvalid(Rejection::UnknownKey))
                );
            }
        }
        harness.advance(REFETCH_COOLDOWN);
        let _guard = old_keys().expect(1).mount_as_scoped(&harness.server).await;
        assert_eq!(
            harness.exchange(&provider).await,
            Err(Error::IdTokenInvalid(Rejection::UnknownKey))
        );
    }

    #[tokio::test]
    async fn an_unreachable_key_set_is_a_discovery_failure() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&harness.server)
            .await;
        harness
            .serve_token(testing::id_token(&harness.claims()))
            .await;
        assert!(matches!(
            harness.exchange(&harness.default_provider()).await,
            Err(Error::Discovery(_))
        ));
    }

    const ACCESS_TOKEN: &str = "access-1";

    impl Harness {
        async fn serve_userinfo_discovery(&self) {
            let mut document = testing::discovery(&self.issuer());
            document["userinfo_endpoint"] = json!(format!("{}/userinfo", self.issuer()));
            self.serve_discovery(document).await;
        }

        async fn serve_tokens(&self, claims: &Claims) {
            Mock::given(method("POST"))
                .and(path("/token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "access_token": ACCESS_TOKEN,
                    "token_type": "Bearer",
                    "id_token": testing::id_token(claims),
                })))
                .mount(&self.server)
                .await;
        }

        async fn serve_userinfo(&self, response: ResponseTemplate, expected: u64) {
            Mock::given(method("GET"))
                .and(path("/userinfo"))
                .and(header("authorization", "Bearer access-1"))
                .respond_with(response)
                .expect(expected)
                .mount(&self.server)
                .await;
        }

        fn trusting_provider(&self) -> Provider {
            Provider::parse(&Fields {
                protocol: Protocol::Oidc,
                display_name: "Test IdP",
                issuer: &self.issuer(),
                client_id: CLIENT_ID,
                client_secret: CLIENT_SECRET,
                allowed_domains: &[],
                require_verified_email: false,
            })
            .unwrap()
        }
    }

    fn userinfo(body: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(body)
    }

    #[tokio::test]
    async fn userinfo_fills_a_missing_email() {
        let harness = Harness::new().await;
        harness.serve_userinfo_discovery().await;
        harness.serve_keys().await;
        harness
            .serve_tokens(&Claims {
                email: None,
                email_verified: None,
                ..harness.claims()
            })
            .await;
        harness
            .serve_userinfo(
                userinfo(json!({
                    "sub": testing::SUBJECT, "email": "Jane@Acme.com", "email_verified": true
                })),
                1,
            )
            .await;
        let identity = harness.exchange(&harness.default_provider()).await.unwrap();
        assert_eq!(identity.email, "jane@acme.com");
        assert_eq!(identity.subject, testing::SUBJECT);
    }

    #[tokio::test]
    async fn userinfo_fills_a_missing_verification_the_policy_requires() {
        let harness = Harness::new().await;
        harness.serve_userinfo_discovery().await;
        harness.serve_keys().await;
        harness
            .serve_tokens(&Claims {
                email_verified: None,
                ..harness.claims()
            })
            .await;
        harness
            .serve_userinfo(
                userinfo(json!({
                    "sub": testing::SUBJECT, "email": testing::EMAIL, "email_verified": "true"
                })),
                1,
            )
            .await;
        assert_eq!(
            harness
                .exchange(&harness.default_provider())
                .await
                .unwrap()
                .email,
            testing::EMAIL
        );
    }

    #[tokio::test]
    async fn userinfo_for_another_subject_is_refused() {
        let harness = Harness::new().await;
        harness.serve_userinfo_discovery().await;
        harness.serve_keys().await;
        harness
            .serve_tokens(&Claims {
                email: None,
                ..harness.claims()
            })
            .await;
        harness
            .serve_userinfo(
                userinfo(json!({
                    "sub": "someone-else", "email": testing::EMAIL, "email_verified": true
                })),
                1,
            )
            .await;
        assert_eq!(
            harness.exchange(&harness.default_provider()).await,
            Err(Error::IdTokenInvalid(Rejection::Subject))
        );
    }

    #[tokio::test]
    async fn without_a_userinfo_endpoint_the_id_token_decides_alone() {
        let harness = Harness::new().await;
        harness.serve_standard_discovery().await;
        harness.serve_keys().await;
        harness
            .serve_tokens(&Claims {
                email: None,
                ..harness.claims()
            })
            .await;
        harness
            .serve_userinfo(userinfo(json!({"sub": testing::SUBJECT})), 0)
            .await;
        assert_eq!(
            harness.exchange(&harness.default_provider()).await,
            Err(Error::EmailMissing)
        );
        assert_eq!(harness.requests("/userinfo").await, 0);
    }

    #[tokio::test]
    async fn a_failing_userinfo_endpoint_is_a_token_exchange_failure() {
        let harness = Harness::new().await;
        harness.serve_userinfo_discovery().await;
        harness.serve_keys().await;
        harness
            .serve_tokens(&Claims {
                email: None,
                ..harness.claims()
            })
            .await;
        harness.serve_userinfo(ResponseTemplate::new(401), 1).await;
        let error = harness
            .exchange(&harness.default_provider())
            .await
            .unwrap_err();
        assert_eq!(error.code(), crate::sso::error::TOKEN_EXCHANGE_FAILED);
        assert!(
            error
                .to_string()
                .contains("userinfo endpoint answered HTTP 401"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn userinfo_that_is_not_json_is_a_token_exchange_failure() {
        let harness = Harness::new().await;
        harness.serve_userinfo_discovery().await;
        harness.serve_keys().await;
        harness
            .serve_tokens(&Claims {
                email: None,
                ..harness.claims()
            })
            .await;
        harness
            .serve_userinfo(
                ResponseTemplate::new(200).set_body_string("eyJ.signed.jwt"),
                1,
            )
            .await;
        assert!(matches!(
            harness.exchange(&harness.default_provider()).await,
            Err(Error::TokenExchange(_))
        ));
    }

    #[tokio::test]
    async fn a_missing_access_token_is_a_token_exchange_failure() {
        let harness = Harness::new().await;
        harness.serve_userinfo_discovery().await;
        harness.serve_keys().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id_token": testing::id_token(&Claims { email: None, ..harness.claims() })
            })))
            .mount(&harness.server)
            .await;
        let error = harness
            .exchange(&harness.default_provider())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("no access token"), "{error}");
    }

    #[tokio::test]
    async fn userinfo_never_overrides_the_id_token() {
        let harness = Harness::new().await;
        harness.serve_userinfo_discovery().await;
        harness.serve_keys().await;
        harness
            .serve_tokens(&Claims {
                email_verified: None,
                ..harness.claims()
            })
            .await;
        harness
            .serve_userinfo(
                userinfo(json!({
                    "sub": testing::SUBJECT, "email": "mallory@acme.com",
                    "email_verified": true, "name": "Mallory"
                })),
                1,
            )
            .await;
        assert_eq!(
            harness.exchange(&harness.default_provider()).await,
            Err(Error::EmailUnverified)
        );
    }

    #[tokio::test]
    async fn userinfo_keeps_the_id_token_name_and_email() {
        let harness = Harness::new().await;
        harness.serve_userinfo_discovery().await;
        harness.serve_keys().await;
        harness
            .serve_tokens(&Claims {
                email_verified: None,
                ..harness.claims()
            })
            .await;
        harness
            .serve_userinfo(
                userinfo(json!({
                    "sub": testing::SUBJECT, "email": "JANE@acme.com",
                    "email_verified": true, "name": "Someone Else"
                })),
                1,
            )
            .await;
        assert_eq!(
            harness.exchange(&harness.default_provider()).await,
            Ok(Identity {
                subject: testing::SUBJECT.into(),
                email: testing::EMAIL.into(),
                name: Some(testing::NAME.into()),
            })
        );
    }

    #[tokio::test]
    async fn complete_claims_skip_userinfo() {
        let harness = Harness::new().await;
        harness.serve_userinfo_discovery().await;
        harness.serve_keys().await;
        harness.serve_tokens(&harness.claims()).await;
        harness
            .serve_userinfo(userinfo(json!({"sub": testing::SUBJECT})), 0)
            .await;
        assert!(harness.exchange(&harness.default_provider()).await.is_ok());
        let explicit = Harness::new().await;
        explicit.serve_userinfo_discovery().await;
        explicit.serve_keys().await;
        explicit
            .serve_tokens(&Claims {
                email_verified: Some(false),
                ..explicit.claims()
            })
            .await;
        explicit
            .serve_userinfo(userinfo(json!({"sub": testing::SUBJECT})), 0)
            .await;
        assert_eq!(
            explicit.exchange(&explicit.default_provider()).await,
            Err(Error::EmailUnverified)
        );
        let trusting = Harness::new().await;
        trusting.serve_userinfo_discovery().await;
        trusting.serve_keys().await;
        trusting
            .serve_tokens(&Claims {
                email_verified: None,
                ..trusting.claims()
            })
            .await;
        trusting
            .serve_userinfo(userinfo(json!({"sub": testing::SUBJECT})), 0)
            .await;
        assert!(
            trusting
                .exchange(&trusting.trusting_provider())
                .await
                .is_ok()
        );
    }

    #[test]
    fn debug_shows_no_internals() {
        assert!(format!("{:?}", Oidc::default()).starts_with("Oidc"));
    }
}
