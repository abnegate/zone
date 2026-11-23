//! A server of its own for single sign-on tests: a fresh database, a trusted license keyring,
//! account mail that is recorded, and helpers for the admin routes. The first account registered
//! is the server admin.

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use chrono::{SubsecRound, TimeDelta, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::{GRACE_DAYS, Keyring, Payload, Plan, SigningKey, TrustedKey, sign};
use zone_server::db::{licenses, migrations};
use zone_server::services::mail::AccountMail;
use zone_server::state::AppState;

use super::mail::Recording;
use super::{Database, TestClient, TestResponse, test_config, test_email, test_password};

pub const SETTINGS: &str = "/api/sso";
pub const PROVIDERS: &str = "/api/sso/providers";
pub const ENFORCEMENT: &str = "/api/sso/enforcement";

const KEY_ID: &str = "test-1";
const SEED: [u8; 32] = [0x11; 32];
const SEATS: u32 = 25;
const MAIL_WAIT: Duration = Duration::from_secs(5);
const MAIL_POLL: Duration = Duration::from_millis(20);

pub struct Account {
    pub id: Uuid,
    pub email: String,
    pub token: String,
    pub refresh: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct Audit {
    pub actor_id: Option<Uuid>,
    pub organization_id: Option<Uuid>,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub old_values: Option<Value>,
    pub new_values: Option<Value>,
}

pub struct Fixture {
    pub database: Database,
    pub client: TestClient,
    pub mail: Arc<Recording>,
}

impl Fixture {
    pub async fn new(prefix: &'static str) -> Self {
        let database = Database::new(prefix).await;
        migrations::run(&database.pool).await.unwrap();
        let mail = Arc::new(Recording::default());
        let state = AppState::new_with_mail(
            test_config(),
            database.pool.clone(),
            None,
            Some(Arc::new(AccountMail::new(mail.clone()))),
        );
        state.disable_mcp();
        state.install_keyring(keyring());
        let client = TestClient::with_state(state);
        Self {
            database,
            client,
            mail,
        }
    }

    /// As [`Fixture::new`], with the Community member cap in force.
    pub async fn capped(prefix: &'static str) -> Self {
        let fixture = Self::new(prefix).await;
        fixture.state().cap_members();
        fixture
    }

    pub fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    pub fn state(&self) -> &AppState {
        self.client.state()
    }

    pub async fn register(&self) -> Account {
        let email = test_email();
        let response = self.register_as(&email).await;
        response.assert_status(StatusCode::CREATED);
        let body = response.json_value();
        Account {
            id: body["user"]["id"].as_str().unwrap().parse().unwrap(),
            email,
            token: body["access_token"].as_str().unwrap().to_owned(),
            refresh: body["refresh_token"].as_str().unwrap().to_owned(),
        }
    }

    pub async fn register_as(&self, email: &str) -> TestResponse {
        self.client
            .post_json(
                "/api/auth/register",
                &json!({ "email": email, "password": test_password() }),
            )
            .await
    }

    pub async fn login(&self, email: &str, password: &str) -> TestResponse {
        self.client
            .post_json(
                "/api/auth/login",
                &json!({ "email": email, "password": password }),
            )
            .await
    }

    /// Installs an Enterprise license that is active, as the admin would.
    pub async fn license(&self, admin: &Account) {
        self.install(admin, &active()).await;
    }

    pub async fn install(&self, admin: &Account, key: &str) {
        self.client
            .put_json_auth("/api/license", &json!({ "key": key }), &admin.token)
            .await
            .assert_status(StatusCode::OK);
    }

    pub async fn remove_license(&self, admin: &Account) {
        self.client
            .delete_auth("/api/license", &admin.token)
            .await
            .assert_status(StatusCode::OK);
    }

    /// Stores a key the license route would refuse, such as a lapsed one, and reloads it.
    pub async fn store_license(&self, key: &str) {
        licenses::replace(self.pool(), key).await.unwrap();
        self.state().licenses().load(self.pool()).await.unwrap();
    }

    pub async fn settings(&self, token: &str) -> TestResponse {
        self.client.get_auth(SETTINGS, token).await
    }

    pub async fn create(&self, token: &str, body: &Value) -> TestResponse {
        self.client.post_json_auth(PROVIDERS, body, token).await
    }

    /// Creates a provider and returns its id, failing the test unless it was created.
    pub async fn created(&self, token: &str, body: &Value) -> Uuid {
        let response = self.create(token, body).await;
        response.assert_status(StatusCode::CREATED);
        response.json_value()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    pub async fn update(
        &self,
        token: &str,
        id: impl std::fmt::Display,
        body: &Value,
    ) -> TestResponse {
        self.client
            .patch_json_auth(&format!("{PROVIDERS}/{id}"), body, token)
            .await
    }

    pub async fn delete(&self, token: &str, id: impl std::fmt::Display) -> TestResponse {
        self.client
            .delete_auth(&format!("{PROVIDERS}/{id}"), token)
            .await
    }

    pub async fn enforce(&self, token: &str, required: bool) -> TestResponse {
        self.client
            .put_json_auth(ENFORCEMENT, &json!({ "required": required }), token)
            .await
    }

    pub async fn providers(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM sso_providers")
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    pub async fn audits(&self, action: &str) -> Vec<Audit> {
        sqlx::query_as(
            "SELECT actor_id, organization_id, resource_type, resource_id, old_values, new_values
             FROM audit_logs WHERE action = $1 ORDER BY created_at",
        )
        .bind(action)
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    /// Every audit row as text, for asserting what it must never contain.
    pub async fn audit_text(&self) -> String {
        sqlx::query_scalar::<_, String>("SELECT row_to_json(a)::text FROM audit_logs a")
            .fetch_all(self.pool())
            .await
            .unwrap()
            .join("\n")
    }

    /// Waits for account mail to reach `recipient`. Mail is sent on a task of its own after the
    /// request has been answered.
    pub async fn mailed(&self, recipient: &str) -> bool {
        let deadline = tokio::time::Instant::now() + MAIL_WAIT;
        while tokio::time::Instant::now() < deadline {
            if self.mail.recipients().iter().any(|sent| sent == recipient) {
                return true;
            }
            tokio::time::sleep(MAIL_POLL).await;
        }
        false
    }

    pub async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

pub fn keyring() -> Keyring {
    let signing = SigningKey::from_bytes(&SEED);
    Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()]).unwrap()
}

fn expiring(expires_in: TimeDelta, plan: Option<Plan>) -> String {
    let expires_at = Utc::now().trunc_subsecs(0) + expires_in;
    sign(
        &Payload {
            id: Uuid::new_v4(),
            customer: "Acme Ltd".into(),
            seats: SEATS,
            issued_at: expires_at - TimeDelta::days(365),
            expires_at,
            key_id: KEY_ID.into(),
            subscription: None,
            plan,
        },
        &SigningKey::from_bytes(&SEED),
    )
}

/// An Enterprise license that names no plan, as every key signed before plans did.
pub fn active() -> String {
    expiring(TimeDelta::days(365), None)
}

pub fn grace() -> String {
    expiring(-TimeDelta::days(1), None)
}

pub fn lapsed() -> String {
    expiring(-TimeDelta::days(GRACE_DAYS + 1), None)
}

pub fn team() -> String {
    expiring(TimeDelta::days(365), Some(Plan::Team))
}

pub fn lapsed_team() -> String {
    expiring(-TimeDelta::days(GRACE_DAYS + 1), Some(Plan::Team))
}
