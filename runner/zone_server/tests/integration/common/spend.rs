//! A server whose LiteLLM is a stand-in, with one organization and one workspace owned by the
//! server admin, and helpers for spend. Wraps [`policy::Fixture`]; every fixture has a database
//! of its own, because the license and the ingest cursor are server-wide.

use std::sync::Arc;

use axum::body::Body;
use axum::http;
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};
use zone_enterprise::spend::{Budget, DEFAULT_THRESHOLD, Microdollars, Month, Target};
use zone_server::config::Config;
use zone_server::db::migrations;
use zone_server::db::spend::budgets::{self, BudgetRow};
use zone_server::db::spend::keys;
use zone_server::services::endpoint::Endpoint;
use zone_server::services::mail::AccountMail;
use zone_server::services::spend::{self, Client, Key, Subject};
use zone_server::state::AppState;

use super::database::Database;
use super::mail::Recording;
use super::{TestClient, TestResponse, policy, sso, test_config};

/// The stand-in LiteLLM's master key, the instance's `LITELLM_KEY`.
pub const MASTER: &str = "sk-spend-master-0123456789";
pub const GENERATE_PATH: &str = "/key/generate";
pub const INFO_PATH: &str = "/key/info";
pub const DELETE_PATH: &str = "/key/delete";
pub const LOGS_PATH: &str = "/spend/logs/v2";

pub fn reports_path(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/spend-reports")
}

pub fn exports_path(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/spend-exports")
}

pub fn budgets_path(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/budgets")
}

pub fn budget_path(organization: Uuid, budget: impl std::fmt::Display) -> String {
    format!("/api/organizations/{organization}/budgets/{budget}")
}

/// A request's spend as ingest stores it, written straight to `spend_records`.
#[derive(Debug, Clone)]
pub struct Spend {
    pub organization: Uuid,
    pub workspace: Option<Uuid>,
    pub user: Option<Uuid>,
    pub automated: bool,
    pub model: String,
    pub provider: String,
    pub microdollars: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub occurred_at: DateTime<Utc>,
}

pub struct Fixture {
    pub policy: policy::Fixture,
    pub litellm: MockServer,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Stored {
    pub request_id: String,
    pub organization_id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub automated: bool,
    pub token: String,
    pub model: String,
    pub provider: String,
    pub spend_microdollars: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub failed: bool,
    pub occurred_at: DateTime<Utc>,
}

impl Fixture {
    pub async fn new(prefix: &'static str) -> Self {
        Self::configured(prefix, |_| {}).await
    }

    /// [`Self::new`] with its config changed by `configure` before the server starts.
    pub async fn configured(prefix: &'static str, configure: impl FnOnce(&mut Config)) -> Self {
        // A pooled server keeps its address across tests, and the key registry is keyed by
        // LiteLLM's address for the whole process.
        let litellm = MockServer::builder().start().await;
        let mut config = Config {
            litellm_host: litellm.uri(),
            litellm_key: MASTER.to_string(),
            ..test_config()
        };
        configure(&mut config);
        let database = Database::new(prefix).await;
        migrations::run(&database.pool).await.unwrap();
        let mail = Arc::new(Recording::default());
        let state = AppState::new_with_mail(
            config,
            database.pool.clone(),
            None,
            Some(Arc::new(AccountMail::new(mail.clone()))),
        );
        state.disable_mcp();
        state.install_keyring(sso::keyring());
        let client = TestClient::with_state(state);
        let policy = policy::Fixture::on(sso::Fixture {
            database,
            client,
            mail,
        })
        .await;
        Self { policy, litellm }
    }

    pub fn pool(&self) -> &PgPool {
        self.policy.pool()
    }

    pub fn state(&self) -> &AppState {
        self.policy.state()
    }

    /// Installs a Team license that is active.
    pub async fn licensed(&self) {
        self.policy.license().await;
    }

    /// Stores a license past its grace period.
    pub async fn lapsed(&self) {
        self.policy.store_license(&sso::lapsed()).await;
    }

    /// The owner's own spend in the fixture's workspace.
    pub fn subject(&self) -> Subject {
        Subject {
            organization: self.policy.organization,
            workspace: self.policy.workspace,
            user: Some(self.policy.owner.id),
        }
    }

    /// The fixture's workspace's automation.
    pub fn automation(&self) -> Subject {
        Subject {
            user: None,
            ..self.subject()
        }
    }

    pub fn client(&self) -> Client {
        spend::client(self.state()).expect("the stand-in LiteLLM is configured")
    }

    /// The instance's LiteLLM under its master key.
    pub fn instance(&self) -> Endpoint {
        Endpoint::instance(self.state().config())
    }

    /// The key the server derives for `subject`.
    pub fn key(&self, subject: &Subject) -> Key {
        Key::derive(self.state().master_key(), subject.workspace, subject.user)
    }

    /// `/key/generate` answering as LiteLLM does, unmounted so a test can set expectations.
    pub fn registration() -> Mock {
        Mock::given(method("POST"))
            .and(path(GENERATE_PATH))
            .respond_with(Registers)
    }

    pub async fn mount_register(&self) {
        Self::registration().mount(&self.litellm).await;
    }

    /// `/spend/logs/v2` answering every page with `rows`, fewer than a page holds.
    pub async fn mount_logs(&self, rows: Vec<Value>) {
        let total = rows.len();
        Mock::given(method("GET"))
            .and(path(LOGS_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": rows,
                "total": total,
                "page": 1,
                "page_size": 1000,
                "total_pages": 1,
                "total_is_capped": false,
            })))
            .mount(&self.litellm)
            .await;
    }

    /// `/key/info` answering that LiteLLM has no such key.
    pub async fn mount_missing_key(&self) {
        Mock::given(method("GET"))
            .and(path(INFO_PATH))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(json!({ "error": { "message": "Key not found in database" } })),
            )
            .mount(&self.litellm)
            .await;
    }

    /// Every request the stand-in LiteLLM received at `route`.
    pub async fn received(&self, route: &str) -> Vec<Request> {
        self.litellm
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|request| request.url.path() == route)
            .collect()
    }

    pub async fn stored(&self) -> Vec<Stored> {
        sqlx::query_as(
            "SELECT request_id, organization_id, workspace_id, user_id, automated, token, model,
                    provider, spend_microdollars, prompt_tokens, completion_tokens, failed,
                    occurred_at
             FROM spend_records ORDER BY request_id",
        )
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    /// A dollar of the owner's spend in the fixture's workspace on `gpt-4o` through OpenAI, now.
    pub fn spend(&self) -> Spend {
        Spend {
            organization: self.policy.organization,
            workspace: Some(self.policy.workspace),
            user: Some(self.policy.owner.id),
            automated: false,
            model: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            microdollars: 1_000_000,
            prompt_tokens: 100,
            completion_tokens: 20,
            occurred_at: Utc::now(),
        }
    }

    pub async fn record(&self, spend: Spend) {
        sqlx::query(
            "INSERT INTO spend_records (request_id, organization_id, workspace_id, user_id,
                 automated, token, model, provider, spend_microdollars, prompt_tokens,
                 completion_tokens, occurred_at)
             VALUES ($1, $2, $3, $4, $5, 'spend-routes-token', $6, $7, $8, $9, $10, $11)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(spend.organization)
        .bind(spend.workspace)
        .bind(spend.user)
        .bind(spend.automated)
        .bind(spend.model)
        .bind(spend.provider)
        .bind(spend.microdollars)
        .bind(spend.prompt_tokens)
        .bind(spend.completion_tokens)
        .bind(spend.occurred_at)
        .execute(self.pool())
        .await
        .unwrap();
    }

    /// Records `subject`'s key as derived, as its first attributed request would, and returns it.
    pub async fn derive(&self, subject: &Subject) -> Key {
        let key = self.key(subject);
        keys::insert(
            self.pool(),
            &key.token,
            subject.organization,
            subject.workspace,
            subject.user,
        )
        .await
        .unwrap();
        key
    }

    /// Derives `subject`'s key and stores `microdollars` of its spend from a minute ago.
    pub async fn charge(&self, subject: &Subject, microdollars: i64) {
        self.derive(subject).await;
        self.record(Spend {
            organization: subject.organization,
            workspace: Some(subject.workspace),
            user: subject.user,
            automated: subject.user.is_none(),
            microdollars,
            occurred_at: Utc::now() - TimeDelta::minutes(1),
            ..self.spend()
        })
        .await;
    }

    fn http(&self) -> &TestClient {
        &self.policy.sso.client
    }

    pub async fn get(&self, uri: &str, token: &str) -> TestResponse {
        self.http().get_auth(uri, token).await
    }

    pub async fn post(&self, uri: &str, token: &str, body: &Value) -> TestResponse {
        self.http().post_json_auth(uri, body, token).await
    }

    /// Posts `body` as it is, such as JSON cut short.
    pub async fn post_raw(&self, uri: &str, token: &str, body: &'static str) -> TestResponse {
        let request = http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(body))
            .unwrap();
        self.http().send_request(request).await
    }

    pub async fn patch(&self, uri: &str, token: &str, body: &Value) -> TestResponse {
        self.http().patch_json_auth(uri, body, token).await
    }

    pub async fn delete(&self, uri: &str, token: &str) -> TestResponse {
        self.http().delete_auth(uri, token).await
    }

    /// A budget of `limit` microdollars a month for `target`, created by the owner and settled
    /// against what is already spent this month.
    pub async fn budget(&self, target: Target, limit: i64, hard: bool) -> BudgetRow {
        let month = Month::current();
        let mut connection = self.pool().acquire().await.unwrap();
        let row = budgets::create(
            &mut connection,
            self.policy.organization,
            &target,
            &Budget {
                limit: Microdollars(limit),
                threshold_percent: DEFAULT_THRESHOLD,
                hard,
            },
            self.policy.owner.id,
            month,
        )
        .await
        .unwrap();
        budgets::refresh(&mut connection, row.id, month)
            .await
            .unwrap();
        row
    }

    /// Changes `row`'s limit to `limit` microdollars, as an admin's edit does, and settles it.
    pub async fn limit(&self, row: &BudgetRow, limit: i64) {
        let month = Month::current();
        let mut connection = self.pool().acquire().await.unwrap();
        budgets::update(
            &mut connection,
            row.id,
            &Budget {
                limit: Microdollars(limit),
                ..row.budget
            },
            self.policy.owner.id,
            month,
        )
        .await
        .unwrap();
        budgets::refresh(&mut connection, row.id, month)
            .await
            .unwrap();
    }

    /// Serves the real router, for a test that needs a WebSocket, and returns its `host:port`.
    pub async fn serve(&self) -> String {
        super::serve(self.state().clone()).await
    }

    /// Who account mail went to, in the order it was sent.
    pub fn mailed(&self) -> Vec<String> {
        self.policy.sso.mail.recipients()
    }

    pub async fn cleanup(self) {
        self.policy.cleanup().await;
    }
}

/// A spend log row as LiteLLM writes it.
pub fn log(request_id: &str, token: &str, start: DateTime<Utc>, spend: f64) -> Value {
    json!({
        "request_id": request_id,
        "api_key": token,
        "spend": spend,
        "prompt_tokens": 100,
        "completion_tokens": 20,
        "startTime": start.to_rfc3339(),
        "model": "gpt-4o-2024-08-06",
        "model_group": "gpt-4o",
        "custom_llm_provider": "openai",
        "status": "success",
    })
}

/// LiteLLM's answer to `/key/generate`: the key it was sent, and its hash as the token.
pub struct Registers;

impl Respond for Registers {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let key = body["key"].as_str().unwrap();
        ResponseTemplate::new(200).set_body_json(json!({ "key": key, "token": Key::hash(key) }))
    }
}
