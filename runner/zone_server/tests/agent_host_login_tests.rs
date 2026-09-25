//! A real claude turn served through the host's own claude sign-in.
//!
//! Ignored because it spends a turn of the host user's Claude subscription and
//! needs that user signed in to claude. Run by hand, from `runner/`, against a
//! migrated database:
//!
//! `CLAUDE_CONFIG_DIR=/tmp/empty cargo test -p zone_server --no-default-features
//! --features zone_context/test-utils --test agent_host_login_tests -- --ignored`
//!
//! `CLAUDE_CONFIG_DIR` points at an empty directory on purpose: the server's own
//! environment must not reach the agent, so claude still finds the host login.

mod common;

use std::path::PathBuf;

use axum::http::StatusCode;
use serde_json::json;
use tempfile::TempDir;
use uuid::Uuid;
use zone_core::llm::{AgentKind, Credential, LlmBackend, LlmClient, LlmConfig, Message};
use zone_server::config::{AgentConfig, Config};
use zone_server::services::backend;
use zone_server::services::stages::AUTO;

use common::{TestClient, test_config, test_email, test_password};

const PROVIDER: &str = "claude_code";

async fn register(client: &TestClient) -> String {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()["access_token"]
        .as_str()
        .expect("an access token")
        .to_string()
}

async fn create(client: &TestClient, token: &str, uri: &str, key: &str) -> Uuid {
    let slug = format!("host-login-{}", Uuid::new_v4().simple());
    let response = client
        .post_json_auth(uri, &json!({"name": "Host login", "slug": slug}), token)
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()[key]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("the new resource's id")
}

#[tokio::test]
#[ignore = "runs the host's real claude under the host user's own sign-in"]
async fn an_organization_without_a_sign_in_of_its_own_runs_the_hosts_claude() {
    let agents = TempDir::new().expect("an agent state root");
    let client = TestClient::with_config(Config {
        agents: AgentConfig {
            state: agents.path().to_path_buf(),
            host_login: true,
            ..AgentConfig::default()
        },
        ..test_config()
    })
    .await;
    let token = register(&client).await;
    let organization = create(&client, &token, "/api/organizations", "organization").await;
    let workspace = create(
        &client,
        &token,
        &format!("/api/organizations/{organization}/workspaces"),
        "workspace",
    )
    .await;
    client
        .put_json_auth(
            &format!("/api/organizations/{organization}/settings/ai"),
            &json!({"provider": PROVIDER}),
            &token,
        )
        .await
        .assert_status(StatusCode::OK);

    let backend = backend::for_workspace(client.state(), workspace)
        .await
        .expect("the host's own sign-in to serve the organization");

    let LlmBackend::Cli { agent, settings } = &backend else {
        panic!("expected the claude CLI, got {backend:?}");
    };
    assert_eq!(*agent, AgentKind::Claude);
    assert!(matches!(settings.credential, Credential::Inherited));
    assert!(
        !settings.variables.contains_key(AgentKind::Claude.home()),
        "a CLAUDE_CONFIG_DIR would hide the host's sign-in"
    );
    let work: PathBuf = agents
        .path()
        .join(organization.to_string())
        .join(AgentKind::Claude.as_str())
        .join("work");
    assert_eq!(settings.working_directory.as_deref(), Some(work.as_path()));

    let nonce = format!("zone-host-login-{}", Uuid::new_v4().simple());
    let response = LlmClient::new(LlmConfig {
        default_model: AUTO.to_string(),
        backend,
        ..LlmConfig::default()
    })
    .chat(
        &[Message::user(format!("Reply with exactly: {nonce}"))],
        None,
    )
    .await
    .expect("claude to answer through the host's sign-in");

    let answer = response
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone())
        .unwrap_or_default();
    assert!(answer.contains(&nonce), "claude answered: {answer}");
}
