use axum::http::StatusCode;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

use super::{Phone, device_named, listed_devices, login, register};
use crate::common::TestClient;
use zone_server::db::devices::{self, Mode};
use zone_server::db::migrations;

/// The device policy is one instance-wide row, so each test that switches it
/// gets a database of its own and every other test keeps signing in.
struct Database {
    admin: PgPool,
    pool: PgPool,
    name: String,
}

impl Database {
    async fn new() -> Self {
        let admin =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("disposable database"))
                .await
                .expect("the test database is reachable");
        // Identifiers contain only a fixed prefix and UUID hexadecimal.
        let name = format!("device_policy_{}", Uuid::new_v4().simple());
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await
            .expect("a database for the policy test");
        let options = (*admin.connect_options()).clone().database(&name);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("the policy test database is reachable");
        migrations::run(&pool)
            .await
            .expect("the policy test database migrates");
        Self { admin, pool, name }
    }

    fn client(&self) -> TestClient {
        TestClient::with_pool(self.pool.clone())
    }

    async fn close(self) {
        self.pool.close().await;
        sqlx::query(AssertSqlSafe(format!("DROP DATABASE {}", self.name)))
            .execute(&self.admin)
            .await
            .expect("the policy test database drops");
        self.admin.close().await;
    }
}

async fn with_mode<T>(pool: &PgPool, mode: Mode, run: impl std::future::Future<Output = T>) -> T {
    let previous = devices::policy(pool).await.expect("policy");
    devices::set_policy(pool, mode).await.expect("set policy");
    let output = run.await;
    devices::set_policy(pool, previous)
        .await
        .expect("restore policy");
    output
}

#[tokio::test]
async fn an_unknown_device_waits_when_only_allowed_devices_may_connect() {
    let database = Database::new().await;
    let client = database.client();
    let phone = Phone::named("Laptop", "desktop");
    let account = register(&client, &phone).await;
    let pool = client.state().db();
    let unknown = Phone::named("New Phone", "android");

    let (lock, pending) = with_mode(pool, Mode::Allowed, async {
        let lock = client
            .put_json_auth(
                &format!("/api/organizations/{}/device-policy", account.org),
                &json!({ "mode": "allowed" }),
                &account.token,
            )
            .await;
        let pending = login(&client, &account, &unknown).await;
        (lock.status, pending)
    })
    .await;

    assert_eq!(lock, StatusCode::OK);
    pending.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(pending.json_value()["code"], json!("device_pending"));

    let listed = listed_devices(&client, &account).await;
    let device = device_named(&listed, "New Phone");
    assert_eq!(device["status"], json!("pending"));

    database.close().await;
}

#[tokio::test]
async fn allowing_a_pending_device_lets_it_sign_in() {
    let database = Database::new().await;
    let client = database.client();
    let phone = Phone::named("Owner", "desktop");
    let account = register(&client, &phone).await;
    let pool = client.state().db();
    let unknown = Phone::named("Tablet", "ios");

    with_mode(pool, Mode::Allowed, async {
        login(&client, &account, &unknown).await;
    })
    .await;

    let listed = listed_devices(&client, &account).await;
    let device_id = device_named(&listed, "Tablet")["id"].as_str().unwrap();
    client
        .patch_json_auth(
            &format!("/api/organizations/{}/devices/{device_id}", account.org),
            &json!({ "status": "allowed" }),
            &account.token,
        )
        .await
        .assert_status(StatusCode::OK);

    let signed_in = with_mode(pool, Mode::Allowed, login(&client, &account, &unknown)).await;
    signed_in.assert_status(StatusCode::OK);

    database.close().await;
}

#[tokio::test]
async fn the_last_allowed_admin_device_cannot_be_blocked_while_locked() {
    let database = Database::new().await;
    let client = database.client();
    let phone = Phone::named("Admin", "desktop");
    let account = register(&client, &phone).await;
    let pool = client.state().db();
    sqlx::query("UPDATE users SET is_admin = true WHERE id = $1")
        .bind(account.user)
        .execute(pool)
        .await
        .unwrap();

    let listed = listed_devices(&client, &account).await;
    let device_id = device_named(&listed, "Admin")["id"].as_str().unwrap();
    let count = devices::allowed_admin_count(pool).await.unwrap();
    assert!(
        devices::is_last_allowed_admin(Mode::Allowed, devices::Status::Allowed, true, count),
        "a fresh database holds one allowed admin device, found {count}"
    );

    let blocked = with_mode(pool, Mode::Allowed, async {
        client
            .put_json_auth(
                &format!("/api/organizations/{}/device-policy", account.org),
                &json!({ "mode": "allowed" }),
                &account.token,
            )
            .await
            .assert_status(StatusCode::OK);
        client
            .patch_json_auth(
                &format!("/api/organizations/{}/devices/{device_id}", account.org),
                &json!({ "status": "blocked" }),
                &account.token,
            )
            .await
    })
    .await;

    blocked.assert_status(StatusCode::CONFLICT);
    assert!(
        blocked.json_value()["error"]
            .as_str()
            .unwrap()
            .contains("last allowed admin"),
        "{}",
        blocked.text()
    );

    database.close().await;
}

#[tokio::test]
async fn open_mode_allows_a_pending_device_on_the_next_login() {
    let database = Database::new().await;
    let client = database.client();
    let phone = Phone::named("Desk", "desktop");
    let account = register(&client, &phone).await;
    let pool = client.state().db();
    let unknown = Phone::named("Visitor", "browser");

    with_mode(pool, Mode::Allowed, login(&client, &account, &unknown)).await;

    let allowed = login(&client, &account, &unknown).await;
    allowed.assert_status(StatusCode::OK);

    let listed = listed_devices(&client, &account).await;
    assert_eq!(device_named(&listed, "Visitor")["status"], json!("allowed"));

    database.close().await;
}
