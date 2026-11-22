//! Single sign-on reaching an account SCIM provisioned. An identity provider that doesn't vouch
//! for email, as SAML never does, links by email only to a pristine SCIM account: one that still
//! has SCIM's unusable password, has never signed in and has no single sign-on identity. Anything
//! else with the same email stays an email conflict.

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flate2::read::DeflateDecoder;
use reqwest::Url;
use serde_json::json;
use uuid::Uuid;
use zone_enterprise::sso::saml::Document;
use zone_enterprise::sso::saml::testing::{self, Options};
use zone_enterprise::sso::testing::{EMAIL, SUBJECT};
use zone_enterprise::sso::{Claims, EMAIL_CONFLICT};
use zone_server::auth::hash_password;
use zone_server::db::audit::actions;
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::password_reset;
use zone_server::db::scim::{Link, NewToken, tokens, users};
use zone_server::services::account::provision;
use zone_server::services::scim::secret;
use zone_server::utils::crypto::generate_token;

use crate::common::identity_provider::IdentityProvider;
use crate::common::scim::{self, CONTENT_TYPE, USER_SCHEMA};
use crate::common::sso::Fixture;
use crate::common::{TestResponse, test_password};

const PREFIX: &str = "sso_scim_link";
const PUBLIC_PROVIDERS: &str = "/api/auth/sso/providers";
const CALLBACKS: &str = "/api/auth/sso/callbacks";
const ACS_PATH: &str = "/api/auth/sso/saml/acs";
const SERVICE_ENTITY_ID: &str = "http://localhost:3000/api/auth/sso/saml/metadata";
const ACS_URL: &str = "http://localhost:3000/api/auth/sso/saml/acs";
const DOMAIN: &str = "acme.com";
const OTHER_EMAIL: &str = "grace@acme.com";
const OTHER_SUBJECT: &str = "00u-other-subject";
const NEW_PASSWORD: &str = "Chosen-by-jane-7";
const FORM: &str = "application/x-www-form-urlencoded";
const SAML_REQUEST: &str = "SAMLRequest";
const SAML_RESPONSE: &str = "SAMLResponse";
const RELAY_STATE: &str = "RelayState";
const LOCK_WAIT: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(20);
const TOKEN_NAME: &str = "Identity provider";

/// Where starting a sign-in sends the browser, and the state the console keeps.
struct Authorization {
    url: Url,
    state: String,
}

/// A started SAML sign-in: the state the console keeps and the ID of the AuthnRequest.
struct Started {
    state: String,
    request_id: String,
}

struct Server {
    fixture: Fixture,
    organization: Uuid,
    oidc: Uuid,
    saml: Uuid,
    identity_provider: IdentityProvider,
}

impl Server {
    /// An Enterprise server with one OIDC and one SAML provider, and an organization SCIM
    /// provisions into.
    async fn new() -> Self {
        let fixture = Fixture::new(PREFIX).await;
        let admin = fixture.register().await;
        fixture.license(&admin).await;
        let identity_provider = IdentityProvider::start().await;
        let oidc = fixture
            .created(&admin.token, &identity_provider.provider())
            .await;
        let saml = fixture
            .created(
                &admin.token,
                &json!({
                    "protocol": "saml",
                    "display_name": "Okta SAML",
                    "metadata": testing::metadata(
                        testing::ENTITY_ID,
                        testing::SSO_URL,
                        &[testing::CERTIFICATE],
                    ),
                    "allowed_domains": [DOMAIN],
                }),
            )
            .await;
        let response = fixture
            .client
            .post_json_auth(
                "/api/organizations",
                &json!({ "name": "Provisioned", "slug": format!("provisioned-{}", Uuid::new_v4()) }),
                &admin.token,
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        let organization = response.json_value()["organization"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        Self {
            fixture,
            organization,
            oidc,
            saml,
            identity_provider,
        }
    }

    /// An account as SCIM creates one: SCIM's own unusable password, an unverified email, no
    /// organization of its own, a membership and the link that records the password SCIM set.
    async fn provisioned(&self, email: &str) -> Uuid {
        let password_hash = hash_password(&generate_token()).unwrap();
        let mut transaction = self.fixture.pool().begin().await.unwrap();
        let user = provision::insert(&mut transaction, email, &password_hash, None, false, false)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        organization_members::add_member(
            self.fixture.pool(),
            self.organization,
            user.id,
            OrgRole::Member,
            None,
        )
        .await
        .unwrap();
        let mut link = Link::new(self.organization, user.id, email);
        link.provisioned_password = Some(password_hash);
        users::upsert(self.fixture.pool(), &link).await.unwrap();
        user.id
    }

    /// An account an identity provider created through `POST /Users`, with a token of its own.
    async fn pushed(&self, email: &str) -> Uuid {
        let secret = secret::generate();
        tokens::create(
            self.fixture.pool(),
            &NewToken {
                organization_id: self.organization,
                name: TOKEN_NAME,
                token_hash: &secret::hash(&secret),
                hint: &secret::hint(&secret),
                created_by: None,
            },
        )
        .await
        .unwrap();
        let body = json!({
            "schemas": [USER_SCHEMA],
            "userName": email,
            "emails": [{ "value": email, "type": "work", "primary": true }],
            "active": true,
        });
        let response = self
            .fixture
            .client
            .send_request(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("{}/Users", scim::base(self.organization)))
                    .header(header::CONTENT_TYPE, CONTENT_TYPE)
                    .header(header::AUTHORIZATION, format!("Bearer {secret}"))
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        response.json_value()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    async fn authorize(&self, provider: Uuid) -> Authorization {
        let response = self
            .fixture
            .client
            .post_json(
                &format!("{PUBLIC_PROVIDERS}/{provider}/authorizations"),
                &json!({}),
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        let body = response.json_value();
        Authorization {
            url: Url::parse(body["url"].as_str().unwrap()).unwrap(),
            state: body["state"].as_str().unwrap().to_owned(),
        }
    }

    async fn callback(&self, state: &str, code: &str) -> TestResponse {
        self.fixture
            .client
            .post_json(CALLBACKS, &json!({ "state": state, "code": code }))
            .await
    }

    async fn oidc_sign_in(&self, subject: &str, email: &str) -> TestResponse {
        let authorization = self.authorize(self.oidc).await;
        let parameters: HashMap<String, String> =
            authorization.url.query_pairs().into_owned().collect();
        let claims = Claims {
            subject: subject.to_owned(),
            email: Some(email.to_owned()),
            ..self.identity_provider.claims(&parameters["nonce"])
        };
        let code = format!("code-{}", Uuid::new_v4().simple());
        self.identity_provider
            .grant(&code, &parameters["code_challenge"], claims);
        self.callback(&authorization.state, &code).await
    }

    async fn saml_start(&self) -> Started {
        let authorization = self.authorize(self.saml).await;
        let parameters: HashMap<String, String> =
            authorization.url.query_pairs().into_owned().collect();
        let compressed = STANDARD.decode(&parameters[SAML_REQUEST]).unwrap();
        let mut xml = Vec::new();
        DeflateDecoder::new(compressed.as_slice())
            .read_to_end(&mut xml)
            .unwrap();
        let request = Document::parse(&xml).unwrap();
        let request_id = request
            .element(request.root())
            .unwrap()
            .attribute("ID")
            .unwrap()
            .to_owned();
        Started {
            state: authorization.state,
            request_id,
        }
    }

    /// Posts the identity provider's signed answer for `subject` and `email` to the ACS and
    /// returns the code it hands the console.
    async fn saml_assert(&self, started: &Started, subject: &str, email: &str) -> String {
        let response = testing::response(&Options {
            response_id: format!("_response-{}", Uuid::new_v4().simple()),
            assertion_id: format!("_assertion-{}", Uuid::new_v4().simple()),
            destination: Some(ACS_URL.to_owned()),
            in_response_to: Some(started.request_id.clone()),
            confirmation_in_response_to: Some(started.request_id.clone()),
            recipient: ACS_URL.to_owned(),
            audiences: vec![SERVICE_ENTITY_ID.to_owned()],
            name_id: subject.to_owned(),
            email: Some(email.to_owned()),
            ..Options::default()
        });
        let mut form = form_urlencoded::Serializer::new(String::new());
        form.append_pair(SAML_RESPONSE, &response);
        form.append_pair(RELAY_STATE, &started.state);
        let posted = self
            .fixture
            .client
            .send_request(
                Request::builder()
                    .method("POST")
                    .uri(ACS_PATH)
                    .header(header::CONTENT_TYPE, FORM)
                    .body(Body::from(form.finish()))
                    .unwrap(),
            )
            .await;
        posted.assert_status(StatusCode::SEE_OTHER);
        let location = Url::parse(posted.header(header::LOCATION.as_str()).unwrap()).unwrap();
        let parameters: HashMap<String, String> = location.query_pairs().into_owned().collect();
        assert_eq!(parameters["state"], started.state);
        parameters["code"].clone()
    }

    async fn saml_sign_in(&self, subject: &str, email: &str) -> TestResponse {
        let started = self.saml_start().await;
        let code = self.saml_assert(&started, subject, email).await;
        self.callback(&started.state, &code).await
    }

    async fn reset_password(&self, user: Uuid) {
        let (token, _) = password_reset::create_reset_token(self.fixture.pool(), user)
            .await
            .unwrap();
        self.fixture
            .client
            .post_json(
                "/api/auth/reset-password",
                &json!({ "token": token, "new_password": NEW_PASSWORD }),
            )
            .await
            .assert_status(StatusCode::OK);
    }

    async fn identities(&self) -> Vec<(Uuid, Uuid, String)> {
        sqlx::query_as(
            "SELECT provider_id, user_id, subject FROM sso_identities ORDER BY created_at",
        )
        .fetch_all(self.fixture.pool())
        .await
        .unwrap()
    }

    async fn password_hash(&self, user: Uuid) -> String {
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(self.fixture.pool())
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

    /// Waits until some statement in this server's database is queued behind a row lock.
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
        .expect("the sign-in queued behind the held lock");
    }

    async fn cleanup(self) {
        self.fixture.cleanup().await;
    }
}

fn signed_in(response: &TestResponse) -> Uuid {
    response.assert_status(StatusCode::OK);
    response.json_value()["user"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

fn conflicted(response: &TestResponse) {
    response.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        response.json_value()["code"],
        json!(EMAIL_CONFLICT),
        "{}",
        response.text()
    );
}

#[tokio::test]
async fn a_pristine_scim_account_signs_in_over_oidc_as_itself_without_being_reclaimed() {
    let server = Server::new().await;
    let user = server.provisioned(EMAIL).await;
    let password_hash = server.password_hash(user).await;

    assert_eq!(signed_in(&server.oidc_sign_in(SUBJECT, EMAIL).await), user);

    assert_eq!(server.users_with_email(EMAIL).await, 1);
    assert_eq!(server.password_hash(user).await, password_hash);
    assert_eq!(
        server.identities().await,
        vec![(server.oidc, user, SUBJECT.to_owned())]
    );
    let audits = server.fixture.audits(actions::USER_SSO_SIGNED_IN).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(
        audits[0].new_values.as_ref().unwrap()["linked"],
        json!(true)
    );
    assert_eq!(
        audits[0].new_values.as_ref().unwrap()["reclaimed"],
        json!(false)
    );
    assert_eq!(signed_in(&server.oidc_sign_in(SUBJECT, EMAIL).await), user);
    server.cleanup().await;
}

#[tokio::test]
async fn a_pristine_scim_account_signs_in_over_saml_as_itself() {
    let server = Server::new().await;
    let user = server.provisioned(EMAIL).await;

    assert_eq!(signed_in(&server.saml_sign_in(SUBJECT, EMAIL).await), user);

    assert_eq!(server.users_with_email(EMAIL).await, 1);
    assert_eq!(
        server.identities().await,
        vec![(server.saml, user, SUBJECT.to_owned())]
    );
    assert_eq!(signed_in(&server.saml_sign_in(SUBJECT, EMAIL).await), user);
    server.cleanup().await;
}

#[tokio::test]
async fn an_account_pushed_through_scim_signs_in_over_saml_as_itself() {
    let server = Server::new().await;
    let user = server.pushed(EMAIL).await;

    assert_eq!(signed_in(&server.saml_sign_in(SUBJECT, EMAIL).await), user);

    assert_eq!(server.users_with_email(EMAIL).await, 1);
    assert_eq!(
        server.identities().await,
        vec![(server.saml, user, SUBJECT.to_owned())]
    );
    server.cleanup().await;
}

#[tokio::test]
async fn pristine_scim_accounts_reach_themselves_over_either_protocol_and_link_only_once() {
    let server = Server::new().await;
    let jane = server.provisioned(EMAIL).await;
    let grace = server.provisioned(OTHER_EMAIL).await;

    assert_eq!(signed_in(&server.oidc_sign_in(SUBJECT, EMAIL).await), jane);
    assert_eq!(
        signed_in(&server.saml_sign_in(OTHER_SUBJECT, OTHER_EMAIL).await),
        grace
    );

    conflicted(&server.saml_sign_in(SUBJECT, EMAIL).await);
    conflicted(&server.oidc_sign_in(OTHER_SUBJECT, OTHER_EMAIL).await);
    assert_eq!(server.identities().await.len(), 2);
    server.cleanup().await;
}

#[tokio::test]
async fn after_a_password_reset_saml_no_longer_links_the_scim_account() {
    let server = Server::new().await;
    let user = server.provisioned(EMAIL).await;

    server.reset_password(user).await;

    conflicted(&server.saml_sign_in(SUBJECT, EMAIL).await);
    assert!(server.identities().await.is_empty());
    server
        .fixture
        .login(EMAIL, NEW_PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn after_any_sign_in_saml_no_longer_links_the_scim_account() {
    let server = Server::new().await;
    let user = server.provisioned(EMAIL).await;
    sqlx::query("UPDATE users SET last_login_at = NOW() WHERE id = $1")
        .bind(user)
        .execute(server.fixture.pool())
        .await
        .unwrap();

    conflicted(&server.saml_sign_in(SUBJECT, EMAIL).await);
    assert!(server.identities().await.is_empty());
    server.cleanup().await;
}

#[tokio::test]
async fn a_local_password_account_with_the_email_still_conflicts_over_saml() {
    let server = Server::new().await;
    let registered = server.fixture.register_as(EMAIL).await;
    registered.assert_status(StatusCode::CREATED);
    let user: Uuid = registered.json_value()["user"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    conflicted(&server.saml_sign_in(SUBJECT, EMAIL).await);

    organization_members::add_member(
        server.fixture.pool(),
        server.organization,
        user,
        OrgRole::Member,
        None,
    )
    .await
    .unwrap();
    users::upsert(
        server.fixture.pool(),
        &Link::new(server.organization, user, EMAIL),
    )
    .await
    .unwrap();
    conflicted(&server.saml_sign_in(SUBJECT, EMAIL).await);

    assert!(server.identities().await.is_empty());
    server
        .fixture
        .login(EMAIL, &test_password())
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn a_password_reset_racing_a_pristine_link_leaves_the_account_unlinked() {
    let server = Server::new().await;
    let user = server.provisioned(EMAIL).await;
    let started = server.saml_start().await;
    let code = server.saml_assert(&started, SUBJECT, EMAIL).await;
    let chosen = hash_password(NEW_PASSWORD).unwrap();
    let mut reset = server.fixture.pool().begin().await.unwrap();
    sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
        .bind(user)
        .bind(&chosen)
        .execute(&mut *reset)
        .await
        .unwrap();

    let (response, ()) = tokio::join!(server.callback(&started.state, &code), async {
        server.queued_behind_a_lock().await;
        reset.commit().await.unwrap();
    });

    conflicted(&response);
    assert!(server.identities().await.is_empty());
    assert_eq!(server.password_hash(user).await, chosen);
    server.cleanup().await;
}
