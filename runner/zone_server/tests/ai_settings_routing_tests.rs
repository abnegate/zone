//! Completions go to the endpoint a workspace's saved AI settings name.
//!
//! Each test stands up three providers: the instance's own `LITELLM_HOST`, the
//! endpoint the organization saved, and the endpoint the workspace saved. The
//! settings are written straight into the tables, so what is under test is the
//! read side alone: which host a completion reaches and which key rides with it.

mod common;

use common::{
    TestClient, create_test_pool, create_test_state, discard, next_frame, seed_chat, serve,
    setup_test_data, setup_workspace_member, test_config,
};
use futures_util::SinkExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as Frame;
use uuid::Uuid;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_core::llm::ReasoningEffort;
use zone_server::config::Config;
use zone_server::db::{chats, tasks};
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
const COMPLETIONS: &str = "/v1/chat/completions";
const ANY_COMPLETIONS: &str = "/chat/completions";
const SHOW: &str = "/api/show";
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
            routed: true,
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
             (organization_id, provider, litellm_host, litellm_key, openai_api_key, openai_base_url, model_fast, completions_routed)
         SELECT organization_id, $2, $3, $4, $5, $6, $7, $8 FROM workspaces WHERE id = $1",
    )
    .bind(workspace)
    .bind(saved.provider)
    .bind(saved.litellm_host.as_deref())
    .bind(saved.litellm_key)
    .bind(saved.openai_api_key)
    .bind(saved.openai_base_url.as_deref())
    .bind(saved.model_fast)
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
             (workspace_id, provider, litellm_host, litellm_key, openai_api_key, openai_base_url, model_fast, completions_routed)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(workspace)
    .bind(saved.provider)
    .bind(saved.litellm_host.as_deref())
    .bind(saved.litellm_key)
    .bind(saved.openai_api_key)
    .bind(saved.openai_base_url.as_deref())
    .bind(saved.model_fast)
    .bind(saved.routed)
    .execute(pool)
    .await
    .expect("the workspace's settings are writable");
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
