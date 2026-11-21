//! A server whose LiteLLM is a stand-in, with one organization and one workspace owned by the
//! server admin, and helpers for spend. Wraps [`policy::Fixture`]; every fixture has a database
//! of its own, because the license and the ingest cursor are server-wide.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};
use zone_server::config::Config;
use zone_server::db::migrations;
use zone_server::services::endpoint::Endpoint;
use zone_server::services::mail::AccountMail;
use zone_server::services::spend::{self, Client, Key, Subject};
use zone_server::state::AppState;

use super::database::Database;
use super::mail::Recording;
use super::{TestClient, policy, sso, test_config};

/// The stand-in LiteLLM's master key, the instance's `LITELLM_KEY`.
pub const MASTER: &str = "sk-spend-master-0123456789";
pub const GENERATE_PATH: &str = "/key/generate";
pub const INFO_PATH: &str = "/key/info";
pub const DELETE_PATH: &str = "/key/delete";
pub const LOGS_PATH: &str = "/spend/logs/v2";

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
        // A pooled server keeps its address across tests, and the key registry is keyed by
        // LiteLLM's address for the whole process.
        let litellm = MockServer::builder().start().await;
        let config = Config {
            litellm_host: litellm.uri(),
            litellm_key: MASTER.to_string(),
            ..test_config()
        };
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
            "SELECT request_id, organization_id, workspace_id, user_id, token, model, provider,
                    spend_microdollars, prompt_tokens, completion_tokens, failed, occurred_at
             FROM spend_records ORDER BY request_id",
        )
        .fetch_all(self.pool())
        .await
        .unwrap()
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
