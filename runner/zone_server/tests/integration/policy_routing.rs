//! A workspace's route under its organization's agent policy: enforced while a Team or
//! Enterprise license is installed, lapsed or not, never on Community, and failing closed when
//! the stored rules can't be read. No agent the policy denies is ever picked, by routing, by the
//! host's sign-in or by a handover.

use abnegate_agent_cli::login::claude::Tokens;
use abnegate_agent_cli::{AgentKind, CliSettings};
use abnegate_secret::SecretValue;
use axum::http::StatusCode;
use chrono::{TimeDelta, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use tempfile::TempDir;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::llm::{LlmBackend, SignIn};
use zone_enterprise::Tier;
use zone_enterprise::policy::{self, Denial, Effective};
use zone_server::config::{AgentConfig, Config};
use zone_server::db::agent_logins::{self, Insert};
use zone_server::db::{ai_settings, licenses, migrations};
use zone_server::services::backend::{self, Routing};
use zone_server::services::chat::handover::{Cause, Handover, Model};
use zone_server::services::endpoint::Origin;
use zone_server::services::login::claude::Sealed;
use zone_server::services::login::identity::LoginIdentity;
use zone_server::services::route::Route;
use zone_server::services::stages::{AUTO, Preferences};
use zone_server::state::AppState;

use crate::common::policy::Fixture as PolicyFixture;
use crate::common::sso::{active, keyring, lapsed};
use crate::common::{Database, agents, create_test_state, test_config};

const PREFIX: &str = "policy_routing";
const CLAUDE: &str = "claude_code";
const CODEX: &str = "codex";
const SELF_HOSTED: &str = "self_hosted";
const LOCAL: &str = "llama3:8b";
const UNVERIFIABLE: &str = "not a license key";
const LICENSE_REQUIRED: &str = "license_required";
const CODEX_CREDENTIALS_FILE: &str = "auth.json";
const CODEX_CREDENTIALS: &str = r#"{"tokens":{"access_token":"fake-codex-access-token","refresh_token":"fake-codex-refresh-token","account_id":"fake-codex-account"}}"#;
const SIGNED_OUT: &str = "Invalid API key · Please run /login";

struct Fixture {
    database: Database,
    state: AppState,
    organization: Uuid,
    workspace: Uuid,
    _agents: TempDir,
}

impl Fixture {
    async fn new(provider: &str) -> Self {
        Self::with_host_login(provider, false).await
    }

    async fn with_host_login(provider: &str, host_login: bool) -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        let directory = TempDir::new().expect("an agent state root");
        let config = Config {
            agents: AgentConfig {
                state: directory.path().to_path_buf(),
                host_login,
                ..agents()
            },
            ..test_config()
        };
        let state = create_test_state(config, database.pool.clone());
        state.install_keyring(keyring());
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Policy', $1::text)")
            .bind(organization)
            .execute(&database.pool)
            .await
            .unwrap();
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) \
             VALUES ($1, $2, 'Policy', $1::text)",
        )
        .bind(workspace)
        .bind(organization)
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO organization_ai_settings (organization_id, provider) VALUES ($1, $2)",
        )
        .bind(organization)
        .bind(provider)
        .execute(&database.pool)
        .await
        .unwrap();
        Self {
            database,
            state,
            organization,
            workspace,
            _agents: directory,
        }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn license(&self, key: &str) {
        licenses::replace(self.pool(), key).await.unwrap();
        self.state.licenses().load(self.pool()).await.unwrap();
    }

    async fn restrict(&self, rules: Value) {
        sqlx::query("INSERT INTO organization_policies (organization_id, rules) VALUES ($1, $2)")
            .bind(self.organization)
            .bind(rules)
            .execute(self.pool())
            .await
            .unwrap();
    }

    async fn route(&self) -> Route {
        Route::for_workspace(&self.state, self.workspace).await
    }

    async fn effective(&self) -> Effective {
        zone_server::services::policy::load(&self.state, self.organization, Some(self.workspace))
            .await
            .expect("a readable policy")
            .expect("an enforced policy")
    }

    async fn claude(&self, label: &str) -> LoginIdentity {
        let sealed = Tokens::new(
            SecretValue::new(format!("claude-token-{label}")),
            None,
            Utc::now() + TimeDelta::days(30),
            None,
            "user:inference",
            None,
        )
        .seal(self.state.master_key())
        .expect("the tokens to seal");
        let id = self.insert(AgentKind::Claude, label, Some(&sealed)).await;
        LoginIdentity {
            id,
            agent: AgentKind::Claude,
            label: label.to_string(),
        }
    }

    async fn codex(&self, label: &str) -> Uuid {
        let login = self.insert(AgentKind::Codex, label, None).await;
        let home = self
            .state
            .config()
            .agents
            .create_login_home(self.organization, AgentKind::Codex, login)
            .expect("the login's home");
        std::fs::write(home.join(CODEX_CREDENTIALS_FILE), CODEX_CREDENTIALS)
            .expect("the codex login's credentials");
        login
    }

    async fn insert(&self, agent: AgentKind, label: &str, credential: Option<&str>) -> Uuid {
        agent_logins::insert(
            self.pool(),
            &Insert {
                organization_id: self.organization,
                agent: agent.as_str(),
                account: Some(label),
                credential,
                label: Some(label),
                expires_at: None,
            },
        )
        .await
        .expect("a stored login")
        .id
    }

    fn handover(&self, current: LoginIdentity) -> Handover {
        let model = Model {
            requested: "sonnet".to_string(),
            preferences: Preferences::from_optional_settings(None, ""),
            message: "Why are there two tides a day?".to_string(),
            image: false,
            agentic: false,
        };
        Handover::new(self.organization, current, model, None, None)
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn agent_of(backend: &LlmBackend) -> Option<AgentKind> {
    match backend {
        LlmBackend::Cli { agent, .. } => Some(*agent),
        LlmBackend::Http => None,
    }
}

#[tokio::test]
async fn community_stores_rules_and_enforces_none_of_them() {
    let fixture = Fixture::new(CLAUDE).await;
    fixture
        .restrict(json!({"providers": [], "agents": [], "models": []}))
        .await;

    let route = fixture.route().await;
    let loaded =
        zone_server::services::policy::load(&fixture.state, fixture.organization, None).await;
    let precheck = route.precheck(Some("gpt-5")).await;
    let admitted = route
        .admit(
            &LlmBackend::cli(AgentKind::Codex, CliSettings::default()),
            "gpt-5",
        )
        .await;
    let usable = route.endpoint().is_ok();
    let policy = route.policy().cloned();
    fixture.cleanup().await;

    assert_eq!(loaded, Ok(None));
    assert!(usable, "Community enforced a stored policy");
    assert_eq!(policy, None);
    assert_eq!(precheck, Ok(()));
    assert_eq!(admitted, Ok(()));
}

#[tokio::test]
async fn an_active_team_license_makes_a_denied_agents_route_unusable() {
    let fixture = Fixture::new(CLAUDE).await;
    fixture.license(&active(Tier::Team)).await;
    fixture.restrict(json!({"agents": []})).await;

    let route = fixture.route().await;
    let unusable = route.endpoint().expect_err("a route the policy denies");
    let resolved = route.backend(&fixture.state).await;
    let precheck = route.precheck(None).await;
    fixture.cleanup().await;

    assert_eq!(unusable.code(), Some(policy::AGENT_DENIED));
    assert_eq!(
        unusable.denial(),
        Some(Denial::Agent(policy::Agent::Claude))
    );
    assert_eq!(
        resolved
            .expect_err("no backend for a denied agent")
            .denial(),
        Some(Denial::Agent(policy::Agent::Claude))
    );
    assert_eq!(precheck, Err(Denial::Agent(policy::Agent::Claude)));
}

#[tokio::test]
async fn a_lapsed_license_still_denies() {
    let fixture = Fixture::new(CLAUDE).await;
    fixture.license(&lapsed(Tier::Team)).await;
    fixture.restrict(json!({"agents": []})).await;

    let route = fixture.route().await;
    let code = route.endpoint().err().and_then(|unusable| unusable.code());
    fixture.cleanup().await;

    assert_eq!(code, Some(policy::AGENT_DENIED));
}

#[tokio::test]
async fn non_strict_routing_never_picks_a_denied_agents_login() {
    let fixture = Fixture::new(CLAUDE).await;
    fixture.license(&active(Tier::Enterprise)).await;
    fixture.restrict(json!({"agents": ["claude"]})).await;
    fixture.codex("codex@example.com").await;

    let route = fixture.route().await;
    let usable = route.endpoint().is_ok();
    let governed = route.backend(&fixture.state).await;
    let ungoverned = route
        .clone()
        .with_policy(None)
        .backend(&fixture.state)
        .await;
    fixture.cleanup().await;

    assert!(usable, "the policy admits the route's own agent");
    assert!(
        matches!(
            governed,
            Err(backend::Error::SignedOut {
                agent: AgentKind::Claude
            })
        ),
        "a denied agent's login ran the turn: {governed:?}"
    );
    let ungoverned = ungoverned.expect("another agent's login without the policy");
    assert_eq!(agent_of(&ungoverned), Some(AgentKind::Codex));
}

#[tokio::test]
async fn the_host_sign_in_never_runs_a_denied_agent() {
    let fixture = Fixture::with_host_login(CODEX, true).await;
    fixture.license(&active(Tier::Team)).await;
    fixture.restrict(json!({"agents": ["claude"]})).await;

    let route = fixture.route().await;
    let unusable = route.endpoint().err();
    let effective = fixture.effective().await;
    let settings = ai_settings::get_effective_ai_settings(
        fixture.pool(),
        fixture.organization,
        fixture.workspace,
    )
    .await
    .unwrap();
    let governed = backend::for_settings(
        &fixture.state,
        fixture.organization,
        &settings,
        Origin::Instance,
        Routing {
            policy: Some(&effective),
            ..Routing::default()
        },
    )
    .await;
    let ungoverned = backend::for_settings(
        &fixture.state,
        fixture.organization,
        &settings,
        Origin::Instance,
        Routing::default(),
    )
    .await;
    fixture.cleanup().await;

    assert_eq!(
        unusable.and_then(|unusable| unusable.denial()),
        Some(Denial::Agent(policy::Agent::Codex))
    );
    assert!(
        matches!(
            governed,
            Err(backend::Error::Denied(Denial::Agent(policy::Agent::Codex)))
        ),
        "the host's sign-in ran a denied agent: {governed:?}"
    );
    let ungoverned = ungoverned.expect("the host's sign-in without the policy");
    assert!(matches!(
        ungoverned.backend,
        LlmBackend::Cli {
            agent: AgentKind::Codex,
            sign_in: SignIn::Host,
            ..
        }
    ));
}

#[tokio::test]
async fn stored_rules_that_cannot_be_read_deny_everything() {
    let fixture = Fixture::new(CLAUDE).await;
    fixture.license(&active(Tier::Team)).await;
    fixture.restrict(json!({"agents": "claude"})).await;

    let route = fixture.route().await;
    let code = route.endpoint().err().and_then(|unusable| unusable.code());
    let loaded =
        zone_server::services::policy::load(&fixture.state, fixture.organization, None).await;
    let admitted = route
        .admit(
            &LlmBackend::cli(AgentKind::Claude, CliSettings::default()),
            "sonnet",
        )
        .await;
    fixture.cleanup().await;

    assert_eq!(code, Some(policy::UNREADABLE));
    assert_eq!(loaded, Err(Denial::Unreadable));
    assert_eq!(admitted, Err(Denial::Unreadable));
}

#[tokio::test]
async fn a_model_the_policy_does_not_list_is_not_admitted() {
    let fixture = Fixture::new(CLAUDE).await;
    fixture.license(&active(Tier::Team)).await;
    fixture
        .restrict(json!({"agents": ["claude"], "models": ["claude-opus-*"]}))
        .await;

    let route = fixture.route().await;
    let backend = LlmBackend::cli(AgentKind::Claude, CliSettings::default());
    let denied = route.admit(&backend, "sonnet").await;
    let allowed = route.admit(&backend, "claude-opus-4-1").await;
    let prechecked = [
        route.precheck(Some("sonnet")).await,
        route.precheck(Some("claude-opus-4-1")).await,
        route.precheck(Some(AUTO)).await,
        route.precheck(None).await,
    ];
    let usable = route.endpoint().is_ok();
    fixture.cleanup().await;

    assert!(usable);
    assert_eq!(denied, Err(Denial::Model));
    assert_eq!(denied.unwrap_err().code(), policy::MODEL_DENIED);
    assert_eq!(allowed, Ok(()));
    assert_eq!(prechecked, [Err(Denial::Model), Ok(()), Ok(()), Ok(())]);
}

#[tokio::test]
async fn a_handover_never_moves_onto_a_denied_agent() {
    let fixture = Fixture::new(CLAUDE).await;
    fixture.license(&active(Tier::Team)).await;
    fixture.restrict(json!({"agents": ["claude"]})).await;
    let alice = fixture.claude("alice@example.com").await;
    let codex = fixture.codex("codex@example.com").await;
    let effective = fixture.effective().await;
    let cause = Cause::signed_out(SIGNED_OUT.to_string());

    let governed = fixture
        .handover(alice.clone())
        .with_policy(Some(effective))
        .next(&fixture.state, &cause, None)
        .await;
    let ungoverned = fixture
        .handover(alice)
        .next(&fixture.state, &cause, None)
        .await;
    fixture.cleanup().await;

    assert!(
        matches!(governed, Err(None)),
        "the turn moved onto a denied agent: {:?}",
        governed.map(|switch| switch.to)
    );
    let switch = ungoverned.expect("another agent's login without the policy");
    assert_eq!((switch.to.id, switch.to.agent), (codex, AgentKind::Codex));
}

#[tokio::test]
async fn a_handover_never_moves_onto_a_model_the_policy_denies() {
    let fixture = Fixture::new(CLAUDE).await;
    fixture.license(&active(Tier::Team)).await;
    fixture.restrict(json!({"models": ["sonnet"]})).await;
    let alice = fixture.claude("alice@example.com").await;
    fixture.codex("codex@example.com").await;
    let effective = fixture.effective().await;

    let governed = fixture
        .handover(alice)
        .with_policy(Some(effective))
        .next(
            &fixture.state,
            &Cause::signed_out(SIGNED_OUT.to_string()),
            None,
        )
        .await;
    fixture.cleanup().await;

    assert!(
        matches!(governed, Err(None)),
        "the turn moved onto a model the policy denies: {:?}",
        governed.map(|switch| switch.model)
    );
}

async fn litellm_claiming_local_routes() -> MockServer {
    let server = MockServer::builder().start().await;
    Mock::given(method("GET"))
        .and(path("/v2/model/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"model_name": "*", "litellm_params": {"model": "ollama_chat/*"}}],
            "total_pages": 1
        })))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn only_an_organizations_litellm_endpoint_is_believed_about_where_it_runs() {
    let fixture = Fixture::new(SELF_HOSTED).await;
    fixture.license(&active(Tier::Team)).await;
    fixture.restrict(json!({"providers": ["local"]})).await;
    let organization = litellm_claiming_local_routes().await;
    let workspace = litellm_claiming_local_routes().await;
    sqlx::query(
        "UPDATE organization_ai_settings SET litellm_host = $2, completions_routed = true \
         WHERE organization_id = $1",
    )
    .bind(fixture.organization)
    .bind(organization.uri())
    .execute(fixture.pool())
    .await
    .unwrap();

    let on_organization = fixture.route().await.admit(&LlmBackend::Http, LOCAL).await;
    sqlx::query(
        "INSERT INTO workspace_ai_settings (workspace_id, litellm_host, completions_routed) \
         VALUES ($1, $2, true)",
    )
    .bind(fixture.workspace)
    .bind(workspace.uri())
    .execute(fixture.pool())
    .await
    .unwrap();
    let route = fixture.route().await;
    let on_workspace = route.admit(&LlmBackend::Http, LOCAL).await;
    let precheck = route.precheck(Some(LOCAL)).await;
    fixture.cleanup().await;

    assert_eq!(on_organization, Ok(()));
    let denied = Err(Denial::Provider(policy::Provider::Other));
    assert_eq!(on_workspace, denied);
    assert_eq!(precheck, denied);
    assert_eq!(on_workspace.unwrap_err().code(), policy::PROVIDER_DENIED);
}

#[tokio::test]
async fn a_stored_key_that_fails_verification_keeps_enforcing_and_refuses_changes() {
    let fixture = PolicyFixture::new(PREFIX).await;
    fixture.write_organization(json!({"agents": []})).await;
    fixture.store_license(UNVERIFIABLE).await;

    let loaded = zone_server::services::policy::load(
        fixture.state(),
        fixture.organization,
        Some(fixture.workspace),
    )
    .await;
    let change = fixture
        .update_organization(&fixture.owner.token, &json!({"agents": null}))
        .await;
    fixture.cleanup().await;

    assert_eq!(
        loaded
            .expect("a readable policy")
            .expect("an enforced policy")
            .agent(policy::Agent::Claude),
        Err(Denial::Agent(policy::Agent::Claude))
    );
    assert_eq!(change.status, StatusCode::FORBIDDEN, "{}", change.text());
    assert_eq!(change.json_value()["code"], json!(LICENSE_REQUIRED));
}
