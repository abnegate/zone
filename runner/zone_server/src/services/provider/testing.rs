use abnegate_agent_cli::login::claude::Tokens;
use abnegate_secret::SecretValue;
use chrono::{TimeDelta, Utc};
use sqlx::PgPool;
use tempfile::TempDir;
use uuid::Uuid;
use zone_core::llm::AgentKind;

use crate::config::{AgentConfig, Config};
use crate::db::agent_logins::{self, Insert};
use crate::services::login::claude::Sealed;
use crate::state::AppState;

const CODEX_CREDENTIALS_FILE: &str = "auth.json";
const CODEX_CREDENTIALS: &str = r#"{"tokens":{"access_token":"fake-codex-access-token","refresh_token":"fake-codex-refresh-token","account_id":"fake-codex-account"}}"#;

/// An organization with no sign-in, whose agents keep their state in a
/// directory of their own, and no host sign-in to fall back on.
pub struct Fixture {
    pub organization: Uuid,
    pub state: AppState,
    pool: PgPool,
    _agents: TempDir,
}

impl Fixture {
    pub async fn new() -> Self {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .expect("the test database accepts connections");
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Offers', $1::text)")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization");
        let agents = TempDir::new().expect("an agent state root");
        let config = Config {
            agents: AgentConfig {
                state: agents.path().to_path_buf(),
                host_login: false,
                ..crate::state::test_agents()
            },
            ollama_host: "http://127.0.0.1:9".to_string(),
            ..crate::state::test_config()
        };
        Self {
            organization,
            state: AppState::new(config, pool.clone(), None),
            pool,
            _agents: agents,
        }
    }

    /// Signs the organization in to Claude with a login that renews itself.
    pub async fn sign_in_claude(&self) {
        let tokens = Tokens::new(
            SecretValue::new("fake-access-token"),
            Some(SecretValue::new("fake-refresh-token")),
            Utc::now() + TimeDelta::hours(8),
            None,
            "user:inference",
            None,
        );
        let sealed = tokens
            .seal(self.state.master_key())
            .expect("tokens to seal");
        agent_logins::insert(
            &self.pool,
            &Insert {
                organization_id: self.organization,
                agent: AgentKind::Claude.as_str(),
                account: None,
                credential: Some(&sealed),
                label: None,
                expires_at: None,
            },
        )
        .await
        .expect("a stored login");
    }

    /// Signs the organization in to Codex with a login holding its own `auth.json`.
    pub async fn sign_in_codex(&self) {
        let login = agent_logins::insert(
            &self.pool,
            &Insert {
                organization_id: self.organization,
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
        let home = self
            .state
            .config()
            .agents
            .create_login_home(self.organization, AgentKind::Codex, login)
            .expect("the login's home");
        std::fs::write(home.join(CODEX_CREDENTIALS_FILE), CODEX_CREDENTIALS)
            .expect("the login's credentials");
    }

    pub async fn cleanup(self) {
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(self.organization)
            .execute(&self.pool)
            .await
            .expect("the organization can be deleted");
    }
}
