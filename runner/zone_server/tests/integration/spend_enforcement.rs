//! Budgets held against attributed LiteLLM traffic, end to end: a member's turn over a real
//! WebSocket and the chat context route are sent under their own derived key and stopped once a
//! hard budget covering them is used up; coding agents, unlicensed servers and soft budgets are
//! never stopped; the pull request repair's fallback onto the instance is held like any other
//! route; and the spend sweep ingests, settles and mails each admin once a month.

use std::time::Duration;

use abnegate_notify::Fanout;
use chrono::{Months, TimeDelta, Utc};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as WsMessage,
};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, Request, ResponseTemplate};
use zone_enterprise::spend::{BUDGET_EXHAUSTED, Exhausted, Month, Scope, Target};
use zone_server::config::AgentConfig;
use zone_server::db::ai_settings::PROVIDER_CLAUDE_CODE;
use zone_server::db::audit::actions::{
    BUDGET_EXHAUSTED as EXHAUSTED_ACTION, BUDGET_THRESHOLD_REACHED,
};
use zone_server::db::audit::resources::BUDGET;
use zone_server::db::chats::{self, ChatRow};
use zone_server::db::organization_members::OrgRole;
use zone_server::db::workspace_members::WorkspaceRole;
use zone_server::services::route::{Reason, Route};
use zone_server::services::spend::sweep::Sweep;

use crate::common::agents;
use crate::common::spend::{Fixture, GENERATE_PATH, MASTER, log};
use crate::common::sso::Account;

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const COMPLETIONS: &str = "/chat/completions";
const ROUTES: &str = "/v2/model/info";
const MODEL: &str = "gpt-4o";
const TITLING: &str = "concise chat title";
const REPLY: &str = "The tide is in.";
const FRAME_WAIT: Duration = Duration::from_secs(10);
const LIMIT: i64 = 50_000_000;
const OVER: i64 = 60_000_000;
const ORGANIZATION_SPENT: &str = "Your organization's monthly AI budget is used up, so this request wasn't sent. An organization admin can raise it under Settings › Spend; it resets on the 1st (UTC).";
const WORKSPACE_SPENT: &str = "This workspace's monthly AI budget is used up, so this request wasn't sent. An organization admin can raise it under Settings › Spend; it resets on the 1st (UTC).";
const MEMBER_SPENT: &str = "Your monthly AI budget is used up, so this request wasn't sent. An organization admin can raise it under Settings › Spend; it resets on the 1st (UTC).";

/// A spend fixture serving a WebSocket, whose stand-in LiteLLM answers completions, with a chat
/// in its workspace.
struct Chatting {
    fixture: Fixture,
    address: String,
    chat: Uuid,
}

impl Chatting {
    async fn new(prefix: &'static str) -> Self {
        let fixture = Fixture::configured(prefix, |config| {
            config.ollama_host = config.litellm_host.clone();
            config.comfyui.enabled = false;
            config.web_search.enabled = false;
            config.agents = AgentConfig {
                host_login: false,
                ..agents()
            };
        })
        .await;
        Mock::given(method("GET"))
            .and(path(ROUTES))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"model_name": "*", "litellm_params": {"model": "openai/*"}}],
                "total_pages": 1
            })))
            .mount(&fixture.litellm)
            .await;
        Mock::given(method("POST"))
            .and(path(COMPLETIONS))
            .respond_with(answer)
            .mount(&fixture.litellm)
            .await;
        fixture.mount_register().await;
        let address = fixture.serve().await;
        let created = fixture
            .policy
            .sso
            .client
            .post_json_auth(
                "/api/chats",
                &json!({"workspace_id": fixture.policy.workspace, "title": "Spend", "model_name": MODEL}),
                &fixture.policy.owner.token,
            )
            .await
            .json_value();
        let chat = created["chat"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("a chat, not {created}"))
            .parse()
            .unwrap();
        Self {
            fixture,
            address,
            chat,
        }
    }

    fn owner(&self) -> &Account {
        &self.fixture.policy.owner
    }

    async fn row(&self) -> ChatRow {
        chats::get_chat(self.fixture.pool(), self.chat)
            .await
            .unwrap()
            .expect("the chat")
    }

    /// The frames of one turn the owner sends, through to the one that ends it.
    async fn turn(&self, content: &str) -> Vec<Value> {
        let mut socket = connect_async(format!("ws://{}/ws/chats/{}", self.address, self.chat))
            .await
            .unwrap()
            .0;
        socket
            .send(WsMessage::Text(
                json!({"type": "auth", "token": self.owner().token})
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

    /// What the chat context route answers the owner's draft with.
    async fn context(&self) -> (u16, Value) {
        let response = self
            .fixture
            .policy
            .sso
            .client
            .post_json_auth(
                &format!("/api/chats/{}/context", self.chat),
                &json!({"content": "Hello"}),
                &self.owner().token,
            )
            .await;
        (response.status.as_u16(), response.json_value())
    }

    /// The completions the turns asked LiteLLM for, titles left out.
    async fn completions(&self) -> Vec<Request> {
        self.fixture
            .received(COMPLETIONS)
            .await
            .into_iter()
            .filter(|request| !String::from_utf8_lossy(&request.body).contains(TITLING))
            .collect()
    }

    async fn cleanup(self) {
        self.fixture.cleanup().await;
    }
}

/// A streamed reply to every completion.
fn answer(_: &Request) -> ResponseTemplate {
    let chunk = |delta: Value, finish: Value| {
        json!({
            "id": "completion", "object": "chat.completion.chunk", "created": 0, "model": MODEL,
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

fn assert_answered(frames: &[Value]) {
    let end = last(frames);
    assert_eq!(end["type"], "message_end", "{frames:?}");
    assert_eq!(end["content"], REPLY, "{frames:?}");
}

fn bearer(request: &Request) -> String {
    request
        .headers
        .get("authorization")
        .map(|value| value.to_str().unwrap_or_default().to_string())
        .unwrap_or_default()
}

fn workspace(fixture: &Fixture) -> Target {
    Target::of(Some(fixture.policy.workspace), None)
}

fn organization() -> Target {
    Target::of(None, None)
}

fn reason(route: &Route) -> Option<Reason> {
    route.endpoint().err().map(|unusable| unusable.reason)
}

fn used_up(scope: Scope) -> Option<Reason> {
    Some(Reason::Exhausted(Exhausted { scope }))
}

/// The organization's completions run on Claude Code.
async fn on_claude_code(fixture: &Fixture) {
    sqlx::query(
        "INSERT INTO organization_ai_settings (organization_id, provider, completions_routed) \
         VALUES ($1, $2, true)",
    )
    .bind(fixture.policy.organization)
    .bind(PROVIDER_CLAUDE_CODE)
    .execute(fixture.pool())
    .await
    .unwrap();
}

#[tokio::test]
async fn a_used_up_hard_workspace_budget_stops_a_members_turn_and_their_draft() {
    let chatting = Chatting::new("spend_blocked").await;
    let fixture = &chatting.fixture;
    fixture.licensed().await;
    fixture.charge(&fixture.automation(), OVER).await;
    fixture.budget(workspace(fixture), LIMIT, true).await;

    let frames = chatting.turn("Hello").await;
    let (status, refused) = chatting.context().await;

    assert_eq!(
        last(&frames),
        &json!({"type": "error", "message": WORKSPACE_SPENT}),
        "{frames:?}"
    );
    assert_eq!(status, 409, "{refused}");
    assert_eq!(refused["error"], WORKSPACE_SPENT);
    assert!(
        chatting.completions().await.is_empty(),
        "a stopped turn reached LiteLLM"
    );
    chatting.cleanup().await;
}

#[tokio::test]
async fn a_soft_budget_past_its_limit_stops_nothing() {
    let chatting = Chatting::new("spend_soft").await;
    let fixture = &chatting.fixture;
    fixture.licensed().await;
    fixture.charge(&fixture.automation(), OVER).await;
    fixture.budget(workspace(fixture), LIMIT, false).await;

    let frames = chatting.turn("Hello").await;

    assert_answered(&frames);
    chatting.cleanup().await;
}

#[tokio::test]
async fn a_members_turns_go_under_their_own_key_registered_once() {
    let chatting = Chatting::new("spend_attributed").await;
    let fixture = &chatting.fixture;
    fixture.licensed().await;
    let key = fixture.key(&fixture.subject());

    assert_answered(&chatting.turn("Hello").await);
    assert_answered(&chatting.turn("Again").await);

    let completions = chatting.completions().await;
    assert_eq!(completions.len(), 2, "{completions:?}");
    for completion in &completions {
        assert_eq!(
            bearer(completion),
            format!("Bearer {}", key.secret.expose())
        );
        assert!(bearer(completion).starts_with("Bearer sk-zone-"));
    }
    let registered: Vec<Request> = fixture
        .received(GENERATE_PATH)
        .await
        .into_iter()
        .filter(|request| {
            serde_json::from_slice::<Value>(&request.body).unwrap_or_default()["key"]
                == key.secret.expose()
        })
        .collect();
    assert_eq!(registered.len(), 1, "the member's key was registered again");
    chatting.cleanup().await;
}

#[tokio::test]
async fn an_unlicensed_server_stops_nothing_and_sends_the_master_key() {
    let chatting = Chatting::new("spend_unlicensed").await;
    let fixture = &chatting.fixture;
    fixture.charge(&fixture.automation(), OVER).await;
    fixture.budget(workspace(fixture), LIMIT, true).await;

    let frames = chatting.turn("Hello").await;

    assert_answered(&frames);
    let completions = chatting.completions().await;
    assert_eq!(completions.len(), 1, "{completions:?}");
    assert_eq!(bearer(&completions[0]), format!("Bearer {MASTER}"));
    assert!(
        fixture.received(GENERATE_PATH).await.is_empty(),
        "an unlicensed server registered a key"
    );
    chatting.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_still_stops_a_used_up_budget() {
    let chatting = Chatting::new("spend_lapsed").await;
    let fixture = &chatting.fixture;
    fixture.lapsed().await;
    fixture.charge(&fixture.automation(), OVER).await;
    fixture.budget(workspace(fixture), LIMIT, true).await;

    let frames = chatting.turn("Hello").await;

    assert_eq!(last(&frames)["message"], WORKSPACE_SPENT, "{frames:?}");
    chatting.cleanup().await;
}

#[tokio::test]
async fn a_members_budget_stops_their_turns_and_not_their_workspaces_automation() {
    let chatting = Chatting::new("spend_member").await;
    let fixture = &chatting.fixture;
    fixture.licensed().await;
    fixture.charge(&fixture.subject(), OVER).await;
    let owner = chatting.owner().id;
    fixture
        .budget(Target::of(None, Some(owner)), LIMIT, true)
        .await;
    let chat = chatting.row().await;

    let turn = Route::for_turn(fixture.state(), &chat, owner).await;
    let automation = Route::for_workspace(fixture.state(), fixture.policy.workspace).await;
    let titles = Route::for_chat(fixture.state(), &chat).await;
    let (status, refused) = chatting.context().await;

    assert_eq!(reason(&turn), used_up(Scope::User));
    let unusable = turn.endpoint().expect_err("the member's turn is stopped");
    assert_eq!(unusable.to_string(), MEMBER_SPENT);
    assert_eq!(unusable.code(), Some(BUDGET_EXHAUSTED));
    assert_eq!(
        (status, refused["error"].as_str()),
        (409, Some(MEMBER_SPENT))
    );
    for route in [&automation, &titles] {
        let endpoint = route
            .endpoint()
            .expect("the workspace's automation is not the member's spend");
        assert_eq!(
            endpoint.key().expose(),
            fixture.key(&fixture.automation()).secret.expose()
        );
    }
    chatting.cleanup().await;
}

#[tokio::test]
async fn a_coding_agent_is_never_stopped_or_attributed() {
    let chatting = Chatting::new("spend_agent").await;
    let fixture = &chatting.fixture;
    fixture.licensed().await;
    on_claude_code(fixture).await;
    fixture.charge(&fixture.automation(), OVER).await;
    fixture.budget(organization(), LIMIT, true).await;
    let chat = chatting.row().await;

    let routes = [
        Route::for_workspace(fixture.state(), fixture.policy.workspace).await,
        Route::for_turn(fixture.state(), &chat, chatting.owner().id).await,
    ];

    for route in &routes {
        let endpoint = route
            .endpoint()
            .expect("a coding agent's route is not stopped");
        assert_eq!(endpoint.key().expose(), MASTER);
    }
    assert!(fixture.received(GENERATE_PATH).await.is_empty());
    chatting.cleanup().await;
}

#[tokio::test]
async fn the_pull_request_repair_fallback_onto_the_instance_is_attributed_and_stopped() {
    let fixture = Fixture::new("spend_repair").await;
    fixture.mount_register().await;
    fixture.licensed().await;
    on_claude_code(&fixture).await;
    fixture.charge(&fixture.automation(), OVER).await;
    let budget = fixture.budget(organization(), LIMIT, true).await;
    let config = fixture.state().config().clone();

    let agent = Route::for_workspace(fixture.state(), fixture.policy.workspace).await;
    let stopped = agent
        .clone()
        .on_instance(&config)
        .spent(fixture.state())
        .await;
    fixture.limit(&budget, OVER * 2).await;
    let raised = agent
        .clone()
        .on_instance(&config)
        .spent(fixture.state())
        .await;

    assert!(
        agent.endpoint().is_ok(),
        "the agent's own route was stopped"
    );
    assert_eq!(reason(&stopped), used_up(Scope::Organization));
    assert_eq!(
        stopped
            .endpoint()
            .err()
            .map(|unusable| unusable.to_string()),
        Some(ORGANIZATION_SPENT.to_string())
    );
    assert_eq!(
        raised
            .endpoint()
            .map(|endpoint| endpoint.key().expose().to_string()),
        Ok(fixture
            .key(&fixture.automation())
            .secret
            .expose()
            .to_string()),
        "the repair on the instance's LiteLLM was not sent under its workspace's key"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_budget_used_up_last_month_stops_nothing_this_month() {
    let fixture = Fixture::new("spend_month").await;
    fixture.mount_register().await;
    fixture.licensed().await;
    fixture.charge(&fixture.automation(), OVER).await;
    let budget = fixture.budget(workspace(&fixture), LIMIT, true).await;
    let last_month = Month::current()
        .date()
        .checked_sub_months(Months::new(1))
        .unwrap();

    let this_month = Route::for_workspace(fixture.state(), fixture.policy.workspace).await;
    sqlx::query("UPDATE budgets SET exhausted_month = $2 WHERE id = $1")
        .bind(budget.id)
        .bind(last_month)
        .execute(fixture.pool())
        .await
        .unwrap();
    let next_month = Route::for_workspace(fixture.state(), fixture.policy.workspace).await;

    assert_eq!(reason(&this_month), used_up(Scope::Workspace));
    assert!(
        next_month.endpoint().is_ok(),
        "a budget used up last month still stopped requests"
    );
    fixture.cleanup().await;
}

#[derive(Debug, sqlx::FromRow)]
struct Announced {
    actor_id: Option<Uuid>,
    organization_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    resource_type: String,
    resource_id: Option<Uuid>,
    new_values: Option<Value>,
}

async fn announced(fixture: &Fixture, action: &str) -> Vec<Announced> {
    sqlx::query_as(
        "SELECT actor_id, organization_id, workspace_id, resource_type, resource_id, new_values
         FROM audit_logs WHERE action = $1 ORDER BY created_at",
    )
    .bind(action)
    .fetch_all(fixture.pool())
    .await
    .unwrap()
}

async fn sweep(fixture: &Fixture) {
    Sweep::default()
        .run(fixture.state(), &Fanout::new())
        .await
        .expect("the sweep runs");
}

#[tokio::test]
async fn the_sweep_ingests_settles_and_mails_every_admin_once_a_month() {
    let fixture = Fixture::new("spend_sweep").await;
    fixture.mount_register().await;
    fixture.licensed().await;
    let mut admins = vec![fixture.policy.owner.email.clone()];
    for _ in 0..8 {
        admins.push(fixture.policy.member(OrgRole::Admin, None).await.email);
    }
    admins.push(
        fixture
            .policy
            .member(OrgRole::Member, Some(WorkspaceRole::Admin))
            .await
            .email,
    );
    let bystander = fixture
        .policy
        .member(OrgRole::Member, Some(WorkspaceRole::Member))
        .await;
    let key = fixture.derive(&fixture.automation()).await;
    fixture
        .mount_logs(vec![log(
            "swept",
            &key.token,
            Utc::now() - TimeDelta::minutes(1),
            60.0,
        )])
        .await;
    let budget = fixture.budget(workspace(&fixture), LIMIT, true).await;
    assert!(
        Route::for_workspace(fixture.state(), fixture.policy.workspace)
            .await
            .endpoint()
            .is_ok(),
        "nothing was spent before the sweep"
    );

    sweep(&fixture).await;
    let mailed = fixture.mailed();
    sweep(&fixture).await;

    let mut sorted = mailed.clone();
    sorted.sort();
    admins.sort();
    assert_eq!(
        sorted, admins,
        "every admin is mailed once, past the mail concurrency bound"
    );
    assert!(!mailed.contains(&bystander.email));
    assert_eq!(fixture.mailed(), mailed, "a second sweep mailed again");
    let stored = fixture.stored().await;
    assert_eq!(stored.len(), 1, "{stored:?}");
    assert_eq!(stored[0].spend_microdollars, OVER);
    let audits = announced(&fixture, EXHAUSTED_ACTION).await;
    assert_eq!(audits.len(), 1, "{audits:?}");
    let audit = &audits[0];
    assert_eq!(audit.actor_id, None);
    assert_eq!(audit.organization_id, Some(fixture.policy.organization));
    assert_eq!(audit.workspace_id, Some(fixture.policy.workspace));
    assert_eq!(audit.resource_type, BUDGET);
    assert_eq!(audit.resource_id, Some(budget.id));
    assert_eq!(
        audit.new_values,
        Some(json!({
            "month": Month::current().label(),
            "spent_microdollars": OVER,
            "limit_microdollars": LIMIT,
            "hard": true,
        }))
    );
    assert!(
        announced(&fixture, BUDGET_THRESHOLD_REACHED)
            .await
            .is_empty()
    );
    assert_eq!(
        reason(&Route::for_workspace(fixture.state(), fixture.policy.workspace).await),
        used_up(Scope::Workspace),
        "the sweep did not mark the budget used up"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn changing_the_limit_rearms_the_notices() {
    let fixture = Fixture::new("spend_rearm").await;
    fixture.mount_register().await;
    fixture.mount_logs(Vec::new()).await;
    fixture.licensed().await;
    fixture.charge(&fixture.automation(), OVER).await;
    let budget = fixture.budget(organization(), LIMIT, false).await;

    sweep(&fixture).await;
    let exhausted = fixture.mailed().len();
    fixture.limit(&budget, 70_000_000).await;
    sweep(&fixture).await;
    let warned = fixture.mailed().len();
    fixture.limit(&budget, 55_000_000).await;
    sweep(&fixture).await;
    sweep(&fixture).await;
    let again = fixture.mailed().len();

    assert_eq!(exhausted, 1);
    assert_eq!(
        warned, 2,
        "raising the limit past the spend did not re-arm the warning"
    );
    assert_eq!(
        again, 3,
        "lowering it under the spend did not re-arm the exhaustion"
    );
    assert_eq!(announced(&fixture, BUDGET_THRESHOLD_REACHED).await.len(), 1);
    let exhaustions = announced(&fixture, EXHAUSTED_ACTION).await;
    assert_eq!(exhaustions.len(), 2);
    assert_eq!(
        exhaustions[1]
            .new_values
            .as_ref()
            .map(|values| values["limit_microdollars"].clone()),
        Some(json!(55_000_000))
    );
    fixture.cleanup().await;
}
