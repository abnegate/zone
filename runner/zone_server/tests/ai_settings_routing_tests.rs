//! Completions go to the endpoint a workspace's saved AI settings name.
//!
//! Each test stands up three providers: the instance's own `LITELLM_HOST`, the
//! endpoint the organization saved, and the endpoint the workspace saved. The
//! settings are written straight into the tables, so what is under test is the
//! read side alone: which host a completion reaches and which key rides with it.

mod common;

use common::{
    TestClient, TestResponse, create_test_pool, create_test_router, create_test_state, discard,
    next_frame, seed_chat, serve, setup_test_data, setup_workspace_member, test_config, test_email,
    test_password,
};
use futures_util::SinkExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as Frame;
use uuid::Uuid;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_core::llm::ReasoningEffort;
use zone_server::config::Config;
use zone_server::db::{ai_settings, chats, tasks};
use zone_server::services::endpoint::{self, Endpoint};
use zone_server::services::hosts::Hosts;
use zone_server::state::AppState;
use zone_server::workers::{task, titles};

const INSTANCE_KEY: &str = "test-key";
const ORGANIZATION_KEY: &str = "sk-organization";
const WORKSPACE_KEY: &str = "sk-workspace";
const OPENAI_KEY: &str = "sk-openai";
const SELF_HOSTED: &str = "self_hosted";
const OPENAI: &str = "openai";
const CHAT_MODEL: &str = "llama3.2:3b";
const OPENAI_MODEL: &str = "gpt-4o-mini";
const TASK_MODEL: &str = "gpt-4";
const REASONING_MODEL: &str = "o3";
const INSTANCE_MODEL: &str = "qwen3:8b";
const COMPLETIONS: &str = "/v1/chat/completions";
const ANY_COMPLETIONS: &str = "/chat/completions";
const SHOW: &str = "/api/show";
const TAGS: &str = "/api/tags";
const UNREACHABLE: &str = "http://127.0.0.1:9";
const REPLY: &str = "Routed reply";
const CLASSIFIER_PROMPT: &str = "Return exactly IMAGE, AUDIO, or CHAT";
const AMBIGUOUS: &str = "Design a logo for Acme";
const TURN: Duration = Duration::from_secs(30);
const SETTLE: Duration = Duration::from_millis(500);

/// The three places a completion could go.
struct Endpoints {
    instance: MockServer,
    organization: MockServer,
    workspace: MockServer,
}

impl Endpoints {
    async fn start() -> Self {
        Self {
            instance: provider().await,
            organization: provider().await,
            workspace: provider().await,
        }
    }

    /// The instance's own configuration: its LiteLLM is the instance mock, and
    /// nothing else a turn might reach for is listening.
    fn config(&self) -> Config {
        let mut config = test_config();
        config.litellm_host = self.instance.uri();
        config.litellm_key = INSTANCE_KEY.to_string();
        config.ollama_host = UNREACHABLE.to_string();
        config.comfyui.enabled = false;
        config
    }
}

/// A provider that streams a reply to a streamed request and answers anything
/// else with a single completion, on whatever path it is asked.
async fn provider() -> MockServer {
    let server = MockServer::start().await;
    let chunk = json!({
        "id": "completion", "object": "chat.completion.chunk", "created": 0, "model": "test",
        "choices": [{"index": 0, "delta": {"role": "assistant", "content": REPLY}, "finish_reason": null}]
    });
    let end = json!({
        "id": "completion", "object": "chat.completion.chunk", "created": 0, "model": "test",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
    });
    Mock::given(method("POST"))
        .and(body_partial_json(json!({"stream": true})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n")),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "completion", "object": "chat.completion", "created": 0, "model": "test",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "CHAT"}, "finish_reason": "stop"}]
        })))
        .mount(&server)
        .await;
    server
}

/// One row of saved AI settings, as the settings page would leave it.
struct Saved<'a> {
    provider: &'a str,
    litellm_host: Option<String>,
    litellm_key: Option<&'a str>,
    openai_api_key: Option<&'a str>,
    openai_base_url: Option<String>,
    model_fast: &'a str,
    model_reasoning: Option<&'a str>,
    routed: bool,
}

impl<'a> Saved<'a> {
    fn litellm(host: &MockServer, key: Option<&'a str>) -> Self {
        Self {
            provider: SELF_HOSTED,
            litellm_host: Some(host.uri()),
            litellm_key: key,
            openai_api_key: None,
            openai_base_url: None,
            model_fast: CHAT_MODEL,
            model_reasoning: None,
            routed: true,
        }
    }

    fn openai(base: &MockServer, key: &'a str) -> Self {
        Self {
            provider: OPENAI,
            litellm_host: None,
            litellm_key: None,
            openai_api_key: Some(key),
            openai_base_url: Some(format!("{}/v1", base.uri())),
            model_fast: OPENAI_MODEL,
            model_reasoning: None,
            routed: true,
        }
    }

    fn reasoning_on(self, model: &'a str) -> Self {
        Self {
            model_reasoning: Some(model),
            ..self
        }
    }

    /// The row as 054 leaves one saved before completions were routed.
    fn unrouted(self) -> Self {
        Self {
            routed: false,
            ..self
        }
    }
}

async fn save_for_organization(pool: &PgPool, workspace: Uuid, saved: &Saved<'_>) {
    let written = sqlx::query(
        "INSERT INTO organization_ai_settings
             (organization_id, provider, litellm_host, litellm_key, openai_api_key, openai_base_url, model_fast, model_reasoning, completions_routed)
         SELECT organization_id, $2, $3, $4, $5, $6, $7, $8, $9 FROM workspaces WHERE id = $1",
    )
    .bind(workspace)
    .bind(saved.provider)
    .bind(saved.litellm_host.as_deref())
    .bind(saved.litellm_key)
    .bind(saved.openai_api_key)
    .bind(saved.openai_base_url.as_deref())
    .bind(saved.model_fast)
    .bind(saved.model_reasoning)
    .bind(saved.routed)
    .execute(pool)
    .await
    .expect("the organization's settings are writable");
    assert_eq!(
        written.rows_affected(),
        1,
        "the workspace resolves to one organization"
    );
}

async fn save_for_workspace(pool: &PgPool, workspace: Uuid, saved: &Saved<'_>) {
    sqlx::query(
        "INSERT INTO workspace_ai_settings
             (workspace_id, provider, litellm_host, litellm_key, openai_api_key, openai_base_url, model_fast, model_reasoning, completions_routed)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(workspace)
    .bind(saved.provider)
    .bind(saved.litellm_host.as_deref())
    .bind(saved.litellm_key)
    .bind(saved.openai_api_key)
    .bind(saved.openai_base_url.as_deref())
    .bind(saved.model_fast)
    .bind(saved.model_reasoning)
    .bind(saved.routed)
    .execute(pool)
    .await
    .expect("the workspace's settings are writable");
}

async fn organization_of(pool: &PgPool, workspace: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT organization_id FROM workspaces WHERE id = $1")
        .bind(workspace)
        .fetch_one(pool)
        .await
        .expect("the workspace's organization")
}

async fn members(pool: &PgPool, workspace: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT user_id FROM workspace_members WHERE workspace_id = $1")
        .bind(workspace)
        .fetch_all(pool)
        .await
        .expect("the workspace's members are readable")
}

/// The completions `server` was sent on `path`.
async fn completions(server: &MockServer, path: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .expect("the provider records its requests")
        .into_iter()
        .filter(|request| request.method.as_str() == "POST" && request.url.path() == path)
        .collect()
}

/// Every completion `server` was sent, on any base path. The instance's
/// `LITELLM_HOST` is used as configured, so a completion that reached it could
/// be on either `/chat/completions` or `/v1/chat/completions`.
async fn any_completions(server: &MockServer) -> Vec<Request> {
    server
        .received_requests()
        .await
        .expect("the provider records its requests")
        .into_iter()
        .filter(|request| {
            request.method.as_str() == "POST" && request.url.path().ends_with(ANY_COMPLETIONS)
        })
        .collect()
}

fn authorization(request: &Request) -> Option<String> {
    request
        .headers
        .get("authorization")
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
}

fn body(request: &Request) -> String {
    String::from_utf8_lossy(&request.body).into_owned()
}

fn all(_: &Request) -> bool {
    true
}

fn classifies(request: &Request) -> bool {
    body(request).contains(CLASSIFIER_PROMPT)
}

/// Where a completion was expected, and the key it was expected to carry.
struct Route<'a> {
    target: &'a MockServer,
    path: &'a str,
    key: Option<&'a str>,
    selected: fn(&Request) -> bool,
}

/// The three claims every test makes: the saved endpoint was sent exactly one
/// matching completion, it carried exactly the saved key and no other, and the
/// instance's own LiteLLM was sent no completion at all.
async fn assert_routed(endpoints: &Endpoints, route: Route<'_>, outcome: &str) {
    let instance = any_completions(&endpoints.instance).await;
    let received: Vec<Request> = completions(route.target, route.path)
        .await
        .into_iter()
        .filter(route.selected)
        .collect();
    assert_eq!(
        received.len(),
        1,
        "the saved endpoint was sent {} completions on {}, and the instance's LITELLM_HOST {} ({outcome})",
        received.len(),
        route.path,
        instance.len(),
    );
    assert_eq!(
        authorization(&received[0]),
        route.key.map(|key| format!("Bearer {key}")),
        "the completion to the saved endpoint carried the wrong credential ({outcome})"
    );
    assert!(
        instance.is_empty(),
        "the instance's LITELLM_HOST was sent {} completions, the first with {:?} ({outcome})",
        instance.len(),
        instance.first().map(authorization),
    );
}

/// A registered member's chat, on `model`, answered without the agent loop.
async fn chat_on(client: &TestClient, model: &str) -> (String, String, Uuid) {
    let (token, chat, workspace) = seed_chat(client, model).await;
    client
        .put_json_auth(
            &format!("/api/chats/{chat}"),
            &json!({"agent_enabled": false}),
            &token,
        )
        .await
        .assert_status(axum::http::StatusCode::OK);
    let workspace = workspace.parse().expect("the workspace id is a uuid");
    (token, chat, workspace)
}

/// Send `content` over the chat's socket and return the frame the turn ended
/// on, or nothing if it never ended.
async fn converse(address: &str, token: &str, chat: &str, content: &str) -> Option<Value> {
    let (mut socket, _) = connect_async(format!("ws://{address}/ws/chats/{chat}"))
        .await
        .expect("the chat socket opens");
    for frame in [
        json!({"type": "auth", "token": token}),
        json!({"type": "send", "content": content}),
    ] {
        socket
            .send(Frame::Text(frame.to_string().into()))
            .await
            .expect("the socket takes a frame");
    }
    loop {
        let frame = next_frame(&mut socket, TURN).await?;
        if matches!(
            frame["type"].as_str(),
            Some("message_end" | "error" | "cancelled")
        ) {
            tokio::time::sleep(SETTLE).await;
            return Some(frame);
        }
    }
}

/// One chat turn in a workspace whose organization saved `organization` and
/// which saved `workspace` itself, if anything.
async fn chat_turn(
    config: Config,
    model: &str,
    content: &str,
    organization: &Saved<'_>,
    workspace: Option<&Saved<'_>>,
) -> String {
    let client = TestClient::with_config(config).await;
    let pool = client.state().db().clone();
    let (token, chat, id) = chat_on(&client, model).await;
    save_for_organization(&pool, id, organization).await;
    if let Some(workspace) = workspace {
        save_for_workspace(&pool, id, workspace).await;
    }
    let address = serve(client.state().clone()).await;
    let ended = converse(&address, &token, &chat, content).await;
    let people = members(&pool, id).await;
    discard(&pool, id, &people).await;
    format!("the turn ended on {ended:?}")
}

#[tokio::test]
async fn a_chat_turn_goes_to_the_organization_litellm_host_with_its_key() {
    let endpoints = Endpoints::start().await;
    let organization = Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY));

    let outcome = chat_turn(endpoints.config(), CHAT_MODEL, "Hello", &organization, None).await;

    assert_routed(
        &endpoints,
        Route {
            target: &endpoints.organization,
            path: COMPLETIONS,
            key: Some(ORGANIZATION_KEY),
            selected: all,
        },
        &outcome,
    )
    .await;
}

#[tokio::test]
async fn a_workspace_litellm_host_overrides_the_organization_host() {
    let endpoints = Endpoints::start().await;
    let organization = Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY));
    let workspace = Saved::litellm(&endpoints.workspace, Some(WORKSPACE_KEY));

    let outcome = chat_turn(
        endpoints.config(),
        CHAT_MODEL,
        "Hello",
        &organization,
        Some(&workspace),
    )
    .await;

    assert_routed(
        &endpoints,
        Route {
            target: &endpoints.workspace,
            path: COMPLETIONS,
            key: Some(WORKSPACE_KEY),
            selected: all,
        },
        &outcome,
    )
    .await;
    assert!(
        any_completions(&endpoints.organization).await.is_empty(),
        "the organization's host was sent a completion the workspace's host overrides ({outcome})"
    );
}

#[tokio::test]
async fn a_workspace_host_never_receives_the_organization_key() {
    let endpoints = Endpoints::start().await;
    let organization = Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY));
    let workspace = Saved::litellm(&endpoints.workspace, None);

    let outcome = chat_turn(
        endpoints.config(),
        CHAT_MODEL,
        "Hello",
        &organization,
        Some(&workspace),
    )
    .await;

    assert_routed(
        &endpoints,
        Route {
            target: &endpoints.workspace,
            path: COMPLETIONS,
            key: None,
            selected: all,
        },
        &outcome,
    )
    .await;
    for request in any_completions(&endpoints.workspace).await {
        let sent = authorization(&request).unwrap_or_default();
        assert!(
            !sent.contains(ORGANIZATION_KEY) && !sent.contains(INSTANCE_KEY),
            "the workspace's host was handed a key it never saved ({outcome})"
        );
    }
}

#[tokio::test]
async fn rows_saved_before_completions_were_routed_keep_sending_to_the_instance() {
    let endpoints = Endpoints::start().await;
    let organization = Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY)).unrouted();
    let workspace = Saved::litellm(&endpoints.workspace, None).unrouted();

    let outcome = chat_turn(
        endpoints.config(),
        CHAT_MODEL,
        "Hello",
        &organization,
        Some(&workspace),
    )
    .await;

    let instance = any_completions(&endpoints.instance).await;
    assert!(
        !instance.is_empty(),
        "the instance's LITELLM_HOST was sent no completion ({outcome})"
    );
    for request in &instance {
        assert_eq!(
            authorization(request),
            Some(format!("Bearer {INSTANCE_KEY}")),
            "the instance was sent a key it does not own ({outcome})"
        );
    }
    for (saved, server) in [
        ("organization", &endpoints.organization),
        ("workspace", &endpoints.workspace),
    ] {
        assert!(
            any_completions(server).await.is_empty(),
            "the {saved}'s unrouted host was sent a completion ({outcome})"
        );
    }
}

#[tokio::test]
async fn an_openai_provider_sends_completions_to_its_base_url_with_its_key() {
    let endpoints = Endpoints::start().await;
    let organization = Saved::openai(&endpoints.organization, OPENAI_KEY);

    let outcome = chat_turn(
        endpoints.config(),
        OPENAI_MODEL,
        "Hello",
        &organization,
        None,
    )
    .await;

    assert_routed(
        &endpoints,
        Route {
            target: &endpoints.organization,
            path: COMPLETIONS,
            key: Some(OPENAI_KEY),
            selected: all,
        },
        &outcome,
    )
    .await;
}

#[tokio::test]
async fn an_image_intent_check_uses_the_organization_endpoint() {
    let endpoints = Endpoints::start().await;
    let organization = Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY));
    let mut config = endpoints.config();
    config.comfyui.enabled = true;
    config.comfyui.base_url = UNREACHABLE.to_string();
    config.comfyui.classifier_timeout_secs = 5;

    let outcome = chat_turn(config, CHAT_MODEL, AMBIGUOUS, &organization, None).await;

    assert_routed(
        &endpoints,
        Route {
            target: &endpoints.organization,
            path: COMPLETIONS,
            key: Some(ORGANIZATION_KEY),
            selected: classifies,
        },
        &outcome,
    )
    .await;
}

#[tokio::test]
async fn a_task_run_uses_the_organization_endpoint() {
    let endpoints = Endpoints::start().await;
    let pool = create_test_pool().await;
    let (_, workspace, user) = setup_workspace_member(&pool).await;
    save_for_organization(
        &pool,
        workspace,
        &Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY)),
    )
    .await;
    let created = tasks::create_task_as(
        &pool,
        workspace,
        &[],
        "Summarise the backlog",
        "Say what is left to do",
        None,
        None,
        true,
        None,
        Some(user),
    )
    .await
    .expect("a task to run");
    sqlx::query("UPDATE tasks SET model_name = $2 WHERE id = $1")
        .bind(created.id)
        .bind(TASK_MODEL)
        .execute(&pool)
        .await
        .expect("the task pins a model");
    let run = tasks::create_task_run_as(&pool, created.id, Some(user))
        .await
        .expect("a run of it");
    let state = create_test_state(endpoints.config(), pool.clone());

    let finished = tokio::time::timeout(TURN, task::execute_task_run(&state, run.id, created.id))
        .await
        .is_ok();
    tokio::time::sleep(SETTLE).await;
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM task_runs WHERE id = $1")
        .bind(run.id)
        .fetch_optional(&pool)
        .await
        .expect("the run is readable");
    discard(&pool, workspace, &[user]).await;

    assert_routed(
        &endpoints,
        Route {
            target: &endpoints.organization,
            path: COMPLETIONS,
            key: Some(ORGANIZATION_KEY),
            selected: all,
        },
        &format!("the run finished: {finished}, with status {status:?}"),
    )
    .await;
}

#[tokio::test]
async fn a_chat_title_uses_the_organization_endpoint() {
    let endpoints = Endpoints::start().await;
    let pool = create_test_pool().await;
    let (_, workspace, user) = setup_test_data(&pool).await;
    save_for_organization(
        &pool,
        workspace,
        &Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY)),
    )
    .await;
    let mut config = endpoints.config();
    config.comfyui.classifier_model = CHAT_MODEL.to_string();
    let state = create_test_state(config, pool.clone());
    let chat = chats::create_chat_with_title(
        &pool,
        Some(workspace),
        "New chat",
        CHAT_MODEL,
        (false, true),
        true,
        false,
        ReasoningEffort::Auto,
        None,
        false,
        None,
    )
    .await
    .expect("a chat awaiting its title");
    let message = chats::create_message(
        &pool,
        chat.id,
        "user",
        "Help me plan a holiday in Japan",
        None,
    )
    .await
    .expect("the chat's first message");

    let mut updates = titles::subscribe();
    titles::spawn(state, &message);
    let title = tokio::time::timeout(TURN, async {
        loop {
            match updates.recv().await {
                Ok((id, title)) if id == chat.id => return Some(title),
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
    })
    .await
    .ok()
    .flatten();
    discard(&pool, workspace, &[user]).await;

    assert_routed(
        &endpoints,
        Route {
            target: &endpoints.organization,
            path: COMPLETIONS,
            key: Some(ORGANIZATION_KEY),
            selected: all,
        },
        &format!("the chat was titled {title:?}"),
    )
    .await;
}

/// The questions the instance's Ollama was asked about a model.
async fn shown(ollama: &MockServer) -> usize {
    ollama
        .received_requests()
        .await
        .expect("Ollama records its requests")
        .iter()
        .filter(|request| request.url.path() == SHOW)
        .count()
}

#[tokio::test]
async fn reading_a_chat_on_a_saved_endpoint_asks_the_instance_ollama_nothing() {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SHOW))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"capabilities": ["completion", "tools", "thinking"]})),
        )
        .mount(&ollama)
        .await;
    let mut config = endpoints.config();
    config.ollama_host = ollama.uri();
    let client = TestClient::with_config(config).await;
    let pool = client.state().db().clone();
    let (token, chat, workspace) = chat_on(&client, CHAT_MODEL).await;
    save_for_organization(
        &pool,
        workspace,
        &Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY)),
    )
    .await;
    let before = shown(&ollama).await;

    let response = client.get_auth(&format!("/api/chats/{chat}"), &token).await;
    let asked = shown(&ollama).await - before;
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    response.assert_status(axum::http::StatusCode::OK);
    assert_eq!(
        asked, 0,
        "the instance's Ollama was asked about a model the saved endpoint runs"
    );
    let read = response.json_value();
    for capability in ["tools", "reasoning"] {
        assert!(
            read["chat"][capability].is_null(),
            "the chat took its {capability} from the instance's Ollama: {read}"
        );
    }
}

/// Make the instance's Ollama call every model it is asked about an embedding
/// model, which a chat cannot be created on.
async fn embeds_everything(ollama: &MockServer) {
    Mock::given(method("POST"))
        .and(path(SHOW))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"capabilities": ["embedding"]})),
        )
        .mount(ollama)
        .await;
}

/// A client on a configuration whose instance Ollama is `ollama`, with a
/// member's token and the workspace they own.
async fn member_on(
    endpoints: &Endpoints,
    ollama: &MockServer,
    tune: impl FnOnce(&mut Config),
) -> (TestClient, String, Uuid) {
    let mut config = endpoints.config();
    config.ollama_host = ollama.uri();
    tune(&mut config);
    let client = TestClient::with_config(config).await;
    let (token, _, workspace) = chat_on(&client, CHAT_MODEL).await;
    (client, token, workspace)
}

#[tokio::test]
async fn creating_a_chat_on_a_saved_endpoint_asks_the_instance_ollama_nothing() {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    let (client, token, workspace) = member_on(&endpoints, &ollama, |_| {}).await;
    let pool = client.state().db().clone();
    save_for_organization(
        &pool,
        workspace,
        &Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY)),
    )
    .await;
    embeds_everything(&ollama).await;
    let before = shown(&ollama).await;

    let response = client
        .post_json_auth(
            "/api/chats",
            &json!({"workspace_id": workspace, "title": "Saved", "model_name": CHAT_MODEL}),
            &token,
        )
        .await;
    let asked = shown(&ollama).await - before;
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    response.assert_status(axum::http::StatusCode::CREATED);
    assert_eq!(
        asked, 0,
        "the instance's Ollama was asked about a model the saved endpoint runs"
    );
    assert_eq!(response.json_value()["chat"]["model_name"], CHAT_MODEL);
}

#[tokio::test]
async fn creating_a_chat_on_the_instance_still_refuses_a_model_that_cannot_chat() {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    let (client, token, workspace) = member_on(&endpoints, &ollama, |_| {}).await;
    let pool = client.state().db().clone();
    embeds_everything(&ollama).await;

    let response = client
        .post_json_auth(
            "/api/chats",
            &json!({"workspace_id": workspace, "title": "Instance", "model_name": CHAT_MODEL}),
            &token,
        )
        .await;
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    response.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn starting_a_project_on_a_saved_endpoint_asks_the_instance_ollama_nothing() {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    let (client, token, workspace) =
        member_on(&endpoints, &ollama, |config| config.auto.enabled = true).await;
    let pool = client.state().db().clone();
    save_for_organization(
        &pool,
        workspace,
        &Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY)),
    )
    .await;
    embeds_everything(&ollama).await;
    let before = shown(&ollama).await;

    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/projects/auto"),
            &json!({"brief": "Plan a small landing page", "model_name": CHAT_MODEL}),
            &token,
        )
        .await;
    let asked = shown(&ollama).await - before;
    tokio::time::sleep(SETTLE).await;
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    response.assert_status(axum::http::StatusCode::ACCEPTED);
    assert_eq!(
        asked, 0,
        "the instance's Ollama was asked about a model the saved endpoint runs"
    );
}

/// An instance Ollama with a model installed that no saved endpoint runs.
async fn installs(ollama: &MockServer) {
    let installed = |name: &str| {
        json!({
            "name": name,
            "size": 1,
            "digest": "sha256:0",
            "modified_at": "2026-01-01T00:00:00Z",
        })
    };
    Mock::given(method("GET"))
        .and(path(TAGS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [installed(CHAT_MODEL), installed(INSTANCE_MODEL)]
        })))
        .mount(ollama)
        .await;
}

fn names(listed: &Value) -> Vec<&str> {
    listed
        .as_array()
        .unwrap_or_else(|| panic!("the models are listed, got {listed}"))
        .iter()
        .filter_map(|model| model["name"].as_str())
        .collect()
}

#[tokio::test]
async fn the_model_picker_offers_only_the_models_a_saved_endpoint_runs() {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    installs(&ollama).await;
    let (client, token, workspace) = member_on(&endpoints, &ollama, |_| {}).await;
    let pool = client.state().db().clone();
    save_for_organization(
        &pool,
        workspace,
        &Saved::openai(&endpoints.organization, OPENAI_KEY).reasoning_on(REASONING_MODEL),
    )
    .await;

    let scoped = client
        .get_auth(&format!("/api/models?workspace_id={workspace}"), &token)
        .await;
    let unscoped = client.get_auth("/api/models", &token).await;
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    scoped.assert_status(axum::http::StatusCode::OK);
    let listed = scoped.json_value();
    assert_eq!(
        names(&listed),
        [OPENAI_MODEL, REASONING_MODEL],
        "the picker offered models the saved endpoint does not run"
    );
    for model in listed.as_array().into_iter().flatten() {
        assert!(
            model["size"].is_u64() && model["modified_at"].is_string(),
            "the console rejects a listed model without a size and a date: {model}"
        );
    }
    unscoped.assert_status(axum::http::StatusCode::OK);
    assert_eq!(
        names(&unscoped.json_value()),
        [CHAT_MODEL, INSTANCE_MODEL],
        "the unscoped listing stopped listing the instance's models"
    );
}

#[tokio::test]
async fn the_model_picker_lists_the_instance_models_for_a_workspace_on_the_instance() {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    installs(&ollama).await;
    let (client, token, workspace) = member_on(&endpoints, &ollama, |_| {}).await;
    let pool = client.state().db().clone();
    save_for_organization(
        &pool,
        workspace,
        &Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY)).unrouted(),
    )
    .await;

    let scoped = client
        .get_auth(&format!("/api/models?workspace_id={workspace}"), &token)
        .await;
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    scoped.assert_status(axum::http::StatusCode::OK);
    assert_eq!(names(&scoped.json_value()), [CHAT_MODEL, INSTANCE_MODEL]);
}

#[tokio::test]
async fn the_model_picker_refuses_a_workspace_the_caller_is_not_in() {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    installs(&ollama).await;
    let (client, _, workspace) = member_on(&endpoints, &ollama, |_| {}).await;
    let pool = client.state().db().clone();
    save_for_organization(
        &pool,
        workspace,
        &Saved::openai(&endpoints.organization, OPENAI_KEY),
    )
    .await;
    let registered = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    let stranger = registered["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("registration returns an access token, got {registered}"));
    let stranger_id: Uuid = registered["user"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("registration returns the user, got {registered}"));

    let response = client
        .get_auth(&format!("/api/models?workspace_id={workspace}"), stranger)
        .await;
    let mut people = members(&pool, workspace).await;
    people.push(stranger_id);
    discard(&pool, workspace, &people).await;

    response.assert_status(axum::http::StatusCode::FORBIDDEN);
    assert!(
        !response.text().contains(OPENAI_MODEL),
        "a stranger was told the workspace's saved models: {}",
        response.text()
    );
}

/// The only host an instance under test allows endpoints on, which none of
/// the mock providers listens on.
const LISTED_HOST: &str = "llm.corp.example";
const UNUSABLE: &str = "This workspace's AI endpoint can't be used";

/// `endpoints`' configuration on an instance whose `ZONE_ENDPOINT_HOSTS` has
/// since been tightened to leave the saved endpoints out.
fn tightened(endpoints: &Endpoints) -> Config {
    let mut config = endpoints.config();
    config.endpoint_hosts = Hosts::parse(LISTED_HOST);
    config
}

/// No completion reached any of the three endpoints.
async fn assert_nothing_sent(endpoints: &Endpoints, outcome: &str) {
    for (name, server) in [
        ("instance's LITELLM_HOST", &endpoints.instance),
        ("organization's endpoint", &endpoints.organization),
        ("workspace's endpoint", &endpoints.workspace),
    ] {
        let sent = any_completions(server).await;
        assert!(
            sent.is_empty(),
            "the {name} was sent {} completions, the first with {:?} ({outcome})",
            sent.len(),
            sent.first().map(authorization),
        );
    }
}

#[tokio::test]
async fn a_chat_turn_on_an_endpoint_the_instance_no_longer_allows_fails_sending_nothing() {
    let endpoints = Endpoints::start().await;
    let organization = Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY));

    let outcome = chat_turn(
        tightened(&endpoints),
        CHAT_MODEL,
        "Hello",
        &organization,
        None,
    )
    .await;

    assert!(
        outcome.contains("\"error\"") && outcome.contains(UNUSABLE),
        "the turn did not fail on its unusable endpoint: {outcome}"
    );
    assert!(!outcome.contains(ORGANIZATION_KEY), "{outcome}");
    assert!(!outcome.contains(INSTANCE_KEY), "{outcome}");
    assert_nothing_sent(&endpoints, &outcome).await;
}

#[tokio::test]
async fn a_task_run_on_an_endpoint_the_instance_no_longer_allows_fails_with_the_reason() {
    let endpoints = Endpoints::start().await;
    let pool = create_test_pool().await;
    let (_, workspace, user) = setup_workspace_member(&pool).await;
    save_for_organization(
        &pool,
        workspace,
        &Saved::openai(&endpoints.organization, OPENAI_KEY),
    )
    .await;
    let created = tasks::create_task_as(
        &pool,
        workspace,
        &[],
        "Summarise the backlog",
        "Say what is left to do",
        None,
        None,
        true,
        None,
        Some(user),
    )
    .await
    .expect("a task to run");
    let run = tasks::create_task_run_as(&pool, created.id, Some(user))
        .await
        .expect("a run of it");
    let state = create_test_state(tightened(&endpoints), pool.clone());

    let finished = tokio::time::timeout(TURN, task::execute_task_run(&state, run.id, created.id))
        .await
        .is_ok();
    tokio::time::sleep(SETTLE).await;
    let ended: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT status, error_message FROM task_runs WHERE id = $1")
            .bind(run.id)
            .fetch_optional(&pool)
            .await
            .expect("the run is readable");
    discard(&pool, workspace, &[user]).await;

    let outcome = format!("the run finished: {finished}, as {ended:?}");
    let (status, error) = ended.expect("the run is still there");
    assert_eq!(status, "failed", "{outcome}");
    let error = error.unwrap_or_default();
    assert!(error.starts_with(UNUSABLE), "{outcome}");
    assert!(!error.contains(OPENAI_KEY), "{outcome}");
    assert_nothing_sent(&endpoints, &outcome).await;
}

#[tokio::test]
async fn a_chat_title_on_an_endpoint_the_instance_no_longer_allows_asks_no_model() {
    const FIRST_MESSAGE: &str = "Help me plan a holiday in Japan";
    let endpoints = Endpoints::start().await;
    let pool = create_test_pool().await;
    let (_, workspace, user) = setup_test_data(&pool).await;
    save_for_organization(
        &pool,
        workspace,
        &Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY)),
    )
    .await;
    let mut config = tightened(&endpoints);
    config.comfyui.classifier_model = CHAT_MODEL.to_string();
    let state = create_test_state(config, pool.clone());
    let chat = chats::create_chat_with_title(
        &pool,
        Some(workspace),
        "New chat",
        CHAT_MODEL,
        (false, true),
        true,
        false,
        ReasoningEffort::Auto,
        None,
        false,
        None,
    )
    .await
    .expect("a chat awaiting its title");
    let message = chats::create_message(&pool, chat.id, "user", FIRST_MESSAGE, None)
        .await
        .expect("the chat's first message");

    let mut updates = titles::subscribe();
    titles::spawn(state, &message);
    let title = tokio::time::timeout(TURN, async {
        loop {
            match updates.recv().await {
                Ok((id, title)) if id == chat.id => return Some(title),
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
    })
    .await
    .ok()
    .flatten();
    discard(&pool, workspace, &[user]).await;

    let outcome = format!("the chat was titled {title:?}");
    assert_eq!(title.as_deref(), Some(FIRST_MESSAGE), "{outcome}");
    assert_nothing_sent(&endpoints, &outcome).await;
}

#[tokio::test]
async fn reading_a_chat_on_an_endpoint_the_instance_no_longer_allows_asks_nothing() {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SHOW))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"capabilities": ["completion", "tools", "thinking"]})),
        )
        .mount(&ollama)
        .await;
    let mut config = tightened(&endpoints);
    config.ollama_host = ollama.uri();
    let client = TestClient::with_config(config).await;
    let pool = client.state().db().clone();
    let (token, chat, workspace) = chat_on(&client, CHAT_MODEL).await;
    save_for_organization(
        &pool,
        workspace,
        &Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY)),
    )
    .await;
    let before = shown(&ollama).await;

    let response = client.get_auth(&format!("/api/chats/{chat}"), &token).await;
    let asked = shown(&ollama).await - before;
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    response.assert_status(axum::http::StatusCode::OK);
    let read = response.json_value();
    assert_eq!(
        asked, 0,
        "the instance's Ollama was asked about a chat whose endpoint cannot be used: {read}"
    );
    assert!(read["chat"]["context"].is_null(), "{read}");
    assert_nothing_sent(&endpoints, &read.to_string()).await;
}

/// A member of a workspace whose organization saved an endpoint on a host the
/// instance has since stopped allowing, with an instance Ollama that would
/// refuse every model if it were asked.
async fn member_on_a_disallowed_endpoint(
    endpoints: &Endpoints,
    ollama: &MockServer,
    tune: impl FnOnce(&mut Config),
) -> (TestClient, String, Uuid) {
    let (client, token, workspace) = member_on(endpoints, ollama, |config| {
        config.endpoint_hosts = Hosts::parse(LISTED_HOST);
        tune(config);
    })
    .await;
    save_for_organization(
        client.state().db(),
        workspace,
        &Saved::litellm(&endpoints.organization, Some(ORGANIZATION_KEY)),
    )
    .await;
    embeds_everything(ollama).await;
    installs(ollama).await;
    (client, token, workspace)
}

#[tokio::test]
async fn creating_a_chat_on_an_endpoint_the_instance_no_longer_allows_is_refused_with_the_reason() {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    let (client, token, workspace) =
        member_on_a_disallowed_endpoint(&endpoints, &ollama, |_| {}).await;
    let pool = client.state().db().clone();
    let before = shown(&ollama).await;

    let response = client
        .post_json_auth(
            "/api/chats",
            &json!({"workspace_id": workspace, "title": "Refused", "model_name": CHAT_MODEL}),
            &token,
        )
        .await;
    let asked = shown(&ollama).await - before;
    let created: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chats WHERE title = 'Refused' AND workspace_id = $1",
    )
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .expect("the chats are readable");
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    response.assert_status(axum::http::StatusCode::CONFLICT);
    assert!(response.text().contains(UNUSABLE), "{}", response.text());
    assert!(
        !response.text().contains(ORGANIZATION_KEY),
        "{}",
        response.text()
    );
    assert_eq!(created, 0, "a chat was created on an unusable endpoint");
    assert_eq!(asked, 0, "the instance's Ollama was asked about the model");
}

#[tokio::test]
async fn starting_a_project_on_an_endpoint_the_instance_no_longer_allows_is_refused_with_the_reason()
 {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    let (client, token, workspace) =
        member_on_a_disallowed_endpoint(&endpoints, &ollama, |config| config.auto.enabled = true)
            .await;
    let pool = client.state().db().clone();
    let before = shown(&ollama).await;

    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/projects/auto"),
            &json!({"brief": "Plan a small landing page", "model_name": CHAT_MODEL}),
            &token,
        )
        .await;
    let asked = shown(&ollama).await - before;
    tokio::time::sleep(SETTLE).await;
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    response.assert_status(axum::http::StatusCode::CONFLICT);
    assert!(response.text().contains(UNUSABLE), "{}", response.text());
    assert_eq!(asked, 0, "the instance's Ollama was asked about the model");
    assert_nothing_sent(&endpoints, &response.text()).await;
}

#[tokio::test]
async fn the_model_picker_says_why_an_endpoint_the_instance_no_longer_allows_offers_nothing() {
    let endpoints = Endpoints::start().await;
    let ollama = MockServer::start().await;
    let (client, token, workspace) =
        member_on_a_disallowed_endpoint(&endpoints, &ollama, |_| {}).await;
    let pool = client.state().db().clone();

    let response = client
        .get_auth(&format!("/api/models?workspace_id={workspace}"), &token)
        .await;
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    response.assert_status(axum::http::StatusCode::CONFLICT);
    let body = response.text();
    assert!(body.contains(UNUSABLE), "{body}");
    assert!(
        !body.contains(CHAT_MODEL) && !body.contains(INSTANCE_MODEL),
        "the picker offered the instance's models for a workspace whose endpoint cannot be used: {body}"
    );
}

#[tokio::test]
async fn a_workspace_key_saved_without_a_url_never_follows_its_organization_to_a_new_url() {
    const MOVED_KEY: &str = "sk-organization-moved";
    let endpoints = Endpoints::start().await;
    let first = provider().await;
    let client = TestClient::with_config(endpoints.config()).await;
    let pool = client.state().db().clone();
    let (token, chat, workspace) = chat_on(&client, CHAT_MODEL).await;
    let organization = organization_of(&pool, workspace).await;
    let organization_settings = format!("/api/organizations/{organization}/settings/ai");

    let opened = client
        .put_json_auth(
            &organization_settings,
            &json!({
                "provider": SELF_HOSTED,
                "litellm_host": first.uri(),
                "litellm_key": ORGANIZATION_KEY,
                "model_fast": CHAT_MODEL,
            }),
            &token,
        )
        .await;
    let keyed = client
        .put_json_auth(
            &format!("/api/organizations/{organization}/workspaces/{workspace}/settings/ai"),
            &json!({"litellm_key": WORKSPACE_KEY}),
            &token,
        )
        .await;
    let moved = client
        .put_json_auth(
            &organization_settings,
            &json!({"litellm_host": endpoints.organization.uri(), "litellm_key": MOVED_KEY}),
            &token,
        )
        .await;
    let address = serve(client.state().clone()).await;
    let ended = converse(&address, &token, &chat, "Hello").await;
    let routed: Option<bool> = sqlx::query_scalar(
        "SELECT completions_routed FROM workspace_ai_settings WHERE workspace_id = $1",
    )
    .bind(workspace)
    .fetch_optional(&pool)
    .await
    .expect("the workspace's settings are readable");
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;
    let outcome = format!("the turn ended on {ended:?}");

    for (name, server) in [
        ("organization's new URL", &endpoints.organization),
        ("organization's old URL", &first),
        ("instance's LITELLM_HOST", &endpoints.instance),
    ] {
        let leaked: Vec<Request> = any_completions(server)
            .await
            .into_iter()
            .filter(|request| {
                authorization(request).is_some_and(|value| value.contains(WORKSPACE_KEY))
            })
            .collect();
        assert!(
            leaked.is_empty(),
            "the workspace key reached the {name} ({outcome})"
        );
    }
    let delivered = any_completions(&endpoints.organization).await;
    assert!(
        delivered
            .iter()
            .any(|request| authorization(request) == Some(format!("Bearer {MOVED_KEY}"))),
        "the turn did not fall back to the organization's own pair ({outcome})"
    );
    opened.assert_status(axum::http::StatusCode::OK);
    assert!(
        opened.json_value().get("notice").is_none(),
        "{}",
        opened.text()
    );
    keyed.assert_status(axum::http::StatusCode::OK);
    moved.assert_status(axum::http::StatusCode::OK);
    assert_eq!(
        moved.json_value()["notice"],
        "1 workspace key waits for its admin to save again.",
        "{}",
        moved.text()
    );
    assert_eq!(routed, Some(false), "the workspace row was left routed");
}

#[tokio::test]
async fn resetting_the_organization_settings_never_sends_a_workspace_key_to_the_default_host() {
    let endpoints = Endpoints::start().await;
    let config = endpoints.config();
    let client = TestClient::with_config(config.clone()).await;
    let pool = client.state().db().clone();
    let (token, chat, workspace) = chat_on(&client, OPENAI_MODEL).await;
    let organization = organization_of(&pool, workspace).await;
    let organization_settings = format!("/api/organizations/{organization}/settings/ai");

    let opened = client
        .put_json_auth(
            &organization_settings,
            &json!({
                "provider": OPENAI,
                "openai_base_url": format!("{}/v1", endpoints.organization.uri()),
                "openai_api_key": ORGANIZATION_KEY,
                "model_fast": OPENAI_MODEL,
            }),
            &token,
        )
        .await;
    let keyed = client
        .put_json_auth(
            &format!("/api/organizations/{organization}/workspaces/{workspace}/settings/ai"),
            &json!({
                "provider": OPENAI,
                "openai_api_key": WORKSPACE_KEY,
                "model_fast": OPENAI_MODEL,
            }),
            &token,
        )
        .await;
    let reset = client.delete_auth(&organization_settings, &token).await;
    let settings = ai_settings::get_effective_ai_settings(&pool, organization, workspace)
        .await
        .expect("the workspace's effective settings are readable");
    let resolved = Endpoint::try_resolve(&config, &settings).map(|resolved| {
        (
            resolved.url().to_string(),
            resolved.key().expose().to_string(),
        )
    });
    let defaulted = resolved
        .as_ref()
        .is_ok_and(|(url, key)| url == endpoint::OPENAI_URL || key == WORKSPACE_KEY);
    // A regression must not send the workspace key to the real default host.
    let ended = if defaulted {
        None
    } else {
        let address = serve(client.state().clone()).await;
        converse(&address, &token, &chat, "Hello").await
    };
    let routed: Option<bool> = sqlx::query_scalar(
        "SELECT completions_routed FROM workspace_ai_settings WHERE workspace_id = $1",
    )
    .bind(workspace)
    .fetch_optional(&pool)
    .await
    .expect("the workspace's settings are readable");
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;
    let outcome = format!("the turn ended on {ended:?}");

    assert!(
        !defaulted,
        "the workspace key resolves to {:?} once its organization's URL is gone",
        resolved.map(|(url, _)| url)
    );
    for (name, server) in [
        ("organization's old URL", &endpoints.organization),
        ("instance's LITELLM_HOST", &endpoints.instance),
        ("workspace's endpoint", &endpoints.workspace),
    ] {
        let leaked: Vec<Request> = any_completions(server)
            .await
            .into_iter()
            .filter(|request| {
                authorization(request).is_some_and(|value| value.contains(WORKSPACE_KEY))
            })
            .collect();
        assert!(
            leaked.is_empty(),
            "the workspace key reached the {name} ({outcome})"
        );
    }
    assert!(
        any_completions(&endpoints.instance)
            .await
            .iter()
            .any(|request| authorization(request) == Some(format!("Bearer {INSTANCE_KEY}"))),
        "the turn did not fall back to the instance ({outcome})"
    );
    opened.assert_status(axum::http::StatusCode::OK);
    keyed.assert_status(axum::http::StatusCode::OK);
    reset.assert_status(axum::http::StatusCode::OK);
    assert_eq!(
        reset.json_value()["notice"],
        "1 workspace key waits for its admin to save again.",
        "{}",
        reset.text()
    );
    assert_eq!(routed, Some(false), "the workspace row was left routed");
}

const LOCK_ATTEMPTS: usize = 500;
const LOCK_PAUSE: Duration = Duration::from_millis(20);

/// A pool beside the server's, for a transaction the test holds open while the
/// server's own connections wait on it.
async fn side_pool() -> PgPool {
    let database = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/zone_test".to_string());
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&database)
        .await
        .expect("the test database is reachable")
}

/// The backend that comes to wait on a lock `holder` holds, if one does.
async fn waiting_on(pool: &PgPool, holder: i32) -> Option<i32> {
    for _ in 0..LOCK_ATTEMPTS {
        let waiting: Option<i32> = sqlx::query_scalar(
            "SELECT pid FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)) LIMIT 1",
        )
        .bind(holder)
        .fetch_optional(pool)
        .await
        .expect("the backends waiting on locks are readable");
        if waiting.is_some() {
            return waiting;
        }
        tokio::time::sleep(LOCK_PAUSE).await;
    }
    None
}

/// Wait until `request` has either answered or come to wait on `holder`.
async fn answered_or_waiting(pool: &PgPool, holder: i32, request: &JoinHandle<TestResponse>) {
    for _ in 0..LOCK_ATTEMPTS {
        if request.is_finished() {
            return;
        }
        let waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)))",
        )
        .bind(holder)
        .fetch_one(pool)
        .await
        .expect("the backends waiting on locks are readable");
        if waiting {
            return;
        }
        tokio::time::sleep(LOCK_PAUSE).await;
    }
}

fn put(state: &AppState, uri: String, body: Value, token: &str) -> JoinHandle<TestResponse> {
    let client = TestClient::new(create_test_router(state.clone()));
    let token = token.to_string();
    tokio::spawn(async move { client.put_json_auth(&uri, &body, &token).await })
}

#[tokio::test]
async fn a_workspace_first_save_racing_an_organization_move_never_stays_routed() {
    const MOVED_KEY: &str = "sk-organization-moved";
    let endpoints = Endpoints::start().await;
    let first = provider().await;
    let client = TestClient::with_config(endpoints.config()).await;
    let pool = client.state().db().clone();
    let (token, _, workspace) = chat_on(&client, CHAT_MODEL).await;
    let organization = organization_of(&pool, workspace).await;
    let organization_settings = format!("/api/organizations/{organization}/settings/ai");
    let opened = client
        .put_json_auth(
            &organization_settings,
            &json!({
                "provider": SELF_HOSTED,
                "litellm_host": first.uri(),
                "litellm_key": ORGANIZATION_KEY,
                "model_fast": CHAT_MODEL,
            }),
            &token,
        )
        .await;

    let side = side_pool().await;
    let mut holder = side.begin().await.expect("the holding transaction starts");
    let holder_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .expect("the holding backend is known");
    sqlx::query("INSERT INTO workspace_ai_settings (workspace_id) VALUES ($1)")
        .bind(workspace)
        .execute(&mut *holder)
        .await
        .expect("the workspace's first row is held uncommitted");

    let keying = put(
        client.state(),
        format!("/api/organizations/{organization}/workspaces/{workspace}/settings/ai"),
        json!({"litellm_key": WORKSPACE_KEY}),
        &token,
    );
    let keyer = waiting_on(&side, holder_pid).await;
    let moving = put(
        client.state(),
        organization_settings,
        json!({"litellm_host": endpoints.organization.uri(), "litellm_key": MOVED_KEY}),
        &token,
    );
    if let Some(keyer) = keyer {
        answered_or_waiting(&side, keyer, &moving).await;
    }
    holder
        .rollback()
        .await
        .expect("the holding transaction rolls back");
    let keyed = keying.await.expect("the workspace save answers");
    let moved = moving.await.expect("the organization move answers");
    let routed: Option<bool> = sqlx::query_scalar(
        "SELECT completions_routed FROM workspace_ai_settings WHERE workspace_id = $1",
    )
    .bind(workspace)
    .fetch_optional(&pool)
    .await
    .expect("the workspace's settings are readable");
    let people = members(&pool, workspace).await;
    discard(&pool, workspace, &people).await;

    assert!(
        keyer.is_some(),
        "the workspace save never reached the row the test held"
    );
    opened.assert_status(axum::http::StatusCode::OK);
    keyed.assert_status(axum::http::StatusCode::OK);
    moved.assert_status(axum::http::StatusCode::OK);
    assert_eq!(
        routed,
        Some(false),
        "a workspace key saved while its organization's URL moved still follows the new URL"
    );
    assert_eq!(
        moved.json_value()["notice"],
        "1 workspace key waits for its admin to save again.",
        "{}",
        moved.text()
    );
}
