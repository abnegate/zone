//! A workspace whose organization saved its own endpoint, and endpoints that
//! record what they were sent, for the stages that route to one.

use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;

use crate::config::Config;
use crate::db::{organizations, workspaces};
use crate::state::AppState;

pub const SAVED_MODEL: &str = "qwen3:32b";
pub const ORGANIZATION_KEY: &str = "sk-organization-5f1c9a";
pub const INSTANCE_KEY: &str = "sk-instance-83be20";
const COMPLETIONS: &str = "/chat/completions$";
const NOTHING_LISTENS: &str = "http://127.0.0.1:1";

/// What the organization saved for its self-hosted endpoint.
#[derive(Default)]
pub struct Saved<'a> {
    pub host: Option<&'a str>,
    pub key: Option<&'a str>,
    pub fast: Option<&'a str>,
}

/// A workspace whose organization saved [`Saved`].
pub struct Organization {
    pub pool: PgPool,
    pub workspace: Uuid,
    id: Uuid,
}

impl Organization {
    pub async fn saving(saved: Saved<'_>) -> Self {
        let pool = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let suffix = Uuid::new_v4().simple().to_string();
        let organization = organizations::create_organization(&pool, "Route", &suffix, None)
            .await
            .expect("an organization");
        let workspace =
            workspaces::create_workspace(&pool, organization.id, "Route", &suffix, None)
                .await
                .expect("a workspace");
        sqlx::query(
            "INSERT INTO organization_ai_settings \
             (organization_id, provider, litellm_host, litellm_key, model_fast, completions_routed) \
             VALUES ($1, $2, $3, $4, $5, true)",
        )
        .bind(organization.id)
        .bind(PROVIDER_SELF_HOSTED)
        .bind(saved.host)
        .bind(saved.key)
        .bind(saved.fast)
        .execute(&pool)
        .await
        .expect("the organization's AI settings");
        Self {
            pool,
            workspace: workspace.id,
            id: organization.id,
        }
    }

    /// An instance whose own endpoint is `instance`, keyed with
    /// [`INSTANCE_KEY`], with no Ollama to list models.
    pub fn state(&self, instance: &str) -> AppState {
        AppState::new(
            Config {
                litellm_host: instance.to_string(),
                litellm_key: INSTANCE_KEY.to_string(),
                ollama_host: NOTHING_LISTENS.to_string(),
                ..crate::state::test_config()
            },
            self.pool.clone(),
            None,
        )
    }

    pub async fn remove(&self) {
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(self.id)
            .execute(&self.pool)
            .await
            .expect("the organization to be removed");
    }
}

/// An endpoint that answers every completion with `content`, ended for
/// `finish_reason`.
pub async fn completing(content: &str, finish_reason: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(COMPLETIONS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "completion",
            "object": "chat.completion",
            "created": 0,
            "model": SAVED_MODEL,
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": finish_reason,
            }],
        })))
        .mount(&server)
        .await;
    server
}

/// Every request `server` was sent.
pub async fn received(server: &MockServer) -> Vec<Request> {
    server
        .received_requests()
        .await
        .expect("the endpoint records its requests")
}

pub fn authorization(request: &Request) -> Option<String> {
    request
        .headers
        .get("authorization")
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
}
