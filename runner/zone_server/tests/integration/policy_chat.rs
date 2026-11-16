//! A chat's turn under its organization's agent policy, end to end over a real WebSocket with a
//! scripted LiteLLM: a refused turn ends on a `denied` frame carrying its `policy_*` code and is
//! audited, the classifier and `auto` choose only what the policy admits, a lapsed license still
//! enforces, Community enforces nothing, and rules that can't be read refuse everything.

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as WsMessage,
};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_enterprise::Tier;
use zone_server::config::{AgentConfig, Config};
use zone_server::db::audit::actions::AGENT_POLICY_TURN_DENIED;
use zone_server::db::reminders::{self, Reminder, Sweep};
use zone_server::db::{licenses, migrations, policies};
use zone_server::state::AppState;
use zone_server::workers::reminders::{Lease, run};

use crate::common::sso::{active, keyring, lapsed};
use crate::common::{
    Database, TestClient, agents, create_test_state, serve, test_config, test_email, test_password,
};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const PREFIX: &str = "policy_chat";
const COMPLETIONS: &str = "/chat/completions";
const ROUTES: &str = "/v2/model/info";
const TAGS: &str = "/api/tags";
const ALLOWED: &str = "allowed-chat";
const DENIED: &str = "denied-chat";
const CLASSIFIER: &str = "denied-classifier";
const CLASSIFYING: &str = "Return exactly IMAGE, AUDIO, or CHAT";
const TITLING: &str = "concise chat title";
const REPLY: &str = "The tide is in.";
const AMBIGUOUS: &str = "write a song about the sea";
const FRAME_WAIT: Duration = Duration::from_secs(10);

/// What the organization's LiteLLM routes every model to.
#[derive(Clone, Copy)]
enum Upstream {
    Vertex,
    Azure,
}

impl Upstream {
    fn route(self) -> &'static str {
        match self {
            Self::Vertex => "vertex_ai/*",
            Self::Azure => "azure/*",
        }
    }
}

struct Owner {
    id: Uuid,
    token: String,
}

struct Fixture {
    database: Database,
    state: AppState,
    address: String,
    litellm: MockServer,
    owner: Owner,
    organization: Uuid,
    workspace: Uuid,
    chat: Uuid,
}

impl Fixture {
    /// A workspace on the instance's LiteLLM, routing every model to `upstream`, and a chat in it
    /// asking for `model`.
    async fn new(model: &str, upstream: Upstream) -> Self {
        Self::configured(model, upstream, |_| {}).await
    }

    async fn configured(
        model: &str,
        upstream: Upstream,
        configure: impl FnOnce(&mut Config),
    ) -> Self {
        let litellm = MockServer::builder().start().await;
        Mock::given(method("GET"))
            .and(path(ROUTES))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"model_name": "*", "litellm_params": {"model": upstream.route()}}],
                "total_pages": 1
            })))
            .mount(&litellm)
            .await;
        Mock::given(method("POST"))
            .and(path(COMPLETIONS))
            .respond_with(answer)
            .mount(&litellm)
            .await;
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        let mut config = Config {
            litellm_host: litellm.uri(),
            ollama_host: litellm.uri(),
            agents: AgentConfig {
                host_login: false,
                ..agents()
            },
            ..test_config()
        };
        config.comfyui.enabled = false;
        config.web_search.enabled = false;
        configure(&mut config);
        let state = create_test_state(config, database.pool.clone());
        state.install_keyring(keyring());
        let client = TestClient::with_state(state.clone());
        let address = serve(state.clone()).await;
        let owner = register(&client).await;
        let suffix = Uuid::new_v4().simple().to_string();
        let organization = id(&client
            .post_json_auth(
                "/api/organizations",
                &json!({"name": "Policy", "slug": format!("policy-{suffix}")}),
                &owner.token,
            )
            .await
            .json_value()["organization"]["id"]);
        let workspace = id(&client
            .post_json_auth(
                &format!("/api/organizations/{organization}/workspaces"),
                &json!({"name": "Policy", "slug": format!("policy-{suffix}")}),
                &owner.token,
            )
            .await
            .json_value()["workspace"]["id"]);
        let chat = id(&client
            .post_json_auth(
                "/api/chats",
                &json!({"workspace_id": workspace, "title": "Policy", "model_name": model}),
                &owner.token,
            )
            .await
            .json_value()["chat"]["id"]);
        Self {
            database,
            state,
            address,
            litellm,
            owner,
            organization,
            workspace,
            chat,
        }
    }

    async fn license(&self, key: &str) {
        licenses::replace(&self.database.pool, key).await.unwrap();
        self.state
            .licenses()
            .load(&self.database.pool)
            .await
            .unwrap();
    }

    async fn restrict(&self, rules: Value) {
        let mut transaction = self.database.pool.begin().await.unwrap();
        policies::save_organization(&mut transaction, self.organization, &rules, self.owner.id)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }

    /// The organization's completions run on `provider`, saved as its AI settings.
    async fn provide(&self, provider: &str) {
        sqlx::query(
            "INSERT INTO organization_ai_settings (organization_id, provider) VALUES ($1, $2)",
        )
        .bind(self.organization)
        .bind(provider)
        .execute(&self.database.pool)
        .await
        .unwrap();
    }

    /// Sends `content` and reads frames until the turn ends.
    async fn turn(&self, content: &str) -> Vec<Value> {
        let mut socket = connect_async(format!("ws://{}/ws/chats/{}", self.address, self.chat))
            .await
            .unwrap()
            .0;
        socket
            .send(WsMessage::Text(
                json!({"type": "auth", "token": self.owner.token})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let init = next(&mut socket).await.expect("the connection is accepted");
        assert_eq!(init["type"], "init", "{init}");
        socket
            .send(WsMessage::Text(
                json!({"type": "send", "content": content})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let mut frames = Vec::new();
        loop {
            let frame = next(&mut socket)
                .await
                .unwrap_or_else(|| panic!("the turn never ended: {frames:?}"));
            let ended = ["denied", "error", "message_end", "cancelled"]
                .contains(&frame["type"].as_str().unwrap_or_default());
            frames.push(frame);
            if ended {
                return frames;
            }
        }
    }

    async fn audits(&self) -> Vec<(String, Option<Uuid>, Option<Uuid>, Option<Uuid>, Value)> {
        sqlx::query_as(
            "SELECT resource_type, resource_id, actor_id, organization_id, new_values \
             FROM audit_logs WHERE action = $1 ORDER BY created_at",
        )
        .bind(AGENT_POLICY_TURN_DENIED)
        .fetch_all(&self.database.pool)
        .await
        .unwrap()
    }

    /// The completions the turns asked LiteLLM for, titles left out.
    async fn completions(&self) -> Vec<Value> {
        self.litellm
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path() == COMPLETIONS)
            .map(|request| serde_json::from_slice::<Value>(&request.body).unwrap())
            .filter(|body| !body.to_string().contains(TITLING))
            .collect()
    }

    async fn stored(&self, role: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE chat_id = $1 AND role = $2")
            .bind(self.chat)
            .bind(role)
            .fetch_one(&self.database.pool)
            .await
            .unwrap()
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

/// A streamed reply to a turn, and `CHAT` to anything that classifies.
fn answer(request: &Request) -> ResponseTemplate {
    let body: Value = serde_json::from_slice(&request.body).unwrap_or_default();
    if body["stream"] != true {
        return ResponseTemplate::new(200).set_body_json(json!({
            "id": "classification", "object": "chat.completion", "created": 0,
            "model": body["model"],
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "CHAT"}, "finish_reason": "stop"}]
        }));
    }
    let chunk = |delta: Value, finish: Value| {
        json!({
            "id": "completion", "object": "chat.completion.chunk", "created": 0, "model": "test",
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]
        })
    };
    let stream = format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        chunk(json!({"content": REPLY}), Value::Null),
        chunk(json!({}), json!("stop")),
    );
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(stream)
}

async fn register(client: &TestClient) -> Owner {
    let body = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    Owner {
        id: id(&body["user"]["id"]),
        token: body["access_token"].as_str().unwrap().to_string(),
    }
}

fn id(value: &Value) -> Uuid {
    value
        .as_str()
        .unwrap_or_else(|| panic!("an id, not {value}"))
        .parse()
        .unwrap()
}

async fn next(socket: &mut Socket) -> Option<Value> {
    loop {
        let frame = tokio::time::timeout(FRAME_WAIT, socket.next())
            .await
            .ok()??
            .ok()?;
        match frame {
            WsMessage::Text(text) => return serde_json::from_str(&text).ok(),
            WsMessage::Close(_) => return None,
            _ => {}
        }
    }
}

fn last(frames: &[Value]) -> &Value {
    frames.last().expect("the turn sent frames")
}

fn assert_denied(frames: &[Value], code: &str, message: &str) {
    assert!(
        !frames.iter().any(|frame| frame["type"] == "error"),
        "a refusal is never a plain error: {frames:?}"
    );
    assert_eq!(
        last(frames),
        &json!({"type": "denied", "code": code, "message": message}),
        "{frames:?}"
    );
}

fn assert_answered(frames: &[Value]) {
    let end = last(frames);
    assert_eq!(end["type"], "message_end", "{frames:?}");
    assert_eq!(end["content"], REPLY, "{frames:?}");
    assert!(end.get("error").is_none(), "{frames:?}");
    assert!(
        !frames.iter().any(|frame| frame["type"] == "denied"),
        "{frames:?}"
    );
}

#[tokio::test]
async fn a_denied_agent_is_refused_with_its_code_and_audited_before_anything_is_stored() {
    let fixture = Fixture::new(ALLOWED, Upstream::Vertex).await;
    fixture.provide("claude_code").await;
    fixture.restrict(json!({"agents": ["codex"]})).await;
    fixture.license(&active(Tier::Enterprise)).await;

    let frames = fixture.turn("Hello").await;

    assert_denied(
        &frames,
        "policy_agent_denied",
        "Your organization's policy doesn't allow Claude Code in this workspace.",
    );
    let audits = fixture.audits().await;
    assert_eq!(audits.len(), 1, "{audits:?}");
    let (resource, subject, actor, organization, values) = &audits[0];
    assert_eq!(resource, "chat");
    assert_eq!(*subject, Some(fixture.chat));
    assert_eq!(*actor, Some(fixture.owner.id));
    assert_eq!(*organization, Some(fixture.organization));
    assert_eq!(
        values,
        &json!({"code": "policy_agent_denied", "detail": null})
    );
    assert_eq!(
        fixture.stored("user").await,
        0,
        "a turn refused before it starts stores nothing, as an unusable route does"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_model_the_policy_denies_is_refused_and_never_sent() {
    let fixture = Fixture::new(DENIED, Upstream::Vertex).await;
    fixture.restrict(json!({"models": ["allowed-*"]})).await;
    fixture.license(&active(Tier::Team)).await;

    let frames = fixture.turn("Hello").await;

    assert_denied(
        &frames,
        "policy_model_denied",
        "Your organization's policy doesn't allow this model in this workspace. Choose an allowed model.",
    );
    assert!(fixture.completions().await.is_empty());
    let audits = fixture.audits().await;
    assert_eq!(audits.len(), 1, "{audits:?}");
    assert_eq!(
        audits[0].4,
        json!({"code": "policy_model_denied", "detail": DENIED})
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_model_litellm_routes_to_a_provider_the_policy_denies_is_refused() {
    let fixture = Fixture::new(ALLOWED, Upstream::Azure).await;
    fixture.restrict(json!({"providers": ["vertex"]})).await;
    fixture.license(&active(Tier::Enterprise)).await;

    let frames = fixture.turn("Hello").await;

    assert_denied(
        &frames,
        "policy_provider_denied",
        "Your organization's policy doesn't allow Azure OpenAI in this workspace.",
    );
    assert!(fixture.completions().await.is_empty());
    assert_eq!(
        fixture.audits().await[0].4,
        json!({"code": "policy_provider_denied", "detail": ALLOWED})
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_turn_the_policy_allows_completes() {
    let fixture = Fixture::new(ALLOWED, Upstream::Vertex).await;
    fixture
        .restrict(json!({"providers": ["vertex"], "models": ["allowed-*"]}))
        .await;
    fixture.license(&active(Tier::Enterprise)).await;

    let frames = fixture.turn("Hello").await;

    assert_answered(&frames);
    let completions = fixture.completions().await;
    assert_eq!(completions.len(), 1, "{completions:?}");
    assert_eq!(completions[0]["model"], ALLOWED);
    assert!(fixture.audits().await.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn auto_runs_on_a_model_the_policy_admits() {
    let fixture = Fixture::new("auto", Upstream::Vertex).await;
    Mock::given(method("GET"))
        .and(path(TAGS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [
                {"name": "llama3.2:3b", "size": 2_000, "details": {"parameter_size": "3B"}},
                {"name": "qwen3:8b", "size": 5_000, "details": {"parameter_size": "8B"}}
            ]
        })))
        .mount(&fixture.litellm)
        .await;
    fixture.restrict(json!({"models": ["qwen*"]})).await;
    fixture.license(&active(Tier::Enterprise)).await;

    let frames = fixture.turn("Hello").await;

    assert_answered(&frames);
    let completions = fixture.completions().await;
    assert_eq!(completions.len(), 1, "{completions:?}");
    assert_eq!(
        completions[0]["model"], "qwen3:8b",
        "auto passed over the smaller model the policy denies"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_classifier_never_runs_on_a_route_the_policy_denies() {
    let fixture = Fixture::configured(ALLOWED, Upstream::Azure, |config| {
        config.comfyui.enabled = true;
        config.comfyui.classifier_model = CLASSIFIER.to_string();
    })
    .await;
    fixture.restrict(json!({"providers": ["vertex"]})).await;
    fixture.license(&active(Tier::Enterprise)).await;

    let frames = fixture.turn(AMBIGUOUS).await;

    assert_denied(
        &frames,
        "policy_provider_denied",
        "Your organization's policy doesn't allow Azure OpenAI in this workspace.",
    );
    assert!(
        fixture.completions().await.is_empty(),
        "neither the classifier nor the turn reached the denied provider"
    );
    let details: Vec<Value> = fixture
        .audits()
        .await
        .into_iter()
        .map(|(_, _, _, _, values)| values)
        .collect();
    assert_eq!(
        details,
        vec![
            json!({"code": "policy_provider_denied", "detail": CLASSIFIER}),
            json!({"code": "policy_provider_denied", "detail": ALLOWED}),
        ],
        "the skipped classification and the refused turn are each audited"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_classifier_runs_on_a_model_the_policy_admits_in_place_of_a_denied_one() {
    let fixture = Fixture::configured(ALLOWED, Upstream::Vertex, |config| {
        config.comfyui.enabled = true;
        config.comfyui.classifier_model = CLASSIFIER.to_string();
    })
    .await;
    fixture.restrict(json!({"models": ["allowed-*"]})).await;
    fixture.license(&active(Tier::Enterprise)).await;

    let frames = fixture.turn(AMBIGUOUS).await;

    assert_answered(&frames);
    let completions = fixture.completions().await;
    let classified: Vec<&Value> = completions
        .iter()
        .filter(|body| body.to_string().contains(CLASSIFYING))
        .collect();
    assert_eq!(classified.len(), 1, "{completions:?}");
    assert_eq!(classified[0]["model"], ALLOWED);
    assert!(
        completions.iter().all(|body| body["model"] != CLASSIFIER),
        "{completions:?}"
    );
    assert!(fixture.audits().await.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_still_enforces() {
    let fixture = Fixture::new(ALLOWED, Upstream::Azure).await;
    fixture.restrict(json!({"providers": ["vertex"]})).await;
    fixture.license(&lapsed(Tier::Team)).await;

    let frames = fixture.turn("Hello").await;

    assert_eq!(
        last(&frames)["code"],
        "policy_provider_denied",
        "{frames:?}"
    );
    assert!(fixture.completions().await.is_empty());
    assert_eq!(fixture.audits().await.len(), 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn community_enforces_nothing() {
    let fixture = Fixture::new(DENIED, Upstream::Azure).await;
    fixture
        .restrict(json!({"providers": ["vertex"], "models": ["allowed-*"]}))
        .await;

    let frames = fixture.turn("Hello").await;

    assert_answered(&frames);
    assert_eq!(fixture.completions().await[0]["model"], DENIED);
    assert!(fixture.audits().await.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_policy_that_cannot_be_read_refuses_the_turn() {
    let fixture = Fixture::new(ALLOWED, Upstream::Vertex).await;
    fixture.restrict(json!({"providers": "everything"})).await;
    fixture.license(&active(Tier::Enterprise)).await;

    let frames = fixture.turn("Hello").await;

    assert_denied(
        &frames,
        "policy_unreadable",
        "Your organization's agent policy couldn't be read, so this can't start. Try again shortly.",
    );
    assert!(fixture.completions().await.is_empty());
    assert_eq!(
        fixture.audits().await[0].4,
        json!({"code": "policy_unreadable", "detail": null})
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_reminder_on_a_denied_route_sends_no_completion() {
    let fixture = Fixture::new(ALLOWED, Upstream::Azure).await;
    fixture.restrict(json!({"providers": ["vertex"]})).await;
    fixture.license(&active(Tier::Enterprise)).await;
    let created = reminders::create(
        &fixture.database.pool,
        fixture.workspace,
        fixture.owner.id,
        fixture.chat,
        Reminder {
            content: "Tide check".into(),
            due_at: (chrono::Utc::now() + chrono::Duration::hours(1)).fixed_offset(),
            rrule: None,
            prompt: Some("Check the tide.".into()),
            timing_mode: None,
            time_zone: None,
        },
    )
    .await
    .unwrap();
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    sqlx::query("UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(reminder)
        .execute(&fixture.database.pool)
        .await
        .unwrap();
    let sweep = Sweep::Workspace(fixture.workspace);
    let lease = Lease::default();
    reminders::deliver_next(&fixture.database.pool, sweep)
        .await
        .unwrap();
    let owed = reminders::claim_turn(&fixture.database.pool, sweep, lease.duration)
        .await
        .unwrap()
        .expect("the firing is owed a turn");

    tokio::time::timeout(Duration::from_secs(30), run(&fixture.state, owed, lease))
        .await
        .expect("the refused firing ends");

    assert!(fixture.completions().await.is_empty());
    assert_eq!(fixture.stored("assistant").await, 0);
    let audits = fixture.audits().await;
    assert_eq!(audits.len(), 1, "{audits:?}");
    assert_eq!(audits[0].1, Some(fixture.chat));
    assert_eq!(audits[0].2, Some(fixture.owner.id));
    fixture.cleanup().await;
}
