//! The single sign-on storage: providers with sealed client secrets or pinned SAML certificates,
//! the identities linked through them, single-use sign-in attempts, the SAML assertions already
//! seen and the enforcement setting.

use abnegate_secret::{MasterKey, SecretValue};
use chrono::{DateTime, TimeDelta, Utc};
use futures::future::join_all;
use reqwest::Url;
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::sso::saml::{self, Certificate};
use zone_enterprise::sso::{Authorization, ClientId, Fields, Protocol, Provider};
use zone_server::auth::hash_password;
use zone_server::db::sso::{
    Asserted, Connection, NewConnection, NewProvider, ProviderChanges, ProviderError,
    ProviderRecord, assertions, attempts, identities, providers, settings,
};
use zone_server::db::users::{self, UserRow};
use zone_server::db::{migrations, sealing::Column};
use zone_server::services::account::provision;
use zone_server::utils::crypto::hash_token;

use crate::common::Database;

const PREFIX: &str = "sso_store";
const ENVELOPE_OPENING: &str = "ENC[v1:";
const SECRET: &str = "s3cret-client-value";
const REPLACED_SECRET: &str = "r0tated-client-value";
const ISSUER: &str = "https://acme.okta.com";
const OTHER_ISSUER: &str = "https://login.example.test";
const AUTHORIZATION_ENDPOINT: &str = "https://acme.okta.com/oauth2/v1/authorize";
const REDIRECT_URI: &str = "https://zone.example.com/sso/callback";
const PASSWORD: &str = "Correct-horse-1";
const UNIQUE_VIOLATION: &str = "23505";
const CHECK_VIOLATION: &str = "23514";
const CONNECTION_CHECK: &str = "sso_providers_connection_check";
const PROTOCOL_CHECK: &str = "sso_providers_protocol_check";
const FLOW_CHECK: &str = "sso_attempts_flow_check";
const ASSERTED_CHECK: &str = "sso_attempts_asserted_check";
const CLAIMED_CHECK: &str = "sso_attempts_claimed_check";
const REQUEST_ID_KEY: &str = "sso_attempts_request_id_key";
const OIDC: &str = "oidc";
const SAML: &str = "saml";
const ENTITY_ID: &str = "http://www.okta.com/exk1zone";
const OTHER_ENTITY_ID: &str = "https://sts.windows.net/tenant/";
const SSO_URL: &str = "https://acme.okta.com/app/zone/exk1zone/sso/saml";
const OTHER_SSO_URL: &str = "https://login.microsoftonline.com/tenant/saml2";
const METADATA_URL: &str = "https://acme.okta.com/app/exk1zone/sso/saml/metadata";
const CERTIFICATE: &str = "MIIDazCCAlOgAwIBAgIUZone";
const ROTATED_CERTIFICATE: &str = "MIIDazCCAlOgAwIBAgIURotated";
const REQUEST_ID: &str = "_8f2c1d0e9b7a";
const CLAIMANTS: usize = 16;
const HANDOFF_CODE: &str = "one-time-handoff-code";
const SUBJECT: &str = "00u1ada";
const EMAIL: &str = "ada@acme.com";
const ASSERTION_ID: &str = "_assertion-1";

/// A provider row written straight to the table, so the constraints are checked on their own.
struct RawProvider {
    protocol: &'static str,
    issuer: &'static str,
    client_id: Option<&'static str>,
    client_secret: Option<&'static str>,
    sso_url: Option<&'static str>,
    certificates: Option<Vec<Option<String>>>,
    metadata_url: Option<&'static str>,
    allowed_domains: Vec<String>,
    require_verified_email: bool,
}

impl RawProvider {
    fn saml(issuer: &'static str) -> Self {
        Self {
            protocol: SAML,
            issuer,
            client_id: None,
            client_secret: None,
            sso_url: Some(SSO_URL),
            certificates: Some(vec![Some(CERTIFICATE.into())]),
            metadata_url: Some(METADATA_URL),
            allowed_domains: vec!["acme.com".into()],
            require_verified_email: false,
        }
    }

    fn oidc(issuer: &'static str) -> Self {
        Self {
            protocol: OIDC,
            issuer,
            client_id: Some("0oa1"),
            client_secret: Some("ENC[v1:sealed]"),
            sso_url: None,
            certificates: None,
            metadata_url: None,
            allowed_domains: Vec::new(),
            require_verified_email: true,
        }
    }
}

struct Fixture {
    database: Database,
    key: MasterKey,
}

impl Fixture {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        Self {
            database,
            key: MasterKey::generate().expect("a key"),
        }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn provider(&self, client_id: &str) -> ProviderRecord {
        providers::create(self.pool(), &self.key, &new_provider(ISSUER, client_id))
            .await
            .unwrap()
    }

    async fn insert(&self, raw: &RawProvider) -> Result<Uuid, sqlx::Error> {
        sqlx::query_scalar(
            "INSERT INTO sso_providers \
               (protocol, display_name, issuer, client_id, client_secret, sso_url, certificates, \
                metadata_url, allowed_domains, require_verified_email) \
             VALUES ($1, 'Okta', $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
        )
        .bind(raw.protocol)
        .bind(raw.issuer)
        .bind(raw.client_id)
        .bind(raw.client_secret)
        .bind(raw.sso_url)
        .bind(&raw.certificates)
        .bind(raw.metadata_url)
        .bind(&raw.allowed_domains)
        .bind(raw.require_verified_email)
        .fetch_one(self.pool())
        .await
    }

    async fn refused(&self, raw: &RawProvider, constraint: &str) {
        let error = self.insert(raw).await.unwrap_err();
        assert_eq!(
            check_constraint(&error).as_deref(),
            Some(constraint),
            "{error}"
        );
    }

    async fn user(&self, email: &str) -> UserRow {
        let mut connection = self.pool().acquire().await.unwrap();
        provision::insert(
            &mut connection,
            email,
            &hash_password(PASSWORD).unwrap(),
            None,
            false,
            false,
        )
        .await
        .unwrap()
    }

    async fn stored_secret(&self, id: Uuid) -> String {
        sqlx::query_scalar("SELECT client_secret FROM sso_providers WHERE id = $1")
            .bind(id)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn new_provider(issuer: &str, client_id: &str) -> NewProvider {
    NewProvider {
        protocol: Protocol::Oidc,
        display_name: "Okta".into(),
        issuer: issuer.into(),
        connection: NewConnection::Oidc {
            client_id: client_id.into(),
            client_secret: SecretValue::new(SECRET),
        },
        allowed_domains: vec!["acme.com".into()],
        require_verified_email: true,
        enabled: true,
    }
}

fn new_saml_provider(entity_id: &str, metadata_url: Option<&str>) -> NewProvider {
    NewProvider {
        protocol: Protocol::Saml,
        display_name: "Okta".into(),
        issuer: entity_id.into(),
        connection: NewConnection::Saml {
            sso_url: SSO_URL.into(),
            certificates: vec![CERTIFICATE.into()],
            metadata_url: metadata_url.map(str::to_owned),
        },
        allowed_domains: vec!["acme.com".into()],
        require_verified_email: false,
        enabled: true,
    }
}

fn authorization() -> Authorization {
    Authorization::new(
        &Url::parse(AUTHORIZATION_ENDPOINT).unwrap(),
        &ClientId::parse("0oa1").unwrap(),
        REDIRECT_URI,
    )
}

fn check_constraint(error: &sqlx::Error) -> Option<String> {
    let database = error.as_database_error()?;
    assert_eq!(database.code().as_deref(), Some(CHECK_VIOLATION), "{error}");
    database.constraint().map(str::to_owned)
}

fn unique_constraint(error: &sqlx::Error) -> Option<String> {
    let database = error.as_database_error()?;
    assert_eq!(
        database.code().as_deref(),
        Some(UNIQUE_VIOLATION),
        "{error}"
    );
    database.constraint().map(str::to_owned)
}

#[tokio::test]
async fn a_provider_round_trips_with_its_secret_sealed_in_the_row() {
    let fixture = Fixture::new().await;

    let created = fixture.provider("0oa1").await;
    assert_eq!(created.protocol, Protocol::Oidc);
    assert_eq!(created.display_name, "Okta");
    assert_eq!(created.issuer, ISSUER);
    assert_eq!(created.client_id(), Some("0oa1"));
    assert_eq!(
        created.client_secret(&fixture.key).unwrap().expose(),
        SECRET
    );
    assert_eq!(created.allowed_domains, ["acme.com"]);
    assert!(created.require_verified_email);
    assert!(created.enabled);

    let stored = fixture.stored_secret(created.id).await;
    assert!(stored.starts_with(ENVELOPE_OPENING), "{stored}");
    assert!(
        !stored.contains(SECRET),
        "the plaintext secret must never be stored"
    );

    let fetched = providers::get(fixture.pool(), created.id).await.unwrap();
    assert_eq!(fetched.as_ref(), Some(&created));
    assert_eq!(
        providers::list(fixture.pool()).await.unwrap(),
        std::slice::from_ref(&created)
    );
    assert_eq!(
        providers::get(fixture.pool(), Uuid::new_v4())
            .await
            .unwrap(),
        None
    );

    let other = MasterKey::generate().unwrap();
    assert!(matches!(
        created.client_secret(&other),
        Err(ProviderError::Unopenable(id, _)) if id == created.id
    ));
    assert!(!format!("{created:?}").contains(SECRET));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_stored_provider_parses_back_into_the_provider_it_was_saved_from() {
    let fixture = Fixture::new().await;
    let domains = vec!["ACME.com".to_owned(), "acme.com".to_owned()];
    let provider = Provider::parse(&Fields {
        protocol: Protocol::Oidc,
        display_name: " Okta ",
        issuer: ISSUER,
        client_id: "0oa1",
        client_secret: SECRET,
        allowed_domains: &domains,
        require_verified_email: false,
    })
    .unwrap();

    let created = providers::create(
        fixture.pool(),
        &fixture.key,
        &NewProvider::new(&provider, false),
    )
    .await
    .unwrap();

    assert_eq!(created.display_name, "Okta");
    assert_eq!(created.allowed_domains, ["acme.com"]);
    assert!(!created.require_verified_email);
    assert!(!created.enabled);
    let secret = created.client_secret(&fixture.key).unwrap();
    assert_eq!(
        Provider::parse(&created.fields(&secret).unwrap()).unwrap(),
        provider
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_sparse_update_keeps_every_field_it_leaves_out() {
    let fixture = Fixture::new().await;
    let created = fixture.provider("0oa1").await;
    let sealed = fixture.stored_secret(created.id).await;

    let renamed = providers::update(
        fixture.pool(),
        &fixture.key,
        created.id,
        &ProviderChanges {
            display_name: Some("Acme Okta".into()),
            enabled: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .expect("the provider exists");

    assert_eq!(renamed.display_name, "Acme Okta");
    assert!(!renamed.enabled);
    assert_eq!(renamed.issuer, created.issuer);
    assert_eq!(renamed.connection, created.connection);
    assert_eq!(renamed.allowed_domains, created.allowed_domains);
    assert_eq!(
        renamed.require_verified_email,
        created.require_verified_email
    );
    assert_eq!(
        renamed.client_secret(&fixture.key).unwrap().expose(),
        SECRET
    );
    assert_eq!(
        fixture.stored_secret(created.id).await,
        sealed,
        "an update without a secret leaves the sealed value byte for byte"
    );
    assert!(renamed.updated_at >= created.updated_at);

    let unchanged = providers::update(
        fixture.pool(),
        &fixture.key,
        created.id,
        &ProviderChanges::default(),
    )
    .await
    .unwrap();
    assert_eq!(unchanged, Some(renamed.clone()));

    let replaced = providers::update(
        fixture.pool(),
        &fixture.key,
        created.id,
        &ProviderChanges {
            client_secret: Some(SecretValue::new(REPLACED_SECRET)),
            allowed_domains: Some(Vec::new()),
            require_verified_email: Some(false),
            issuer: Some(OTHER_ISSUER.into()),
            client_id: Some("client-2".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .expect("the provider exists");
    assert_eq!(
        replaced.client_secret(&fixture.key).unwrap().expose(),
        REPLACED_SECRET
    );
    assert_eq!(replaced.issuer, OTHER_ISSUER);
    assert_eq!(replaced.client_id(), Some("client-2"));
    assert!(replaced.allowed_domains.is_empty());
    assert!(!replaced.require_verified_email);
    let resealed = fixture.stored_secret(created.id).await;
    assert!(resealed.starts_with(ENVELOPE_OPENING), "{resealed}");
    assert!(!resealed.contains(REPLACED_SECRET));

    assert_eq!(
        providers::update(
            fixture.pool(),
            &fixture.key,
            Uuid::new_v4(),
            &ProviderChanges {
                enabled: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap(),
        None
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_same_issuer_and_client_id_cannot_be_saved_twice() {
    let fixture = Fixture::new().await;
    fixture.provider("0oa1").await;
    let second = fixture.provider("0oa2").await;

    assert!(matches!(
        providers::create(fixture.pool(), &fixture.key, &new_provider(ISSUER, "0oa1")).await,
        Err(ProviderError::Duplicate)
    ));
    assert!(matches!(
        providers::update(
            fixture.pool(),
            &fixture.key,
            second.id,
            &ProviderChanges {
                client_id: Some("0oa1".into()),
                ..Default::default()
            },
        )
        .await,
        Err(ProviderError::Duplicate)
    ));
    providers::create(
        fixture.pool(),
        &fixture.key,
        &new_provider(OTHER_ISSUER, "0oa1"),
    )
    .await
    .expect("the same client ID at another issuer is a different provider");
    assert_eq!(fixture.count("sso_providers").await, 3);
    fixture.cleanup().await;
}

#[tokio::test]
async fn only_enabled_providers_are_listed_for_sign_in_and_counted() {
    let fixture = Fixture::new().await;
    let first = fixture.provider("0oa1").await;
    let disabled = fixture.provider("0oa2").await;
    let last = fixture.provider("0oa3").await;
    providers::update(
        fixture.pool(),
        &fixture.key,
        disabled.id,
        &ProviderChanges {
            enabled: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let listed = providers::list_enabled(fixture.pool()).await.unwrap();

    assert_eq!(
        listed.iter().map(|listing| listing.id).collect::<Vec<_>>(),
        [first.id, last.id]
    );
    assert!(listed.iter().all(|listing| listing.display_name == "Okta"));
    assert_eq!(providers::count_enabled(fixture.pool()).await.unwrap(), 2);
    assert_eq!(providers::list(fixture.pool()).await.unwrap().len(), 3);
    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_a_provider_deletes_its_identities_and_attempts() {
    let fixture = Fixture::new().await;
    let doomed = fixture.provider("0oa1").await;
    let kept = fixture.provider("0oa2").await;
    let user = fixture.user("ada@acme.com").await;
    identities::link(
        fixture.pool(),
        doomed.id,
        user.id,
        "subject-1",
        "ada@acme.com",
    )
    .await
    .unwrap();
    identities::link(
        fixture.pool(),
        kept.id,
        user.id,
        "subject-1",
        "ada@acme.com",
    )
    .await
    .unwrap();
    attempts::issue(fixture.pool(), doomed.id, &authorization())
        .await
        .unwrap()
        .expect("room for the attempt");
    let surviving = authorization();
    attempts::issue(fixture.pool(), kept.id, &surviving)
        .await
        .unwrap()
        .expect("room for the attempt");

    assert!(providers::delete(fixture.pool(), doomed.id).await.unwrap());

    assert_eq!(fixture.count("sso_identities").await, 1);
    assert_eq!(fixture.count("sso_attempts").await, 1);
    assert!(
        identities::find(fixture.pool(), kept.id, "subject-1")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        attempts::consume(fixture.pool(), &surviving.state)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        users::get_user_by_id(fixture.pool(), user.id)
            .await
            .unwrap()
            .is_some(),
        "the user outlives the provider"
    );
    assert!(!providers::delete(fixture.pool(), doomed.id).await.unwrap());
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_attempt_is_consumed_once_and_stores_only_the_hash_of_its_state() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let authorization = authorization();

    let expires_at = attempts::issue(fixture.pool(), provider.id, &authorization)
        .await
        .unwrap()
        .expect("room for the attempt");

    let lifetime = expires_at - Utc::now();
    assert!(
        lifetime > attempts::LIFETIME - TimeDelta::minutes(1),
        "{lifetime}"
    );
    assert!(lifetime <= attempts::LIFETIME, "{lifetime}");
    let hashes: Vec<String> = sqlx::query_scalar("SELECT state_hash FROM sso_attempts")
        .fetch_all(fixture.pool())
        .await
        .unwrap();
    assert_eq!(hashes, [hash_token(&authorization.state)]);

    let consumed = attempts::consume(fixture.pool(), &authorization.state)
        .await
        .unwrap()
        .expect("a live attempt");
    assert_eq!(consumed.provider_id, provider.id);
    assert_eq!(
        consumed.nonce.as_deref(),
        Some(authorization.nonce.as_str())
    );
    assert_eq!(
        consumed.verifier.as_deref(),
        Some(authorization.verifier.as_str())
    );
    assert_eq!(consumed.request_id, None);
    assert_eq!(consumed.asserted, None);
    assert_eq!(consumed.expires_at, expires_at);

    assert_eq!(
        attempts::consume(fixture.pool(), &authorization.state)
            .await
            .unwrap(),
        None,
        "a state is good for one callback"
    );
    assert_eq!(
        attempts::consume(fixture.pool(), "an-unknown-state")
            .await
            .unwrap(),
        None
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_expired_attempt_is_refused_and_pruned_by_the_next_issue() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let expired = authorization();
    attempts::issue(fixture.pool(), provider.id, &expired)
        .await
        .unwrap()
        .expect("room for the attempt");
    sqlx::query("UPDATE sso_attempts SET expires_at = now() - interval '1 second'")
        .execute(fixture.pool())
        .await
        .unwrap();

    assert_eq!(
        attempts::consume(fixture.pool(), &expired.state)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        fixture.count("sso_attempts").await,
        1,
        "a refused attempt stays until pruned"
    );

    let fresh = authorization();
    attempts::issue(fixture.pool(), provider.id, &fresh)
        .await
        .unwrap()
        .expect("room for the attempt");

    let hashes: Vec<String> = sqlx::query_scalar("SELECT state_hash FROM sso_attempts")
        .fetch_all(fixture.pool())
        .await
        .unwrap();
    assert_eq!(hashes, [hash_token(&fresh.state)]);
    fixture.cleanup().await;
}

#[tokio::test]
async fn no_attempt_is_issued_while_the_table_is_full_of_live_ones() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    sqlx::query(
        "INSERT INTO sso_attempts (state_hash, provider_id, nonce, code_verifier, expires_at) \
         SELECT md5(series::text), $1, 'nonce', 'verifier', now() + interval '10 minutes' \
         FROM generate_series(1, $2::bigint) AS series",
    )
    .bind(provider.id)
    .bind(attempts::CAPACITY)
    .execute(fixture.pool())
    .await
    .unwrap();

    let refused = authorization();
    assert_eq!(
        attempts::issue(fixture.pool(), provider.id, &refused)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        attempts::issue_request(fixture.pool(), provider.id, &refused.state, REQUEST_ID)
            .await
            .unwrap(),
        None,
        "a SAML request shares the capacity"
    );
    assert_eq!(fixture.count("sso_attempts").await, attempts::CAPACITY);

    sqlx::query("UPDATE sso_attempts SET expires_at = now() - interval '1 second'")
        .execute(fixture.pool())
        .await
        .unwrap();
    let admitted = authorization();
    assert!(
        attempts::issue(fixture.pool(), provider.id, &admitted)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(fixture.count("sso_attempts").await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_saml_provider_row_needs_a_saml_connection_and_no_client() {
    let fixture = Fixture::new().await;
    fixture
        .insert(&RawProvider::saml(ENTITY_ID))
        .await
        .expect("a complete SAML row is accepted");
    fixture
        .insert(&RawProvider {
            metadata_url: None,
            ..RawProvider::saml(OTHER_ENTITY_ID)
        })
        .await
        .expect("pasted metadata leaves no metadata URL");

    let refusals = [
        RawProvider {
            client_id: Some("0oa1"),
            ..RawProvider::saml("https://idp.example.com/client-id")
        },
        RawProvider {
            client_secret: Some("ENC[v1:sealed]"),
            ..RawProvider::saml("https://idp.example.com/client-secret")
        },
        RawProvider {
            require_verified_email: true,
            ..RawProvider::saml("https://idp.example.com/verified-email")
        },
        RawProvider {
            allowed_domains: Vec::new(),
            ..RawProvider::saml("https://idp.example.com/no-domains")
        },
        RawProvider {
            certificates: Some(Vec::new()),
            ..RawProvider::saml("https://idp.example.com/no-certificates")
        },
        RawProvider {
            certificates: None,
            ..RawProvider::saml("https://idp.example.com/null-certificates")
        },
        RawProvider {
            certificates: Some(vec![Some(CERTIFICATE.into()), None]),
            ..RawProvider::saml("https://idp.example.com/null-certificate")
        },
        RawProvider {
            sso_url: None,
            ..RawProvider::saml("https://idp.example.com/no-sso-url")
        },
    ];
    for raw in &refusals {
        fixture.refused(raw, CONNECTION_CHECK).await;
    }
    assert_eq!(fixture.count("sso_providers").await, 2);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_oidc_provider_row_needs_a_client_and_no_saml_connection() {
    let fixture = Fixture::new().await;
    fixture
        .insert(&RawProvider::oidc(ISSUER))
        .await
        .expect("a complete OIDC row is accepted");

    let refusals = [
        RawProvider {
            client_id: None,
            ..RawProvider::oidc(OTHER_ISSUER)
        },
        RawProvider {
            client_secret: None,
            ..RawProvider::oidc(OTHER_ISSUER)
        },
        RawProvider {
            sso_url: Some(SSO_URL),
            ..RawProvider::oidc(OTHER_ISSUER)
        },
        RawProvider {
            certificates: Some(vec![Some(CERTIFICATE.into())]),
            ..RawProvider::oidc(OTHER_ISSUER)
        },
        RawProvider {
            metadata_url: Some(METADATA_URL),
            ..RawProvider::oidc(OTHER_ISSUER)
        },
    ];
    for raw in &refusals {
        fixture.refused(raw, CONNECTION_CHECK).await;
    }
    fixture
        .refused(
            &RawProvider {
                protocol: "ws-federation",
                ..RawProvider::oidc(OTHER_ISSUER)
            },
            PROTOCOL_CHECK,
        )
        .await;
    assert_eq!(fixture.count("sso_providers").await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_saml_entity_id_is_saved_once_but_may_match_an_oidc_issuer() {
    let fixture = Fixture::new().await;
    fixture.insert(&RawProvider::saml(ENTITY_ID)).await.unwrap();

    let duplicate = fixture
        .insert(&RawProvider::saml(ENTITY_ID))
        .await
        .unwrap_err();
    assert_eq!(
        unique_constraint(&duplicate).as_deref(),
        Some(providers::SAML_DUPLICATE_KEY)
    );

    let other = fixture
        .insert(&RawProvider::saml(OTHER_ENTITY_ID))
        .await
        .unwrap();
    assert!(matches!(
        providers::update(
            fixture.pool(),
            &fixture.key,
            other,
            &ProviderChanges {
                issuer: Some(ENTITY_ID.into()),
                ..Default::default()
            },
        )
        .await,
        Err(ProviderError::Duplicate)
    ));

    let oidc = providers::create(
        fixture.pool(),
        &fixture.key,
        &new_provider(ENTITY_ID, "0oa1"),
    )
    .await
    .expect("an OIDC provider may share its issuer with a SAML entity ID");
    assert_eq!(oidc.issuer, ENTITY_ID);
    providers::create(
        fixture.pool(),
        &fixture.key,
        &new_provider(ENTITY_ID, "0oa2"),
    )
    .await
    .expect("another OIDC client at the same issuer");
    assert_eq!(fixture.count("sso_providers").await, 4);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_saml_provider_round_trips_with_its_pinned_certificates_and_no_client() {
    let fixture = Fixture::new().await;

    let created = providers::create(
        fixture.pool(),
        &fixture.key,
        &new_saml_provider(ENTITY_ID, Some(METADATA_URL)),
    )
    .await
    .unwrap();

    assert_eq!(created.protocol, Protocol::Saml);
    assert_eq!(created.display_name, "Okta");
    assert_eq!(created.issuer, ENTITY_ID);
    assert_eq!(
        created.connection,
        Connection::Saml {
            sso_url: SSO_URL.into(),
            certificates: vec![CERTIFICATE.into()],
            metadata_url: Some(METADATA_URL.into()),
        }
    );
    assert_eq!(created.allowed_domains, ["acme.com"]);
    assert!(!created.require_verified_email);
    assert!(created.enabled);
    assert_eq!(created.client_id(), None);
    assert!(matches!(
        created.client_secret(&fixture.key),
        Err(ProviderError::Mismatch(id)) if id == created.id
    ));
    let secret = SecretValue::new(SECRET);
    assert!(matches!(
        created.fields(&secret),
        Err(ProviderError::Mismatch(id)) if id == created.id
    ));
    let client: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT client_id, client_secret FROM sso_providers WHERE id = $1")
            .bind(created.id)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(client, (None, None));

    assert_eq!(
        providers::get(fixture.pool(), created.id).await.unwrap(),
        Some(created.clone())
    );
    assert_eq!(
        providers::list(fixture.pool()).await.unwrap(),
        std::slice::from_ref(&created)
    );
    assert!(matches!(
        providers::create(
            fixture.pool(),
            &fixture.key,
            &new_saml_provider(ENTITY_ID, None),
        )
        .await,
        Err(ProviderError::Duplicate)
    ));

    let pasted = providers::create(
        fixture.pool(),
        &fixture.key,
        &new_saml_provider(OTHER_ENTITY_ID, None),
    )
    .await
    .unwrap();
    let fetched = providers::get(fixture.pool(), pasted.id)
        .await
        .unwrap()
        .expect("the provider exists");
    assert!(matches!(
        fetched.connection,
        Connection::Saml {
            metadata_url: None,
            ..
        }
    ));
    assert_eq!(fixture.count("sso_providers").await, 2);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_stored_saml_provider_parses_back_into_the_provider_it_was_saved_from() {
    let fixture = Fixture::new().await;
    let metadata = saml::Metadata::parse(
        saml::testing::metadata(
            saml::testing::ENTITY_ID,
            saml::testing::SSO_URL,
            &[saml::testing::CERTIFICATE, saml::testing::OTHER_CERTIFICATE],
        )
        .as_bytes(),
    )
    .unwrap();
    let domains = vec!["ACME.com".to_owned(), "acme.com".to_owned()];
    let provider = saml::Provider::from_metadata(" Okta ", &metadata, &domains, false).unwrap();

    let created = providers::create(
        fixture.pool(),
        &fixture.key,
        &NewProvider {
            protocol: Protocol::Saml,
            display_name: provider.display_name.as_str().to_owned(),
            issuer: provider.entity_id.as_str().to_owned(),
            connection: NewConnection::Saml {
                sso_url: provider.sso_url.as_str().to_owned(),
                certificates: provider
                    .certificates
                    .iter()
                    .map(Certificate::encoded)
                    .collect(),
                metadata_url: None,
            },
            allowed_domains: provider.policy.domains.as_slice().to_vec(),
            require_verified_email: provider.policy.require_verified_email,
            enabled: true,
        },
    )
    .await
    .unwrap();

    assert_eq!(created.display_name, "Okta");
    assert_eq!(created.issuer, saml::testing::ENTITY_ID);
    assert_eq!(created.allowed_domains, ["acme.com"]);
    let Connection::Saml {
        sso_url,
        certificates,
        ..
    } = &created.connection
    else {
        panic!("a SAML provider is stored with a SAML connection");
    };
    assert_eq!(certificates.len(), 2);
    assert_eq!(
        saml::Provider::parse(&saml::Fields {
            display_name: &created.display_name,
            entity_id: &created.issuer,
            sso_url,
            certificates,
            allowed_domains: &created.allowed_domains,
            require_verified_email: created.require_verified_email,
        })
        .unwrap(),
        provider
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_sparse_saml_update_keeps_every_field_it_leaves_out() {
    let fixture = Fixture::new().await;
    let created = providers::create(
        fixture.pool(),
        &fixture.key,
        &new_saml_provider(ENTITY_ID, Some(METADATA_URL)),
    )
    .await
    .unwrap();

    let rotated = providers::update(
        fixture.pool(),
        &fixture.key,
        created.id,
        &ProviderChanges {
            certificates: Some(vec![CERTIFICATE.into(), ROTATED_CERTIFICATE.into()]),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .expect("the provider exists");

    assert_eq!(
        rotated.connection,
        Connection::Saml {
            sso_url: SSO_URL.into(),
            certificates: vec![CERTIFICATE.into(), ROTATED_CERTIFICATE.into()],
            metadata_url: Some(METADATA_URL.into()),
        }
    );
    assert_eq!(rotated.issuer, created.issuer);
    assert_eq!(rotated.display_name, created.display_name);
    assert_eq!(rotated.allowed_domains, created.allowed_domains);
    assert_eq!(rotated.enabled, created.enabled);
    assert!(rotated.updated_at >= created.updated_at);

    let moved = providers::update(
        fixture.pool(),
        &fixture.key,
        created.id,
        &ProviderChanges {
            issuer: Some(OTHER_ENTITY_ID.into()),
            sso_url: Some(OTHER_SSO_URL.into()),
            metadata_url: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .expect("the provider exists");

    assert_eq!(moved.protocol, Protocol::Saml);
    assert_eq!(moved.issuer, OTHER_ENTITY_ID);
    assert_eq!(
        moved.connection,
        Connection::Saml {
            sso_url: OTHER_SSO_URL.into(),
            certificates: vec![CERTIFICATE.into(), ROTATED_CERTIFICATE.into()],
            metadata_url: None,
        }
    );
    assert_eq!(
        providers::get(fixture.pool(), created.id).await.unwrap(),
        Some(moved)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_request_attempt_is_asserted_once_then_consumed_with_what_was_asserted() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let state = authorization().state;

    let expires_at = attempts::issue_request(fixture.pool(), provider.id, &state, REQUEST_ID)
        .await
        .unwrap()
        .expect("room for the attempt");
    let lifetime = expires_at - Utc::now();
    assert!(
        lifetime > attempts::LIFETIME - TimeDelta::minutes(1),
        "{lifetime}"
    );
    assert!(lifetime <= attempts::LIFETIME, "{lifetime}");
    let hashes: Vec<String> = sqlx::query_scalar("SELECT state_hash FROM sso_attempts")
        .fetch_all(fixture.pool())
        .await
        .unwrap();
    assert_eq!(hashes, [hash_token(&state)]);

    let claimed = attempts::claim(fixture.pool(), &state)
        .await
        .unwrap()
        .expect("a live request");
    assert_eq!(claimed.provider_id, provider.id);
    assert_eq!(claimed.request_id.as_deref(), Some(REQUEST_ID));
    assert_eq!(claimed.nonce, None);
    assert_eq!(claimed.verifier, None);
    assert_eq!(claimed.asserted, None);
    assert_eq!(claimed.expires_at, expires_at);
    assert_eq!(
        attempts::claim(fixture.pool(), &state).await.unwrap(),
        None,
        "a request is claimed once"
    );
    assert_eq!(
        attempts::claim(fixture.pool(), "an-unknown-state")
            .await
            .unwrap(),
        None
    );

    assert!(
        attempts::assert(
            fixture.pool(),
            claimed.id,
            HANDOFF_CODE,
            SUBJECT,
            EMAIL,
            Some("Ada Lovelace"),
        )
        .await
        .unwrap()
    );
    assert!(
        !attempts::assert(
            fixture.pool(),
            claimed.id,
            "a-second-code",
            "another-subject",
            "eve@acme.com",
            None,
        )
        .await
        .unwrap(),
        "a request is asserted once"
    );
    assert_eq!(
        attempts::claim(fixture.pool(), &state).await.unwrap(),
        None,
        "an asserted request is no longer waiting for a response"
    );

    let consumed = attempts::consume(fixture.pool(), &state)
        .await
        .unwrap()
        .expect("an asserted attempt");
    assert_eq!(consumed.id, claimed.id);
    assert_eq!(consumed.request_id.as_deref(), Some(REQUEST_ID));
    assert_eq!(consumed.nonce, None);
    assert_eq!(
        consumed.asserted,
        Some(Asserted {
            code_hash: hash_token(HANDOFF_CODE),
            subject: SUBJECT.into(),
            email: EMAIL.into(),
            display_name: Some("Ada Lovelace".into()),
        })
    );
    let handoff = consumed.expires_at - Utc::now();
    assert!(handoff <= attempts::HANDOFF_LIFETIME, "{handoff}");
    assert!(
        handoff > attempts::HANDOFF_LIFETIME - TimeDelta::minutes(1),
        "{handoff}"
    );
    assert!(consumed.expires_at < expires_at);
    assert_eq!(
        attempts::consume(fixture.pool(), &state).await.unwrap(),
        None,
        "a handoff is redeemed once"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn racing_claims_for_one_request_admit_exactly_one() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let state = authorization().state;
    attempts::issue_request(fixture.pool(), provider.id, &state, REQUEST_ID)
        .await
        .unwrap()
        .unwrap();

    let claims = join_all((0..CLAIMANTS).map(|_| attempts::claim(fixture.pool(), &state))).await;

    let claimed: Vec<_> = claims.into_iter().filter_map(Result::unwrap).collect();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].request_id.as_deref(), Some(REQUEST_ID));
    let stamped: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT claimed_at FROM sso_attempts WHERE id = $1")
            .bind(claimed[0].id)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert!(stamped.is_some());
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_unclaimed_request_cannot_be_asserted() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let state = authorization().state;
    attempts::issue_request(fixture.pool(), provider.id, &state, REQUEST_ID)
        .await
        .unwrap()
        .unwrap();
    let id: Uuid = sqlx::query_scalar("SELECT id FROM sso_attempts WHERE state_hash = $1")
        .bind(hash_token(&state))
        .fetch_one(fixture.pool())
        .await
        .unwrap();

    assert!(
        !attempts::assert(fixture.pool(), id, HANDOFF_CODE, SUBJECT, EMAIL, None)
            .await
            .unwrap(),
        "only the post that claimed a request asserts it"
    );
    assert!(
        attempts::claim(fixture.pool(), &state)
            .await
            .unwrap()
            .is_some(),
        "the refused assert left the request waiting"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_claimed_request_left_unasserted_never_hands_off() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let state = authorization().state;
    attempts::issue_request(fixture.pool(), provider.id, &state, REQUEST_ID)
        .await
        .unwrap()
        .unwrap();
    attempts::claim(fixture.pool(), &state)
        .await
        .unwrap()
        .expect("a live request");

    assert_eq!(
        attempts::claim(fixture.pool(), &state).await.unwrap(),
        None,
        "a post dropped mid-check can't be followed by another"
    );
    let consumed = attempts::consume(fixture.pool(), &state)
        .await
        .unwrap()
        .expect("the claimed attempt until it expires");
    assert_eq!(consumed.asserted, None, "nothing for the console to redeem");
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_assertion_without_a_display_name_is_stored_without_one() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let state = authorization().state;
    attempts::issue_request(fixture.pool(), provider.id, &state, REQUEST_ID)
        .await
        .unwrap()
        .expect("room for the attempt");
    let claimed = attempts::claim(fixture.pool(), &state)
        .await
        .unwrap()
        .unwrap();

    assert!(
        attempts::assert(
            fixture.pool(),
            claimed.id,
            HANDOFF_CODE,
            SUBJECT,
            EMAIL,
            None
        )
        .await
        .unwrap()
    );

    let asserted = attempts::consume(fixture.pool(), &state)
        .await
        .unwrap()
        .and_then(|attempt| attempt.asserted)
        .expect("asserted");
    assert_eq!(asserted.display_name, None);
    assert_eq!(asserted.subject, SUBJECT);
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_handoff_never_outlives_the_request_it_came_from() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let state = authorization().state;
    attempts::issue_request(fixture.pool(), provider.id, &state, REQUEST_ID)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE sso_attempts SET expires_at = now() + interval '30 seconds'")
        .execute(fixture.pool())
        .await
        .unwrap();
    let claimed = attempts::claim(fixture.pool(), &state)
        .await
        .unwrap()
        .unwrap();

    assert!(
        attempts::assert(
            fixture.pool(),
            claimed.id,
            HANDOFF_CODE,
            SUBJECT,
            EMAIL,
            None
        )
        .await
        .unwrap()
    );

    let consumed = attempts::consume(fixture.pool(), &state)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(consumed.expires_at, claimed.expires_at);
    fixture.cleanup().await;
}

#[tokio::test]
async fn only_a_live_request_attempt_can_be_asserted() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let oidc = authorization();
    attempts::issue(fixture.pool(), provider.id, &oidc)
        .await
        .unwrap()
        .unwrap();
    let expired = authorization().state;
    attempts::issue_request(fixture.pool(), provider.id, &expired, REQUEST_ID)
        .await
        .unwrap()
        .unwrap();
    let expired_id: Uuid = attempts::claim(fixture.pool(), &expired)
        .await
        .unwrap()
        .unwrap()
        .id;
    sqlx::query("UPDATE sso_attempts SET expires_at = now() - interval '1 second' WHERE id = $1")
        .bind(expired_id)
        .execute(fixture.pool())
        .await
        .unwrap();
    let oidc_id: Uuid = sqlx::query_scalar("SELECT id FROM sso_attempts WHERE state_hash = $1")
        .bind(hash_token(&oidc.state))
        .fetch_one(fixture.pool())
        .await
        .unwrap();

    assert_eq!(
        attempts::claim(fixture.pool(), &oidc.state).await.unwrap(),
        None,
        "an OIDC attempt is never waiting for a SAML response"
    );
    assert_eq!(
        attempts::claim(fixture.pool(), &expired).await.unwrap(),
        None
    );
    for id in [oidc_id, expired_id, Uuid::new_v4()] {
        assert!(
            !attempts::assert(fixture.pool(), id, HANDOFF_CODE, SUBJECT, EMAIL, None)
                .await
                .unwrap(),
            "{id}"
        );
    }
    let asserted: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sso_attempts WHERE code_hash IS NOT NULL")
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(asserted, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_discarded_request_attempt_is_gone() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let state = authorization().state;
    let kept = authorization();
    attempts::issue_request(fixture.pool(), provider.id, &state, REQUEST_ID)
        .await
        .unwrap()
        .unwrap();
    attempts::issue(fixture.pool(), provider.id, &kept)
        .await
        .unwrap()
        .unwrap();
    let claimed = attempts::claim(fixture.pool(), &state)
        .await
        .unwrap()
        .unwrap();

    assert!(attempts::discard(fixture.pool(), claimed.id).await.unwrap());
    assert!(!attempts::discard(fixture.pool(), claimed.id).await.unwrap());

    assert_eq!(attempts::claim(fixture.pool(), &state).await.unwrap(), None);
    assert_eq!(
        attempts::consume(fixture.pool(), &state).await.unwrap(),
        None
    );
    assert_eq!(fixture.count("sso_attempts").await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn discarding_an_asserted_attempt_keeps_its_handoff() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let state = authorization().state;
    attempts::issue_request(fixture.pool(), provider.id, &state, REQUEST_ID)
        .await
        .unwrap()
        .unwrap();
    let claimed = attempts::claim(fixture.pool(), &state)
        .await
        .unwrap()
        .unwrap();
    assert!(
        attempts::assert(
            fixture.pool(),
            claimed.id,
            HANDOFF_CODE,
            SUBJECT,
            EMAIL,
            None
        )
        .await
        .unwrap()
    );

    assert!(
        !attempts::discard(fixture.pool(), claimed.id).await.unwrap(),
        "a refused second post must not end the first post's sign-in"
    );

    let consumed = attempts::consume(fixture.pool(), &state)
        .await
        .unwrap()
        .expect("the asserted attempt survives");
    assert_eq!(consumed.asserted.unwrap().subject, SUBJECT);
    fixture.cleanup().await;
}

#[tokio::test]
async fn clearing_a_provider_ends_only_its_own_attempts() {
    let fixture = Fixture::new().await;
    let cleared = fixture.provider("0oa1").await;
    let kept = fixture.provider("0oa2").await;
    let asserted = authorization().state;
    attempts::issue_request(fixture.pool(), cleared.id, &asserted, REQUEST_ID)
        .await
        .unwrap()
        .unwrap();
    let claimed = attempts::claim(fixture.pool(), &asserted)
        .await
        .unwrap()
        .unwrap();
    assert!(
        attempts::assert(
            fixture.pool(),
            claimed.id,
            HANDOFF_CODE,
            SUBJECT,
            EMAIL,
            None
        )
        .await
        .unwrap()
    );
    attempts::issue(fixture.pool(), cleared.id, &authorization())
        .await
        .unwrap()
        .unwrap();
    let surviving = authorization();
    attempts::issue(fixture.pool(), kept.id, &surviving)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        attempts::clear(fixture.pool(), cleared.id).await.unwrap(),
        2
    );

    assert_eq!(
        attempts::consume(fixture.pool(), &asserted).await.unwrap(),
        None
    );
    assert!(
        attempts::consume(fixture.pool(), &surviving.state)
            .await
            .unwrap()
            .is_some()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_request_id_is_issued_once() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    attempts::issue_request(
        fixture.pool(),
        provider.id,
        &authorization().state,
        REQUEST_ID,
    )
    .await
    .unwrap()
    .unwrap();

    let error = attempts::issue_request(
        fixture.pool(),
        provider.id,
        &authorization().state,
        REQUEST_ID,
    )
    .await
    .unwrap_err();
    assert_eq!(unique_constraint(&error).as_deref(), Some(REQUEST_ID_KEY));
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_attempt_row_is_either_an_oidc_or_a_saml_flow() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let insert = |columns: &'static str, values: &'static str| {
        let pool = fixture.pool().clone();
        async move {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO sso_attempts (state_hash, provider_id, expires_at, {columns}) \
                 VALUES (md5(random()::text), $1, now() + interval '10 minutes', {values})"
            )))
            .bind(provider.id)
            .execute(&pool)
            .await
        }
    };

    let refusals = [
        (
            "nonce, code_verifier, request_id",
            "'n', 'v', '_r1'",
            FLOW_CHECK,
        ),
        ("nonce", "'n'", FLOW_CHECK),
        ("request_id, nonce", "'_r2', 'n'", FLOW_CHECK),
        ("request_id, code_hash", "'_r3', 'h'", ASSERTED_CHECK),
        (
            "request_id, code_hash, subject",
            "'_r4', 'h', 's'",
            ASSERTED_CHECK,
        ),
        (
            "request_id, subject, email",
            "'_r5', 's', 'e'",
            ASSERTED_CHECK,
        ),
        ("request_id, display_name", "'_r6', 'Ada'", ASSERTED_CHECK),
        (
            "nonce, code_verifier, code_hash, subject, email",
            "'n', 'v', 'h', 's', 'e'",
            ASSERTED_CHECK,
        ),
        (
            "nonce, code_verifier, claimed_at",
            "'n', 'v', now()",
            CLAIMED_CHECK,
        ),
        (
            "request_id, code_hash, subject, email",
            "'_r7', 'h', 's', 'e'",
            CLAIMED_CHECK,
        ),
    ];
    for (columns, values, constraint) in refusals {
        let error = insert(columns, values).await.unwrap_err();
        assert_eq!(
            check_constraint(&error).as_deref(),
            Some(constraint),
            "{columns}"
        );
    }
    insert(
        "request_id, claimed_at, code_hash, subject, email",
        "'_r8', now(), 'h', 's', 'e'",
    )
    .await
    .expect("an asserted request without a display name");
    insert("request_id, claimed_at", "'_r9', now()")
        .await
        .expect("a claimed request waiting for its response check");
    insert("nonce, code_verifier", "'n', 'v'")
        .await
        .expect("an OIDC attempt");
    assert_eq!(fixture.count("sso_attempts").await, 3);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_assertion_is_recorded_once_per_provider_until_it_expires() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let other = fixture.provider("0oa2").await;
    let expires_at = Utc::now() + TimeDelta::minutes(5);

    assert!(
        assertions::record(fixture.pool(), provider.id, ASSERTION_ID, expires_at)
            .await
            .unwrap()
    );
    assert!(
        !assertions::record(fixture.pool(), provider.id, ASSERTION_ID, expires_at)
            .await
            .unwrap(),
        "a replayed assertion is refused"
    );
    assert!(
        assertions::record(fixture.pool(), other.id, ASSERTION_ID, expires_at)
            .await
            .unwrap(),
        "assertion IDs are per provider"
    );
    assert_eq!(fixture.count("sso_assertions").await, 2);
    fixture.cleanup().await;
}

#[tokio::test]
async fn recording_an_assertion_prunes_the_expired_ones() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let expires_at = Utc::now() + TimeDelta::minutes(5);
    for id in [ASSERTION_ID, "_assertion-2", "_assertion-3"] {
        assert!(
            assertions::record(fixture.pool(), provider.id, id, expires_at)
                .await
                .unwrap()
        );
    }
    sqlx::query(
        "UPDATE sso_assertions SET expires_at = now() - interval '1 second' \
         WHERE assertion_id <> '_assertion-3'",
    )
    .execute(fixture.pool())
    .await
    .unwrap();

    assert!(
        assertions::record(fixture.pool(), provider.id, ASSERTION_ID, expires_at)
            .await
            .unwrap(),
        "an expired assertion ID is taken over"
    );

    let kept: Vec<String> =
        sqlx::query_scalar("SELECT assertion_id FROM sso_assertions ORDER BY assertion_id")
            .fetch_all(fixture.pool())
            .await
            .unwrap();
    assert_eq!(kept, [ASSERTION_ID, "_assertion-3"]);
    let renewed: DateTime<Utc> =
        sqlx::query_scalar("SELECT expires_at FROM sso_assertions WHERE assertion_id = $1")
            .bind(ASSERTION_ID)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert!(renewed > Utc::now(), "{renewed}");
    assert!(
        !assertions::record(fixture.pool(), provider.id, "_assertion-3", expires_at)
            .await
            .unwrap()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_a_provider_deletes_its_assertions_and_request_attempts() {
    let fixture = Fixture::new().await;
    let doomed = fixture.provider("0oa1").await;
    let kept = fixture.provider("0oa2").await;
    let expires_at = Utc::now() + TimeDelta::minutes(5);
    assertions::record(fixture.pool(), doomed.id, ASSERTION_ID, expires_at)
        .await
        .unwrap();
    assertions::record(fixture.pool(), kept.id, ASSERTION_ID, expires_at)
        .await
        .unwrap();
    attempts::issue_request(
        fixture.pool(),
        doomed.id,
        &authorization().state,
        REQUEST_ID,
    )
    .await
    .unwrap()
    .unwrap();

    assert!(providers::delete(fixture.pool(), doomed.id).await.unwrap());

    let remaining: Vec<Uuid> = sqlx::query_scalar("SELECT provider_id FROM sso_assertions")
        .fetch_all(fixture.pool())
        .await
        .unwrap();
    assert_eq!(remaining, [kept.id]);
    assert_eq!(fixture.count("sso_attempts").await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn enforcement_defaults_to_off_and_round_trips() {
    let fixture = Fixture::new().await;
    assert!(!settings::required(fixture.pool()).await.unwrap());

    let mut transaction = fixture.pool().begin().await.unwrap();
    settings::lock(&mut transaction).await.unwrap();
    assert!(
        !settings::set_required(&mut transaction, true)
            .await
            .unwrap()
    );
    transaction.commit().await.unwrap();
    assert!(settings::required(fixture.pool()).await.unwrap());

    let mut transaction = fixture.pool().begin().await.unwrap();
    assert!(
        settings::set_required(&mut transaction, true)
            .await
            .unwrap()
    );
    assert!(
        settings::set_required(&mut transaction, false)
            .await
            .unwrap()
    );
    transaction.commit().await.unwrap();
    assert!(!settings::required(fixture.pool()).await.unwrap());

    let mut transaction = fixture.pool().begin().await.unwrap();
    assert!(
        !settings::set_required(&mut transaction, true)
            .await
            .unwrap()
    );
    transaction.rollback().await.unwrap();
    assert!(!settings::required(fixture.pool()).await.unwrap());
    assert_eq!(fixture.count("sso_settings").await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_settings_lock_holds_off_a_second_writer_until_the_first_commits() {
    let fixture = Fixture::new().await;
    let mut first = fixture.pool().begin().await.unwrap();
    settings::lock(&mut first).await.unwrap();

    let mut second = fixture.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '200ms'")
        .execute(&mut *second)
        .await
        .unwrap();
    let blocked = settings::lock(&mut second).await.unwrap_err();
    assert_eq!(
        blocked
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("55P03"),
        "{blocked}"
    );
    second.rollback().await.unwrap();

    first.commit().await.unwrap();
    let mut third = fixture.pool().begin().await.unwrap();
    settings::lock(&mut third).await.unwrap();
    third.rollback().await.unwrap();
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_identity_is_unique_per_provider_subject_and_per_provider_user() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let other_provider = fixture.provider("0oa2").await;
    let ada = fixture.user("ada@acme.com").await;
    let grace = fixture.user("grace@acme.com").await;

    let linked = identities::link(
        fixture.pool(),
        provider.id,
        ada.id,
        "subject-1",
        "ada@acme.com",
    )
    .await
    .unwrap();
    assert_eq!(linked.provider_id, provider.id);
    assert_eq!(linked.user_id, ada.id);
    assert_eq!(linked.subject, "subject-1");
    assert_eq!(linked.email, "ada@acme.com");

    let same_subject = identities::link(
        fixture.pool(),
        provider.id,
        grace.id,
        "subject-1",
        "grace@acme.com",
    )
    .await
    .unwrap_err();
    assert_eq!(
        unique_constraint(&same_subject).as_deref(),
        Some(identities::SUBJECT_KEY)
    );

    let same_user = identities::link(
        fixture.pool(),
        provider.id,
        ada.id,
        "subject-2",
        "ada@acme.com",
    )
    .await
    .unwrap_err();
    assert_eq!(
        unique_constraint(&same_user).as_deref(),
        Some(identities::USER_KEY)
    );

    identities::link(
        fixture.pool(),
        other_provider.id,
        ada.id,
        "subject-1",
        "ada@acme.com",
    )
    .await
    .expect("another provider may use the same subject for the same user");

    assert_eq!(
        identities::find(fixture.pool(), provider.id, "subject-1")
            .await
            .unwrap(),
        Some(linked.clone())
    );
    assert!(identities::linked(fixture.pool(), ada.id).await.unwrap());
    assert_eq!(
        identities::find(fixture.pool(), provider.id, "subject-2")
            .await
            .unwrap(),
        None
    );
    assert!(!identities::linked(fixture.pool(), grace.id).await.unwrap());
    fixture.cleanup().await;
}

#[tokio::test]
async fn touching_an_identity_records_the_latest_email_and_sign_in() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider("0oa1").await;
    let ada = fixture.user("ada@acme.com").await;
    let linked = identities::link(
        fixture.pool(),
        provider.id,
        ada.id,
        "subject-1",
        "ada@acme.com",
    )
    .await
    .unwrap();
    sqlx::query("UPDATE sso_identities SET last_signed_in_at = now() - interval '1 day'")
        .execute(fixture.pool())
        .await
        .unwrap();

    identities::touch(fixture.pool(), linked.id, "ada.lovelace@acme.com")
        .await
        .unwrap();

    let touched = identities::find(fixture.pool(), provider.id, "subject-1")
        .await
        .unwrap()
        .expect("still linked");
    assert_eq!(touched.email, "ada.lovelace@acme.com");
    assert!(touched.last_signed_in_at > Utc::now() - TimeDelta::minutes(1));
    assert_eq!(touched.created_at, linked.created_at);
    assert!(matches!(
        identities::touch(fixture.pool(), Uuid::new_v4(), "x@acme.com").await,
        Err(sqlx::Error::RowNotFound)
    ));
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_email_is_stored_in_lower_case_and_looked_up_ignoring_case() {
    let fixture = Fixture::new().await;
    let ada = fixture.user("Ada@Acme.com").await;
    let grace = fixture.user("grace@acme.com").await;

    assert_eq!(ada.email, "ada@acme.com");
    let found = users::find_by_email_ignoring_case(fixture.pool(), "ADA@acme.COM")
        .await
        .unwrap();
    assert_eq!(
        found.iter().map(|user| user.id).collect::<Vec<_>>(),
        [ada.id]
    );
    assert!(
        users::find_by_email_ignoring_case(fixture.pool(), "nobody@acme.com")
            .await
            .unwrap()
            .is_empty()
    );

    sqlx::query(
        "UPDATE users SET is_admin = true, is_active = false, email_verified = true WHERE id = $1",
    )
    .bind(ada.id)
    .execute(fixture.pool())
    .await
    .unwrap();
    let account = users::find_by_email_ignoring_case(fixture.pool(), "ada@acme.com")
        .await
        .unwrap()[0]
        .account(false);
    assert_eq!(account.id, ada.id);
    assert!(account.admin);
    assert!(!account.active);
    assert!(account.email_verified);
    assert!(!account.pristine);
    let account = users::find_by_email_ignoring_case(fixture.pool(), "GRACE@acme.com")
        .await
        .unwrap()[0]
        .account(true);
    assert_eq!(account.id, grace.id);
    assert!(!account.admin);
    assert!(account.active);
    assert!(!account.email_verified);
    assert!(account.pristine);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_case_variant_of_an_email_names_the_lower_case_key() {
    let fixture = Fixture::new().await;
    fixture.user("ada@acme.com").await;

    let error = users::create_user(fixture.pool(), "ADA@acme.com", "hash", None, false)
        .await
        .unwrap_err();

    assert_eq!(
        unique_constraint(&error).as_deref(),
        Some(users::EMAIL_LOWER_KEY)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_duplicate_email_names_the_users_email_key() {
    let fixture = Fixture::new().await;
    fixture.user("ada@acme.com").await;
    let mut connection = fixture.pool().acquire().await.unwrap();

    let error = provision::insert(&mut connection, "ada@acme.com", "hash", None, false, false)
        .await
        .unwrap_err();

    assert_eq!(unique_constraint(&error).as_deref(), Some(users::EMAIL_KEY));
    drop(connection);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_insert_rolled_back_with_its_transaction_leaves_no_user() {
    let fixture = Fixture::new().await;
    let mut transaction = fixture.pool().begin().await.unwrap();

    let user = provision::insert(
        &mut transaction,
        "ada@acme.com",
        "hash",
        Some("Ada"),
        false,
        true,
    )
    .await
    .unwrap();
    assert!(user.email_verified);
    transaction.rollback().await.unwrap();

    assert!(
        users::get_user_by_id(fixture.pool(), user.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(fixture.count("user_roles").await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_insert_commits_the_user_role_and_the_verified_flag_together() {
    let fixture = Fixture::new().await;
    let mut transaction = fixture.pool().begin().await.unwrap();

    let user = provision::insert(
        &mut transaction,
        "ada@acme.com",
        "hash",
        Some("Ada"),
        false,
        true,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let stored = users::get_user_with_permissions(fixture.pool(), user.id)
        .await
        .unwrap()
        .expect("committed");
    assert!(stored.user.email_verified);
    assert_eq!(stored.user.display_name.as_deref(), Some("Ada"));
    assert_eq!(stored.user.is_admin, Some(false));
    assert_eq!(stored.roles, [provision::ROLE]);

    let unverified = fixture.user("grace@acme.com").await;
    assert!(!unverified.email_verified);
    fixture.cleanup().await;
}

#[tokio::test]
async fn furnishing_gives_the_user_an_organization_and_workspace_they_own() {
    let fixture = Fixture::new().await;
    let mut transaction = fixture.pool().begin().await.unwrap();
    let user = provision::insert(
        &mut transaction,
        "ada@acme.com",
        "hash",
        Some("Ada"),
        false,
        true,
    )
    .await
    .unwrap();

    provision::furnish(&mut transaction, &user).await.unwrap();
    transaction.commit().await.unwrap();

    let owned: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT o.name, w.name, wm.role::text FROM organizations o \
         JOIN organization_members om ON om.organization_id = o.id AND om.user_id = $1 \
         JOIN workspaces w ON w.organization_id = o.id \
         JOIN workspace_members wm ON wm.workspace_id = w.id AND wm.user_id = $1",
    )
    .bind(user.id)
    .fetch_all(fixture.pool())
    .await
    .unwrap();
    assert_eq!(
        owned,
        [(
            "Ada's Organization".to_owned(),
            "Default Workspace".to_owned(),
            "owner".to_owned()
        )]
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_failed_furnishing_rolls_back_alone_and_the_account_still_commits() {
    let fixture = Fixture::new().await;
    let mut transaction = fixture.pool().begin().await.unwrap();
    let user = provision::insert(
        &mut transaction,
        "ada@acme.com",
        "hash",
        Some("Ada"),
        false,
        true,
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO organizations (name, slug) VALUES ('Taken', $1)")
        .bind(provision::organization_slug(
            &provision::organization_name(user.display_name.as_deref()),
            user.id,
        ))
        .execute(&mut *transaction)
        .await
        .unwrap();

    provision::furnish(&mut transaction, &user).await.unwrap();
    transaction.commit().await.unwrap();

    let memberships: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM organization_members WHERE user_id = $1")
            .bind(user.id)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(memberships, 0);
    assert!(
        users::get_user_by_id(fixture.pool(), user.id)
            .await
            .unwrap()
            .is_some()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn reclaiming_an_account_replaces_its_password_and_verifies_its_email() {
    let fixture = Fixture::new().await;
    let squatted = fixture.user("ada@acme.com").await;
    let bystander = fixture.user("grace@acme.com").await;
    let replacement = hash_password("Another-horse-2").unwrap();
    let mut connection = fixture.pool().acquire().await.unwrap();

    assert!(
        users::reclaim(&mut connection, squatted.id, &replacement)
            .await
            .unwrap()
    );

    let stored = users::get_user_by_email(fixture.pool(), "ada@acme.com")
        .await
        .unwrap()
        .expect("still there");
    assert_eq!(stored.password_hash, replacement);
    assert!(stored.email_verified);
    assert!(!zone_server::auth::verify_password(PASSWORD, &stored.password_hash).unwrap());
    let untouched = users::get_user_by_email(fixture.pool(), "grace@acme.com")
        .await
        .unwrap()
        .expect("still there");
    assert!(!untouched.email_verified);
    assert!(zone_server::auth::verify_password(PASSWORD, &untouched.password_hash).unwrap());
    assert_eq!(untouched.id, bystander.id);
    assert!(
        !users::reclaim(&mut connection, Uuid::new_v4(), &replacement)
            .await
            .unwrap()
    );
    drop(connection);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_account_verified_or_promoted_after_it_was_resolved_is_not_reclaimed() {
    let fixture = Fixture::new().await;
    let verified = fixture.user("ada@acme.com").await;
    let promoted = fixture.user("grace@acme.com").await;
    sqlx::query("UPDATE users SET email_verified = TRUE WHERE id = $1")
        .bind(verified.id)
        .execute(fixture.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE users SET is_admin = TRUE WHERE id = $1")
        .bind(promoted.id)
        .execute(fixture.pool())
        .await
        .unwrap();
    let replacement = hash_password("Another-horse-2").unwrap();
    let mut connection = fixture.pool().acquire().await.unwrap();

    for user in [&verified, &promoted] {
        assert!(
            !users::reclaim(&mut connection, user.id, &replacement)
                .await
                .unwrap(),
            "{}",
            user.email
        );
        let stored = users::get_user_by_email(fixture.pool(), &user.email)
            .await
            .unwrap()
            .expect("still there");
        assert!(zone_server::auth::verify_password(PASSWORD, &stored.password_hash).unwrap());
    }
    drop(connection);
    fixture.cleanup().await;
}

#[test]
fn the_client_secret_is_a_strict_sealed_column_of_sso_providers() {
    assert!(Column::ALL.contains(&Column::SsoClientSecret));
    assert_eq!(Column::SsoClientSecret.table(), "sso_providers");
    assert_eq!(Column::SsoClientSecret.name(), "client_secret");
    assert_eq!(
        Column::SsoClientSecret.policy(),
        zone_server::db::sealing::Policy::Strict
    );
}
