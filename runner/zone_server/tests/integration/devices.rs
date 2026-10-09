use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::common::{TestClient, TestResponse, test_email, test_password};
use zone_server::db::devices::{self, Mode};

static POLICY: Mutex<()> = Mutex::const_new(());

struct Account {
    token: String,
    refresh: String,
    user: Uuid,
    email: String,
    password: String,
    org: Uuid,
}

struct Phone {
    id: Uuid,
    name: &'static str,
    platform: &'static str,
    ip: &'static str,
}

impl Phone {
    fn named(name: &'static str, platform: &'static str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            platform,
            ip: "192.168.4.31",
        }
    }
}

async fn register(client: &TestClient, phone: &Phone) -> Account {
    let email = test_email();
    let password = test_password();
    let response = post_auth(
        client,
        "/api/auth/register",
        &json!({
            "email": email,
            "password": password,
            "display_name": "Device Owner"
        }),
        phone,
        None,
    )
    .await;
    response.assert_status(StatusCode::CREATED);
    account_from(client, email, password, response.json_value()).await
}

async fn login(client: &TestClient, account: &Account, phone: &Phone) -> TestResponse {
    post_auth(
        client,
        "/api/auth/login",
        &json!({ "email": account.email, "password": account.password }),
        phone,
        None,
    )
    .await
}

async fn account_from(
    client: &TestClient,
    email: String,
    password: String,
    body: Value,
) -> Account {
    let token = body["access_token"].as_str().unwrap().to_string();
    let refresh = body["refresh_token"].as_str().unwrap().to_string();
    let user = Uuid::parse_str(body["user"]["id"].as_str().unwrap()).unwrap();
    let orgs = client.get_auth("/api/organizations", &token).await;
    orgs.assert_status(StatusCode::OK);
    let org = Uuid::parse_str(
        orgs.json_value()["organizations"][0]["id"]
            .as_str()
            .expect("register creates a default organization"),
    )
    .unwrap();
    Account {
        token,
        refresh,
        user,
        email,
        password,
        org,
    }
}

async fn post_auth(
    client: &TestClient,
    uri: &str,
    body: &Value,
    phone: &Phone,
    token: Option<&str>,
) -> TestResponse {
    let mut request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("x-zone-device", phone.id.to_string())
        .header("x-zone-device-name", phone.name)
        .header("x-zone-device-platform", phone.platform)
        .header("x-forwarded-for", phone.ip)
        .header("user-agent", "Zone/1");
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    client
        .send_request(
            request
                .body(Body::from(serde_json::to_string(body).unwrap()))
                .unwrap(),
        )
        .await
}

async fn listed_devices(client: &TestClient, account: &Account) -> Value {
    let response = client
        .get_auth(
            &format!("/api/organizations/{}/devices", account.org),
            &account.token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    response.json_value()
}

fn device_named<'a>(body: &'a Value, name: &str) -> &'a Value {
    body["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("device {name} missing from {body}"))
}

async fn with_mode<T>(
    pool: &sqlx::PgPool,
    mode: Mode,
    run: impl std::future::Future<Output = T>,
) -> T {
    let _lock = POLICY.lock().await;
    let previous = devices::policy(pool).await.expect("policy");
    devices::set_policy(pool, mode).await.expect("set policy");
    let output = run.await;
    devices::set_policy(pool, previous)
        .await
        .expect("restore policy");
    output
}

#[tokio::test]
async fn login_records_the_device_and_lists_it_connected() {
    let client = TestClient::with_db().await;
    let phone = Phone::named("S23 Ultra", "android");
    let account = register(&client, &phone).await;

    let body = listed_devices(&client, &account).await;
    let device = device_named(&body, "S23 Ultra");
    assert_eq!(device["platform"], json!("android"));
    assert_eq!(device["status"], json!("allowed"));
    assert_eq!(device["connected"], json!(true));
    assert_eq!(device["last_ip"], json!("192.168.4.31"));
    assert_eq!(device["email"], json!(account.email));
    assert!(device["session_count"].as_i64().unwrap() >= 1);

    let policy = client
        .get_auth(
            &format!("/api/organizations/{}/device-policy", account.org),
            &account.token,
        )
        .await;
    policy.assert_status(StatusCode::OK);
    assert_eq!(policy.json_value()["mode"], json!("open"));
}

#[tokio::test]
async fn block_signs_the_device_out_and_refuses_it_again() {
    let client = TestClient::with_db().await;
    let phone = Phone::named("Chrome", "browser");
    let account = register(&client, &phone).await;
    let listed = listed_devices(&client, &account).await;
    let device_id = device_named(&listed, "Chrome")["id"].as_str().unwrap();

    let blocked = client
        .patch_json_auth(
            &format!("/api/organizations/{}/devices/{device_id}", account.org),
            &json!({ "status": "blocked" }),
            &account.token,
        )
        .await;
    blocked.assert_status(StatusCode::OK);
    assert_eq!(blocked.json_value()["status"], json!("blocked"));

    client
        .get_auth("/api/organizations", &account.token)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    let refused = login(&client, &account, &phone).await;
    refused.assert_status(StatusCode::FORBIDDEN);
    let body = refused.json_value();
    assert_eq!(body["code"], json!("device_blocked"));
    assert!(
        body["error"].as_str().unwrap().contains("blocked"),
        "{body}"
    );
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
async fn a_member_cannot_list_or_block_devices() {
    let client = TestClient::with_db().await;
    let owner_phone = Phone::named("Owner", "desktop");
    let owner = register(&client, &owner_phone).await;
    let member_phone = Phone::named("Member", "browser");
    let member = register(&client, &member_phone).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{}/members", owner.org),
            &json!({ "user_id": member.user, "role": "member" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    client
        .get_auth(
            &format!("/api/organizations/{}/devices", owner.org),
            &member.token,
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);

    let listed = listed_devices(&client, &owner).await;
    let device_id = listed["devices"][0]["id"].as_str().unwrap();
    client
        .patch_json_auth(
            &format!("/api/organizations/{}/devices/{device_id}", owner.org),
            &json!({ "status": "blocked" }),
            &member.token,
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn refresh_without_a_device_header_reuses_the_session_device() {
    let client = TestClient::with_db().await;
    let phone = Phone::named("CLI", "cli");
    let account = register(&client, &phone).await;

    let response = client
        .post_json(
            "/api/auth/refresh",
            &json!({ "refresh_token": account.refresh }),
        )
        .await;
    response.assert_status(StatusCode::OK);
    assert!(response.json_value()["access_token"].is_string());

    let listed = listed_devices(&client, &account).await;
    assert_eq!(listed["devices"].as_array().unwrap().len(), 1);
    assert_eq!(device_named(&listed, "CLI")["platform"], json!("cli"));
}

#[tokio::test]
async fn an_admin_can_rename_a_device() {
    let client = TestClient::with_db().await;
    let phone = Phone::named("Phone", "android");
    let account = register(&client, &phone).await;
    let listed = listed_devices(&client, &account).await;
    let device_id = device_named(&listed, "Phone")["id"].as_str().unwrap();

    let renamed = client
        .patch_json_auth(
            &format!("/api/organizations/{}/devices/{device_id}", account.org),
            &json!({ "name": "Galaxy" }),
            &account.token,
        )
        .await;
    renamed.assert_status(StatusCode::OK);
    assert_eq!(renamed.json_value()["name"], json!("Galaxy"));
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
