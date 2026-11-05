//! A chat member chooses which AI provider, and optionally which model, one
//! chat runs on. No choice keeps the chat on its workspace's AI settings, a
//! choice that stops being usable fails in the chat instead of falling back,
//! and coding agents offer their own models and reasoning efforts.

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::llm::AgentKind;
use zone_core::llm::provider::Occupancy;
use zone_server::config::{AgentConfig, Config};
use zone_server::db::agent_logins::{self, Insert};
use zone_server::db::chats::occupancy::{self, Reported};
use zone_server::services::model::UNSUPPORTED;

use crate::common::{TestClient, TestResponse, agents, test_config, test_email, test_password};

const CODEX_CREDENTIALS_FILE: &str = "auth.json";
const CODEX_CREDENTIALS: &str = r#"{"tokens":{"access_token":"fake-codex-access-token","refresh_token":"fake-codex-refresh-token","account_id":"fake-codex-account"}}"#;
const OPENAI_KEY: &str = "sk-chat-provider-the-tenants-own-openai-key";
const METADATA_URL: &str = "http://169.254.169.254/v1";
const HTTP_EFFORTS: [&str; 4] = ["off", "low", "medium", "high"];
const CLAUDE_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const CODEX_EFFORTS: [&str; 4] = ["low", "medium", "high", "xhigh"];
const CODEX_ULTRA_EFFORTS: [&str; 6] = ["low", "medium", "high", "xhigh", "max", "ultra"];
const CODEX_MAX_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const OFFER_FIELDS: [&str; 7] = [
    "provider",
    "label",
    "usable",
    "reason",
    "models",
    "efforts",
    "requires_model",
];
const LABELS: [(&str, &str); 6] = [
    ("self_hosted", "Self-Hosted"),
    ("openai", "OpenAI"),
    ("anthropic", "Anthropic"),
    ("bedrock", "AWS Bedrock"),
    ("claude_code", "Claude Code"),
    ("codex", "Codex"),
];

struct Account {
    token: String,
    user: String,
}

async fn account(client: &TestClient) -> Account {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    let body = response.json_value();
    Account {
        token: body["access_token"]
            .as_str()
            .expect("registration returns an access token")
            .to_string(),
        user: body["user"]["id"]
            .as_str()
            .expect("registration returns the user")
            .to_string(),
    }
}

async fn organization(client: &TestClient, token: &str) -> String {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Tenant", "slug": uuid::Uuid::new_v4().to_string() }),
            token,
        )
        .await;
    response.json_value()["organization"]["id"]
        .as_str()
        .expect("organization is created")
        .to_string()
}

async fn workspace(client: &TestClient, token: &str, organization: &str) -> String {
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Tenant workspace", "slug": uuid::Uuid::new_v4().to_string() }),
            token,
        )
        .await;
    response.json_value()["workspace"]["id"]
        .as_str()
        .expect("workspace is created")
        .to_string()
}

/// Seat `user` in the organization and then in the workspace, because the
/// workspace settings check organization membership before workspace role.
async fn seat(
    client: &TestClient,
    token: &str,
    organization: &str,
    workspace: &str,
    user: &str,
    workspace_role: &str,
) {
    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": user, "role": "member" }),
            token,
        )
        .await
        .assert_status(StatusCode::CREATED);
    client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/members"),
            &json!({ "user_id": user, "role": workspace_role }),
            token,
        )
        .await
        .assert_status(StatusCode::CREATED);
}

struct Tenant {
    client: TestClient,
    owner: Account,
    organization: String,
    workspace: String,
}

impl Tenant {
    async fn new() -> Self {
        Self::on(test_config()).await
    }

    /// A tenant whose instance asks the Ollama at `host` what its models do.
    async fn on_ollama(host: &str) -> Self {
        Self::on(Config {
            ollama_host: host.to_string(),
            ..test_config()
        })
        .await
    }

    async fn on(config: Config) -> Self {
        let client = TestClient::with_config(Config {
            agents: AgentConfig {
                host_login: false,
                ..agents()
            },
            ..config
        })
        .await;
        let owner = account(&client).await;
        let organization = organization(&client, &owner.token).await;
        let workspace = workspace(&client, &owner.token, &organization).await;
        Self {
            client,
            owner,
            organization,
            workspace,
        }
    }

    async fn seated(&self, workspace_role: &str) -> Account {
        let person = account(&self.client).await;
        seat(
            &self.client,
            &self.owner.token,
            &self.organization,
            &self.workspace,
            &person.user,
            workspace_role,
        )
        .await;
        person
    }

    async fn save_organization_settings(&self, settings: Value) {
        let saved = self
            .client
            .put_json_auth(
                &format!("/api/organizations/{}/settings/ai", self.organization),
                &settings,
                &self.owner.token,
            )
            .await;
        assert_eq!(
            saved.status,
            StatusCode::OK,
            "the organization's AI settings were not saved: {}",
            saved.text()
        );
    }

    /// Signs the organization in to Codex with a login holding its own `auth.json`.
    async fn sign_in_codex(&self) {
        let organization: Uuid = self.organization.parse().expect("an organization id");
        let state = self.client.state();
        let login = agent_logins::insert(
            state.db(),
            &Insert {
                organization_id: organization,
                agent: AgentKind::Codex.as_str(),
                account: None,
                credential: None,
                label: None,
                expires_at: None,
            },
        )
        .await
        .expect("a stored login")
        .id;
        let home = state
            .config()
            .agents
            .create_login_home(organization, AgentKind::Codex, login)
            .expect("the login's home");
        std::fs::write(home.join(CODEX_CREDENTIALS_FILE), CODEX_CREDENTIALS)
            .expect("the login's credentials");
    }

    async fn refuse_the_saved_openai_url(&self) {
        let organization: Uuid = self.organization.parse().expect("an organization id");
        let updated = sqlx::query(
            "UPDATE organization_ai_settings SET openai_base_url = $2 WHERE organization_id = $1",
        )
        .bind(organization)
        .bind(METADATA_URL)
        .execute(self.client.state().db())
        .await
        .expect("the saved URL is rewritten");
        assert_eq!(
            updated.rows_affected(),
            1,
            "the organization has saved AI settings"
        );
    }

    async fn create_chat(&self, token: &str, extra: Value) -> TestResponse {
        let mut body = json!({
            "workspace_id": self.workspace,
            "title": "Provider chat",
            "model_name": "gpt-4",
        });
        if let (Some(body), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
            body.extend(extra.clone());
        }
        self.client.post_json_auth("/api/chats", &body, token).await
    }

    async fn chat(&self, token: &str) -> String {
        let created = self.create_chat(token, json!({})).await;
        let body = expect(&created, StatusCode::CREATED, "a Default chat is created");
        body["chat"]["id"]
            .as_str()
            .expect("the created chat's id")
            .to_string()
    }

    async fn chats(&self, token: &str) -> Vec<Value> {
        let listed = self
            .client
            .get_auth(
                &format!("/api/chats?workspace_id={}", self.workspace),
                token,
            )
            .await;
        expect(&listed, StatusCode::OK, "the workspace's chats are listed")["chats"]
            .as_array()
            .expect("a list of chats")
            .clone()
    }

    async fn get_chat(&self, token: &str, chat: &str) -> TestResponse {
        self.client
            .get_auth(&format!("/api/chats/{chat}"), token)
            .await
    }

    async fn providers(&self, token: &str, chat: &str) -> TestResponse {
        self.client
            .get_auth(&format!("/api/chats/{chat}/providers"), token)
            .await
    }

    async fn offers(&self, token: &str, chat: &str) -> Value {
        let listed = self.providers(token, chat).await;
        expect(&listed, StatusCode::OK, "the chat's providers are listed")
    }

    async fn workspace_providers(&self, token: &str) -> TestResponse {
        self.client
            .get_auth(
                &format!("/api/workspaces/{}/chat-providers", self.workspace),
                token,
            )
            .await
    }

    async fn workspace_offers(&self) -> Value {
        let listed = self.workspace_providers(&self.owner.token).await;
        expect(
            &listed,
            StatusCode::OK,
            "the workspace's chat providers are listed",
        )
    }

    async fn choose(&self, token: &str, chat: &str, choice: Value) -> TestResponse {
        self.client
            .put_json_auth(&format!("/api/chats/{chat}/provider"), &choice, token)
            .await
    }

    async fn context(&self, token: &str, chat: &str) -> TestResponse {
        self.client
            .post_json_auth(
                &format!("/api/chats/{chat}/context"),
                &json!({ "content": "What would this turn cost?" }),
                token,
            )
            .await
    }

    async fn update_chat(&self, token: &str, chat: &str, update: Value) -> TestResponse {
        self.client
            .put_json_auth(&format!("/api/chats/{chat}"), &update, token)
            .await
    }
}

fn expect(response: &TestResponse, status: StatusCode, what: &str) -> Value {
    assert_eq!(
        response.status,
        status,
        "{what}: expected {status}, got {}: {}",
        response.status,
        response.text()
    );
    response.json_value()
}

fn assert_refused(response: &TestResponse, status: StatusCode, code: &str, what: &str) {
    let body = expect(response, status, what);
    assert_eq!(body["code"], json!(code), "{what}: {body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty()),
        "{what}: the refusal says why: {body}"
    );
}

fn names(offers: &Value) -> Vec<String> {
    offers
        .as_array()
        .expect("a list of offers")
        .iter()
        .map(|offer| {
            offer["provider"]
                .as_str()
                .expect("every offer names its provider")
                .to_string()
        })
        .collect()
}

fn offer<'a>(offers: &'a Value, provider: &str) -> &'a Value {
    offers
        .as_array()
        .expect("a list of offers")
        .iter()
        .find(|offer| offer["provider"] == provider)
        .unwrap_or_else(|| panic!("{provider} is offered: {offers}"))
}

fn efforts(efforts: &[&str]) -> Value {
    json!(efforts)
}

fn label(provider: &str) -> &'static str {
    LABELS
        .iter()
        .find(|(name, _)| *name == provider)
        .map(|(_, label)| *label)
        .unwrap_or_else(|| panic!("{provider} is a provider"))
}

fn assert_offer_shape(offer: &Value) {
    let fields = offer.as_object().expect("an offer is an object");
    for field in fields.keys() {
        assert!(
            OFFER_FIELDS.contains(&field.as_str()) || field == "model",
            "an offer carries only what the contract names, not {field}: {offer}"
        );
    }
    let provider = offer["provider"].as_str().expect("an offer's provider");
    assert_eq!(
        offer["label"],
        json!(label(provider)),
        "{provider} is offered under its label"
    );
    assert!(
        offer["usable"].is_boolean(),
        "an offer says whether it is usable: {offer}"
    );
    assert!(
        offer["requires_model"].is_boolean(),
        "an offer says whether it needs a model: {offer}"
    );
    assert!(
        offer["models"].is_array() && offer["efforts"].is_array(),
        "an offer lists its models and efforts: {offer}"
    );
    if offer["usable"] == json!(true) {
        assert!(
            offer["reason"].is_null(),
            "a usable offer has no reason against it: {offer}"
        );
    }
}

fn assert_shapes(listing: &Value) {
    assert_offer_shape(&listing["default"]);
    for offer in listing["providers"]
        .as_array()
        .expect("a list of providers")
    {
        assert_offer_shape(offer);
        assert_eq!(
            offer["usable"],
            json!(true),
            "only usable providers are listed: {offer}"
        );
    }
    if !listing["selected"].is_null() {
        assert_offer_shape(&listing["selected"]);
    }
}

fn assert_hides_the_key(response: &TestResponse, what: &str) {
    assert!(
        !response.text().contains(OPENAI_KEY),
        "{what} carries the saved key: {}",
        response.text()
    );
}

fn signed_out(label: &str) -> String {
    format!(
        "{label} is not signed in for this organization. An organization admin can sign in under Organization Settings > AI Settings."
    )
}

#[tokio::test]
async fn a_new_chat_follows_the_default_and_the_default_follows_the_organization() {
    let tenant = Tenant::new().await;
    let chat = tenant.chat(&tenant.owner.token).await;

    let read = tenant.get_chat(&tenant.owner.token, &chat).await;
    let read = expect(&read, StatusCode::OK, "the chat is read");
    assert!(
        read["chat"]["provider"].is_null() && read["chat"]["provider_model"].is_null(),
        "a new chat chose no provider: {read}"
    );

    let listed = tenant.offers(&tenant.owner.token, &chat).await;
    assert_shapes(&listed);
    assert_eq!(
        listed["selected"],
        Value::Null,
        "a new chat follows the default"
    );
    assert_eq!(listed["default"]["provider"], json!("self_hosted"));
    assert_eq!(listed["default"]["usable"], json!(true));
    assert_eq!(listed["default"]["efforts"], efforts(&HTTP_EFFORTS));

    tenant
        .save_organization_settings(json!({
            "provider": "openai",
            "openai_api_key": OPENAI_KEY,
            "model_fast": "gpt-4o-mini",
            "model_reasoning": "gpt-4o",
        }))
        .await;

    let listed = tenant.offers(&tenant.owner.token, &chat).await;
    assert_shapes(&listed);
    assert_eq!(
        listed["selected"],
        Value::Null,
        "the chat still follows the default"
    );
    let default = &listed["default"];
    assert_eq!(
        default["provider"],
        json!("openai"),
        "the default follows the organization's change: {listed}"
    );
    assert_eq!(default["usable"], json!(true));
    assert_eq!(default["requires_model"], json!(false));
    assert_eq!(default["efforts"], efforts(&HTTP_EFFORTS));
    let models = default["models"].as_array().expect("the default's models");
    let model_names: Vec<&str> = models
        .iter()
        .map(|model| model["name"].as_str().expect("a model name"))
        .collect();
    assert_eq!(
        model_names,
        ["gpt-4o-mini", "gpt-4o"],
        "the saved Fast and Reasoning models are offered"
    );
    for model in models {
        assert_eq!(
            model["efforts"],
            efforts(&HTTP_EFFORTS),
            "an endpoint model offers the HTTP efforts: {model}"
        );
    }
}

#[tokio::test]
async fn only_usable_choosable_providers_are_listed_and_no_key_is_returned() {
    let tenant = Tenant::new().await;
    let member = tenant.seated("member").await;
    tenant
        .save_organization_settings(json!({
            "openai_api_key": OPENAI_KEY,
            "bedrock_region": "us-east-1",
            "bedrock_use_iam_role": true,
        }))
        .await;

    let listed = tenant.workspace_providers(&member.token).await;
    assert_hides_the_key(&listed, "the workspace's chat providers");
    let listed = expect(
        &listed,
        StatusCode::OK,
        "a member lists the workspace's chat providers",
    );
    assert_shapes(&listed);
    assert_eq!(
        names(&listed["providers"]),
        ["self_hosted", "openai"],
        "Anthropic has nothing saved, the agents are signed out and Bedrock can't be chosen: {listed}"
    );
    let openai = offer(&listed["providers"], "openai");
    assert_eq!(
        openai["requires_model"],
        json!(true),
        "OpenAI saved no model of its own: {openai}"
    );
    assert_eq!(openai["models"], json!([]));
    assert_eq!(openai["efforts"], efforts(&HTTP_EFFORTS));
    assert_eq!(
        offer(&listed["providers"], "self_hosted")["requires_model"],
        json!(false),
        "the instance's LiteLLM runs without a chosen model"
    );

    let chat = tenant.chat(&member.token).await;
    let listed = tenant.providers(&member.token, &chat).await;
    assert_hides_the_key(&listed, "the chat's providers");
    let listed = expect(
        &listed,
        StatusCode::OK,
        "a member lists the chat's providers",
    );
    assert_shapes(&listed);
    assert_eq!(names(&listed["providers"]), ["self_hosted", "openai"]);

    let chosen = tenant
        .choose(
            &member.token,
            &chat,
            json!({ "provider": "openai", "model": "gpt-4o" }),
        )
        .await;
    assert_hides_the_key(&chosen, "the chosen chat");
    expect(&chosen, StatusCode::OK, "a member chooses OpenAI");

    let listed = tenant.providers(&member.token, &chat).await;
    assert_hides_the_key(&listed, "the chat's providers with a choice");
    assert_shapes(&expect(&listed, StatusCode::OK, "the choice is listed"));
    let read = tenant.get_chat(&member.token, &chat).await;
    assert_hides_the_key(&read, "the chosen chat");
    let chats = tenant
        .client
        .get_auth(
            &format!("/api/chats?workspace_id={}", tenant.workspace),
            &member.token,
        )
        .await;
    assert_hides_the_key(&chats, "the chat list");
}

#[tokio::test]
async fn a_member_chooses_and_clears_a_provider_but_cannot_change_ai_settings() {
    let tenant = Tenant::new().await;
    let member = tenant.seated("member").await;
    tenant
        .save_organization_settings(json!({ "openai_api_key": OPENAI_KEY }))
        .await;
    let chat = tenant.chat(&member.token).await;

    let chosen = tenant
        .choose(
            &member.token,
            &chat,
            json!({ "provider": "openai", "model": "gpt-4o" }),
        )
        .await;
    let chosen = expect(&chosen, StatusCode::OK, "a member chooses OpenAI");
    assert_eq!(chosen["chat"]["provider"], json!("openai"), "{chosen}");
    assert_eq!(
        chosen["chat"]["provider_model"],
        json!("gpt-4o"),
        "{chosen}"
    );

    let read = tenant.get_chat(&tenant.owner.token, &chat).await;
    let read = expect(&read, StatusCode::OK, "the owner reads the chat");
    assert_eq!(read["chat"]["provider"], json!("openai"));
    assert_eq!(read["chat"]["provider_model"], json!("gpt-4o"));

    let listed = tenant.offers(&member.token, &chat).await;
    assert_shapes(&listed);
    let selected = &listed["selected"];
    assert_eq!(selected["provider"], json!("openai"), "{listed}");
    assert_eq!(selected["model"], json!("gpt-4o"), "{listed}");
    assert_eq!(selected["usable"], json!(true), "{listed}");

    let cleared = tenant
        .choose(
            &member.token,
            &chat,
            json!({ "provider": null, "model": null }),
        )
        .await;
    let cleared = expect(&cleared, StatusCode::OK, "a member clears the choice");
    assert!(
        cleared["chat"]["provider"].is_null() && cleared["chat"]["provider_model"].is_null(),
        "the chat follows the default again: {cleared}"
    );
    assert_eq!(
        tenant.offers(&member.token, &chat).await["selected"],
        Value::Null,
        "nothing is selected once the choice is cleared"
    );

    let missing = tenant
        .choose(
            &member.token,
            &Uuid::new_v4().to_string(),
            json!({ "provider": "openai", "model": "gpt-4o" }),
        )
        .await;
    assert_eq!(
        missing.status,
        StatusCode::NOT_FOUND,
        "a chat that does not exist: {}",
        missing.text()
    );

    let seized = tenant
        .client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                tenant.organization, tenant.workspace
            ),
            &json!({ "provider": "openai" }),
            &member.token,
        )
        .await;
    assert_eq!(
        seized.status,
        StatusCode::FORBIDDEN,
        "choosing for one chat is not a licence to change the workspace's AI settings: {}",
        seized.text()
    );
}

#[tokio::test]
async fn the_listing_says_whether_the_chats_turns_run_on_a_cli() {
    let tenant = Tenant::new().await;
    tenant.sign_in_codex().await;
    let chat = tenant.chat(&tenant.owner.token).await;

    assert_eq!(
        tenant.offers(&tenant.owner.token, &chat).await["cli"],
        json!(false),
        "a chat on the self-hosted default runs over HTTP"
    );

    tenant
        .save_organization_settings(json!({ "provider": "claude_code" }))
        .await;
    assert_eq!(
        tenant.offers(&tenant.owner.token, &chat).await["cli"],
        json!(true),
        "a chat following an agent default starts on that agent"
    );

    let chosen = tenant
        .choose(
            &tenant.owner.token,
            &chat,
            json!({ "provider": "codex", "model": null }),
        )
        .await;
    expect(&chosen, StatusCode::OK, "the chat chooses Codex");
    assert_eq!(
        tenant.offers(&tenant.owner.token, &chat).await["cli"],
        json!(true),
        "a chat's choice comes before its default"
    );

    let chosen = tenant
        .choose(
            &tenant.owner.token,
            &chat,
            json!({ "provider": "self_hosted", "model": null }),
        )
        .await;
    expect(&chosen, StatusCode::OK, "the chat chooses Self-Hosted");
    assert_eq!(
        tenant.offers(&tenant.owner.token, &chat).await["cli"],
        json!(false),
        "a chat that chose an HTTP provider runs over HTTP under an agent default"
    );
}

#[tokio::test]
async fn a_viewer_reads_the_providers_but_cannot_choose_one() {
    let tenant = Tenant::new().await;
    let viewer = tenant.seated("viewer").await;
    let chat = tenant.chat(&tenant.owner.token).await;

    let listed = tenant.providers(&viewer.token, &chat).await;
    let listed = expect(
        &listed,
        StatusCode::OK,
        "a viewer lists the chat's providers",
    );
    assert_shapes(&listed);
    let listed = tenant.workspace_providers(&viewer.token).await;
    expect(
        &listed,
        StatusCode::OK,
        "a viewer lists the workspace's chat providers",
    );

    let refused = tenant
        .choose(
            &viewer.token,
            &chat,
            json!({ "provider": "self_hosted", "model": null }),
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a viewer chose the chat's provider: {}",
        refused.text()
    );
    assert_eq!(
        tenant.offers(&tenant.owner.token, &chat).await["selected"],
        Value::Null,
        "the viewer's refused choice left nothing behind"
    );
}

#[tokio::test]
async fn an_unconfigured_provider_is_unavailable() {
    let tenant = Tenant::new().await;
    let chat = tenant.chat(&tenant.owner.token).await;

    let refused = tenant
        .choose(
            &tenant.owner.token,
            &chat,
            json!({ "provider": "anthropic", "model": "claude-sonnet-4-5" }),
        )
        .await;
    assert_refused(
        &refused,
        StatusCode::CONFLICT,
        "provider_unavailable",
        "Anthropic has no key, endpoint or credentials saved",
    );

    tenant
        .save_organization_settings(json!({ "provider": "anthropic" }))
        .await;
    let listed = tenant.offers(&tenant.owner.token, &chat).await;
    assert_shapes(&listed);
    let default = &listed["default"];
    assert_eq!(default["provider"], json!("anthropic"), "{listed}");
    assert_eq!(
        default["usable"],
        json!(false),
        "an unusable default is still reported"
    );
    assert_eq!(
        default["reason"],
        json!("No key, endpoint or credentials are saved for Anthropic in AI Settings."),
        "{listed}"
    );
    assert!(
        !names(&listed["providers"]).contains(&"anthropic".to_string()),
        "an unusable provider is not offered: {listed}"
    );
}

#[tokio::test]
async fn a_signed_out_agent_is_unavailable() {
    let tenant = Tenant::new().await;
    let chat = tenant.chat(&tenant.owner.token).await;

    for (provider, label) in [("claude_code", "Claude Code"), ("codex", "Codex")] {
        let refused = tenant
            .choose(
                &tenant.owner.token,
                &chat,
                json!({ "provider": provider, "model": null }),
            )
            .await;
        assert_refused(
            &refused,
            StatusCode::CONFLICT,
            "provider_unavailable",
            &format!("{label} has no sign-in for this organization"),
        );
    }
    assert_eq!(
        tenant.offers(&tenant.owner.token, &chat).await["selected"],
        Value::Null,
        "a refused agent leaves the chat on the default"
    );
}

#[tokio::test]
async fn bedrock_cannot_be_chosen_for_a_chat() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({
            "provider": "bedrock",
            "bedrock_region": "us-east-1",
            "bedrock_use_iam_role": true,
        }))
        .await;
    let chat = tenant.chat(&tenant.owner.token).await;

    let refused = tenant
        .choose(
            &tenant.owner.token,
            &chat,
            json!({ "provider": "bedrock", "model": null }),
        )
        .await;
    assert_refused(
        &refused,
        StatusCode::CONFLICT,
        "provider_unavailable",
        "Bedrock runs on the instance like Self-Hosted",
    );

    let listed = tenant.offers(&tenant.owner.token, &chat).await;
    assert_shapes(&listed);
    assert_eq!(
        listed["default"]["provider"],
        json!("bedrock"),
        "a Bedrock default is still reported: {listed}"
    );
    assert_eq!(
        listed["default"]["reason"],
        json!(
            "AWS Bedrock runs on this instance like Self-Hosted and can't be chosen for a single chat."
        ),
        "{listed}"
    );
    assert!(
        !names(&listed["providers"]).contains(&"bedrock".to_string()),
        "Bedrock is never offered: {listed}"
    );
}

#[tokio::test]
async fn an_endpoint_provider_without_a_saved_model_requires_one() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({ "openai_api_key": OPENAI_KEY }))
        .await;
    let chat = tenant.chat(&tenant.owner.token).await;

    for choice in [
        json!({ "provider": "openai", "model": null }),
        json!({ "provider": "openai" }),
    ] {
        let refused = tenant.choose(&tenant.owner.token, &chat, choice).await;
        assert_refused(
            &refused,
            StatusCode::BAD_REQUEST,
            "provider_model_required",
            "OpenAI saved no model, so the chat must name one",
        );
    }
    assert_eq!(
        tenant.offers(&tenant.owner.token, &chat).await["selected"],
        Value::Null,
        "a refused choice leaves the chat on the default"
    );
}

#[tokio::test]
async fn an_unknown_provider_and_a_blank_or_orphaned_model_are_refused() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({ "openai_api_key": OPENAI_KEY }))
        .await;
    let chat = tenant.chat(&tenant.owner.token).await;

    for (choice, status, code, what) in [
        (
            json!({ "provider": "gemini", "model": null }),
            StatusCode::BAD_REQUEST,
            "provider_unknown",
            "a provider Zone does not know",
        ),
        (
            json!({ "provider": "openai", "model": "   " }),
            StatusCode::BAD_REQUEST,
            "provider_model_invalid",
            "a blank model",
        ),
        (
            json!({ "provider": null, "model": "gpt-4o" }),
            StatusCode::BAD_REQUEST,
            "provider_model_invalid",
            "a model with no provider",
        ),
    ] {
        let refused = tenant.choose(&tenant.owner.token, &chat, choice).await;
        assert_refused(&refused, status, code, what);
    }

    let read = tenant.get_chat(&tenant.owner.token, &chat).await;
    let read = expect(&read, StatusCode::OK, "the chat is read");
    assert!(
        read["chat"]["provider"].is_null() && read["chat"]["provider_model"].is_null(),
        "no refused choice was stored: {read}"
    );
}

#[tokio::test]
async fn a_choice_that_loses_its_key_fails_in_the_chat_without_falling_back() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({ "openai_api_key": OPENAI_KEY }))
        .await;
    let chat = tenant.chat(&tenant.owner.token).await;
    let chosen = tenant
        .choose(
            &tenant.owner.token,
            &chat,
            json!({ "provider": "openai", "model": "gpt-4o" }),
        )
        .await;
    expect(
        &chosen,
        StatusCode::OK,
        "OpenAI is chosen while its key is saved",
    );

    tenant
        .save_organization_settings(json!({ "openai_api_key": "" }))
        .await;

    let listed = tenant.offers(&tenant.owner.token, &chat).await;
    assert_shapes(&listed);
    let selected = &listed["selected"];
    assert_eq!(
        selected["provider"],
        json!("openai"),
        "an unusable choice is still reported: {listed}"
    );
    assert_eq!(selected["model"], json!("gpt-4o"), "{listed}");
    assert_eq!(selected["usable"], json!(false), "{listed}");
    assert_eq!(
        selected["reason"],
        json!("No key, endpoint or credentials are saved for OpenAI in AI Settings."),
        "{listed}"
    );

    let previewed = tenant.context(&tenant.owner.token, &chat).await;
    let previewed = expect(
        &previewed,
        StatusCode::CONFLICT,
        "the chat's context preview fails on the lost provider",
    );
    assert_eq!(
        previewed["error"],
        json!(
            "This chat's AI provider, OpenAI, can't be used: no key, endpoint or credentials are saved for it. Choose another provider for this chat, or ask an admin to set it up in AI Settings."
        ),
        "{previewed}"
    );

    let read = tenant.get_chat(&tenant.owner.token, &chat).await;
    let read = expect(
        &read,
        StatusCode::OK,
        "the chat still opens on an unusable provider",
    );
    assert_eq!(read["chat"]["provider"], json!("openai"));
    assert_eq!(
        read["chat"]["context"],
        Value::Null,
        "no context is measured on the instance instead: {read}"
    );
}

#[tokio::test]
async fn a_choice_whose_saved_endpoint_is_refused_fails_in_the_chat() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({
            "openai_api_key": OPENAI_KEY,
            "openai_base_url": "https://openai-gateway.example/v1",
        }))
        .await;
    let chat = tenant.chat(&tenant.owner.token).await;
    let chosen = tenant
        .choose(
            &tenant.owner.token,
            &chat,
            json!({ "provider": "openai", "model": "gpt-4o" }),
        )
        .await;
    expect(
        &chosen,
        StatusCode::OK,
        "OpenAI is chosen while its endpoint is allowed",
    );

    tenant.refuse_the_saved_openai_url().await;

    let listed = tenant.offers(&tenant.owner.token, &chat).await;
    let selected = &listed["selected"];
    assert_eq!(selected["usable"], json!(false), "{listed}");
    assert_eq!(
        selected["reason"],
        json!(
            "OpenAI's saved endpoint can't be used: The URL must not point at a link-local or cloud metadata address."
        ),
        "{listed}"
    );

    let previewed = tenant.context(&tenant.owner.token, &chat).await;
    let previewed = expect(
        &previewed,
        StatusCode::CONFLICT,
        "the chat's context preview fails on the refused endpoint",
    );
    assert_eq!(
        previewed["error"],
        json!(
            "This chat's AI provider, OpenAI, can't be used: its saved endpoint is refused (the URL must not point at a link-local or cloud metadata address). Choose another provider for this chat, or ask an admin to fix it in AI Settings."
        ),
        "{previewed}"
    );
}

#[tokio::test]
async fn a_choice_belongs_to_one_chat() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({ "openai_api_key": OPENAI_KEY }))
        .await;
    let chosen_chat = tenant.chat(&tenant.owner.token).await;
    let other_chat = tenant.chat(&tenant.owner.token).await;

    let chosen = tenant
        .choose(
            &tenant.owner.token,
            &chosen_chat,
            json!({ "provider": "openai", "model": "gpt-4o" }),
        )
        .await;
    expect(&chosen, StatusCode::OK, "OpenAI is chosen for one chat");

    assert_eq!(
        tenant.offers(&tenant.owner.token, &chosen_chat).await["selected"]["provider"],
        json!("openai")
    );
    assert_eq!(
        tenant.offers(&tenant.owner.token, &other_chat).await["selected"],
        Value::Null,
        "the other chat still follows the default"
    );
    let other = tenant.get_chat(&tenant.owner.token, &other_chat).await;
    let other = expect(&other, StatusCode::OK, "the other chat is read");
    assert!(
        other["chat"]["provider"].is_null(),
        "the choice leaked into another chat: {other}"
    );
    assert_eq!(
        tenant.workspace_offers().await["default"]["provider"],
        json!("self_hosted"),
        "a chat's choice does not change the workspace's default"
    );
}

#[tokio::test]
async fn the_workspace_lists_chat_providers_before_any_chat_exists() {
    let tenant = Tenant::new().await;

    let listed = tenant.workspace_offers().await;
    assert_shapes(&listed);
    assert!(
        listed.get("selected").is_none(),
        "a workspace has no selection of its own: {listed}"
    );
    let default = &listed["default"];
    assert_eq!(default["provider"], json!("self_hosted"), "{listed}");
    assert_eq!(default["label"], json!("Self-Hosted"));
    assert_eq!(default["usable"], json!(true));
    assert_eq!(default["reason"], Value::Null);
    assert_eq!(default["requires_model"], json!(false));
    assert_eq!(default["efforts"], efforts(&HTTP_EFFORTS));
    assert_eq!(
        names(&listed["providers"]),
        ["self_hosted"],
        "a workspace with nothing saved offers only the instance: {listed}"
    );
    assert_eq!(
        listed["recent"],
        Value::Null,
        "a workspace without chats has no recent settings: {listed}"
    );

    let stranger = account(&tenant.client).await;
    let refused = tenant.workspace_providers(&stranger.token).await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a stranger listed the workspace's providers: {}",
        refused.text()
    );
}

#[tokio::test]
async fn the_workspace_reports_the_settings_of_the_chat_it_used_last() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({ "openai_api_key": OPENAI_KEY }))
        .await;

    let created = tenant
        .create_chat(
            &tenant.owner.token,
            json!({
                "provider": "openai",
                "provider_model": "gpt-4o",
                "reasoning_effort": "high",
                "agent_enabled": true,
                "auto_approve": true,
                "offline": true,
            }),
        )
        .await;
    let created = expect(&created, StatusCode::CREATED, "a chat is created on OpenAI");
    let chosen = created["chat"]["id"].as_str().expect("the chat's id");
    let listed = tenant.workspace_offers().await;
    assert_hides_the_key(
        &tenant.workspace_providers(&tenant.owner.token).await,
        "the listing",
    );
    assert_eq!(
        listed["recent"],
        json!({
            "model_name": "gpt-4",
            "provider": "openai",
            "provider_model": "gpt-4o",
            "reasoning_effort": "high",
            "agent_enabled": true,
            "offline": true,
            "context_tokens": null,
        }),
        "a new chat starts from the one just created: {listed}"
    );

    tenant.chat(&tenant.owner.token).await;
    let recent = &tenant.workspace_offers().await["recent"];
    assert_eq!(recent["provider"], Value::Null, "{recent}");
    assert_eq!(recent["provider_model"], Value::Null, "{recent}");
    assert_eq!(recent["model_name"], json!("gpt-4"), "{recent}");
    assert_eq!(recent["reasoning_effort"], json!("auto"), "{recent}");

    let chose = tenant
        .choose(
            &tenant.owner.token,
            chosen,
            json!({ "provider": "openai", "model": "gpt-4.1" }),
        )
        .await;
    expect(&chose, StatusCode::OK, "the older chat changes its model");
    let recent = &tenant.workspace_offers().await["recent"];
    assert_eq!(
        (&recent["provider"], &recent["provider_model"]),
        (&json!("openai"), &json!("gpt-4.1")),
        "changing a chat's model makes it the one a new chat starts from: {recent}"
    );

    let updated = tenant
        .update_chat(
            &tenant.owner.token,
            chosen,
            json!({ "reasoning_effort": "low" }),
        )
        .await;
    expect(&updated, StatusCode::OK, "the chat changes its effort");
    let recent = &tenant.workspace_offers().await["recent"];
    assert_eq!(recent["reasoning_effort"], json!("low"), "{recent}");

    let viewer = tenant.seated("viewer").await;
    let seen = tenant.workspace_providers(&viewer.token).await;
    let seen = expect(&seen, StatusCode::OK, "a viewer lists the providers");
    assert_eq!(
        seen["recent"], *recent,
        "a viewer reads the same recent settings"
    );
}

#[tokio::test]
async fn create_stores_a_chosen_provider_and_a_refused_one_creates_nothing() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({ "openai_api_key": OPENAI_KEY }))
        .await;

    let created = tenant
        .create_chat(
            &tenant.owner.token,
            json!({ "provider": "openai", "provider_model": "gpt-4o" }),
        )
        .await;
    assert_hides_the_key(&created, "the created chat");
    let created = expect(&created, StatusCode::CREATED, "a chat is created on OpenAI");
    assert_eq!(created["chat"]["provider"], json!("openai"), "{created}");
    assert_eq!(
        created["chat"]["provider_model"],
        json!("gpt-4o"),
        "{created}"
    );
    let chat = created["chat"]["id"].as_str().expect("the chat's id");
    let selected = &tenant.offers(&tenant.owner.token, chat).await["selected"];
    assert_eq!(selected["provider"], json!("openai"));
    assert_eq!(selected["model"], json!("gpt-4o"));

    for (choice, status, code, what) in [
        (
            json!({ "provider": "anthropic", "provider_model": "claude-sonnet-4-5" }),
            StatusCode::CONFLICT,
            "provider_unavailable",
            "an unconfigured provider",
        ),
        (
            json!({ "provider": "claude_code" }),
            StatusCode::CONFLICT,
            "provider_unavailable",
            "a signed-out agent",
        ),
        (
            json!({ "provider": "bedrock" }),
            StatusCode::CONFLICT,
            "provider_unavailable",
            "Bedrock",
        ),
        (
            json!({ "provider": "gemini" }),
            StatusCode::BAD_REQUEST,
            "provider_unknown",
            "an unknown provider",
        ),
        (
            json!({ "provider": "openai" }),
            StatusCode::BAD_REQUEST,
            "provider_model_required",
            "OpenAI without a model",
        ),
        (
            json!({ "provider": "openai", "provider_model": " " }),
            StatusCode::BAD_REQUEST,
            "provider_model_invalid",
            "a blank model",
        ),
    ] {
        let refused = tenant.create_chat(&tenant.owner.token, choice).await;
        assert_refused(&refused, status, code, what);
    }

    let chats = tenant.chats(&tenant.owner.token).await;
    assert_eq!(
        chats.len(),
        1,
        "a refused create left a chat behind: {chats:?}"
    );
    assert!(
        chats[0].get("provider").is_none() && chats[0].get("provider_model").is_none(),
        "the chat list omits the provider fields: {}",
        chats[0]
    );
}

#[tokio::test]
async fn create_on_a_usable_choice_succeeds_while_the_workspace_default_is_unusable() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({
            "provider": "openai",
            "openai_api_key": OPENAI_KEY,
            "openai_base_url": "https://openai-gateway.example/v1",
            "model_fast": "gpt-4o",
        }))
        .await;
    tenant.refuse_the_saved_openai_url().await;

    let on_default = tenant.create_chat(&tenant.owner.token, json!({})).await;
    let on_default = expect(
        &on_default,
        StatusCode::CONFLICT,
        "a Default chat cannot be created on an unusable workspace route",
    );
    assert_eq!(
        on_default["error"],
        json!(
            "This workspace's AI endpoint can't be used: its saved URL is refused (the URL must not point at a link-local or cloud metadata address). Check AI Settings."
        ),
        "{on_default}"
    );

    let listed = tenant.workspace_offers().await;
    assert_shapes(&listed);
    assert_eq!(listed["default"]["provider"], json!("openai"));
    assert_eq!(listed["default"]["usable"], json!(false));
    assert_eq!(
        listed["default"]["reason"],
        json!(
            "OpenAI's saved endpoint can't be used: The URL must not point at a link-local or cloud metadata address."
        ),
        "{listed}"
    );
    assert_eq!(names(&listed["providers"]), ["self_hosted"], "{listed}");

    let created = tenant
        .create_chat(&tenant.owner.token, json!({ "provider": "self_hosted" }))
        .await;
    let created = expect(
        &created,
        StatusCode::CREATED,
        "a chat chosen onto Self-Hosted does not need the workspace's route",
    );
    assert_eq!(
        created["chat"]["provider"],
        json!("self_hosted"),
        "{created}"
    );
    assert!(
        created["chat"]["provider_model"].is_null(),
        "Self-Hosted on the instance needs no model: {created}"
    );
}

#[tokio::test]
async fn an_agent_default_offers_its_own_models_and_efforts() {
    let tenant = Tenant::new().await;

    tenant
        .save_organization_settings(json!({ "provider": "claude_code" }))
        .await;
    let listed = tenant.workspace_offers().await;
    assert_shapes(&listed);
    let claude = &listed["default"];
    assert_eq!(claude["provider"], json!("claude_code"), "{listed}");
    assert_eq!(claude["usable"], json!(false), "{listed}");
    assert_eq!(claude["reason"], json!(signed_out("Claude Code")));
    assert_eq!(claude["requires_model"], json!(false));
    assert_eq!(claude["efforts"], efforts(&CLAUDE_EFFORTS));
    assert_eq!(
        claude["models"],
        json!([
            { "name": "sonnet", "label": "Sonnet", "efforts": CLAUDE_EFFORTS },
            { "name": "opus", "label": "Opus", "efforts": CLAUDE_EFFORTS },
            { "name": "haiku", "label": "Haiku", "efforts": [] },
            { "name": "fable", "label": "Fable", "efforts": CLAUDE_EFFORTS },
        ]),
        "Claude Code offers its own models, ranked and labelled"
    );
    assert!(
        !names(&listed["providers"]).contains(&"claude_code".to_string()),
        "a signed-out agent is not offered: {listed}"
    );

    tenant
        .save_organization_settings(json!({ "provider": "codex" }))
        .await;
    let listed = tenant.workspace_offers().await;
    assert_shapes(&listed);
    let codex = &listed["default"];
    assert_eq!(codex["provider"], json!("codex"), "{listed}");
    assert_eq!(codex["usable"], json!(false), "{listed}");
    assert_eq!(codex["reason"], json!(signed_out("Codex")));
    assert_eq!(codex["requires_model"], json!(false));
    assert_eq!(
        codex["efforts"],
        efforts(&CODEX_EFFORTS),
        "Codex with no model offers only what every model accepts"
    );
    assert_eq!(
        codex["models"],
        json!([
            { "name": "gpt-6-astra", "label": "gpt-6-astra", "efforts": CODEX_ULTRA_EFFORTS },
            { "name": "gpt-6-sol", "label": "gpt-6-sol", "efforts": CODEX_ULTRA_EFFORTS },
            { "name": "gpt-6-luna", "label": "gpt-6-luna", "efforts": CODEX_MAX_EFFORTS },
            { "name": "gpt-5.6-sol", "label": "gpt-5.6-sol", "efforts": CODEX_ULTRA_EFFORTS },
            { "name": "gpt-5.6-terra", "label": "gpt-5.6-terra", "efforts": CODEX_ULTRA_EFFORTS },
            { "name": "gpt-5.6-luna", "label": "gpt-5.6-luna", "efforts": CODEX_MAX_EFFORTS },
            { "name": "gpt-5.5", "label": "gpt-5.5", "efforts": CODEX_EFFORTS },
        ]),
        "Codex offers each model's own efforts"
    );
}

#[tokio::test]
async fn a_chat_keeps_the_agent_only_efforts() {
    let tenant = Tenant::new().await;
    let chat = tenant.chat(&tenant.owner.token).await;

    for effort in ["xhigh", "max", "ultra"] {
        let updated = tenant
            .update_chat(
                &tenant.owner.token,
                &chat,
                json!({ "reasoning_effort": effort }),
            )
            .await;
        let updated = expect(
            &updated,
            StatusCode::OK,
            &format!("the chat takes the {effort} effort"),
        );
        assert_eq!(updated["chat"]["reasoning_effort"], json!(effort));

        let read = tenant.get_chat(&tenant.owner.token, &chat).await;
        let read = expect(&read, StatusCode::OK, "the chat is read");
        assert_eq!(
            read["chat"]["reasoning_effort"],
            json!(effort),
            "the chat kept the {effort} effort"
        );
    }

    let created = tenant
        .create_chat(&tenant.owner.token, json!({ "reasoning_effort": "ultra" }))
        .await;
    let created = expect(
        &created,
        StatusCode::CREATED,
        "a chat is created with the ultra effort",
    );
    assert_eq!(created["chat"]["reasoning_effort"], json!("ultra"));
}

#[tokio::test]
async fn a_default_chat_changes_its_model_but_not_to_a_blank_one() {
    let tenant = Tenant::new().await;
    let chat = tenant.chat(&tenant.owner.token).await;

    let updated = tenant
        .update_chat(
            &tenant.owner.token,
            &chat,
            json!({ "model_name": "gpt-4o-mini" }),
        )
        .await;
    let updated = expect(&updated, StatusCode::OK, "a Default chat changes its model");
    assert_eq!(
        updated["chat"]["model_name"],
        json!("gpt-4o-mini"),
        "{updated}"
    );

    let blank = tenant
        .update_chat(&tenant.owner.token, &chat, json!({ "model_name": "  " }))
        .await;
    let blank = expect(&blank, StatusCode::BAD_REQUEST, "a blank model is refused");
    assert_eq!(blank["error"], json!("Model cannot be blank"), "{blank}");

    let read = tenant.get_chat(&tenant.owner.token, &chat).await;
    let read = expect(&read, StatusCode::OK, "the chat is read");
    assert_eq!(
        read["chat"]["model_name"],
        json!("gpt-4o-mini"),
        "the refused blank model left the chat's model alone"
    );
}

#[tokio::test]
async fn a_chat_is_not_moved_to_a_model_that_cannot_chat() {
    const EMBEDDER: &str = "nomic-embed-text";
    const UNKNOWN: &str = "a-model-the-instance-has-not-heard-of";
    let ollama = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/show"))
        .and(body_json(json!({ "model": EMBEDDER })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "capabilities": ["embedding"] })),
        )
        .mount(&ollama)
        .await;
    let tenant = Tenant::on_ollama(&ollama.uri()).await;
    let chat = tenant.chat(&tenant.owner.token).await;

    let embedder = tenant
        .update_chat(
            &tenant.owner.token,
            &chat,
            json!({ "model_name": EMBEDDER }),
        )
        .await;
    let embedder = expect(
        &embedder,
        StatusCode::BAD_REQUEST,
        "a model that only embeds is refused",
    );
    assert_eq!(embedder["error"], json!(UNSUPPORTED), "{embedder}");

    let unknown = tenant
        .update_chat(&tenant.owner.token, &chat, json!({ "model_name": UNKNOWN }))
        .await;
    let unknown = expect(
        &unknown,
        StatusCode::OK,
        "a model the instance cannot describe is taken",
    );
    assert_eq!(unknown["chat"]["model_name"], json!(UNKNOWN), "{unknown}");

    let read = tenant.get_chat(&tenant.owner.token, &chat).await;
    let read = expect(&read, StatusCode::OK, "the chat is read");
    assert_eq!(
        read["chat"]["model_name"],
        json!(UNKNOWN),
        "only the accepted model was kept"
    );
}

#[tokio::test]
async fn a_chat_cannot_change_its_model_while_its_workspace_route_is_refused() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({
            "provider": "openai",
            "openai_api_key": OPENAI_KEY,
            "model_fast": "gpt-4o-mini",
            "model_reasoning": "gpt-4o",
        }))
        .await;
    let chat = tenant.chat(&tenant.owner.token).await;
    tenant.refuse_the_saved_openai_url().await;

    let refused = tenant
        .update_chat(
            &tenant.owner.token,
            &chat,
            json!({ "model_name": "gpt-4o-mini" }),
        )
        .await;
    let refused = expect(
        &refused,
        StatusCode::CONFLICT,
        "a model change on an unusable route is refused",
    );
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty()),
        "the refusal says why: {refused}"
    );

    let viewer = tenant.seated("viewer").await;
    let forbidden = tenant
        .update_chat(&viewer.token, &chat, json!({ "model_name": "gpt-4o" }))
        .await;
    assert_eq!(
        forbidden.status,
        StatusCode::FORBIDDEN,
        "a viewer cannot change the model: {}",
        forbidden.text()
    );
}

#[tokio::test]
async fn an_agent_is_chosen_only_with_a_model_it_offers() {
    let tenant = Tenant::new().await;
    tenant.sign_in_codex().await;
    let chat = tenant.chat(&tenant.owner.token).await;

    for model in ["gpt-4o", "gpt-7", "opus"] {
        let refused = tenant
            .choose(
                &tenant.owner.token,
                &chat,
                json!({ "provider": "codex", "model": model }),
            )
            .await;
        assert_refused(
            &refused,
            StatusCode::BAD_REQUEST,
            "provider_model_invalid",
            &format!("Codex does not offer {model}"),
        );
    }
    let read = tenant.get_chat(&tenant.owner.token, &chat).await;
    let read = expect(&read, StatusCode::OK, "the chat is read");
    assert!(
        read["chat"]["provider"].is_null(),
        "no refused choice was stored: {read}"
    );

    let chosen = tenant
        .choose(
            &tenant.owner.token,
            &chat,
            json!({ "provider": "codex", "model": "gpt-6-sol" }),
        )
        .await;
    let chosen = expect(&chosen, StatusCode::OK, "Codex runs a model it offers");
    assert_eq!(chosen["chat"]["provider"], json!("codex"), "{chosen}");
    assert_eq!(
        chosen["chat"]["provider_model"],
        json!("gpt-6-sol"),
        "{chosen}"
    );
}

#[tokio::test]
async fn a_chosen_chat_changes_its_model_while_its_workspace_route_is_refused() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({
            "provider": "openai",
            "openai_api_key": OPENAI_KEY,
            "model_fast": "gpt-4o-mini",
        }))
        .await;
    tenant.refuse_the_saved_openai_url().await;
    let created = tenant
        .create_chat(&tenant.owner.token, json!({ "provider": "self_hosted" }))
        .await;
    let created = expect(
        &created,
        StatusCode::CREATED,
        "a chat is created on Self-Hosted",
    );
    let chat = created["chat"]["id"].as_str().expect("the chat's id");

    let updated = tenant
        .update_chat(
            &tenant.owner.token,
            chat,
            json!({ "model_name": "gpt-4o-mini" }),
        )
        .await;
    let updated = expect(
        &updated,
        StatusCode::OK,
        "a chat on its own provider does not use the refused workspace route",
    );
    assert_eq!(
        updated["chat"]["model_name"],
        json!("gpt-4o-mini"),
        "{updated}"
    );
    assert_eq!(
        updated["chat"]["provider"],
        json!("self_hosted"),
        "{updated}"
    );
}

#[tokio::test]
async fn a_stranger_neither_lists_nor_chooses_a_chats_provider() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({ "openai_api_key": OPENAI_KEY }))
        .await;
    let chat = tenant.chat(&tenant.owner.token).await;
    let stranger = account(&tenant.client).await;
    let elsewhere = organization(&tenant.client, &stranger.token).await;
    workspace(&tenant.client, &stranger.token, &elsewhere).await;

    let listed = tenant.providers(&stranger.token, &chat).await;
    assert_eq!(
        listed.status,
        StatusCode::FORBIDDEN,
        "a stranger listed the chat's providers: {}",
        listed.text()
    );
    assert_hides_the_key(&listed, "a stranger's listing");

    let chosen = tenant
        .choose(
            &stranger.token,
            &chat,
            json!({ "provider": "openai", "model": "gpt-4o" }),
        )
        .await;
    assert_eq!(
        chosen.status,
        StatusCode::FORBIDDEN,
        "a stranger chose the chat's provider: {}",
        chosen.text()
    );
    assert_eq!(
        tenant.offers(&tenant.owner.token, &chat).await["selected"],
        Value::Null,
        "the stranger's refused choice left nothing behind"
    );
}

#[tokio::test]
async fn a_member_runs_only_a_model_the_settings_save_on_the_workspaces_key() {
    let tenant = Tenant::new().await;
    let member = tenant.seated("member").await;
    tenant
        .save_organization_settings(json!({
            "provider": "openai",
            "openai_api_key": OPENAI_KEY,
            "model_fast": "gpt-4o-mini",
        }))
        .await;
    let chat = tenant.chat(&member.token).await;

    let refused = tenant
        .choose(
            &member.token,
            &chat,
            json!({ "provider": "openai", "model": "o1-pro" }),
        )
        .await;
    assert_refused(
        &refused,
        StatusCode::BAD_REQUEST,
        "provider_model_invalid",
        "a model the admin did not save",
    );
    let created = tenant
        .create_chat(
            &member.token,
            json!({ "provider": "openai", "provider_model": "o1-pro" }),
        )
        .await;
    assert_refused(
        &created,
        StatusCode::BAD_REQUEST,
        "provider_model_invalid",
        "a new chat on a model the admin did not save",
    );
    assert_eq!(
        tenant.offers(&member.token, &chat).await["selected"],
        Value::Null,
        "the refused choice left the chat on the default"
    );

    let chosen = tenant
        .choose(
            &member.token,
            &chat,
            json!({ "provider": "openai", "model": "gpt-4o-mini" }),
        )
        .await;
    let chosen = expect(&chosen, StatusCode::OK, "a saved model is chosen");
    assert_eq!(
        chosen["chat"]["provider_model"],
        json!("gpt-4o-mini"),
        "{chosen}"
    );
}

#[tokio::test]
async fn a_chosen_chat_is_held_to_models_that_can_chat() {
    const EMBEDDER: &str = "nomic-embed-text";
    let ollama = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/show"))
        .and(body_json(json!({ "model": EMBEDDER })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "capabilities": ["embedding"] })),
        )
        .mount(&ollama)
        .await;
    let tenant = Tenant::on_ollama(&ollama.uri()).await;

    let created = tenant
        .create_chat(
            &tenant.owner.token,
            json!({ "provider": "self_hosted", "model_name": EMBEDDER }),
        )
        .await;
    let created = expect(
        &created,
        StatusCode::BAD_REQUEST,
        "a chosen chat is not created on a stored model that only embeds",
    );
    assert_eq!(created["error"], json!(UNSUPPORTED), "{created}");
    assert!(
        tenant.chats(&tenant.owner.token).await.is_empty(),
        "the refused create left a chat behind"
    );

    let created = tenant
        .create_chat(&tenant.owner.token, json!({ "provider": "self_hosted" }))
        .await;
    let created = expect(&created, StatusCode::CREATED, "a chat on Self-Hosted");
    let chat = created["chat"]["id"].as_str().expect("the chat's id");

    let moved = tenant
        .update_chat(&tenant.owner.token, chat, json!({ "model_name": EMBEDDER }))
        .await;
    let moved = expect(
        &moved,
        StatusCode::BAD_REQUEST,
        "a chosen chat is not moved to a stored model that only embeds",
    );
    assert_eq!(moved["error"], json!(UNSUPPORTED), "{moved}");

    let chosen = tenant
        .choose(
            &tenant.owner.token,
            chat,
            json!({ "provider": "self_hosted", "model": EMBEDDER }),
        )
        .await;
    assert_refused(
        &chosen,
        StatusCode::BAD_REQUEST,
        "provider_model_invalid",
        "the instance lists the model as one that only embeds",
    );
    let read = tenant.get_chat(&tenant.owner.token, chat).await;
    let read = expect(&read, StatusCode::OK, "the chat is read");
    assert!(
        read["chat"]["provider_model"].is_null(),
        "the refused model was stored: {read}"
    );
    assert_eq!(read["chat"]["model_name"], json!("gpt-4"), "{read}");
}

/// Records what a coding agent reported of `chat`'s last turn, as a finished turn does.
async fn report(tenant: &Tenant, chat: &str, model: &str, window: u64, used: Option<u64>) {
    let chat: Uuid = chat.parse().expect("a chat id");
    occupancy::set(
        tenant.client.state().db(),
        chat,
        &Reported {
            agent: AgentKind::Claude,
            model: model.to_string(),
            occupancy: Occupancy::reported(Some(window), used).expect("a reported window"),
        },
    )
    .await
    .expect("the report is kept");
}

async fn previewed_context(tenant: &Tenant, chat: &str) -> Value {
    let previewed = tenant.context(&tenant.owner.token, chat).await;
    expect(
        &previewed,
        StatusCode::OK,
        "the chat's context is previewed",
    )["context"]
        .clone()
}

#[tokio::test]
async fn a_cli_chats_window_is_the_one_its_agent_reported_never_a_kept_context() {
    let tenant = Tenant::new().await;
    tenant
        .save_organization_settings(json!({ "provider": "claude_code" }))
        .await;
    let created = tenant
        .create_chat(
            &tenant.owner.token,
            json!({ "model_name": "sonnet", "context_tokens": 8192 }),
        )
        .await;
    let chat = expect(
        &created,
        StatusCode::CREATED,
        "a Claude Code chat is created",
    )["chat"]["id"]
        .as_str()
        .expect("the created chat's id")
        .to_string();

    let fresh = previewed_context(&tenant, &chat).await;
    assert_eq!(
        fresh["limit"],
        Value::Null,
        "a fresh CLI chat has no window until its agent reports one, not the 8192 it kept: {fresh}"
    );
    assert_eq!(fresh["source"], json!("unknown"), "{fresh}");

    report(&tenant, &chat, "sonnet", 200_000, Some(18_400)).await;

    let reported = previewed_context(&tenant, &chat).await;
    assert_eq!(reported["limit"], json!(200_000), "{reported}");
    assert_eq!(reported["source"], json!("runtime"), "{reported}");
    assert!(
        reported["used"].as_u64().expect("a count of used tokens") >= 18_400,
        "the agent's own count of its last request is the floor of what is used: {reported}"
    );

    let read = tenant.get_chat(&tenant.owner.token, &chat).await;
    let read = expect(&read, StatusCode::OK, "the chat opens");
    assert_eq!(
        read["chat"]["context"]["limit"],
        json!(200_000),
        "a reloaded chat shows the reported window: {read}"
    );

    report(&tenant, &chat, "opus", 1_000_000, None).await;
    let another = previewed_context(&tenant, &chat).await;
    assert_eq!(
        another["limit"],
        Value::Null,
        "a window reported for another model is not this model's: {another}"
    );
}

#[tokio::test]
async fn a_self_hosted_chat_measures_its_kept_window_through_its_endpoint() {
    let tenant = Tenant::new().await;
    let created = tenant
        .create_chat(&tenant.owner.token, json!({ "context_tokens": 8192 }))
        .await;
    let chat = expect(
        &created,
        StatusCode::CREATED,
        "a Self-Hosted chat is created",
    )["chat"]["id"]
        .as_str()
        .expect("the created chat's id")
        .to_string();
    report(&tenant, &chat, "gpt-4", 200_000, Some(18_400)).await;

    let following = previewed_context(&tenant, &chat).await;
    assert_eq!(following["limit"], json!(8192), "{following}");
    assert_eq!(following["source"], json!("configured"), "{following}");

    let chosen = tenant
        .choose(
            &tenant.owner.token,
            &chat,
            json!({ "provider": "self_hosted", "model": "gpt-4" }),
        )
        .await;
    expect(
        &chosen,
        StatusCode::OK,
        "the chat chooses Self-Hosted by name",
    );

    let named = previewed_context(&tenant, &chat).await;
    assert_eq!(
        named["limit"],
        json!(8192),
        "the same self-hosted endpoint chosen by name keeps the chat's window: {named}"
    );
    assert_eq!(named["source"], json!("configured"), "{named}");
}
