use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;
use zone_enterprise::{
    Edition, Keyring, License, Payload, SigningKey, Status, Tier, TrustedKey, sign,
};
use zone_server::db::{licenses, migrations};
use zone_server::services::license::{Licenses, SetError};
use zone_server::state::AppState;

use crate::common::{Database, TestClient};

const PREFIX: &str = "license_store";
const CHECK_VIOLATION: &str = "23514";
const SINGLETON_CHECK: &str = "licenses_singleton_check";
const TRUSTED: &str = "test-1";
const UNTRUSTED: &str = "test-2";
const TRUSTED_SEED: [u8; 32] = [0x11; 32];
const UNTRUSTED_SEED: [u8; 32] = [0x22; 32];
const LINE_WIDTH: usize = 64;

struct Fixture {
    database: Database,
    client: TestClient,
}

impl Fixture {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        let client = TestClient::with_pool(database.pool.clone());
        client.state().install_keyring(keyring());
        Self { database, client }
    }

    fn state(&self) -> &AppState {
        self.client.state()
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn rows(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM licenses")
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn trusted() -> SigningKey {
    SigningKey::from_bytes(&TRUSTED_SEED)
}

fn untrusted() -> SigningKey {
    SigningKey::from_bytes(&UNTRUSTED_SEED)
}

fn keyring() -> Keyring {
    Keyring::new([TrustedKey::new(TRUSTED, trusted().verifying_key().to_bytes()).unwrap()]).unwrap()
}

fn payload(tier: Tier, expires_at: DateTime<Utc>) -> Payload {
    Payload {
        id: Uuid::new_v4(),
        customer: "Acme Ltd".into(),
        tier,
        seats: 25,
        issued_at: expires_at - TimeDelta::days(365),
        expires_at,
        key_id: TRUSTED.into(),
    }
}

fn key(tier: Tier, expires_in: TimeDelta) -> String {
    sign(&payload(tier, Utc::now() + expires_in), &trusted())
}

fn active(tier: Tier) -> String {
    key(tier, TimeDelta::days(365))
}

fn in_grace(tier: Tier) -> String {
    key(tier, -TimeDelta::days(1))
}

fn lapsed(tier: Tier) -> String {
    key(tier, -TimeDelta::days(31))
}

fn wrap(text: &str) -> String {
    text.as_bytes()
        .chunks(LINE_WIDTH)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

fn verified(text: &str) -> License {
    keyring().verify(text).unwrap()
}

#[tokio::test]
async fn a_second_license_row_violates_the_singleton_check() {
    let fixture = Fixture::new().await;

    let error = sqlx::query("INSERT INTO licenses (id, key) VALUES (2, 'second')")
        .execute(fixture.pool())
        .await
        .unwrap_err();

    let error = error.as_database_error().expect("a database error");
    assert_eq!(error.code().as_deref(), Some(CHECK_VIOLATION));
    assert_eq!(error.constraint(), Some(SINGLETON_CHECK));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_stored_key_reads_back() {
    let fixture = Fixture::new().await;

    licenses::set(fixture.pool(), "first").await.unwrap();

    assert_eq!(
        licenses::get(fixture.pool()).await.unwrap().as_deref(),
        Some("first")
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_second_key_replaces_the_first() {
    let fixture = Fixture::new().await;
    licenses::set(fixture.pool(), "first").await.unwrap();
    let first: DateTime<Utc> = sqlx::query_scalar("SELECT updated_at FROM licenses")
        .fetch_one(fixture.pool())
        .await
        .unwrap();

    licenses::set(fixture.pool(), "second").await.unwrap();

    let second: DateTime<Utc> = sqlx::query_scalar("SELECT updated_at FROM licenses")
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    assert_eq!(
        licenses::get(fixture.pool()).await.unwrap().as_deref(),
        Some("second")
    );
    assert_eq!(fixture.rows().await, 1);
    assert!(second > first, "{second} is not after {first}");
    fixture.cleanup().await;
}

#[tokio::test]
async fn removing_returns_the_stored_key_once() {
    let fixture = Fixture::new().await;
    licenses::set(fixture.pool(), "stored").await.unwrap();

    assert_eq!(
        licenses::remove(fixture.pool()).await.unwrap().as_deref(),
        Some("stored")
    );
    assert_eq!(licenses::remove(fixture.pool()).await.unwrap(), None);
    assert_eq!(licenses::get(fixture.pool()).await.unwrap(), None);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_empty_server_is_community() {
    let fixture = Fixture::new().await;

    assert_eq!(
        fixture
            .state()
            .licenses()
            .load(fixture.pool())
            .await
            .unwrap(),
        None
    );
    assert_eq!(fixture.state().license().await, None);
    assert_eq!(fixture.state().edition().await, Edition::Community);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_team_key_gives_team_and_an_enterprise_key_replaces_it() {
    let fixture = Fixture::new().await;
    let licenses = fixture.state().licenses();
    let team = verified(&active(Tier::Team));
    let enterprise = verified(&active(Tier::Enterprise));

    let change = licenses
        .set(fixture.pool(), team.text(), Utc::now())
        .await
        .unwrap();
    assert_eq!(change.previous, None);
    assert_eq!(change.current, team);
    assert_eq!(fixture.state().edition().await, Edition::Team);

    let change = licenses
        .set(fixture.pool(), enterprise.text(), Utc::now())
        .await
        .unwrap();
    assert_eq!(change.previous, Some(team));
    assert_eq!(change.current, enterprise);
    assert_eq!(fixture.state().edition().await, Edition::Enterprise);
    assert_eq!(
        licenses.load(fixture.pool()).await.unwrap(),
        Some(enterprise.clone())
    );
    assert_eq!(fixture.rows().await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_wrapped_key_is_stored_without_its_whitespace() {
    let fixture = Fixture::new().await;
    let text = active(Tier::Team);

    fixture
        .state()
        .licenses()
        .set(fixture.pool(), &format!("  {}\n", wrap(&text)), Utc::now())
        .await
        .unwrap();

    assert_eq!(licenses::get(fixture.pool()).await.unwrap(), Some(text));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_key_past_grace_is_refused_and_the_stored_one_stays() {
    let fixture = Fixture::new().await;
    let licenses = fixture.state().licenses();

    let error = licenses
        .set(fixture.pool(), &lapsed(Tier::Team), Utc::now())
        .await
        .unwrap_err();
    assert!(matches!(error, SetError::Lapsed), "{error:?}");
    assert_eq!(fixture.rows().await, 0);

    let stored = verified(&active(Tier::Team));
    licenses
        .set(fixture.pool(), stored.text(), Utc::now())
        .await
        .unwrap();
    let error = licenses
        .set(fixture.pool(), &lapsed(Tier::Enterprise), Utc::now())
        .await
        .unwrap_err();
    assert!(matches!(error, SetError::Lapsed), "{error:?}");
    assert_eq!(
        licenses::get(fixture.pool()).await.unwrap().as_deref(),
        Some(stored.text())
    );
    assert_eq!(fixture.state().edition().await, Edition::Team);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_key_in_grace_is_stored_with_its_tier() {
    let fixture = Fixture::new().await;

    for (tier, edition) in [
        (Tier::Team, Edition::Team),
        (Tier::Enterprise, Edition::Enterprise),
    ] {
        let change = fixture
            .state()
            .licenses()
            .set(fixture.pool(), &in_grace(tier), Utc::now())
            .await
            .unwrap();

        let loaded = fixture
            .state()
            .licenses()
            .load(fixture.pool())
            .await
            .unwrap()
            .expect("the key in grace was stored");
        assert_eq!(loaded, change.current);
        assert_eq!(loaded.status(Utc::now()), Status::Grace);
        assert_eq!(fixture.state().edition().await, edition);
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_key_stored_directly_keeps_its_tier() {
    let fixture = Fixture::new().await;
    let text = lapsed(Tier::Enterprise);
    licenses::set(fixture.pool(), &text).await.unwrap();

    let license = fixture
        .state()
        .license()
        .await
        .expect("a lapsed key still verifies");

    assert_eq!(license, verified(&text));
    assert_eq!(license.status(Utc::now()), Status::Lapsed);
    assert_eq!(fixture.state().edition().await, Edition::Enterprise);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_stored_key_the_keyring_does_not_trust_loads_as_community() {
    let fixture = Fixture::new().await;
    let unknown = sign(
        &Payload {
            key_id: UNTRUSTED.into(),
            ..payload(Tier::Enterprise, Utc::now() + TimeDelta::days(365))
        },
        &untrusted(),
    );
    let forged = sign(
        &payload(Tier::Enterprise, Utc::now() + TimeDelta::days(365)),
        &untrusted(),
    );

    for text in [unknown, forged] {
        licenses::set(fixture.pool(), &text).await.unwrap();

        assert_eq!(
            fixture
                .state()
                .licenses()
                .load(fixture.pool())
                .await
                .unwrap(),
            None
        );
        assert_eq!(fixture.state().edition().await, Edition::Community);
        assert_eq!(licenses::get(fixture.pool()).await.unwrap(), Some(text));
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn removing_returns_the_license_and_leaves_community() {
    let fixture = Fixture::new().await;
    let licenses = fixture.state().licenses();
    let stored = verified(&active(Tier::Team));
    licenses
        .set(fixture.pool(), stored.text(), Utc::now())
        .await
        .unwrap();

    assert_eq!(licenses.remove(fixture.pool()).await.unwrap(), Some(stored));
    assert_eq!(fixture.state().edition().await, Edition::Community);
    assert_eq!(licenses.remove(fixture.pool()).await.unwrap(), None);
    assert_eq!(fixture.rows().await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_last_license_that_loaded_survives_a_database_failure() {
    let fixture = Fixture::new().await;
    let licenses = Licenses::with_reload_after(Duration::ZERO);
    licenses.install_keyring(keyring());
    let stored = verified(&active(Tier::Enterprise));
    licenses::set(fixture.pool(), stored.text()).await.unwrap();
    assert_eq!(licenses.current(fixture.pool()).await, Some(stored.clone()));

    fixture.pool().close().await;

    assert!(licenses.load(fixture.pool()).await.is_err());
    assert_eq!(licenses.current(fixture.pool()).await, Some(stored.clone()));
    assert_eq!(licenses.current(fixture.pool()).await, Some(stored));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_failed_reload_waits_an_interval_before_asking_again() {
    let fixture = Fixture::new().await;
    let licenses = Licenses::new();
    licenses.install_keyring(keyring());
    let closed =
        PgPoolOptions::new().connect_lazy_with((*fixture.pool().connect_options()).clone());
    closed.close().await;
    assert_eq!(licenses.current(&closed).await, None);

    let stored = verified(&active(Tier::Team));
    licenses::set(fixture.pool(), stored.text()).await.unwrap();

    assert_eq!(licenses.current(fixture.pool()).await, None);
    assert_eq!(
        licenses.load(fixture.pool()).await.unwrap(),
        Some(stored.clone())
    );
    assert_eq!(licenses.current(fixture.pool()).await, Some(stored));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_reload_sees_a_key_another_instance_stored() {
    let fixture = Fixture::new().await;
    let licenses = Licenses::with_reload_after(Duration::ZERO);
    licenses.install_keyring(keyring());
    assert_eq!(licenses.current(fixture.pool()).await, None);

    let stored = verified(&active(Tier::Team));
    licenses::set(fixture.pool(), stored.text()).await.unwrap();

    assert_eq!(licenses.current(fixture.pool()).await, Some(stored));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_fresh_cache_answers_without_the_database() {
    let fixture = Fixture::new().await;
    let stored = verified(&active(Tier::Team));
    fixture
        .state()
        .licenses()
        .set(fixture.pool(), stored.text(), Utc::now())
        .await
        .unwrap();

    licenses::remove(fixture.pool()).await.unwrap();

    assert_eq!(fixture.state().license().await, Some(stored));
    assert_eq!(
        fixture
            .state()
            .licenses()
            .load(fixture.pool())
            .await
            .unwrap(),
        None
    );
    assert_eq!(fixture.state().edition().await, Edition::Community);
    fixture.cleanup().await;
}
