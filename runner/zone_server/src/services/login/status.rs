//! How an organization's sign-in to each coding agent looks to its members.

mod prompt;
mod source;
mod state;
mod viewer;

use chrono::{DateTime, SubsecRound, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_core::llm::AgentKind;
use zone_core::secret::redact;

pub use prompt::Prompt;
pub use source::Source;
pub use state::State;
pub use viewer::Viewer;

use super::claude::Tokens;
use super::probe::{self, Probe};
use super::{codex, devices};
use crate::config::Config;
use crate::db::agent_logins::{self, AgentLoginRow};
use crate::db::ai_settings;
use crate::state::AppState;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatus {
    #[serde(with = "name")]
    pub agent: AgentKind,
    pub provider: String,
    pub state: State,
    pub source: Option<Source>,
    pub label: Option<String>,
    /// When Zone's Claude access token runs out. It renews while a refresh token lasts.
    pub expires_at: Option<DateTime<Utc>>,
    pub models: Vec<String>,
    pub pending: Option<Prompt>,
    /// Why the last codex device sign-in failed, until the next one starts.
    pub error: Option<String>,
}

impl AgentStatus {
    /// `agent`'s status for `organization`, as `viewer` may see it.
    ///
    /// Signed in means Zone holds credentials the agent can use, not that they still work:
    /// neither CLI checks them until a turn needs them.
    pub async fn read(
        state: &AppState,
        organization: Uuid,
        agent: AgentKind,
        viewer: Viewer,
    ) -> Result<Self, sqlx::Error> {
        let login = agent_logins::get(state.db(), organization, agent.as_str()).await?;
        let mut status = Self::signed_out(agent);
        let pending = match agent {
            AgentKind::Claude => None,
            AgentKind::Codex => {
                status.error = devices::failure(organization);
                devices::pending(organization)
            }
        };

        match (pending, login) {
            (Some((prompt, initiator)), login) => {
                status.state = State::Pending;
                status.pending = Some(Prompt::shown(&prompt, viewer.sees_code(initiator)));
                if let Some(login) = login {
                    status.source = Some(Source::Zone);
                    status.label = login.label;
                }
            }
            (None, Some(login)) => {
                let key = state.encryption_key();
                let now = Utc::now();
                status.state = if usable(state.config(), key, organization, agent, &login, now) {
                    State::SignedIn
                } else {
                    State::Expired
                };
                status.source = Some(Source::Zone);
                status.expires_at = login.expires_at.map(|expiry| expiry.trunc_subsecs(0));
                status.label = login.label;
            }
            (None, None) => {
                if let Some(probe) = host(state.config(), agent).await {
                    status.state = State::SignedIn;
                    status.source = Some(Source::Host);
                    status.label = probe.label;
                }
            }
        }
        Ok(status)
    }

    fn signed_out(agent: AgentKind) -> Self {
        Self {
            agent,
            provider: ai_settings::provider(agent).to_string(),
            state: State::SignedOut,
            source: None,
            label: None,
            expires_at: None,
            models: agent.models().iter().map(ToString::to_string).collect(),
            pending: None,
            error: None,
        }
    }
}

/// Whether the sign-in Zone keeps for the organization can still be used at `now`: a Claude login
/// until its access token runs out, or after that while it has a refresh token, and a codex login
/// while codex's own `auth.json` is in the organization's home.
fn usable(
    config: &Config,
    key: &[u8; 32],
    organization: Uuid,
    agent: AgentKind,
    login: &AgentLoginRow,
    now: DateTime<Utc>,
) -> bool {
    match agent {
        AgentKind::Claude => {
            login.expires_at.is_some_and(|expiry| expiry > now) || renewable(key, login)
        }
        AgentKind::Codex => codex::signed_in(&config.agents.home(organization, agent)),
    }
}

fn renewable(key: &[u8; 32], login: &AgentLoginRow) -> bool {
    login
        .credential
        .as_ref()
        .and_then(|sealed| Tokens::open(key, sealed.expose()).ok())
        .is_some_and(|tokens| tokens.refresh.is_some())
}

/// The host's own sign-in, when the server may fall back on it and the host's CLI says it is
/// signed in.
async fn host(config: &Config, agent: AgentKind) -> Option<Probe> {
    if !config.agents.host_login {
        return None;
    }
    let executable = config.agent_executable(agent);
    match probe::check(agent, &executable, &devices::variables(agent)).await {
        Ok(probe) => probe.signed_in.then_some(probe),
        Err(codex::Error::Unavailable {
            executable,
            message,
        }) => {
            tracing::debug!(%agent, %executable, %message, "no CLI on the host to fall back on");
            None
        }
        Err(error) => {
            tracing::warn!(
                %agent,
                error = %redact(&error.to_string()),
                "could not ask the host's CLI whether it is signed in"
            );
            None
        }
    }
}

/// An agent travels as its CLI's name.
mod name {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};
    use zone_core::llm::AgentKind;

    pub(super) fn serialize<S: Serializer>(
        agent: &AgentKind,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(agent.as_str())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<AgentKind, D::Error> {
        let name = String::deserialize(deserializer)?;
        AgentKind::named(&name).ok_or_else(|| D::Error::custom(format!("no agent is named {name}")))
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::{Value, json};
    use tempfile::TempDir;
    use zone_core::secret::SecretValue;

    use super::*;
    use crate::config::AgentConfig;

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/agents.json"
    ));

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("a valid timestamp")
    }

    fn key() -> [u8; 32] {
        let mut key = [0u8; 32];
        rand::fill(&mut key);
        key
    }

    fn sealed(key: &[u8; 32], refresh: Option<&str>) -> SecretValue {
        let tokens = Tokens {
            access: SecretValue::new("fake-access-token"),
            refresh: refresh.map(SecretValue::new),
            expires_at: at(1_790_000_000),
            scope: "user:inference".to_string(),
            subscription: None,
        };
        SecretValue::new(tokens.seal(key).expect("seal"))
    }

    fn login(
        agent: AgentKind,
        credential: Option<SecretValue>,
        expires_at: Option<DateTime<Utc>>,
    ) -> AgentLoginRow {
        AgentLoginRow {
            id: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            agent: agent.as_str().to_string(),
            credential,
            label: None,
            expires_at,
            created_at: at(1_780_000_000),
            updated_at: at(1_780_000_000),
        }
    }

    fn config(state: &TempDir) -> Config {
        Config {
            agents: AgentConfig {
                state: state.path().to_path_buf(),
                ..AgentConfig::default()
            },
            ..crate::state::test_config()
        }
    }

    #[test]
    fn a_claude_login_is_usable_until_it_expires_and_after_that_while_it_can_renew() {
        let key = key();
        let state = TempDir::new().expect("a state root");
        let config = config(&state);
        let now = at(1_790_000_000);
        let organization = Uuid::new_v4();
        let renewable = sealed(&key, Some("fake-refresh-token"));
        let final_token = sealed(&key, None);
        let claude = |key: &[u8; 32], login: &AgentLoginRow| {
            usable(&config, key, organization, AgentKind::Claude, login, now)
        };

        for (credential, expires_at, expected) in [
            (
                Some(final_token.clone()),
                Some(now + TimeDelta::seconds(1)),
                true,
            ),
            (
                Some(renewable.clone()),
                Some(now - TimeDelta::days(1)),
                true,
            ),
            (Some(final_token.clone()), Some(now), false),
            (Some(final_token), None, false),
            (
                Some(SecretValue::new("never-sealed")),
                Some(now - TimeDelta::days(1)),
                false,
            ),
            (None, Some(now - TimeDelta::days(1)), false),
        ] {
            let login = login(AgentKind::Claude, credential, expires_at);

            assert_eq!(claude(&key, &login), expected, "expires {expires_at:?}");
        }
        let renewable_elsewhere = login(
            AgentKind::Claude,
            Some(renewable),
            Some(now - TimeDelta::days(1)),
        );
        assert!(
            !claude(&[0; 32], &renewable_elsewhere),
            "a login sealed with another key was taken for one that can renew"
        );
    }

    #[test]
    fn a_codex_login_is_usable_while_its_auth_file_is_in_the_organizations_home() {
        let state = TempDir::new().expect("a state root");
        let config = config(&state);
        let organization = Uuid::new_v4();
        let login = login(AgentKind::Codex, None, None);
        let home = config
            .agents
            .create_home(organization, AgentKind::Codex)
            .expect("the organization's codex home");
        let codex = |organization| {
            usable(
                &config,
                &key(),
                organization,
                AgentKind::Codex,
                &login,
                Utc::now(),
            )
        };

        assert!(!codex(organization));

        std::fs::write(home.join("auth.json"), "{}").expect("codex's login");
        assert!(codex(organization));
        assert!(
            !codex(Uuid::new_v4()),
            "another organization's home counted"
        );
    }

    #[test]
    fn a_status_serialises_to_exactly_the_shared_fixture() {
        let fixture: Value = serde_json::from_str(FIXTURE).expect("the fixture is JSON");
        let expected = vec![
            AgentStatus {
                agent: AgentKind::Claude,
                provider: "claude_code".to_string(),
                state: State::SignedIn,
                source: Some(Source::Zone),
                label: Some("Claude Max".to_string()),
                expires_at: Some(at(1_821_672_000)),
                models: vec![
                    "sonnet".to_string(),
                    "opus".to_string(),
                    "haiku".to_string(),
                ],
                pending: None,
                error: None,
            },
            AgentStatus {
                agent: AgentKind::Codex,
                provider: "codex".to_string(),
                state: State::Pending,
                source: None,
                label: None,
                expires_at: None,
                models: vec![
                    "gpt-6-astra".to_string(),
                    "gpt-6-sol".to_string(),
                    "gpt-6-luna".to_string(),
                ],
                pending: Some(Prompt {
                    verification_url: "https://auth.openai.com/codex/device".to_string(),
                    user_code: Some("ABCD-EFGHI".to_string()),
                    expires_at: at(1_790_136_900),
                }),
                error: None,
            },
        ];

        assert_eq!(json!({ "agents": expected }), fixture);
        let parsed: Vec<AgentStatus> = serde_json::from_value(fixture["agents"].clone())
            .expect("the fixture reads as statuses");
        assert_eq!(parsed, expected);
        assert_eq!(json!({ "agents": parsed }), fixture);
    }

    #[test]
    fn a_signed_out_status_offers_the_agents_own_models_and_nothing_else() {
        for agent in AgentKind::ALL {
            let status = AgentStatus::signed_out(agent);

            assert_eq!(
                serde_json::to_value(&status).expect("serialise"),
                json!({
                    "agent": agent.as_str(),
                    "provider": ai_settings::provider(agent),
                    "state": "signed_out",
                    "source": null,
                    "label": null,
                    "expires_at": null,
                    "models": agent.models(),
                    "pending": null,
                    "error": null,
                })
            );
        }
    }

    #[test]
    fn an_unknown_agent_is_refused() {
        let mut status =
            serde_json::to_value(AgentStatus::signed_out(AgentKind::Claude)).expect("serialise");
        status["agent"] = json!("gemini");

        assert!(serde_json::from_value::<AgentStatus>(status).is_err());
    }
}
