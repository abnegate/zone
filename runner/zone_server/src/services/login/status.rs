//! How an organization's sign-in to each coding agent looks to its members.

mod host;
mod login;
mod prompt;
mod reading;
mod source;
mod state;
mod usage;
mod viewer;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use abnegate_secret::redact;
use chrono::{DateTime, SubsecRound, Utc};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zone_core::llm::AgentKind;

pub use login::LoginStatus;
pub use prompt::Prompt;
pub use source::Source;
pub use state::State;
pub use usage::UsageStatus;
pub use viewer::Viewer;

use super::claude::Tokens;
use super::probe::{self, Probe};
use super::router::Chosen;
use super::usage::{Availability, Snapshot, refresh};
use super::{codex, credential, devices, oauth};
use crate::config::Config;
use crate::db::agent_logins::{self, AgentLoginRow};
use crate::db::ai_settings;
use crate::state::AppState;
use host::Host;
use reading::Reading;

/// How long one check of the host's own sign-in answers for every organization.
const FRESH: Duration = Duration::from_secs(30);

static HOST: LazyLock<Host> = LazyLock::new(|| Host::new(FRESH));

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentStatus {
    #[serde(with = "name")]
    pub agent: AgentKind,
    pub provider: String,
    /// The best of the organization's logins, as its `label` is: the one a turn would start on.
    pub state: State,
    pub source: Option<Source>,
    pub label: Option<String>,
    /// When the best login's Claude access token runs out, for one with no refresh token to renew
    /// it, as each login's own `expires_at` is.
    pub expires_at: Option<DateTime<Utc>>,
    pub models: Vec<String>,
    pub pending: Option<Prompt>,
    /// Why a sign-in that finished away from the panel failed, until the next one starts: the
    /// organization's last codex device sign-in, or the viewer's own Claude sign-in `attempt`.
    pub error: Option<String>,
    /// Every login the organization holds of the agent, oldest first.
    pub logins: Vec<LoginStatus>,
}

impl AgentStatus {
    /// `agent`'s status for `organization`, as `viewer` may see it, with why their Claude sign-in
    /// `attempt` failed when it has. Each login shows its usage, read again first when what was
    /// last read for it is older than the TTL, as a session's router reads it.
    ///
    /// Signed in means Zone holds credentials the agent can use, not that they still work:
    /// neither CLI checks them until a turn needs them.
    pub async fn read(
        state: &AppState,
        organization: Uuid,
        agent: AgentKind,
        viewer: Viewer,
        attempt: Option<Uuid>,
    ) -> Result<Self, sqlx::Error> {
        let logins = agent_logins::list_for(state.db(), organization, agent.as_str()).await?;
        let snapshots = refreshed(state, &logins).await;
        let now = Utc::now();
        let readings: Vec<Reading> = logins
            .iter()
            .zip(snapshots)
            .map(|(login, snapshot)| read(state, login, snapshot, logins.len(), now))
            .collect();
        let mut status = Self::signed_out(agent);
        let pending = match agent {
            AgentKind::Claude => {
                status.error = attempt
                    .filter(|_| viewer.manages)
                    .and_then(|attempt| oauth::failure(attempt, organization, viewer.user));
                None
            }
            AgentKind::Codex => {
                status.error = devices::failure(organization);
                devices::pending(organization)
            }
        };

        let best = readings
            .iter()
            .max_by(|reading, other| reading.compare(other, now));
        match (pending, best) {
            (Some((prompt, initiator)), best) => {
                status.state = State::Pending;
                status.pending =
                    prompt.map(|prompt| Prompt::shown(&prompt, viewer.sees_code(initiator)));
                if let Some(best) = best {
                    status.source = Some(Source::Zone);
                    status.label.clone_from(&best.login.label);
                }
            }
            (None, Some(best)) => status.lead(&best.login),
            (None, None) => {
                if let Some(probe) = host(state.config(), agent).await {
                    status.state = State::SignedIn;
                    status.source = Some(Source::Host);
                    status.label = probe.label;
                }
            }
        }
        status.logins = readings.into_iter().map(|reading| reading.login).collect();
        Ok(status)
    }

    /// Summarises the agent's status as `best`, the login a turn would start on.
    fn lead(&mut self, best: &LoginStatus) {
        self.state = best.state;
        self.expires_at = best.expires_at;
        self.source = Some(Source::Zone);
        self.label.clone_from(&best.label);
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
            logins: vec![],
        }
    }
}

/// Each of `logins`' usage, in order: brought up to date through the router's TTL-guarded
/// refresh, or as stored for a login that cannot be resolved to read it with.
async fn refreshed(state: &AppState, logins: &[AgentLoginRow]) -> Vec<Option<Snapshot>> {
    let resolutions = join_all(logins.iter().map(|login| async move {
        let agent = AgentKind::named(&login.agent)?;
        match credential::resolve(state, login).await {
            Ok(resolved) => Some(Chosen {
                login: login.clone(),
                agent,
                resolved,
                snapshot: login.snapshot(),
            }),
            Err(error) => {
                tracing::debug!(login = %login.id, %error, "Showing the login's stored usage");
                None
            }
        }
    }))
    .await;
    let mut chosen: Vec<Chosen> = resolutions.into_iter().flatten().collect();
    refresh(state, &mut chosen).await;
    let mut fresh: HashMap<_, _> = chosen
        .into_iter()
        .map(|chosen| (chosen.login.id, chosen.snapshot))
        .collect();
    logins
        .iter()
        .map(|login| fresh.remove(&login.id).unwrap_or_else(|| login.snapshot()))
        .collect()
}

/// How `login`, one of `held` logins of its agent, looks at `now` with its usage `snapshot`.
fn read(
    state: &AppState,
    login: &AgentLoginRow,
    snapshot: Option<Snapshot>,
    held: usize,
    now: DateTime<Utc>,
) -> Reading {
    let (state, expires_at, plan) = match AgentKind::named(&login.agent) {
        Some(AgentKind::Claude) => {
            let tokens = opened(state.encryption_key(), login);
            let (judged, expires_at) = judged(tokens.as_ref(), now);
            (judged, expires_at, tokens.and_then(|tokens| tokens.label()))
        }
        Some(AgentKind::Codex) => {
            let (saved, plan) = saved(state.config(), login, held);
            (saved, None, plan)
        }
        None => (State::Expired, None, None),
    };
    let availability = availability(snapshot.as_ref(), login.exhausted_until);
    Reading {
        login: LoginStatus {
            id: login.id,
            label: login.label.clone(),
            plan,
            state,
            expires_at,
            exhausted_until: login.exhausted_until,
            usage: snapshot.map(|snapshot| UsageStatus {
                windows: snapshot.windows,
                headroom: snapshot.headroom,
                fetched_at: snapshot.fetched_at,
            }),
            last_used_at: login.last_used_at,
        },
        availability,
    }
}

/// When a login can take a turn again, from its last `snapshot` and when it was marked exhausted
/// until: the later of the two, and unknown when a spent window gives no reset time.
fn availability(
    snapshot: Option<&Snapshot>,
    exhausted_until: Option<DateTime<Utc>>,
) -> Availability {
    snapshot
        .map_or(Availability::Now, Snapshot::availability)
        .max(exhausted_until.map_or(Availability::Now, Availability::At))
}

fn opened(key: &[u8; 32], login: &AgentLoginRow) -> Option<Tokens> {
    login
        .credential
        .as_ref()
        .and_then(|sealed| Tokens::open(key, sealed.expose()).ok())
}

/// A Zone-managed Claude login's state at `now`, judged by the credential a turn would run with,
/// and when it runs out. A login that renews itself never shows an expiry, and one Zone cannot
/// open has expired.
fn judged(tokens: Option<&Tokens>, now: DateTime<Utc>) -> (State, Option<DateTime<Utc>>) {
    match tokens {
        None => (State::Expired, None),
        Some(tokens) if tokens.refresh.is_some() => (State::SignedIn, None),
        Some(tokens) => {
            let state = if tokens.expires_at > now {
                State::SignedIn
            } else {
                State::Expired
            };
            (state, Some(tokens.expires_at.trunc_subsecs(0)))
        }
    }
}

/// A Zone-managed codex login is signed in while codex's own `auth.json` is in its home, and is
/// on the plan that file names. The organization's only login may still have its file where codex
/// kept one login before logins had homes.
fn saved(config: &Config, login: &AgentLoginRow, held: usize) -> (State, Option<String>) {
    let organization = login.organization_id;
    let home = config
        .agents
        .login_home(organization, AgentKind::Codex, login.id);
    let root = config.agents.home(organization, AgentKind::Codex);
    let home = if codex::signed_in(&home) {
        home
    } else if held == 1 && codex::signed_in(&root) {
        root
    } else {
        return (State::Expired, None);
    };
    let plan = codex::signed_in_as(&home)
        .and_then(|account| account.plan)
        .and_then(|plan| codex::plan::label(&plan))
        .map(str::to_string);
    (State::SignedIn, plan)
}

/// The host's own sign-in, when the server may fall back on it and the host's CLI says it is
/// signed in.
async fn host(config: &Config, agent: AgentKind) -> Option<Probe> {
    if !config.agents.host_login {
        return None;
    }
    let executable = config.agent_executable(agent);
    HOST.check(agent, executable.clone(), ask(agent, executable))
        .await
}

async fn ask(agent: AgentKind, executable: PathBuf) -> Option<Probe> {
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
    use abnegate_secret::SecretValue;
    use chrono::TimeDelta;
    use futures::future::join_all;
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use tempfile::TempDir;

    use zone_core::llm::Window;

    use super::*;
    use crate::config::{AgentConfig, ModelBackend};
    use crate::db::agent_logins::Insert;
    use crate::services::login::codex::testing::fake;

    const STATUSES: usize = 5;
    const JAKE: &str = "jake@example.com";
    const ADA: &str = "ada@example.com";
    const OLD: &str = "old@example.com";
    const PRO: &str = r#"{"tokens":{"id_token":"eyJhbGciOiJub25lIn0.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9wbGFuX3R5cGUiOiJwcm8ifX0.","account_id":"00000000-0000-4000-8000-000000000000"}}"#;

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
            issued_at: None,
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
            account: None,
            credential,
            label: None,
            expires_at,
            windows: None,
            headroom: None,
            usage_fetched_at: None,
            exhausted_until: None,
            last_used_at: None,
            created_at: at(1_780_000_000),
            updated_at: at(1_780_000_000),
        }
    }

    fn subscribed(key: &[u8; 32], expires_at: DateTime<Utc>, refresh: Option<&str>) -> String {
        let tokens = Tokens {
            access: SecretValue::new("fake-access-token"),
            refresh: refresh.map(SecretValue::new),
            expires_at,
            issued_at: None,
            scope: "user:inference user:profile".to_string(),
            subscription: Some("max".to_string()),
        };
        tokens.seal(key).expect("seal")
    }

    fn snapshot(used_percent: f64, fetched_at: DateTime<Utc>) -> Snapshot {
        Snapshot {
            windows: vec![Window {
                name: "5h".to_string(),
                used_percent: Some(used_percent),
                used: None,
                limit: None,
                resets_at: Some((fetched_at + TimeDelta::hours(3)).trunc_subsecs(0)),
            }],
            headroom: Some(100.0 - used_percent),
            fetched_at: fetched_at.trunc_subsecs(0),
        }
    }

    fn claude(credential: Option<SecretValue>, expires_at: DateTime<Utc>) -> AgentLoginRow {
        login(AgentKind::Claude, credential, Some(expires_at))
    }

    fn sealed(
        key: &[u8; 32],
        login: &AgentLoginRow,
        now: DateTime<Utc>,
    ) -> (State, Option<DateTime<Utc>>) {
        judged(opened(key, login).as_ref(), now)
    }

    fn config(state: &TempDir) -> Config {
        Config {
            agents: AgentConfig {
                state: state.path().to_path_buf(),
                ..crate::state::test_agents()
            },
            ..crate::state::test_config()
        }
    }

    #[test]
    fn a_login_is_available_once_both_its_snapshot_and_its_exhaustion_allow() {
        let now = at(1_790_000_000);
        let fresh = snapshot(20.0, now);
        let spent = snapshot(100.0, now);
        let resets = spent.windows[0].resets_at.expect("a reset time");
        let later = resets + TimeDelta::hours(1);
        let mut unknown = snapshot(100.0, now);
        unknown.windows[0].resets_at = None;

        assert_eq!(availability(None, None), Availability::Now);
        assert_eq!(availability(Some(&fresh), None), Availability::Now);
        assert_eq!(availability(Some(&spent), None), Availability::At(resets));
        assert_eq!(
            availability(Some(&spent), Some(later)),
            Availability::At(later)
        );
        assert_eq!(
            availability(Some(&fresh), Some(later)),
            Availability::At(later)
        );
        assert_eq!(
            availability(Some(&unknown), Some(later)),
            Availability::Unknown,
            "a spent window with no reset time leaves the login spent with no known end"
        );
    }

    #[test]
    fn a_claude_login_that_renews_itself_is_signed_in_with_no_expiry_to_show() {
        let key = key();
        let now = at(1_790_000_000);

        for expires_at in [now + TimeDelta::hours(8), now - TimeDelta::days(1)] {
            let renewable = seal(&key, expires_at, Some("fake-refresh-token"));

            assert_eq!(
                sealed(&key, &claude(Some(renewable), expires_at), now),
                (State::SignedIn, None),
                "a login that renews itself showed when its access token runs out"
            );
        }
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
    fn a_codex_login_is_signed_in_while_its_auth_file_is_in_its_own_home() {
        let state = TempDir::new().expect("a state root");
        let config = config(&state);
        let organization = Uuid::new_v4();
        let mut login = login(AgentKind::Codex, None, None);
        login.organization_id = organization;
        let home = config
            .agents
            .create_login_home(organization, AgentKind::Codex, login.id)
            .expect("the login's home");

        assert_eq!(saved(&config, &login, 1), (State::Expired, None));

        std::fs::write(home.join("auth.json"), PRO).expect("codex's login");
        assert_eq!(
            saved(&config, &login, 2),
            (State::SignedIn, Some("ChatGPT Pro".to_string()))
        );
        let elsewhere = AgentLoginRow {
            id: Uuid::new_v4(),
            ..login.clone()
        };
        assert_eq!(
            saved(&config, &elsewhere, 2),
            (State::Expired, None),
            "another login's home counted"
        );
    }

    #[test]
    fn the_only_codex_login_is_signed_in_by_a_file_left_where_codex_kept_one_login() {
        let state = TempDir::new().expect("a state root");
        let config = config(&state);
        let organization = Uuid::new_v4();
        let mut login = login(AgentKind::Codex, None, None);
        login.organization_id = organization;
        let root = config
            .agents
            .create_home(organization, AgentKind::Codex)
            .expect("the organization's codex root");
        std::fs::write(root.join("auth.json"), "{}").expect("a login from before login homes");

        assert_eq!(saved(&config, &login, 1), (State::SignedIn, None));
        assert_eq!(
            saved(&config, &login, 2),
            (State::Expired, None),
            "with two logins, the legacy file could belong to either"
        );
    }

    #[tokio::test]
    async fn the_hosts_sign_in_is_checked_once_for_everyone_who_asks() {
        let directory = TempDir::new().expect("a temporary directory");
        let checked = directory.path().join("checked");
        let codex = fake(
            &directory,
            "login status",
            &format!(
                "echo checked >> '{}'\nsleep 0.2\necho 'Logged in using ChatGPT' >&2\nexit 0",
                checked.display()
            ),
        );
        let config = Config {
            model_backend: ModelBackend::Cli {
                agent: AgentKind::Codex,
                executable: Some(codex),
            },
            agents: AgentConfig {
                state: directory.path().join("agents"),
                host_login: true,
                ..crate::state::test_agents()
            },
            ..crate::state::test_config()
        };

        let answers = join_all((0..STATUSES).map(|_| host(&config, AgentKind::Codex))).await;
        let later = host(&config, AgentKind::Codex).await;

        let expected = Some(Probe {
            signed_in: true,
            label: Some("ChatGPT".to_string()),
        });
        for answer in answers.into_iter().chain([later]) {
            assert_eq!(answer, expected);
        }
        let checks = std::fs::read_to_string(&checked).expect("codex was asked");
        assert_eq!(
            checks.lines().count(),
            1,
            "every status asked the host's codex for itself"
        );
    }

    #[tokio::test]
    async fn a_status_lists_every_login_with_its_snapshot_and_summarises_the_best() {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .expect("the test database accepts connections");
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Statuses', $1::text)")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization to sign in");
        let directory = TempDir::new().expect("a state root");
        let state = AppState::new(config(&directory), pool.clone(), None);
        let key = *state.encryption_key();
        let now = Utc::now();
        let lasting = now + TimeDelta::days(365);
        let lapsed = now - TimeDelta::days(1);
        let sign_in = |account: &'static str, credential: String, expires_at| {
            let pool = pool.clone();
            async move {
                agent_logins::insert(
                    &pool,
                    &Insert {
                        organization_id: organization,
                        agent: AgentKind::Claude.as_str(),
                        account: Some(account),
                        credential: Some(&credential),
                        label: Some(account),
                        expires_at: Some(expires_at),
                    },
                )
                .await
                .expect("a stored login")
            }
        };
        let jake = sign_in(
            JAKE,
            subscribed(&key, lasting, Some("fake-refresh")),
            lasting,
        )
        .await;
        let ada = sign_in(
            ADA,
            subscribed(&key, lasting, Some("fake-refresh")),
            lasting,
        )
        .await;
        let old = sign_in(OLD, subscribed(&key, lapsed, None), lapsed).await;
        let jakes = snapshot(62.0, now);
        let adas = snapshot(20.0, now);
        agent_logins::observe(&pool, jake.id, &jakes)
            .await
            .expect("jake's usage");
        agent_logins::observe(&pool, ada.id, &adas)
            .await
            .expect("ada's usage");
        let resets = (now + TimeDelta::hours(2)).trunc_subsecs(0);
        agent_logins::exhaust(&pool, ada.id, resets)
            .await
            .expect("ada at her limit");
        let used = (now - TimeDelta::minutes(10)).trunc_subsecs(0);
        agent_logins::touch(&pool, jake.id, used)
            .await
            .expect("jake used");
        let viewer = Viewer {
            user: Uuid::new_v4(),
            manages: false,
        };

        let status = AgentStatus::read(&state, organization, AgentKind::Claude, viewer, None)
            .await
            .expect("the status");

        let ids: Vec<Uuid> = status.logins.iter().map(|login| login.id).collect();
        assert_eq!(ids, [jake.id, ada.id, old.id], "every login, oldest first");
        let [jake_status, ada_status, old_status] = status.logins.as_slice() else {
            panic!("three logins, got {:?}", status.logins);
        };
        assert_eq!(jake_status.label.as_deref(), Some(JAKE));
        assert_eq!(jake_status.plan.as_deref(), Some("Claude Max"));
        assert_eq!(jake_status.state, State::SignedIn);
        assert_eq!(jake_status.last_used_at, Some(used));
        assert_eq!(
            jake_status
                .usage
                .as_ref()
                .map(|usage| (&usage.windows, usage.headroom)),
            Some((&jakes.windows, jakes.headroom))
        );
        assert_eq!(ada_status.exhausted_until, Some(resets));
        assert_eq!(jake_status.exhausted_until, None);
        assert_eq!(
            jake_status.expires_at, None,
            "a login that renews itself showed an expiry"
        );
        assert_eq!(old_status.state, State::Expired);
        assert_eq!(old_status.expires_at, Some(lapsed.trunc_subsecs(0)));
        assert_eq!(old_status.usage, None, "a login never read shows no usage");
        assert_eq!(
            (
                status.state,
                status.source,
                status.label.as_deref(),
                status.expires_at
            ),
            (State::SignedIn, Some(Source::Zone), Some(JAKE), None),
            "ada has more headroom but is at her limit, so jake is the best login"
        );

        agent_logins::delete(&pool, organization, jake.id)
            .await
            .expect("jake signed out");
        let status = AgentStatus::read(&state, organization, AgentKind::Claude, viewer, None)
            .await
            .expect("the status");
        assert_eq!(
            status.label.as_deref(),
            Some(ADA),
            "an exhausted login beats an expired one"
        );
        assert_eq!(status.state, State::SignedIn);

        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("the organization can be deleted");
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
                label: Some("jake@example.com".to_string()),
                expires_at: None,
                models: vec![
                    "sonnet".to_string(),
                    "opus".to_string(),
                    "haiku".to_string(),
                ],
                pending: None,
                error: None,
                logins: vec![LoginStatus {
                    id: Uuid::parse_str("3f2b9c1e-6d4a-4f0b-9c7e-1a2b3c4d5e6f")
                        .expect("a login id"),
                    label: Some("jake@example.com".to_string()),
                    plan: Some("Claude Max".to_string()),
                    state: State::SignedIn,
                    expires_at: None,
                    exhausted_until: None,
                    usage: Some(UsageStatus {
                        windows: vec![
                            Window {
                                name: "5h".to_string(),
                                used_percent: Some(62.0),
                                used: None,
                                limit: None,
                                resets_at: Some(at(1_790_143_800)),
                            },
                            Window {
                                name: "7d".to_string(),
                                used_percent: Some(31.0),
                                used: None,
                                limit: None,
                                resets_at: Some(at(1_790_568_000)),
                            },
                        ],
                        headroom: Some(38.0),
                        fetched_at: at(1_790_136_000),
                    }),
                    last_used_at: Some(at(1_790_135_400)),
                }],
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
                logins: vec![],
            },
        ];

        assert_eq!(json!({ "agents": expected }), fixture);
        let parsed: Vec<AgentStatus> = serde_json::from_value(fixture["agents"].clone())
            .expect("the fixture reads as statuses");
        assert_eq!(parsed, expected);
        assert_eq!(json!({ "agents": parsed }), fixture);
    }

    #[test]
    fn each_fixture_status_summarises_its_best_login_as_a_status_does() {
        let fixture: Value = serde_json::from_str(FIXTURE).expect("the fixture is JSON");
        let statuses: Vec<AgentStatus> = serde_json::from_value(fixture["agents"].clone())
            .expect("the fixture reads as statuses");

        let led: Vec<&AgentStatus> = statuses
            .iter()
            .filter(|status| status.pending.is_none())
            .collect();
        assert!(!led.is_empty(), "the fixture summarises no login");
        for status in led {
            let [best] = status.logins.as_slice() else {
                panic!("{} lists one login: {:?}", status.agent, status.logins);
            };
            let mut summarised = status.clone();
            summarised.lead(best);

            assert_eq!(
                &summarised, status,
                "{}'s summary is not what a status says of its only login",
                status.agent
            );
        }
    }

    #[tokio::test]
    async fn a_login_marked_spent_before_its_usage_was_ever_read_still_shows_until_when() {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .expect("the test database accepts connections");
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Spent', $1::text)")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization to sign in");
        let directory = TempDir::new().expect("a state root");
        let state = AppState::new(config(&directory), pool.clone(), None);
        let now = Utc::now();
        let lasting = now + TimeDelta::days(365);
        let credential = subscribed(state.encryption_key(), lasting, Some("fake-refresh"));
        let login = agent_logins::insert(
            &pool,
            &Insert {
                organization_id: organization,
                agent: AgentKind::Claude.as_str(),
                account: Some(JAKE),
                credential: Some(&credential),
                label: Some(JAKE),
                expires_at: Some(lasting),
            },
        )
        .await
        .expect("a stored login");
        let resets = (now + TimeDelta::hours(2)).trunc_subsecs(0);
        agent_logins::exhaust(&pool, login.id, resets)
            .await
            .expect("the login at its limit");
        let viewer = Viewer {
            user: Uuid::new_v4(),
            manages: false,
        };

        let status = AgentStatus::read(&state, organization, AgentKind::Claude, viewer, None)
            .await
            .expect("the status");
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("the organization can be deleted");

        let [spent] = status.logins.as_slice() else {
            panic!("one login, got {:?}", status.logins);
        };
        assert_eq!(spent.usage, None, "no usage was ever read for the login");
        assert_eq!(spent.state, State::SignedIn);
        assert_eq!(
            spent.exhausted_until,
            Some(resets),
            "a login with no snapshot hid that it is spent"
        );
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
                    "logins": [],
                })
            );
        }
    }

    #[test]
    fn a_status_with_no_sign_in_lists_no_logins() {
        for agent in AgentKind::ALL {
            let status = AgentStatus::signed_out(agent);
            let serialised = serde_json::to_value(&status).expect("serialise");

            assert!(
                status.logins.is_empty(),
                "{agent} listed {:?}",
                status.logins
            );
            assert_eq!(serialised["logins"], json!([]), "{agent}");
            assert_eq!(
                serde_json::from_value::<AgentStatus>(serialised).expect("deserialise"),
                status,
                "{agent}"
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
