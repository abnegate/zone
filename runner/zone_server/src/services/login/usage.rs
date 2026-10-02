//! How much of each login's subscription is left.

mod availability;
mod error;
mod flight;
mod snapshot;

pub use availability::Availability;
pub use error::Error;
pub use snapshot::Snapshot;

use std::sync::LazyLock;
use std::time::Duration;

use aiusg::provider::{claude, codex, is_signed_out};
use aiusg::store::Credential;
use chrono::{DateTime, TimeDelta, Utc};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use futures::future::join_all;
use uuid::Uuid;
use zone_core::llm::{AgentKind, Window};

use super::credential::Login;
use super::router::Chosen;
use crate::config::AgentConfig;
use crate::db::agent_logins;
use crate::state::AppState;
use flight::Flight;

/// The longest a reading of a login's usage may take before routing goes on without it.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The client every read of an agent's service for a login goes through: its usage, and the
/// profile naming a Claude sign-in's account.
pub(super) static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("the agent service client builds")
});

static FLIGHTS: LazyLock<DashMap<Uuid, Flight>> = LazyLock::new(DashMap::new);

/// Brings each of `logins`' snapshots up to date when it is older than the configured TTL.
///
/// The stale ones are read at once, and a login's usage is read once however many refreshes want
/// it: a refresh that finds a reading of the login under way, or one started within the TTL,
/// shares it. A login whose usage cannot be read keeps the snapshot it had, and a login whose
/// agent refuses its token stays a candidate with its usage as it was.
pub async fn refresh(state: &AppState, logins: &mut [Chosen]) {
    let now = Utc::now();
    let ttl = ttl(state);
    join_all(
        logins
            .iter_mut()
            .filter(|chosen| {
                stale(
                    chosen.snapshot.as_ref().map(|snapshot| snapshot.fetched_at),
                    now,
                    ttl,
                )
            })
            .map(|chosen| async move {
                if let Some(snapshot) = read(state, chosen).await {
                    chosen.snapshot = Some(snapshot);
                }
            }),
    )
    .await;
}

/// The token `login` reads its agent's usage with: a Claude login's own, or the one the Codex
/// CLI stored in a codex login's home.
pub fn credential(login: &Login) -> Result<Credential, Error> {
    match login {
        Login::Claude { token } => Ok(Credential::bearer(token.expose())),
        Login::Codex { home } => codex::discover_in(home)
            .map_err(|error| Error::Unreadable(format!("{error:#}")))?
            .into_iter()
            .next()
            .map(|discovered| discovered.credential)
            .ok_or(Error::Missing),
    }
}

/// Reads the usage of `agent`'s login holding `credential` from the agent's service, giving up
/// after [`TIMEOUT`].
pub async fn fetch(
    config: &AgentConfig,
    agent: AgentKind,
    credential: &Credential,
) -> Result<Snapshot, Error> {
    let fetched_at = Utc::now();
    let reading = async {
        match agent {
            AgentKind::Claude => claude::fetch_at(&HTTP, &config.claude_api_url, credential).await,
            AgentKind::Codex => codex::fetch_at(&HTTP, &config.codex_api_url, credential).await,
        }
    };
    let fetched = tokio::time::timeout(TIMEOUT, reading)
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|error| {
            if is_signed_out(&error) {
                Error::SignedOut
            } else {
                Error::Unreadable(format!("{error:#}"))
            }
        })?;
    Ok(Snapshot::new(
        fetched.windows.into_iter().map(window).collect(),
        fetched_at,
    ))
}

/// `window` as Zone records it.
pub fn window(window: aiusg::model::Window) -> Window {
    Window {
        name: window.name,
        used_percent: window.used_percent,
        used: window.used,
        limit: window.limit,
        resets_at: window.resets_at,
    }
}

/// A fresh snapshot of `chosen`'s usage, or `None` when none could be had.
async fn read(state: &AppState, chosen: &Chosen) -> Option<Snapshot> {
    let login = chosen.login.id;
    let credential = match credential(&chosen.resolved) {
        Ok(credential) => credential,
        Err(error) => {
            tracing::warn!(
                %login,
                agent = %chosen.agent,
                %error,
                "Could not read the login's usage; routing on the snapshot it had"
            );
            return None;
        }
    };
    let flight = match FLIGHTS.entry(login) {
        Entry::Occupied(entry) if entry.get().current(state.config().agents.usage_ttl) => {
            entry.get().clone()
        }
        entry => {
            let flight = Flight::start(fly(state.clone(), login, chosen.agent, credential));
            entry.insert(flight.clone());
            flight
        }
    };
    flight.landed().await
}

/// Reads `login`'s usage and stores it, unless another server stored a fresh reading meanwhile.
async fn fly(
    state: AppState,
    login: Uuid,
    agent: AgentKind,
    credential: Credential,
) -> Option<Snapshot> {
    match agent_logins::get(state.db(), login).await {
        Ok(None) => return None,
        Ok(Some(row)) => {
            if let Some(snapshot) = row
                .snapshot()
                .filter(|snapshot| !stale(Some(snapshot.fetched_at), Utc::now(), ttl(&state)))
            {
                return Some(snapshot);
            }
        }
        Err(error) => tracing::warn!(%login, %error, "Could not read the login's stored usage"),
    }
    match fetch(&state.config().agents, agent, &credential).await {
        Ok(snapshot) => {
            if let Err(error) = agent_logins::observe(state.db(), login, &snapshot).await {
                tracing::warn!(%login, %error, "Could not store the login's usage");
            }
            Some(snapshot)
        }
        Err(error) => {
            tracing::warn!(
                %login,
                %agent,
                %error,
                "Could not read the login's usage; routing on the snapshot it had"
            );
            None
        }
    }
}

fn ttl(state: &AppState) -> TimeDelta {
    TimeDelta::from_std(state.config().agents.usage_ttl).unwrap_or(TimeDelta::MAX)
}

fn stale(fetched_at: Option<DateTime<Utc>>, now: DateTime<Utc>, ttl: TimeDelta) -> bool {
    fetched_at.is_none_or(|fetched_at| now.signed_duration_since(fetched_at) >= ttl)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Instant;

    use abnegate_secret::SecretValue;
    use tempfile::TempDir;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const CLAUDE_USAGE: &str = "/api/oauth/usage";
    const CODEX_USAGE: &str = "/backend-api/wham/usage";
    const CLAUDE_READING: &str = r#"{"five_hour":{"utilization":62.0,"resets_at":"2026-09-23T06:10:00Z"},"seven_day":{"utilization":31.0,"resets_at":"2026-09-28T04:00:00Z"}}"#;
    const CODEX_READING: &str = r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":40,"limit_window_seconds":18000,"reset_at":1790000000},"secondary_window":{"used_percent":75,"limit_window_seconds":604800,"reset_at":1790400000}}}"#;

    fn config(usage: &MockServer) -> AgentConfig {
        AgentConfig {
            claude_api_url: usage.uri(),
            codex_api_url: usage.uri(),
            ..AgentConfig::default()
        }
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .expect("a valid time")
            .with_timezone(&Utc)
    }

    fn claude(token: &str) -> Login {
        Login::Claude {
            token: SecretValue::new(token.to_string()),
        }
    }

    fn codex_home(auth: &str) -> TempDir {
        let home = TempDir::new().expect("a codex home");
        std::fs::write(home.path().join("auth.json"), auth).expect("the codex sign-in");
        home
    }

    fn codex(home: &Path) -> Login {
        Login::Codex {
            home: home.to_path_buf(),
        }
    }

    #[tokio::test]
    async fn a_claude_usage_fetch_reads_headroom_from_the_most_used_window() {
        let usage = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(CLAUDE_USAGE))
            .and(header("authorization", "Bearer claude-access"))
            .and(header("anthropic-beta", "oauth-2025-04-20"))
            .respond_with(ResponseTemplate::new(200).set_body_string(CLAUDE_READING))
            .expect(1)
            .mount(&usage)
            .await;
        let credential = credential(&claude("claude-access")).expect("a Claude token");
        let before = Utc::now();

        let snapshot = fetch(&config(&usage), AgentKind::Claude, &credential)
            .await
            .expect("the usage to be read");

        assert_eq!(snapshot.headroom, Some(38.0));
        assert_eq!(
            snapshot.windows,
            [
                Window {
                    name: "5h".to_string(),
                    used_percent: Some(62.0),
                    used: None,
                    limit: None,
                    resets_at: Some(at("2026-09-23T06:10:00Z")),
                },
                Window {
                    name: "7d".to_string(),
                    used_percent: Some(31.0),
                    used: None,
                    limit: None,
                    resets_at: Some(at("2026-09-28T04:00:00Z")),
                },
            ]
        );
        assert!((before..=Utc::now()).contains(&snapshot.fetched_at));
        assert_eq!(snapshot.availability(), Availability::Now);
        usage.verify().await;
    }

    #[tokio::test]
    async fn a_codex_usage_fetch_sends_the_account_id_and_bearer() {
        let usage = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(CODEX_USAGE))
            .and(header("authorization", "Bearer codex-access"))
            .and(header("chatgpt-account-id", "acct-42"))
            .respond_with(ResponseTemplate::new(200).set_body_string(CODEX_READING))
            .expect(1)
            .mount(&usage)
            .await;
        let home = codex_home(
            r#"{"tokens":{"access_token":"codex-access","refresh_token":"codex-refresh","account_id":"acct-42"}}"#,
        );
        let credential = credential(&codex(home.path())).expect("the codex CLI's token");

        let snapshot = fetch(&config(&usage), AgentKind::Codex, &credential)
            .await
            .expect("the usage to be read");

        assert_eq!(
            snapshot
                .windows
                .iter()
                .map(|window| (window.name.as_str(), window.used_percent))
                .collect::<Vec<_>>(),
            [("5h", Some(40.0)), ("7d", Some(75.0))]
        );
        assert_eq!(snapshot.headroom, Some(25.0));
        usage.verify().await;
    }

    #[tokio::test]
    async fn a_refused_usage_token_reads_as_signed_out_and_a_failure_as_unreadable() {
        let usage = MockServer::start().await;
        for (token, status) in [("refused", 401), ("forbidden", 403), ("failing", 500)] {
            Mock::given(path(CLAUDE_USAGE))
                .and(header("authorization", format!("Bearer {token}").as_str()))
                .respond_with(ResponseTemplate::new(status))
                .mount(&usage)
                .await;
        }
        let config = config(&usage);

        for (token, signed_out) in [("refused", true), ("forbidden", true), ("failing", false)] {
            let credential = credential(&claude(token)).expect("a Claude token");
            let error = fetch(&config, AgentKind::Claude, &credential)
                .await
                .expect_err("the usage to be refused");
            assert_eq!(
                matches!(error, Error::SignedOut),
                signed_out,
                "{token}: {error}"
            );
            if !signed_out {
                assert!(
                    matches!(&error, Error::Unreadable(message) if message.contains("500")),
                    "{error}"
                );
            }
        }
    }

    #[tokio::test]
    async fn a_usage_fetch_gives_up_after_its_timeout() {
        let usage = MockServer::start().await;
        Mock::given(path(CODEX_USAGE))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(CODEX_READING)
                    .set_delay(TIMEOUT * 4),
            )
            .mount(&usage)
            .await;
        let started = Instant::now();

        let error = fetch(
            &config(&usage),
            AgentKind::Codex,
            &Credential::bearer("slow"),
        )
        .await
        .expect_err("the slow endpoint to be given up on");

        assert!(matches!(error, Error::Timeout), "{error}");
        assert!(started.elapsed() < TIMEOUT * 2, "{:?}", started.elapsed());
    }

    #[test]
    fn a_codex_home_without_a_sign_in_has_no_credential_and_a_broken_one_says_why() {
        let empty = TempDir::new().expect("a codex home");
        let broken = codex_home("{\"tokens\":");

        assert!(matches!(
            credential(&codex(empty.path())),
            Err(Error::Missing)
        ));
        assert!(matches!(
            credential(&codex(broken.path())),
            Err(Error::Unreadable(message)) if message.contains("auth.json")
        ));
    }

    #[test]
    fn a_window_keeps_every_reading_aiusg_made() {
        let resets_at = at("2026-09-23T06:10:00Z");
        let counted =
            aiusg::model::Window::from_count("Extra usage", 30, 120).resetting_at(Some(resets_at));

        assert_eq!(
            window(counted),
            Window {
                name: "Extra usage".to_string(),
                used_percent: Some(25.0),
                used: Some(30),
                limit: Some(120),
                resets_at: Some(resets_at),
            }
        );
    }

    #[test]
    fn a_snapshot_agrees_with_aiusg_on_when_a_login_is_available() {
        let resets_at = at("2026-09-23T06:10:00Z");
        let windows = vec![
            aiusg::model::Window::from_percent("5h", 100.0).resetting_at(Some(resets_at)),
            aiusg::model::Window::from_percent("7d", 40.0),
        ];
        let usage = aiusg::model::Usage {
            account: aiusg::model::AccountId::new(aiusg::model::Provider::Claude, "jake"),
            provider: aiusg::model::Provider::Claude,
            label: "jake".to_string(),
            plan: None,
            windows: windows.clone(),
            fetched_at: Utc::now(),
        };

        let snapshot = Snapshot::new(windows.into_iter().map(window).collect(), Utc::now());

        assert_eq!(snapshot.availability(), usage.availability().into());
        assert_eq!(snapshot.availability(), Availability::At(resets_at));
        assert_eq!(snapshot.headroom, Some(usage.headroom()));
    }

    #[test]
    fn a_snapshot_is_stale_once_it_is_as_old_as_the_ttl_and_a_missing_one_always_is() {
        let now = Utc::now();
        let ttl = TimeDelta::seconds(60);

        assert!(stale(None, now, ttl));
        assert!(!stale(Some(now - TimeDelta::seconds(59)), now, ttl));
        assert!(stale(Some(now - ttl), now, ttl));
    }
}
