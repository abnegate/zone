//! Every REST route that chooses a provider, agent or model answers a choice its agent policy
//! denies with 403 and the denial's code, audited as a denied turn, and marks what the policy
//! denies in the offers, models and agent statuses it lists. Sign-ins and an organization's AI
//! settings answer to the organization's own policy; a workspace's choices to both layers.
//! Community enforces nothing, and a lapsed license still does.

use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_server::db::audit::actions::AGENT_POLICY_TURN_DENIED;
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::db::{licenses, migrations, organizations, policies, workspaces};

use crate::common::sso::{Account, Audit, active, keyring, lapsed};
use crate::common::{
    Database, TestClient, TestResponse, create_test_state, test_config, test_email, test_password,
};

const PREFIX: &str = "policy_rest";
const OPENAI_KEY: &str = "sk-policy-rest-the-tenants-own-openai-key";
const FAST: &str = "gpt-4o-mini";
const REASONING: &str = "gpt-4o";
const INSTALLED: &str = "qwen3:8b";
const UNLISTED: &str = "llama3.2:3b";
const PROVIDER_DENIED: &str = "policy_provider_denied";
const AGENT_DENIED: &str = "policy_agent_denied";
const MODEL_DENIED: &str = "policy_model_denied";
const UNREADABLE: &str = "policy_unreadable";
const ORGANIZATION: &str = "organization";
const WORKSPACE: &str = "workspace";
const CHAT: &str = "chat";
const TASK: &str = "task";

struct Fixture {
    database: Database,
    client: TestClient,
    _ollama: MockServer,
    /// The server admin, who owns the organization and its workspace.
    owner: Account,
    organization: Uuid,
    workspace: Uuid,
}

impl Fixture {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        let ollama = MockServer::start().await;
        installs(&ollama).await;
        let mut config = test_config();
        config.ollama_host = ollama.uri();
        config.litellm_host = ollama.uri();
        config.auto.enabled = true;
        let state = create_test_state(config, database.pool.clone());
        state.install_keyring(keyring());
        let client = TestClient::with_state(state);
        let owner = register(&client).await;
        let organization = organizations::create_organization(
            &database.pool,
            "Policy",
            &format!("policy-{}", Uuid::new_v4()),
            None,
        )
        .await
        .unwrap()
        .id;
        organization_members::add_member(
            &database.pool,
            organization,
            owner.id,
            OrgRole::Owner,
            None,
        )
        .await
        .unwrap();
        let workspace = workspaces::create_workspace(
            &database.pool,
            organization,
            "Policy",
            &format!("policy-{}", Uuid::new_v4()),
            None,
        )
        .await
        .unwrap()
        .id;
        workspace_members::add_member(
            &database.pool,
            workspace,
            owner.id,
            WorkspaceRole::Owner,
            None,
        )
        .await
        .unwrap();
        Self {
            database,
            client,
            _ollama: ollama,
            owner,
            organization,
            workspace,
        }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    fn token(&self) -> &str {
        &self.owner.token
    }

    async fn license(&self) {
        self.client
            .put_json_auth("/api/license", &json!({ "key": active() }), self.token())
            .await
            .assert_status(StatusCode::OK);
    }

    /// Stores a lapsed key, which the license route would refuse, and reloads it.
    async fn lapse(&self) {
        licenses::replace(self.pool(), &lapsed()).await.unwrap();
        self.client
            .state()
            .licenses()
            .load(self.pool())
            .await
            .unwrap();
    }

    /// Registers someone with `role` in the organization and a member's seat in its workspace.
    async fn member(&self, role: OrgRole) -> Account {
        self.seated(role, WorkspaceRole::Member).await
    }

    async fn seated(&self, role: OrgRole, seat: WorkspaceRole) -> Account {
        let account = register(&self.client).await;
        organization_members::add_member(self.pool(), self.organization, account.id, role, None)
            .await
            .unwrap();
        workspace_members::add_member(self.pool(), self.workspace, account.id, seat, None)
            .await
            .unwrap();
        account
    }

    /// Stores `rules` as they are, without validating them, as the organization's layer.
    async fn restrict(&self, rules: Value) {
        let mut transaction = self.pool().begin().await.unwrap();
        policies::save_organization(&mut transaction, self.organization, &rules, self.owner.id)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }

    /// Stores `rules` as they are, without validating them, as the workspace's layer.
    async fn restrict_workspace(&self, rules: Value) {
        let mut transaction = self.pool().begin().await.unwrap();
        policies::save_workspace(&mut transaction, self.workspace, &rules, self.owner.id)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }

    async fn organization_settings(&self, token: &str, settings: Value) -> TestResponse {
        self.client
            .put_json_auth(
                &format!("/api/organizations/{}/settings/ai", self.organization),
                &settings,
                token,
            )
            .await
    }

    async fn workspace_settings(&self, settings: Value) -> TestResponse {
        self.client
            .put_json_auth(
                &format!(
                    "/api/organizations/{}/workspaces/{}/settings/ai",
                    self.organization, self.workspace
                ),
                &settings,
                self.token(),
            )
            .await
    }

    /// Sends the workspace's chats to OpenAI on the organization's own key, saving its Fast and
    /// Reasoning models.
    async fn save_openai(&self) {
        self.organization_settings(
            self.token(),
            json!({
                "provider": "openai",
                "openai_api_key": OPENAI_KEY,
                "model_fast": FAST,
                "model_reasoning": REASONING,
            }),
        )
        .await
        .assert_status(StatusCode::OK);
    }

    async fn create_chat(&self, model: &str) -> TestResponse {
        self.client
            .post_json_auth(
                "/api/chats",
                &json!({
                    "workspace_id": self.workspace,
                    "title": "Policy chat",
                    "model_name": model,
                }),
                self.token(),
            )
            .await
    }

    async fn chat(&self, model: &str) -> String {
        let created = self.create_chat(model).await;
        created.assert_status(StatusCode::CREATED);
        created.json_value()["chat"]["id"]
            .as_str()
            .expect("a chat id")
            .to_owned()
    }

    async fn change_model(&self, chat: &str, model: &str) -> TestResponse {
        self.client
            .put_json_auth(
                &format!("/api/chats/{chat}"),
                &json!({ "model_name": model }),
                self.token(),
            )
            .await
    }

    async fn choose(&self, chat: &str, choice: Value) -> TestResponse {
        self.client
            .put_json_auth(
                &format!("/api/chats/{chat}/provider"),
                &choice,
                self.token(),
            )
            .await
    }

    async fn workspace_offers(&self) -> Value {
        let listed = self
            .client
            .get_auth(
                &format!("/api/workspaces/{}/chat-providers", self.workspace),
                self.token(),
            )
            .await;
        listed.assert_status(StatusCode::OK);
        listed.json_value()
    }

    async fn chat_offers(&self, chat: &str) -> Value {
        let listed = self
            .client
            .get_auth(&format!("/api/chats/{chat}/providers"), self.token())
            .await;
        listed.assert_status(StatusCode::OK);
        listed.json_value()
    }

    async fn models(&self) -> TestResponse {
        self.client
            .get_auth(
                &format!("/api/models?workspace_id={}", self.workspace),
                self.token(),
            )
            .await
    }

    async fn plan(&self, model: &str) -> TestResponse {
        self.client
            .post_json_auth(
                &format!("/api/workspaces/{}/projects/auto", self.workspace),
                &json!({ "brief": "Plan a small landing page", "model_name": model }),
                self.token(),
            )
            .await
    }

    async fn task(&self) -> String {
        let created = self
            .client
            .post_json_auth(
                &format!("/api/workspaces/{}/tasks", self.workspace),
                &json!({ "title": "Policy task", "description": "Write the release notes" }),
                self.token(),
            )
            .await;
        created.assert_status(StatusCode::CREATED);
        created.json_value()["task"]["id"]
            .as_str()
            .expect("a task id")
            .to_owned()
    }

    async fn update_task(&self, task: &str, model: &str) -> TestResponse {
        self.update_task_as(task, model, self.token()).await
    }

    async fn update_task_as(&self, task: &str, model: &str, token: &str) -> TestResponse {
        self.client
            .put_json_auth(
                &format!("/api/tasks/{task}"),
                &json!({ "model_name": model }),
                token,
            )
            .await
    }

    async fn task_model(&self, task: &str) -> Value {
        let read = self
            .client
            .get_auth(&format!("/api/tasks/{task}"), self.token())
            .await;
        read.assert_status(StatusCode::OK);
        read.json_value()["task"]["model_name"].clone()
    }

    async fn store_task_model(&self, task: &str, model: Option<&str>) {
        sqlx::query("UPDATE tasks SET model_name = $2 WHERE id = $1::uuid")
            .bind(task)
            .bind(model)
            .execute(self.pool())
            .await
            .unwrap();
    }

    async fn run(&self, task: &str) -> TestResponse {
        self.run_as(task, self.token()).await
    }

    async fn run_as(&self, task: &str, token: &str) -> TestResponse {
        self.client
            .post_json_auth(&format!("/api/tasks/{task}/runs"), &json!({}), token)
            .await
    }

    async fn runs(&self, task: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM task_runs WHERE task_id = $1::uuid")
            .bind(task)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn chats(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM chats WHERE workspace_id = $1")
            .bind(self.workspace)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    fn agents_path(&self) -> String {
        format!("/api/organizations/{}/agents", self.organization)
    }

    async fn agents(&self) -> Value {
        let listed = self
            .client
            .get_auth(&self.agents_path(), self.token())
            .await;
        listed.assert_status(StatusCode::OK);
        listed.json_value()
    }

    async fn sign_in(&self, agent: &str, token: &str) -> TestResponse {
        self.client
            .post_json_auth(
                &format!("{}/{agent}/login", self.agents_path()),
                &json!({ "flow": "paste" }),
                token,
            )
            .await
    }

    async fn audits(&self) -> Vec<Audit> {
        sqlx::query_as(
            "SELECT actor_id, organization_id, resource_type, resource_id, old_values, new_values
             FROM audit_logs WHERE action = $1 ORDER BY created_at",
        )
        .bind(AGENT_POLICY_TURN_DENIED)
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

async fn register(client: &TestClient) -> Account {
    let email = test_email();
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": email, "password": test_password() }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    Account {
        id: body["user"]["id"].as_str().unwrap().parse().unwrap(),
        email,
        token: body["access_token"].as_str().unwrap().to_owned(),
        refresh: body["refresh_token"].as_str().unwrap().to_owned(),
    }
}

/// An instance Ollama with two chat models installed.
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
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [installed(INSTALLED), installed(UNLISTED)]
        })))
        .mount(ollama)
        .await;
}

/// Asserts `response` is the policy's refusal with `code`, worded as the policy words it.
fn refused(response: &TestResponse, code: &str, what: &str) {
    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{what}: {}",
        response.text()
    );
    let body = response.json_value();
    assert_eq!(body["code"], json!(code), "{what}: {body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.starts_with("Your organization's")),
        "{what} says why in the policy's words: {body}"
    );
}

fn accepted(response: &TestResponse, status: StatusCode, what: &str) {
    assert_eq!(response.status, status, "{what}: {}", response.text());
}

fn offer<'a>(offers: &'a Value, provider: &str) -> &'a Value {
    offers
        .as_array()
        .expect("a list of offers")
        .iter()
        .find(|offer| offer["provider"] == provider)
        .unwrap_or_else(|| panic!("{provider} is offered: {offers}"))
}

fn names(offer: &Value) -> Vec<&str> {
    offer["models"]
        .as_array()
        .expect("an offer's models")
        .iter()
        .filter_map(|model| model["name"].as_str())
        .collect()
}

fn listed<'a>(models: &'a Value, name: &str) -> &'a Value {
    models
        .as_array()
        .unwrap_or_else(|| panic!("the models are listed: {models}"))
        .iter()
        .find(|model| model["name"] == name)
        .unwrap_or_else(|| panic!("{name} is listed: {models}"))
}

fn status<'a>(agents: &'a Value, agent: &str) -> &'a Value {
    agents["agents"]
        .as_array()
        .expect("the agents' statuses")
        .iter()
        .find(|status| status["agent"] == agent)
        .unwrap_or_else(|| panic!("{agent} has a status: {agents}"))
}

fn denial(audit: &Audit) -> (&str, Value) {
    (
        audit.resource_type.as_str(),
        audit.new_values.clone().unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn chat_providers_mark_a_denied_offer_and_offer_only_the_models_the_policy_admits() {
    let fixture = Fixture::new().await;
    fixture.license().await;
    fixture.save_openai().await;
    fixture
        .restrict(json!({ "models": [FAST, "sonnet"] }))
        .await;

    let listed = fixture.workspace_offers().await;
    let default = &listed["default"];
    assert_eq!(default["provider"], "openai");
    assert_eq!(default["usable"], json!(true), "{default}");
    assert_eq!(names(default), [FAST], "{REASONING} is denied: {default}");
    assert!(
        default.get("denial").is_none(),
        "an allowed offer carries no denial: {default}"
    );
    let chat = fixture.chat(FAST).await;

    fixture
        .restrict(json!({ "providers": ["anthropic"], "agents": [] }))
        .await;
    for (listed, what) in [
        (fixture.workspace_offers().await, "the workspace's offers"),
        (fixture.chat_offers(&chat).await, "the chat's offers"),
    ] {
        let default = &listed["default"];
        assert_eq!(default["usable"], json!(false), "{what}: {default}");
        assert_eq!(
            default["denial"],
            json!(PROVIDER_DENIED),
            "{what}: {default}"
        );
        assert!(
            default["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("OpenAI API")),
            "{what} says why: {default}"
        );
        assert_eq!(default["models"], json!([]), "{what}: {default}");
        let openai = offer(&listed["providers"], "openai");
        assert_eq!(
            (&openai["usable"], &openai["denial"]),
            (&json!(false), &json!(PROVIDER_DENIED)),
            "{what} lists the denied provider so a chat sees why: {listed}"
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_chat_choosing_a_denied_provider_or_model_is_refused_and_audited() {
    let fixture = Fixture::new().await;
    fixture.license().await;
    fixture.save_openai().await;
    let chat = fixture.chat(FAST).await;
    let before = fixture.chats().await;
    fixture.restrict(json!({ "models": [FAST] })).await;

    refused(
        &fixture.change_model(&chat, REASONING).await,
        MODEL_DENIED,
        "changing the chat to a denied model",
    );
    accepted(
        &fixture.change_model(&chat, FAST).await,
        StatusCode::OK,
        "changing the chat to an allowed model",
    );
    refused(
        &fixture.create_chat(REASONING).await,
        MODEL_DENIED,
        "creating a chat on a denied model",
    );
    refused(
        &fixture
            .choose(&chat, json!({ "provider": "openai", "model": REASONING }))
            .await,
        MODEL_DENIED,
        "choosing a denied model",
    );
    refused(
        &fixture.plan(REASONING).await,
        MODEL_DENIED,
        "planning on a denied model",
    );
    assert_eq!(fixture.chats().await, before, "no refused chat was created");

    fixture
        .restrict(json!({ "providers": ["anthropic"], "agents": [] }))
        .await;
    refused(
        &fixture
            .choose(&chat, json!({ "provider": "openai", "model": FAST }))
            .await,
        PROVIDER_DENIED,
        "choosing a denied provider",
    );
    refused(
        &fixture
            .choose(&chat, json!({ "provider": "claude_code" }))
            .await,
        AGENT_DENIED,
        "choosing a denied agent, signed out or not",
    );
    refused(
        &fixture.create_chat(FAST).await,
        PROVIDER_DENIED,
        "creating a chat on a workspace whose provider is denied",
    );
    refused(
        &fixture.plan("auto").await,
        PROVIDER_DENIED,
        "planning on a workspace whose provider is denied",
    );
    assert_eq!(fixture.chats().await, before);

    let audits = fixture.audits().await;
    let denials: Vec<(&str, Value)> = audits.iter().map(denial).collect();
    assert_eq!(
        denials,
        [
            (CHAT, json!({ "code": MODEL_DENIED, "detail": REASONING })),
            (
                WORKSPACE,
                json!({ "code": MODEL_DENIED, "detail": REASONING })
            ),
            (CHAT, json!({ "code": MODEL_DENIED, "detail": REASONING })),
            (
                WORKSPACE,
                json!({ "code": MODEL_DENIED, "detail": REASONING })
            ),
            (CHAT, json!({ "code": PROVIDER_DENIED, "detail": "openai" })),
            (
                CHAT,
                json!({ "code": AGENT_DENIED, "detail": "claude_code" })
            ),
            (
                WORKSPACE,
                json!({ "code": PROVIDER_DENIED, "detail": FAST })
            ),
            (
                WORKSPACE,
                json!({ "code": PROVIDER_DENIED, "detail": "auto" })
            ),
        ],
        "every refusal is audited as a denied turn"
    );
    for audit in &audits {
        assert_eq!(audit.actor_id, Some(fixture.owner.id));
        assert_eq!(audit.organization_id, Some(fixture.organization));
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_task_on_a_denied_model_keeps_its_model_and_starts_no_run() {
    let fixture = Fixture::new().await;
    fixture.license().await;
    fixture.save_openai().await;
    fixture.restrict(json!({ "models": [FAST] })).await;
    let task = fixture.task().await;

    refused(
        &fixture.update_task(&task, REASONING).await,
        MODEL_DENIED,
        "giving a task a denied model",
    );
    assert_eq!(fixture.task_model(&task).await, Value::Null);
    accepted(
        &fixture.update_task(&task, FAST).await,
        StatusCode::OK,
        "giving a task an allowed model",
    );
    assert_eq!(fixture.task_model(&task).await, json!(FAST));

    fixture.store_task_model(&task, Some(REASONING)).await;
    refused(
        &fixture.run(&task).await,
        MODEL_DENIED,
        "running a task on a denied model",
    );
    fixture.store_task_model(&task, None).await;
    fixture
        .restrict(json!({ "providers": ["anthropic"] }))
        .await;
    refused(
        &fixture.run(&task).await,
        PROVIDER_DENIED,
        "running a task on a workspace whose provider is denied",
    );
    assert_eq!(fixture.runs(&task).await, 0, "no refused run started");

    let audits = fixture.audits().await;
    let task_id: Uuid = task.parse().unwrap();
    assert_eq!(audits.len(), 3, "{audits:?}");
    for audit in &audits {
        assert_eq!(audit.resource_type, TASK);
        assert_eq!(audit.resource_id, Some(task_id));
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_viewer_is_refused_a_task_change_or_run_before_the_policy_is_asked() {
    let fixture = Fixture::new().await;
    fixture.license().await;
    fixture.save_openai().await;
    fixture.restrict(json!({ "models": [FAST] })).await;
    let task = fixture.task().await;
    fixture.store_task_model(&task, Some(REASONING)).await;
    let viewer = fixture.seated(OrgRole::Member, WorkspaceRole::Viewer).await;

    for (response, what) in [
        (
            fixture
                .update_task_as(&task, REASONING, &viewer.token)
                .await,
            "a viewer giving a task a denied model",
        ),
        (
            fixture.run_as(&task, &viewer.token).await,
            "a viewer running a task on a denied model",
        ),
    ] {
        assert_eq!(
            response.status,
            StatusCode::NOT_FOUND,
            "{what}: {}",
            response.text()
        );
        assert_eq!(
            response.json_value().get("code"),
            None,
            "{what} carries no policy code: {}",
            response.text()
        );
    }
    assert_eq!(fixture.runs(&task).await, 0);
    let audits = fixture.audits().await;
    assert!(audits.is_empty(), "nothing is audited: {audits:?}");
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_workspace_models_mark_what_the_policy_denies_on_either_endpoint() {
    let fixture = Fixture::new().await;
    fixture.license().await;
    fixture
        .restrict(json!({ "models": ["qwen3*", FAST] }))
        .await;

    let installed = fixture.models().await;
    accepted(&installed, StatusCode::OK, "the instance's models");
    let installed = installed.json_value();
    assert!(
        listed(&installed, INSTALLED).get("denial").is_none(),
        "{installed}"
    );
    assert_eq!(listed(&installed, UNLISTED)["denial"], json!(MODEL_DENIED));

    fixture.restrict(json!({})).await;
    fixture.save_openai().await;
    fixture
        .restrict(json!({ "models": ["qwen3*", FAST] }))
        .await;
    let saved = fixture.models().await;
    accepted(&saved, StatusCode::OK, "the saved endpoint's models");
    let saved = saved.json_value();
    assert!(listed(&saved, FAST).get("denial").is_none(), "{saved}");
    assert_eq!(listed(&saved, REASONING)["denial"], json!(MODEL_DENIED));

    fixture
        .restrict(json!({ "providers": ["anthropic"] }))
        .await;
    let denied = fixture.models().await;
    accepted(&denied, StatusCode::OK, "a denied provider's models");
    let denied = denied.json_value();
    for model in [FAST, REASONING] {
        assert_eq!(
            listed(&denied, model)["denial"],
            json!(PROVIDER_DENIED),
            "{denied}"
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_sign_in_to_an_agent_the_organization_denies_is_refused_and_audited() {
    let fixture = Fixture::new().await;
    fixture.license().await;
    fixture.restrict(json!({ "agents": ["codex"] })).await;
    fixture.restrict_workspace(json!({ "agents": [] })).await;
    let member = fixture.member(OrgRole::Member).await;

    refused(
        &fixture.sign_in("claude", fixture.token()).await,
        AGENT_DENIED,
        "signing in to a denied agent",
    );
    refused(
        &fixture
            .client
            .post_json_auth(
                &format!("{}/claude/login/code", fixture.agents_path()),
                &json!({ "code": "code#state" }),
                fixture.token(),
            )
            .await,
        AGENT_DENIED,
        "finishing a sign-in to a denied agent",
    );
    let outsider = register(&fixture.client).await;
    let redeemed = fixture
        .client
        .post_json_auth(
            &format!("{}/claude/login/receipt", fixture.agents_path()),
            &json!({ "receipt": "receipt" }),
            &outsider.token,
        )
        .await;
    assert_eq!(
        redeemed.status,
        StatusCode::NOT_FOUND,
        "someone outside the organization learns nothing of its policy: {}",
        redeemed.text()
    );
    let forbidden = fixture.sign_in("claude", &member.token).await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert!(
        forbidden.json_value().get("code").is_none(),
        "a member who may not sign in is told so before the policy: {}",
        forbidden.text()
    );

    let agents = fixture.agents().await;
    assert_eq!(status(&agents, "claude")["denial"], json!(AGENT_DENIED));
    assert!(
        status(&agents, "codex").get("denial").is_none(),
        "the workspace's own policy never denies a sign-in: {agents}"
    );
    let claude = fixture
        .client
        .get_auth(
            &format!("{}/claude", fixture.agents_path()),
            fixture.token(),
        )
        .await;
    assert_eq!(claude.json_value()["denial"], json!(AGENT_DENIED));

    let audits = fixture.audits().await;
    assert_eq!(audits.len(), 2, "{audits:?}");
    for audit in &audits {
        assert_eq!(
            denial(audit),
            (
                ORGANIZATION,
                json!({ "code": AGENT_DENIED, "detail": "claude" })
            )
        );
        assert_eq!(audit.resource_id, Some(fixture.organization));
        assert_eq!(audit.actor_id, Some(fixture.owner.id));
    }

    fixture.restrict(json!({ "agents": ["claude"] })).await;
    accepted(
        &fixture.sign_in("claude", fixture.token()).await,
        StatusCode::OK,
        "signing in to an allowed agent",
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn ai_settings_naming_a_denied_provider_agent_or_model_are_not_saved() {
    let fixture = Fixture::new().await;
    fixture.license().await;
    fixture
        .restrict(json!({
            "providers": ["anthropic"],
            "agents": ["codex"],
            "models": ["claude-*", "gpt-6-*"],
        }))
        .await;
    let member = fixture.member(OrgRole::Member).await;

    for (settings, code) in [
        (json!({ "provider": "openai" }), PROVIDER_DENIED),
        (json!({ "provider": "claude_code" }), AGENT_DENIED),
        (json!({ "model_fast": REASONING }), MODEL_DENIED),
    ] {
        refused(
            &fixture
                .organization_settings(fixture.token(), settings.clone())
                .await,
            code,
            &format!("saving {settings} for the organization"),
        );
    }
    let forbidden = fixture
        .organization_settings(&member.token, json!({ "provider": "openai" }))
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert!(
        forbidden.json_value().get("code").is_none(),
        "a member who may not save is told so before the policy: {}",
        forbidden.text()
    );
    accepted(
        &fixture
            .organization_settings(
                fixture.token(),
                json!({ "model_fast": "claude-sonnet-4-5" }),
            )
            .await,
        StatusCode::OK,
        "saving an allowed model",
    );

    fixture
        .restrict_workspace(json!({ "models": ["claude-sonnet-*"] }))
        .await;
    refused(
        &fixture
            .workspace_settings(json!({ "model_reasoning": "claude-opus-4" }))
            .await,
        MODEL_DENIED,
        "saving a model the workspace's layer denies",
    );
    accepted(
        &fixture
            .organization_settings(
                fixture.token(),
                json!({ "model_reasoning": "claude-opus-4" }),
            )
            .await,
        StatusCode::OK,
        "the organization's settings answer to its own layer alone",
    );

    let audits = fixture.audits().await;
    let denials: Vec<(&str, Value)> = audits.iter().map(denial).collect();
    assert_eq!(
        denials,
        [
            (
                ORGANIZATION,
                json!({ "code": PROVIDER_DENIED, "detail": "openai" })
            ),
            (
                ORGANIZATION,
                json!({ "code": AGENT_DENIED, "detail": "claude_code" })
            ),
            (
                ORGANIZATION,
                json!({ "code": MODEL_DENIED, "detail": REASONING })
            ),
            (
                WORKSPACE,
                json!({ "code": MODEL_DENIED, "detail": "claude-opus-4" })
            ),
        ]
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn community_enforces_nothing_and_a_lapsed_license_still_refuses() {
    let fixture = Fixture::new().await;
    fixture.save_openai().await;
    fixture
        .restrict(json!({ "providers": [], "agents": [], "models": [] }))
        .await;

    let chat = fixture.chat(REASONING).await;
    accepted(
        &fixture.change_model(&chat, FAST).await,
        StatusCode::OK,
        "Community changes a chat's model",
    );
    let default = &fixture.workspace_offers().await["default"];
    assert_eq!(default["usable"], json!(true), "{default}");
    assert!(default.get("denial").is_none(), "{default}");
    let models = fixture.models().await.json_value();
    assert!(
        listed(&models, REASONING).get("denial").is_none(),
        "{models}"
    );
    let task = fixture.task().await;
    accepted(
        &fixture.update_task(&task, REASONING).await,
        StatusCode::OK,
        "Community gives a task any model",
    );
    accepted(
        &fixture
            .organization_settings(fixture.token(), json!({ "model_fast": REASONING }))
            .await,
        StatusCode::OK,
        "Community saves any model",
    );
    assert!(
        status(&fixture.agents().await, "claude")
            .get("denial")
            .is_none()
    );
    accepted(
        &fixture.sign_in("claude", fixture.token()).await,
        StatusCode::OK,
        "Community signs in to any agent",
    );
    assert!(fixture.audits().await.is_empty());

    fixture.lapse().await;
    refused(
        &fixture.sign_in("claude", fixture.token()).await,
        AGENT_DENIED,
        "a lapsed license still refuses a sign-in",
    );
    refused(
        &fixture
            .organization_settings(fixture.token(), json!({ "model_fast": FAST }))
            .await,
        MODEL_DENIED,
        "a lapsed license still refuses a save",
    );
    refused(
        &fixture.change_model(&chat, FAST).await,
        PROVIDER_DENIED,
        "a lapsed license still refuses a chat's model",
    );
    assert_eq!(
        status(&fixture.agents().await, "claude")["denial"],
        json!(AGENT_DENIED)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_policy_that_cannot_be_read_refuses_everything_it_governs() {
    let fixture = Fixture::new().await;
    fixture.license().await;
    fixture.save_openai().await;
    let chat = fixture.chat(FAST).await;
    fixture.restrict(json!({ "agents": "claude" })).await;

    refused(
        &fixture.change_model(&chat, FAST).await,
        UNREADABLE,
        "a chat's model under an unreadable policy",
    );
    refused(
        &fixture.sign_in("codex", fixture.token()).await,
        UNREADABLE,
        "a sign-in under an unreadable policy",
    );
    let agents = fixture.agents().await;
    for agent in ["claude", "codex"] {
        assert_eq!(status(&agents, agent)["denial"], json!(UNREADABLE));
    }
    let default = &fixture.workspace_offers().await["default"];
    assert_eq!(default["denial"], json!(UNREADABLE), "{default}");
    let models = fixture.models().await;
    accepted(
        &models,
        StatusCode::OK,
        "the models under an unreadable policy",
    );
    let models = models.json_value();
    assert_eq!(
        listed(&models, FAST)["denial"],
        json!(UNREADABLE),
        "{models}"
    );
    fixture.cleanup().await;
}
