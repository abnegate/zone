//! SAML 2.0 single sign-on end to end: the service provider's URLs and metadata, adding SAML
//! providers from pasted or fetched metadata, the license gate, and signing in through the ACS and
//! the console handoff, with every refusal on the way.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{TimeDelta, Utc};
use flate2::read::DeflateDecoder;
use futures::future::join_all;
use reqwest::Url;
use serde_json::{Value, json};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_enterprise::LICENSE_REQUIRED;
use zone_enterprise::sso::saml::testing::{self, Key, Options};
use zone_enterprise::sso::saml::{Certificate, Document, ServiceProvider, service_metadata, uri};
use zone_enterprise::sso::testing::{EMAIL, SUBJECT};
use zone_enterprise::sso::{
    ASSERTION_INVALID, DISPLAY_NAME_INVALID, DOMAIN_NOT_ALLOWED, DOMAIN_REQUIRED, EMAIL_CONFLICT,
    METADATA_INVALID, PROTOCOL_MISMATCH, PROVIDER_DUPLICATE, PROVIDER_NOT_FOUND, SIGN_IN_FAILED,
    SIGN_IN_REFUSED, SSO_REQUIRED, STATE_INVALID, VERIFIED_EMAIL_UNSUPPORTED,
};
use zone_server::db::audit::actions;
use zone_server::db::sso::attempts;
use zone_server::routes::sso::saml::{BODY_LIMIT, RELAY_STATE_LIMIT};
use zone_server::utils::crypto::hash_token;

use crate::common::identity_provider::IdentityProvider;
use crate::common::sso::{Account, Fixture, lapsed};
use crate::common::{TestResponse, test_password};

const PREFIX: &str = "sso_saml";
const METADATA_PATH: &str = "/api/auth/sso/saml/metadata";
const ACS_PATH: &str = "/api/auth/sso/saml/acs";
const PUBLIC_PROVIDERS: &str = "/api/auth/sso/providers";
const CALLBACKS: &str = "/api/auth/sso/callbacks";
const SERVICE_ENTITY_ID: &str = "http://localhost:3000/api/auth/sso/saml/metadata";
const ACS_URL: &str = "http://localhost:3000/api/auth/sso/saml/acs";
const CALLBACK_URL: &str = "http://localhost:3000/sso/callback";
const IDP_METADATA_PATH: &str = "/metadata";
const DISPLAY_NAME: &str = "Okta SAML";
const DOMAIN: &str = "acme.com";
const OTHER_ENTITY_ID: &str = "https://idp.example.com/other-metadata";
const ELSEWHERE: &str = "https://elsewhere.example.com";
const RESPONDER: &str = "urn:oasis:names:tc:SAML:2.0:status:Responder";
const FORM: &str = "application/x-www-form-urlencoded";
const NO_STORE: &str = "no-store";
const NO_REFERRER: &str = "no-referrer";
const SAML_REQUEST: &str = "SAMLRequest";
const SAML_RESPONSE: &str = "SAMLResponse";
const RELAY_STATE: &str = "RelayState";
const STATE: &str = "state";
const CODE: &str = "code";
const ERROR: &str = "error";
const ASSERTION_START: &str = "<saml2:Assertion ";
const ASSERTION_END: &str = "</saml2:Assertion>";
const ATTACKER: &str = "attacker";
const ATTACKER_EMAIL: &str = "attacker@acme.com";
const CONCURRENT_POSTS: usize = 16;

type Adjustment = Box<dyn FnOnce(Options) -> Options>;

/// What the browser carries from starting a sign-in to the identity provider: the authorization
/// URL, the state the console keeps, and the ID of the AuthnRequest inside the URL.
struct Started {
    url: Url,
    state: String,
    request_id: String,
}

/// Where the ACS sent the browser: the URL without its query, and the query.
struct Redirect {
    target: String,
    parameters: HashMap<String, String>,
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
}

impl Server {
    /// An Enterprise server with an admin and one enabled SAML provider from pasted metadata.
    async fn new() -> Self {
        let fixture = Fixture::new(PREFIX).await;
        let admin = fixture.register().await;
        fixture.license(&admin).await;
        let provider = fixture.created(&admin.token, &saml_provider()).await;
        Self {
            fixture,
            admin,
            provider,
        }
    }

    async fn authorize(&self) -> TestResponse {
        self.fixture
            .client
            .post_json(
                &format!("{PUBLIC_PROVIDERS}/{}/authorizations", self.provider),
                &json!({}),
            )
            .await
    }

    async fn start(&self) -> Started {
        let response = self.authorize().await;
        response.assert_status(StatusCode::CREATED);
        let body = response.json_value();
        let url = Url::parse(body["url"].as_str().unwrap()).unwrap();
        let state = body["state"].as_str().unwrap().to_owned();
        let request = authn_request(&url);
        let root = request.root();
        let request_id = request
            .element(root)
            .unwrap()
            .attribute("ID")
            .unwrap()
            .to_owned();
        Started {
            url,
            state,
            request_id,
        }
    }

    async fn acs(&self, relay_state: Option<&str>, response: &str) -> TestResponse {
        let mut form = form_urlencoded::Serializer::new(String::new());
        form.append_pair(SAML_RESPONSE, response);
        if let Some(relay_state) = relay_state {
            form.append_pair(RELAY_STATE, relay_state);
        }
        self.fixture
            .client
            .send_request(
                Request::builder()
                    .method("POST")
                    .uri(ACS_PATH)
                    .header(header::CONTENT_TYPE, FORM)
                    .body(Body::from(form.finish()))
                    .unwrap(),
            )
            .await
    }

    /// Posts the identity provider's answer to `started`, as `adjust` changes it, to the ACS.
    async fn answer(&self, started: &Started, adjust: impl FnOnce(Options) -> Options) -> Redirect {
        let response = testing::response(&adjust(options(started)));
        redirected(&self.acs(Some(&started.state), &response).await)
    }

    /// Answers `started` and expects the ACS to hand the console a code.
    async fn asserted(&self, started: &Started, adjust: impl FnOnce(Options) -> Options) -> String {
        let redirect = self.answer(started, adjust).await;
        assert_eq!(redirect.target, CALLBACK_URL);
        assert_eq!(redirect.parameters.len(), 2, "{:?}", redirect.parameters);
        assert_eq!(redirect.parameters[STATE], started.state);
        redirect.parameters[CODE].clone()
    }

    async fn callback(&self, state: &str, code: &str) -> TestResponse {
        self.fixture
            .client
            .post_json(CALLBACKS, &json!({ "state": state, "code": code }))
            .await
    }

    /// The whole flow, from starting the sign-in to the console's callback, for the identity
    /// provider's valid answer as `adjust` changes it.
    async fn sign_in(&self, adjust: impl FnOnce(Options) -> Options) -> TestResponse {
        let started = self.start().await;
        let code = self.asserted(&started, adjust).await;
        self.callback(&started.state, &code).await
    }

    async fn signed_in(&self, adjust: impl FnOnce(Options) -> Options) -> Session {
        let response = self.sign_in(adjust).await;
        response.assert_status(StatusCode::OK);
        session(&response)
    }

    async fn update(&self, body: &Value) -> TestResponse {
        self.fixture
            .update(&self.admin.token, self.provider, body)
            .await
    }

    async fn attempts(&self, state: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM sso_attempts WHERE state_hash = $1")
            .bind(hash_token(state))
            .fetch_one(self.fixture.pool())
            .await
            .unwrap()
    }

    async fn stored_request_id(&self, state: &str) -> Option<String> {
        sqlx::query_scalar("SELECT request_id FROM sso_attempts WHERE state_hash = $1")
            .bind(hash_token(state))
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

    async fn users_with_email(&self, email: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE lower(email) = lower($1)")
            .bind(email)
            .fetch_one(self.fixture.pool())
            .await
            .unwrap()
    }

    async fn email_verified(&self, user: Uuid) -> bool {
        sqlx::query_scalar("SELECT email_verified FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(self.fixture.pool())
            .await
            .unwrap()
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

fn saml_provider() -> Value {
    json!({
        "protocol": "saml",
        "display_name": DISPLAY_NAME,
        "metadata": testing::metadata(testing::ENTITY_ID, testing::SSO_URL, &[testing::CERTIFICATE]),
        "allowed_domains": [DOMAIN],
    })
}

fn with(overrides: Value) -> Value {
    let mut body = saml_provider();
    for (key, value) in overrides.as_object().unwrap() {
        if value.is_null() {
            body.as_object_mut().unwrap().remove(key);
        } else {
            body[key] = value.clone();
        }
    }
    body
}

fn fingerprint(certificate: &str) -> String {
    Certificate::parse(certificate)
        .unwrap()
        .fingerprint()
        .to_owned()
}

fn unique(prefix: &str) -> String {
    format!("_{prefix}-{}", Uuid::new_v4().simple())
}

/// The identity provider's valid answer to `started`: signed on the assertion, addressed to this
/// server's ACS and entity ID, with IDs of its own so that no two answers replay each other.
fn options(started: &Started) -> Options {
    Options {
        response_id: unique("response"),
        assertion_id: unique("assertion"),
        destination: Some(ACS_URL.to_owned()),
        in_response_to: Some(started.request_id.clone()),
        confirmation_in_response_to: Some(started.request_id.clone()),
        recipient: ACS_URL.to_owned(),
        audiences: vec![SERVICE_ENTITY_ID.to_owned()],
        ..Options::default()
    }
}

fn unchanged(options: Options) -> Options {
    options
}

fn as_person(subject: &'static str, email: String) -> impl FnOnce(Options) -> Options {
    move |options| Options {
        name_id: subject.to_owned(),
        email: Some(email),
        ..options
    }
}

fn redirected(response: &TestResponse) -> Redirect {
    response.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        response.header(header::CACHE_CONTROL.as_str()),
        Some(NO_STORE)
    );
    assert_eq!(
        response.header(header::REFERRER_POLICY.as_str()),
        Some(NO_REFERRER)
    );
    let location = Url::parse(response.header(header::LOCATION.as_str()).unwrap()).unwrap();
    let mut target = location.clone();
    target.set_query(None);
    Redirect {
        target: target.to_string(),
        parameters: location.query_pairs().into_owned().collect(),
    }
}

fn failure(state: &str, code: &str) -> HashMap<String, String> {
    HashMap::from([
        (STATE.to_owned(), state.to_owned()),
        (ERROR.to_owned(), code.to_owned()),
    ])
}

fn assert_failed(redirect: &Redirect, state: &str, code: &str) {
    assert_eq!(redirect.target, CALLBACK_URL);
    assert_eq!(redirect.parameters, failure(state, code));
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

fn between<'a>(xml: &'a str, start: &str, end: &str) -> &'a str {
    let from = xml.find(start).expect("the XML has the start marker");
    let to = xml[from..].find(end).expect("the XML has the end marker") + from + end.len();
    &xml[from..to]
}

/// A signed answer whose assertion has been moved under `Extensions`, with an unsigned assertion
/// for an attacker put where the signed one was.
fn wrapped(options: &Options) -> String {
    let signed = testing::response_xml(options);
    let assertion = between(&signed, ASSERTION_START, ASSERTION_END).to_owned();
    let unsigned = testing::response_xml(&Options {
        assertion_id: unique("forged"),
        name_id: ATTACKER.to_owned(),
        email: Some(ATTACKER_EMAIL.to_owned()),
        sign_assertion: false,
        sign_response: false,
        ..options.clone()
    });
    let forged = between(&unsigned, ASSERTION_START, ASSERTION_END);
    let wrapped = signed.replacen(
        &assertion,
        &format!("<saml2p:Extensions>{assertion}</saml2p:Extensions>{forged}"),
        1,
    );
    STANDARD.encode(wrapped)
}

fn authn_request(url: &Url) -> Document {
    let parameters: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let compressed = STANDARD.decode(&parameters[SAML_REQUEST]).unwrap();
    let mut xml = Vec::new();
    DeflateDecoder::new(compressed.as_slice())
        .read_to_end(&mut xml)
        .unwrap();
    Document::parse(&xml).unwrap()
}

#[tokio::test]
async fn the_settings_name_the_service_provider_urls() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = fixture.register().await;

    for licensed in [false, true] {
        if licensed {
            fixture.license(&admin).await;
        }
        let response = fixture.settings(&admin.token).await;
        response.assert_status(StatusCode::OK);
        assert_eq!(
            response.json_value()["service_provider"],
            json!({
                "entity_id": SERVICE_ENTITY_ID,
                "acs_url": ACS_URL,
                "metadata_url": SERVICE_ENTITY_ID,
            }),
            "licensed: {licensed}"
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_service_provider_metadata_is_public_xml() {
    let fixture = Fixture::new(PREFIX).await;

    let response = fixture.client.get(METADATA_PATH).await;

    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.header(header::CONTENT_TYPE.as_str()),
        Some(service_metadata::CONTENT_TYPE)
    );
    let body = response.text();
    assert_eq!(
        body,
        service_metadata::render(&ServiceProvider {
            entity_id: SERVICE_ENTITY_ID.to_owned(),
            acs_url: ACS_URL.to_owned(),
        })
    );
    assert!(
        body.contains(&format!("entityID=\"{SERVICE_ENTITY_ID}\"")),
        "{body}"
    );
    assert!(body.contains(&format!("Location=\"{ACS_URL}\"")), "{body}");
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_admin_adds_a_saml_provider_from_pasted_metadata() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = fixture.register().await;
    fixture.license(&admin).await;

    let response = fixture
        .create(
            &admin.token,
            &with(json!({
                "display_name": format!(" {DISPLAY_NAME} "),
                "metadata": testing::metadata(
                    testing::ENTITY_ID,
                    testing::SSO_URL,
                    &[testing::CERTIFICATE, testing::OTHER_CERTIFICATE],
                ),
                "allowed_domains": [" ACME.com", DOMAIN, "acme.io"],
                "enabled": false,
            })),
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let created = response.json_value();
    let id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    let mut expected = json!({
        "id": id,
        "protocol": "saml",
        "display_name": DISPLAY_NAME,
        "issuer": testing::ENTITY_ID,
        "client_id": null,
        "sso_url": testing::SSO_URL,
        "metadata_url": null,
        "certificate_fingerprints": [
            fingerprint(testing::CERTIFICATE),
            fingerprint(testing::OTHER_CERTIFICATE),
        ],
        "allowed_domains": [DOMAIN, "acme.io"],
        "require_verified_email": false,
        "enabled": false,
    });
    for timestamp in ["created_at", "updated_at"] {
        assert!(created[timestamp].is_string(), "{created}");
        expected[timestamp] = created[timestamp].clone();
    }
    assert_eq!(created, expected);
    assert_eq!(
        fixture.settings(&admin.token).await.json_value()["providers"],
        json!([expected])
    );

    let audits = fixture.audits(actions::SSO_PROVIDER_CREATED).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].resource_id, Some(id));
    assert_eq!(
        audits[0].new_values,
        Some(json!({
            "protocol": "saml",
            "display_name": DISPLAY_NAME,
            "issuer": testing::ENTITY_ID,
            "sso_url": testing::SSO_URL,
            "metadata_url": null,
            "certificate_fingerprints": [
                fingerprint(testing::CERTIFICATE),
                fingerprint(testing::OTHER_CERTIFICATE),
            ],
            "allowed_domains": [DOMAIN, "acme.io"],
            "require_verified_email": false,
            "enabled": false,
        }))
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_admin_adds_a_saml_provider_from_a_metadata_url() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = fixture.register().await;
    fixture.license(&admin).await;
    let identity_provider = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(IDP_METADATA_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    header::CONTENT_TYPE.as_str(),
                    service_metadata::CONTENT_TYPE,
                )
                .set_body_string(testing::metadata(
                    testing::ENTITY_ID,
                    testing::SSO_URL,
                    &[testing::CERTIFICATE],
                )),
        )
        .expect(1)
        .mount(&identity_provider)
        .await;
    let metadata_url = format!("{}{IDP_METADATA_PATH}", identity_provider.uri());

    let response = fixture
        .create(
            &admin.token,
            &with(json!({ "metadata": null, "metadata_url": metadata_url })),
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let created = response.json_value();
    assert_eq!(created["protocol"], "saml", "{created}");
    assert_eq!(created["issuer"], testing::ENTITY_ID, "{created}");
    assert_eq!(created["sso_url"], testing::SSO_URL, "{created}");
    assert_eq!(created["metadata_url"], json!(metadata_url), "{created}");
    assert_eq!(
        created["certificate_fingerprints"],
        json!([fingerprint(testing::CERTIFICATE)]),
        "{created}"
    );
    identity_provider.verify().await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_saml_provider_is_refused_with_the_code_of_what_is_wrong() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = fixture.register().await;
    fixture.license(&admin).await;
    let doctype = testing::metadata(
        testing::ENTITY_ID,
        testing::SSO_URL,
        &[testing::CERTIFICATE],
    )
    .replacen(
        "?>\n",
        "?>\n<!DOCTYPE md:EntityDescriptor [<!ENTITY idp \"https://idp.example.com\">]>\n",
        1,
    );
    let cases = [
        (
            "no domains",
            json!({ "allowed_domains": [] }),
            DOMAIN_REQUIRED,
        ),
        (
            "verified email",
            json!({ "require_verified_email": true }),
            VERIFIED_EMAIL_UNSUPPORTED,
        ),
        (
            "a client secret",
            json!({ "client_secret": "s3cret" }),
            PROTOCOL_MISMATCH,
        ),
        (
            "a client ID",
            json!({ "client_id": "zone" }),
            PROTOCOL_MISMATCH,
        ),
        (
            "an issuer",
            json!({ "issuer": testing::ENTITY_ID }),
            PROTOCOL_MISMATCH,
        ),
        (
            "malformed XML",
            json!({ "metadata": "<md:EntityDescriptor" }),
            METADATA_INVALID,
        ),
        (
            "a DOCTYPE",
            json!({ "metadata": doctype }),
            METADATA_INVALID,
        ),
        (
            "plain http to a remote host",
            json!({ "metadata": null, "metadata_url": "http://idp.example.com/metadata" }),
            METADATA_INVALID,
        ),
        (
            "both sources",
            json!({ "metadata_url": "https://idp.example.com/metadata" }),
            METADATA_INVALID,
        ),
        ("no source", json!({ "metadata": null }), METADATA_INVALID),
    ];

    for (case, overrides, code) in cases {
        let response = fixture.create(&admin.token, &with(overrides)).await;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{case}: {}",
            response.text()
        );
        assert_eq!(
            response.json_value()["code"],
            code,
            "{case}: {}",
            response.text()
        );
    }
    assert_eq!(fixture.providers().await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn settings_that_cannot_work_are_refused_before_the_metadata_is_fetched() {
    let server = Server::new().await;
    let identity_provider = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&identity_provider)
        .await;
    let metadata_url = format!("{}{IDP_METADATA_PATH}", identity_provider.uri());
    let cases = [
        (
            "no domains",
            json!({ "allowed_domains": [] }),
            DOMAIN_REQUIRED,
        ),
        (
            "verified email",
            json!({ "require_verified_email": true }),
            VERIFIED_EMAIL_UNSUPPORTED,
        ),
        (
            "no display name",
            json!({ "display_name": " " }),
            DISPLAY_NAME_INVALID,
        ),
    ];

    for (case, overrides, code) in &cases {
        let mut body = with(json!({ "metadata": null, "metadata_url": metadata_url }));
        for (key, value) in overrides.as_object().unwrap() {
            body[key] = value.clone();
        }
        let created = server.fixture.create(&server.admin.token, &body).await;
        assert_eq!(created.status, StatusCode::BAD_REQUEST, "{case}");
        assert_eq!(created.json_value()["code"], *code, "{case}");

        let mut revision = overrides.clone();
        revision["metadata_url"] = json!(metadata_url);
        let updated = server.update(&revision).await;
        assert_eq!(updated.status, StatusCode::BAD_REQUEST, "{case}");
        assert_eq!(updated.json_value()["code"], *code, "{case}");
    }
    assert_eq!(server.fixture.providers().await, 1);
    identity_provider.verify().await;
    server.cleanup().await;
}

#[tokio::test]
async fn an_update_cannot_cross_protocols() {
    let server = Server::new().await;
    let identity_provider = IdentityProvider::start().await;
    let mut empty_saml_fields = identity_provider.provider();
    empty_saml_fields["metadata_url"] = json!("");
    empty_saml_fields["metadata"] = json!("");
    let oidc = server
        .fixture
        .created(&server.admin.token, &empty_saml_fields)
        .await;

    for body in [
        json!({ "client_secret": "s3cret" }),
        json!({ "client_id": "zone" }),
        json!({ "issuer": testing::ENTITY_ID }),
    ] {
        refused(
            &server.update(&body).await,
            StatusCode::BAD_REQUEST,
            PROTOCOL_MISMATCH,
        );
    }
    refused(
        &server
            .update(&json!({ "require_verified_email": true }))
            .await,
        StatusCode::BAD_REQUEST,
        VERIFIED_EMAIL_UNSUPPORTED,
    );
    for body in [
        json!({ "metadata": testing::metadata(testing::ENTITY_ID, testing::SSO_URL, &[testing::CERTIFICATE]) }),
        json!({ "metadata_url": "https://idp.example.com/metadata" }),
    ] {
        refused(
            &server
                .fixture
                .update(&server.admin.token, oidc, &body)
                .await,
            StatusCode::BAD_REQUEST,
            PROTOCOL_MISMATCH,
        );
    }

    server
        .update(&json!({ "issuer": "", "client_id": "", "client_secret": "" }))
        .await
        .assert_status(StatusCode::OK);
    server
        .fixture
        .update(
            &server.admin.token,
            oidc,
            &json!({ "metadata_url": "", "metadata": "", "display_name": "Okta OIDC" }),
        )
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn the_same_entity_id_cannot_be_added_twice() {
    let server = Server::new().await;

    let again = server
        .fixture
        .create(
            &server.admin.token,
            &with(json!({ "display_name": "Okta again" })),
        )
        .await;

    refused(&again, StatusCode::CONFLICT, PROVIDER_DUPLICATE);
    assert_eq!(server.fixture.providers().await, 1);
    server.cleanup().await;
}

#[tokio::test]
async fn community_cannot_add_a_saml_provider() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = fixture.register().await;

    refused(
        &fixture.create(&admin.token, &saml_provider()).await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );
    assert_eq!(fixture.providers().await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_keeps_saml_working_and_allows_only_a_same_entity_metadata_refresh() {
    let server = Server::new().await;
    server.fixture.store_license(&lapsed()).await;

    let first = server.signed_in(unchanged).await;

    let rotated = server
        .update(&json!({
            "metadata": testing::metadata(
                testing::ENTITY_ID,
                testing::SSO_URL,
                &[testing::OTHER_CERTIFICATE],
            ),
        }))
        .await;
    rotated.assert_status(StatusCode::OK);
    assert_eq!(
        rotated.json_value()["certificate_fingerprints"],
        json!([fingerprint(testing::OTHER_CERTIFICATE)])
    );
    let again = server
        .signed_in(|options| Options {
            signing_key: Key::Other,
            ..options
        })
        .await;
    assert_eq!(again.user, first.user);

    for body in [
        json!({
            "metadata": testing::metadata(
                OTHER_ENTITY_ID,
                testing::SSO_URL,
                &[testing::OTHER_CERTIFICATE],
            ),
        }),
        json!({ "display_name": "Renamed" }),
    ] {
        refused(
            &server.update(&body).await,
            StatusCode::FORBIDDEN,
            LICENSE_REQUIRED,
        );
    }
    assert_eq!(server.identities().await.len(), 1);

    server
        .update(&json!({ "enabled": false }))
        .await
        .assert_status(StatusCode::OK);
    server
        .fixture
        .delete(&server.admin.token, server.provider)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(server.fixture.providers().await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn a_new_entity_id_unlinks_everyone_and_is_audited() {
    let server = Server::new().await;
    server.signed_in(unchanged).await;

    let moved = server
        .update(&json!({
            "metadata": testing::metadata(
                OTHER_ENTITY_ID,
                testing::SSO_URL,
                &[testing::CERTIFICATE],
            ),
        }))
        .await;

    moved.assert_status(StatusCode::OK);
    assert_eq!(moved.json_value()["issuer"], OTHER_ENTITY_ID);
    assert!(server.identities().await.is_empty());
    let updated = server.fixture.audits(actions::SSO_PROVIDER_UPDATED).await;
    assert_eq!(updated.len(), 1);
    let recorded = updated[0].new_values.clone().unwrap();
    assert_eq!(recorded["issuer"], OTHER_ENTITY_ID, "{recorded}");
    assert_eq!(recorded["identities_cleared"], 1, "{recorded}");

    let relinked = server
        .sign_in(|options| Options {
            issuer: OTHER_ENTITY_ID.to_owned(),
            ..options
        })
        .await;
    refused(&relinked, StatusCode::CONFLICT, EMAIL_CONFLICT);
    assert_eq!(server.users_with_email(EMAIL).await, 1);
    assert!(server.identities().await.is_empty());
    server.cleanup().await;
}

#[tokio::test]
async fn starting_a_saml_sign_in_sends_an_authn_request_over_the_redirect_binding() {
    let server = Server::new().await;

    let started = server.start().await;

    let mut endpoint = started.url.clone();
    endpoint.set_query(None);
    assert_eq!(endpoint.as_str(), testing::SSO_URL);
    let parameters: HashMap<String, String> = started.url.query_pairs().into_owned().collect();
    assert_eq!(parameters.len(), 2, "{parameters:?}");
    assert_eq!(parameters[RELAY_STATE], started.state);

    let request = authn_request(&started.url);
    let root = request.root();
    assert!(request.is(root, uri::PROTOCOL, "AuthnRequest"));
    let element = request.element(root).unwrap();
    assert_eq!(element.attribute("Destination"), Some(testing::SSO_URL));
    assert_eq!(
        element.attribute("AssertionConsumerServiceURL"),
        Some(ACS_URL)
    );
    assert_eq!(element.attribute("ProtocolBinding"), Some(uri::HTTP_POST));
    let issuer = request
        .children_named(root, uri::ASSERTION, "Issuer")
        .next()
        .unwrap();
    assert_eq!(request.text(issuer).as_deref(), Some(SERVICE_ENTITY_ID));
    assert_eq!(
        server.stored_request_id(&started.state).await.as_deref(),
        Some(started.request_id.as_str())
    );
    assert_ne!(started.request_id, started.state);
    server.cleanup().await;
}

#[tokio::test]
async fn a_first_saml_sign_in_provisions_an_unverified_account_through_the_handoff() {
    let server = Server::new().await;
    let started = server.start().await;

    let code = server.asserted(&started, unchanged).await;
    let response = server.callback(&started.state, &code).await;

    response.assert_status(StatusCode::OK);
    let session = session(&response);
    let body = response.json_value();
    assert_eq!(body["user"]["email"], EMAIL, "{body}");
    assert!(!server.email_verified(session.user).await);
    assert_eq!(
        server.identities().await,
        [(session.user, SUBJECT.to_owned(), EMAIL.to_owned())]
    );
    assert_eq!(server.attempts(&started.state).await, 0);
    server
        .fixture
        .client
        .get_auth("/api/auth/sessions", &session.access)
        .await
        .assert_status(StatusCode::OK);

    let created = server.fixture.audits(actions::USER_CREATED).await;
    let provisioned: Vec<_> = created
        .iter()
        .filter(|audit| audit.resource_id == Some(session.user))
        .collect();
    assert_eq!(provisioned.len(), 1);
    assert_eq!(
        provisioned[0].new_values,
        Some(json!({ "provider_id": server.provider }))
    );
    let signed_in = server.fixture.audits(actions::USER_SSO_SIGNED_IN).await;
    assert_eq!(signed_in.len(), 1);
    assert_eq!(
        signed_in[0].new_values,
        Some(json!({
            "provider_id": server.provider,
            "subject": SUBJECT,
            "provisioned": true,
            "linked": false,
            "reclaimed": false,
        }))
    );
    server.cleanup().await;
}

#[tokio::test]
async fn a_later_saml_sign_in_by_the_same_subject_reaches_the_same_account() {
    let server = Server::new().await;
    let first = server.signed_in(unchanged).await;

    let second = server
        .signed_in(|options| Options {
            sign_assertion: false,
            sign_response: true,
            ..options
        })
        .await;

    assert_eq!(second.user, first.user);
    assert_eq!(server.users_with_email(EMAIL).await, 1);
    let signed_in = server.fixture.audits(actions::USER_SSO_SIGNED_IN).await;
    let provisioned: Vec<_> = signed_in
        .iter()
        .map(|audit| audit.new_values.as_ref().unwrap()["provisioned"].clone())
        .collect();
    assert_eq!(provisioned, [json!(true), json!(false)]);
    server.cleanup().await;
}

#[tokio::test]
async fn a_replayed_assertion_is_refused() {
    let server = Server::new().await;
    let first = server.start().await;
    let replayed = unique("assertion");
    let assertion_id = replayed.clone();
    server
        .asserted(&first, |options| Options {
            assertion_id,
            ..options
        })
        .await;

    let second = server.start().await;
    let redirect = server
        .answer(&second, |options| Options {
            assertion_id: replayed,
            ..options
        })
        .await;

    assert_failed(&redirect, &second.state, ASSERTION_INVALID);
    assert_eq!(server.attempts(&second.state).await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn a_relay_state_is_asserted_only_once() {
    let server = Server::new().await;
    let started = server.start().await;
    server.asserted(&started, unchanged).await;

    let again = server.answer(&started, unchanged).await;

    assert_failed(&again, &started.state, STATE_INVALID);
    server.cleanup().await;
}

#[tokio::test]
async fn concurrent_posts_for_one_relay_state_check_only_one_response() {
    let server = Server::new().await;
    let started = server.start().await;
    let response = testing::response(&options(&started));

    let posted =
        join_all((0..CONCURRENT_POSTS).map(|_| server.acs(Some(&started.state), &response))).await;

    let redirects: Vec<Redirect> = posted.iter().map(redirected).collect();
    let (handed_off, refused): (Vec<&Redirect>, Vec<&Redirect>) = redirects
        .iter()
        .partition(|redirect| redirect.parameters.contains_key(CODE));
    assert_eq!(
        handed_off.len(),
        1,
        "{:?}",
        redirects
            .iter()
            .map(|redirect| &redirect.parameters)
            .collect::<Vec<_>>()
    );
    for redirect in refused {
        assert_failed(redirect, &started.state, STATE_INVALID);
    }
    server
        .callback(&started.state, &handed_off[0].parameters[CODE])
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(server.users_with_email(EMAIL).await, 1);
    server.cleanup().await;
}

#[tokio::test]
async fn a_sign_in_whose_post_was_dropped_mid_check_cannot_be_finished() {
    let server = Server::new().await;
    let started = server.start().await;
    attempts::claim(server.fixture.pool(), &started.state)
        .await
        .unwrap()
        .expect("a live request");

    let posted = server.answer(&started, unchanged).await;

    assert_failed(&posted, &started.state, STATE_INVALID);
    refused(
        &server.callback(&started.state, "a-guessed-code").await,
        StatusCode::BAD_REQUEST,
        STATE_INVALID,
    );
    assert_eq!(server.attempts(&started.state).await, 0);
    assert_eq!(server.users_with_email(EMAIL).await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn a_response_is_refused_unchecked_while_every_check_is_running() {
    let server = Server::new().await;
    let semaphore = Arc::clone(server.fixture.client.state().saml_semaphore());
    let permits = semaphore.available_permits();
    let started = server.start().await;
    let held = Arc::clone(&semaphore)
        .acquire_many_owned(u32::try_from(permits).unwrap())
        .await
        .unwrap();

    let busy = server.answer(&started, unchanged).await;

    assert_failed(&busy, &started.state, SIGN_IN_FAILED);
    assert_eq!(server.attempts(&started.state).await, 0);
    assert_eq!(server.users_with_email(EMAIL).await, 0);
    drop(held);
    assert_eq!(semaphore.available_permits(), permits);
    let retried = server.answer(&started, unchanged).await;
    assert_failed(&retried, &started.state, STATE_INVALID);

    server.signed_in(unchanged).await;
    assert_eq!(semaphore.available_permits(), permits);
    server.cleanup().await;
}

#[tokio::test]
async fn a_new_identity_provider_voids_the_sign_ins_already_asserted() {
    let server = Server::new().await;

    let rotated = server.start().await;
    let code = server.asserted(&rotated, unchanged).await;
    server
        .update(&json!({
            "metadata": testing::metadata(
                testing::ENTITY_ID,
                testing::SSO_URL,
                &[testing::OTHER_CERTIFICATE],
            ),
        }))
        .await
        .assert_status(StatusCode::OK);
    refused(
        &server.callback(&rotated.state, &code).await,
        StatusCode::BAD_REQUEST,
        STATE_INVALID,
    );

    let moved = server.start().await;
    let code = server
        .asserted(&moved, |options| Options {
            signing_key: Key::Other,
            ..options
        })
        .await;
    server
        .update(&json!({
            "metadata": testing::metadata(
                OTHER_ENTITY_ID,
                testing::SSO_URL,
                &[testing::OTHER_CERTIFICATE],
            ),
        }))
        .await
        .assert_status(StatusCode::OK);
    refused(
        &server.callback(&moved.state, &code).await,
        StatusCode::BAD_REQUEST,
        STATE_INVALID,
    );
    assert_eq!(server.users_with_email(EMAIL).await, 0);

    let renamed = server.start().await;
    let code = server
        .asserted(&renamed, |options| Options {
            issuer: OTHER_ENTITY_ID.to_owned(),
            signing_key: Key::Other,
            ..options
        })
        .await;
    server
        .update(&json!({ "display_name": "Okta SAML renamed" }))
        .await
        .assert_status(StatusCode::OK);
    server
        .callback(&renamed.state, &code)
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn a_wrong_handoff_code_is_refused_and_spends_the_sign_in() {
    let server = Server::new().await;
    let started = server.start().await;
    let code = server.asserted(&started, unchanged).await;

    refused(
        &server.callback(&started.state, "not-the-code").await,
        StatusCode::BAD_REQUEST,
        STATE_INVALID,
    );
    refused(
        &server.callback(&started.state, &code).await,
        StatusCode::BAD_REQUEST,
        STATE_INVALID,
    );
    assert_eq!(server.users_with_email(EMAIL).await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn an_invalid_response_is_refused_and_ends_the_attempt() {
    let server = Server::new().await;
    let past = Utc::now() - TimeDelta::hours(1);
    let cases: [(&str, Adjustment); 5] = [
        (
            "expired",
            Box::new(move |options| Options {
                issue_instant: past,
                not_before: past,
                not_on_or_after: past + TimeDelta::minutes(5),
                confirmation_not_on_or_after: past + TimeDelta::minutes(5),
                ..options
            }),
        ),
        (
            "for another service provider",
            Box::new(|options| Options {
                audiences: vec![ELSEWHERE.to_owned()],
                ..options
            }),
        ),
        (
            "unsigned",
            Box::new(|options| Options {
                sign_assertion: false,
                sign_response: false,
                ..options
            }),
        ),
        (
            "signed by a key the metadata doesn't pin",
            Box::new(|options| Options {
                signing_key: Key::Other,
                ..options
            }),
        ),
        (
            "for another request",
            Box::new(|options| Options {
                in_response_to: Some(unique("request")),
                confirmation_in_response_to: None,
                ..options
            }),
        ),
    ];

    for (case, adjust) in cases {
        let started = server.start().await;
        let redirect = server.answer(&started, adjust).await;
        assert_eq!(
            redirect.parameters,
            failure(&started.state, ASSERTION_INVALID),
            "{case}"
        );
        assert_eq!(server.attempts(&started.state).await, 0, "{case}");
        let retried = server.answer(&started, unchanged).await;
        assert_eq!(
            retried.parameters,
            failure(&started.state, STATE_INVALID),
            "{case}"
        );
    }

    let started = server.start().await;
    let redirect = redirected(
        &server
            .acs(Some(&started.state), &wrapped(&options(&started)))
            .await,
    );
    assert_failed(&redirect, &started.state, ASSERTION_INVALID);
    assert_eq!(server.attempts(&started.state).await, 0);

    assert_eq!(server.users_with_email(EMAIL).await, 0);
    assert_eq!(server.users_with_email(ATTACKER_EMAIL).await, 0);
    assert!(server.identities().await.is_empty());
    server.cleanup().await;
}

#[tokio::test]
async fn an_identity_provider_error_is_a_refused_sign_in() {
    let server = Server::new().await;
    let started = server.start().await;

    let redirect = server
        .answer(&started, |options| Options {
            status: RESPONDER.to_owned(),
            sign_assertion: false,
            sign_response: false,
            ..options
        })
        .await;

    assert_failed(&redirect, &started.state, SIGN_IN_REFUSED);
    assert_eq!(server.attempts(&started.state).await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn a_response_zone_did_not_ask_for_is_refused() {
    let server = Server::new().await;
    let started = server.start().await;
    let response = testing::response(&options(&started));

    let unsolicited = redirected(&server.acs(None, &response).await);
    assert_eq!(unsolicited.target, CALLBACK_URL);
    assert_eq!(
        unsolicited.parameters,
        HashMap::from([(ERROR.to_owned(), STATE_INVALID.to_owned())])
    );

    let unknown = redirected(&server.acs(Some("never-issued"), &response).await);
    assert_failed(&unknown, "never-issued", STATE_INVALID);

    server.asserted(&started, unchanged).await;
    server.cleanup().await;
}

#[tokio::test]
async fn an_oversized_post_or_relay_state_is_refused_without_a_state() {
    let server = Server::new().await;
    let started = server.start().await;
    let state_less = HashMap::from([(ERROR.to_owned(), STATE_INVALID.to_owned())]);

    let padding = "A".repeat(BODY_LIMIT);
    let oversized = redirected(&server.acs(Some(&started.state), &padding).await);
    assert_eq!(oversized.target, CALLBACK_URL);
    assert_eq!(oversized.parameters, state_less);

    let long = format!("{}{}", started.state, "s".repeat(RELAY_STATE_LIMIT));
    let response = testing::response(&options(&started));
    let unbounded = redirected(&server.acs(Some(&long), &response).await);
    assert_eq!(unbounded.target, CALLBACK_URL);
    assert_eq!(unbounded.parameters, state_less);

    server.asserted(&started, unchanged).await;
    server.cleanup().await;
}

#[tokio::test]
async fn a_disabled_provider_cannot_start_or_finish_a_saml_sign_in() {
    let server = Server::new().await;
    let started = server.start().await;
    server
        .update(&json!({ "enabled": false }))
        .await
        .assert_status(StatusCode::OK);

    let redirect = server.answer(&started, unchanged).await;

    assert_failed(&redirect, &started.state, PROVIDER_NOT_FOUND);
    refused(
        &server.authorize().await,
        StatusCode::NOT_FOUND,
        PROVIDER_NOT_FOUND,
    );
    assert_eq!(server.users_with_email(EMAIL).await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn an_existing_account_with_the_email_is_never_linked() {
    let server = Server::new().await;
    server
        .fixture
        .register_as(EMAIL)
        .await
        .assert_status(StatusCode::CREATED);
    sqlx::query("UPDATE users SET email_verified = true WHERE lower(email) = lower($1)")
        .bind(EMAIL)
        .execute(server.fixture.pool())
        .await
        .unwrap();

    refused(
        &server.sign_in(unchanged).await,
        StatusCode::CONFLICT,
        EMAIL_CONFLICT,
    );

    assert!(server.identities().await.is_empty());
    assert_eq!(server.users_with_email(EMAIL).await, 1);
    server
        .fixture
        .login(EMAIL, &test_password())
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn an_email_outside_the_allowed_domains_is_refused() {
    let server = Server::new().await;
    let started = server.start().await;

    let redirect = server
        .answer(&started, as_person(SUBJECT, "jane@globex.com".to_owned()))
        .await;

    assert_failed(&redirect, &started.state, DOMAIN_NOT_ALLOWED);
    assert_eq!(server.users_with_email("jane@globex.com").await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn enforcement_with_only_a_saml_provider_keeps_the_break_glass_admin() {
    let server = Server::new().await;
    let member = server.fixture.register().await;
    server.enforce().await;

    refused(
        &server.fixture.login(&member.email, &test_password()).await,
        StatusCode::FORBIDDEN,
        SSO_REQUIRED,
    );
    server
        .fixture
        .login(&server.admin.email, &test_password())
        .await
        .assert_status(StatusCode::OK);
    let audits = server
        .fixture
        .audits(actions::USER_BREAK_GLASS_SIGNED_IN)
        .await;
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].actor_id, Some(server.admin.id));

    server.signed_in(unchanged).await;
    server.cleanup().await;
}

#[tokio::test]
async fn disabling_a_saml_provider_ends_the_sessions_signed_in_through_it() {
    let server = Server::new().await;
    let signed_in = server.signed_in(unchanged).await;
    let other = server
        .signed_in(as_person(
            "second-person",
            format!("second-{}@{DOMAIN}", Uuid::new_v4().simple()),
        ))
        .await;

    server
        .update(&json!({ "enabled": false }))
        .await
        .assert_status(StatusCode::OK);

    for ended in [&signed_in, &other] {
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
