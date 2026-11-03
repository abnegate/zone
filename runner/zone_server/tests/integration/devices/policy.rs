use std::panic::AssertUnwindSafe;

use axum::http::StatusCode;
use futures::FutureExt;
use serde_json::json;
use tokio::sync::Mutex;

use super::{Phone, device_named, listed_devices, login, register};
use crate::common::TestClient;
use zone_server::db::devices::{self, Mode};

static POLICY: Mutex<()> = Mutex::const_new(());

async fn with_mode<T>(
    pool: &sqlx::PgPool,
    mode: Mode,
    run: impl std::future::Future<Output = T>,
) -> T {
    let _lock = POLICY.lock().await;
    let previous = devices::policy(pool).await.expect("policy");
    devices::set_policy(pool, mode).await.expect("set policy");
    let output = AssertUnwindSafe(run).catch_unwind().await;
    devices::set_policy(pool, previous)
        .await
        .expect("restore policy");
    output.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

#[tokio::test]
async fn an_unknown_device_waits_when_only_allowed_devices_may_connect() {
    let client = TestClient::with_db().await;
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
}

#[tokio::test]
async fn allowing_a_pending_device_lets_it_sign_in() {
    let client = TestClient::with_db().await;
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
}

#[tokio::test]
async fn the_last_allowed_admin_device_cannot_be_blocked_while_locked() {
    let client = TestClient::with_db().await;
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
    if !devices::is_last_allowed_admin(Mode::Allowed, devices::Status::Allowed, true, count) {
        return;
    }

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
}

#[tokio::test]
async fn open_mode_allows_a_pending_device_on_the_next_login() {
    let client = TestClient::with_db().await;
    let phone = Phone::named("Desk", "desktop");
    let account = register(&client, &phone).await;
    let pool = client.state().db();
    let unknown = Phone::named("Visitor", "browser");

    with_mode(pool, Mode::Allowed, login(&client, &account, &unknown)).await;

    let allowed = login(&client, &account, &unknown).await;
    allowed.assert_status(StatusCode::OK);

    let listed = listed_devices(&client, &account).await;
    assert_eq!(device_named(&listed, "Visitor")["status"], json!("allowed"));
}
