//! The single sign-on storage: providers with sealed client secrets, the identities linked
//! through them, single-use sign-in attempts and the enforcement setting.

use abnegate_secret::{MasterKey, SecretValue};
use chrono::{TimeDelta, Utc};
use reqwest::Url;
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::sso::{Authorization, ClientId, Fields, Protocol, Provider};
use zone_server::auth::hash_password;
use zone_server::db::sso::{
    NewProvider, ProviderChanges, ProviderError, ProviderRecord, attempts, identities, providers,
    settings,
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
        client_id: client_id.into(),
        client_secret: SecretValue::new(SECRET),
        allowed_domains: vec!["acme.com".into()],
        require_verified_email: true,
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
    assert_eq!(created.client_id, "0oa1");
    assert_eq!(created.client_secret.expose(), SECRET);
    assert_eq!(created.allowed_domains, ["acme.com"]);
    assert!(created.require_verified_email);
    assert!(created.enabled);

    let stored = fixture.stored_secret(created.id).await;
    assert!(stored.starts_with(ENVELOPE_OPENING), "{stored}");
    assert!(
        !stored.contains(SECRET),
        "the plaintext secret must never be stored"
    );

    let fetched = providers::get(fixture.pool(), &fixture.key, created.id)
        .await
        .unwrap();
    assert_eq!(fetched.as_ref(), Some(&created));
    assert_eq!(
        providers::list(fixture.pool(), &fixture.key).await.unwrap(),
        std::slice::from_ref(&created)
    );
    assert_eq!(
        providers::get(fixture.pool(), &fixture.key, Uuid::new_v4())
            .await
            .unwrap(),
        None
    );

    let other = MasterKey::generate().unwrap();
    assert!(matches!(
        providers::get(fixture.pool(), &other, created.id).await,
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
    assert_eq!(Provider::parse(&created.fields()).unwrap(), provider);
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
    assert_eq!(renamed.client_id, created.client_id);
    assert_eq!(renamed.allowed_domains, created.allowed_domains);
    assert_eq!(
        renamed.require_verified_email,
        created.require_verified_email
    );
    assert_eq!(renamed.client_secret.expose(), SECRET);
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
    assert_eq!(replaced.client_secret.expose(), REPLACED_SECRET);
    assert_eq!(replaced.issuer, OTHER_ISSUER);
    assert_eq!(replaced.client_id, "client-2");
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
    assert_eq!(
        providers::list(fixture.pool(), &fixture.key)
            .await
            .unwrap()
            .len(),
        3
    );
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
        .unwrap();
    let surviving = authorization();
    attempts::issue(fixture.pool(), kept.id, &surviving)
        .await
        .unwrap();

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
        .unwrap();

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
    assert_eq!(consumed.nonce, authorization.nonce);
    assert_eq!(consumed.verifier, authorization.verifier);
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
        .unwrap();
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
        .unwrap();

    let hashes: Vec<String> = sqlx::query_scalar("SELECT state_hash FROM sso_attempts")
        .fetch_all(fixture.pool())
        .await
        .unwrap();
    assert_eq!(hashes, [hash_token(&fresh.state)]);
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
    assert_eq!(
        identities::find_for_user(fixture.pool(), provider.id, ada.id)
            .await
            .unwrap(),
        Some(linked.clone())
    );
    assert_eq!(
        identities::find(fixture.pool(), provider.id, "subject-2")
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        identities::find_for_user(fixture.pool(), provider.id, grace.id)
            .await
            .unwrap(),
        None
    );
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
async fn an_email_is_looked_up_ignoring_case() {
    let fixture = Fixture::new().await;
    let mixed = fixture.user("Ada@Acme.com").await;
    let lower = fixture.user("ada@acme.com").await;
    fixture.user("grace@acme.com").await;

    let found = users::find_by_email_ignoring_case(fixture.pool(), "ADA@acme.COM")
        .await
        .unwrap();

    assert_eq!(
        found.iter().map(|user| user.id).collect::<Vec<_>>(),
        [mixed.id, lower.id]
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
    .bind(mixed.id)
    .execute(fixture.pool())
    .await
    .unwrap();
    let found = users::find_by_email_ignoring_case(fixture.pool(), "ada@acme.com")
        .await
        .unwrap();
    let account = found[0].account();
    assert_eq!(account.id, mixed.id);
    assert!(account.admin);
    assert!(!account.active);
    assert!(account.email_verified);
    let account = found[1].account();
    assert!(!account.admin);
    assert!(account.active);
    assert!(!account.email_verified);
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
    let mut connection = fixture.pool().acquire().await.unwrap();
    let user = provision::insert(
        &mut connection,
        "ada@acme.com",
        "hash",
        Some("Ada"),
        false,
        true,
    )
    .await
    .unwrap();
    drop(connection);

    provision::furnish(fixture.pool(), &user).await;

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
async fn reclaiming_an_account_replaces_its_password_and_verifies_its_email() {
    let fixture = Fixture::new().await;
    let squatted = fixture.user("ada@acme.com").await;
    let bystander = fixture.user("grace@acme.com").await;
    let replacement = hash_password("Another-horse-2").unwrap();

    users::reclaim(fixture.pool(), squatted.id, &replacement)
        .await
        .unwrap();

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
    assert!(matches!(
        users::reclaim(fixture.pool(), Uuid::new_v4(), &replacement).await,
        Err(sqlx::Error::RowNotFound)
    ));
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
