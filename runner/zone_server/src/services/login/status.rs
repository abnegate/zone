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
                (status.state, status.expires_at) = match agent {
                    AgentKind::Claude => sealed(state.encryption_key(), &login, Utc::now()),
                    AgentKind::Codex => (saved(state.config(), organization), None),
                };
                status.source = Some(Source::Zone);
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

/// A Zone-managed Claude login's state at `now`, judged by the credential a turn would run with,
/// and when its access token runs out. A login Zone cannot open has expired.
fn sealed(
    key: &[u8; 32],
    login: &AgentLoginRow,
    now: DateTime<Utc>,
) -> (State, Option<DateTime<Utc>>) {
    let tokens = login
        .credential
        .as_ref()
        .and_then(|sealed| Tokens::open(key, sealed.expose()).ok());
    match tokens {
        None => (State::Expired, None),
        Some(tokens) => {
            let state = if tokens.expires_at > now || tokens.refresh.is_some() {
                State::SignedIn
            } else {
                State::Expired
            };
            (state, Some(tokens.expires_at.trunc_subsecs(0)))
        }
    }
}

/// A Zone-managed codex login is signed in while codex's own `auth.json` is in the organization's
/// home.
fn saved(config: &Config, organization: Uuid) -> State {
    if codex::signed_in(&config.agents.home(organization, AgentKind::Codex)) {
        State::SignedIn
    } else {
        State::Expired
    }
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

    fn seal(key: &[u8; 32], expires_at: DateTime<Utc>, refresh: Option<&str>) -> SecretValue {
        let tokens = Tokens {
            access: SecretValue::new("fake-access-token"),
            refresh: refresh.map(SecretValue::new),
            expires_at,
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

    fn claude(credential: Option<SecretValue>, expires_at: DateTime<Utc>) -> AgentLoginRow {
        login(AgentKind::Claude, credential, Some(expires_at))
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
    fn a_claude_login_that_renews_itself_is_signed_in_after_its_token_runs_out() {
        let key = key();
        let now = at(1_790_000_000);
        let expires_at = now - TimeDelta::days(1);
        let renewable = seal(&key, expires_at, Some("fake-refresh-token"));

        assert_eq!(
            sealed(&key, &claude(Some(renewable), expires_at), now),
            (State::SignedIn, Some(expires_at))
        );
    }

    #[test]
    fn a_claude_login_that_cannot_renew_shows_when_its_token_runs_out() {
        let key = key();
        let now = at(1_790_000_000);
        let lasting = now + TimeDelta::days(365);
        let lapsed = now - TimeDelta::seconds(1);

        for (expires_at, state) in [
            (lasting, State::SignedIn),
            (now, State::Expired),
            (lapsed, State::Expired),
        ] {
            let final_token = seal(&key, expires_at, None);

            assert_eq!(
                sealed(&key, &claude(Some(final_token), expires_at), now),
                (state, Some(expires_at)),
                "expires {expires_at}"
            );
        }
    }

    #[test]
    fn a_claude_login_is_judged_by_its_sealed_token_and_not_by_the_row() {
        let key = key();
        let now = at(1_790_000_000);
        let final_token = seal(&key, now - TimeDelta::days(1), None);

        assert_eq!(
            sealed(
                &key,
                &claude(Some(final_token), now + TimeDelta::days(1)),
                now
            ),
            (State::Expired, Some(now - TimeDelta::days(1)))
        );
    }

    #[test]
    fn a_claude_login_zone_cannot_open_has_expired() {
        let (sealing, reading) = (key(), key());
        let now = at(1_790_000_000);
        let lasting = now + TimeDelta::days(365);

        for (credential, reason) in [
            (
                Some(seal(&sealing, lasting, Some("fake-refresh-token"))),
                "sealed with another key",
            ),
            (Some(SecretValue::new("never-sealed")), "never sealed"),
            (None, "holding nothing"),
        ] {
            assert_eq!(
                sealed(&reading, &claude(credential, lasting), now),
                (State::Expired, None),
                "a login {reason} was taken for one that can run"
            );
        }
    }

    #[test]
    fn a_codex_login_is_signed_in_while_its_auth_file_is_in_the_organizations_home() {
        let state = TempDir::new().expect("a state root");
        let config = config(&state);
        let organization = Uuid::new_v4();
        let home = config
            .agents
            .create_home(organization, AgentKind::Codex)
            .expect("the organization's codex home");

        assert_eq!(saved(&config, organization), State::Expired);

        std::fs::write(home.join("auth.json"), "{}").expect("codex's login");
        assert_eq!(saved(&config, organization), State::SignedIn);
        assert_eq!(
            saved(&config, Uuid::new_v4()),
            State::Expired,
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
