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

use super::credential::{self, Login};
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

/// `chosen`'s usage read from its agent now, however recently it was read, and stored; `None`
/// when it cannot be read. Every refresh of the login shares this reading from now on.
pub async fn reread(state: &AppState, chosen: &Chosen) -> Option<Snapshot> {
    let credential = readable(chosen)?;
    let ttl = state.config().agents.usage_ttl;
    let reading = land(
        state.clone(),
        chosen.login.id,
        chosen.agent,
        credential,
        chosen.login.snapshot(),
    );
    board(chosen.login.id, ttl, true, || Flight::start(reading))
        .landed()
        .await
}

/// The time a snapshot no reading of the agent began is taken at: as long ago as the TTL, so the
/// next refresh reads the login's usage rather than routing on the windows a turn reported.
pub fn unread_at(state: &AppState, now: DateTime<Utc>) -> DateTime<Utc> {
    now.checked_sub_signed(ttl(state))
        .unwrap_or(DateTime::UNIX_EPOCH)
}

/// A fresh snapshot of `chosen`'s usage, or `None` when none could be had.
async fn read(state: &AppState, chosen: &Chosen) -> Option<Snapshot> {
    let credential = readable(chosen)?;
    let login = chosen.login.id;
    let ttl = state.config().agents.usage_ttl;
    board(login, ttl, false, || {
        Flight::start(fly(state.clone(), login, chosen.agent, credential))
    })
    .landed()
    .await
}

/// The token `chosen`'s usage is read with, or `None`, said why, when it has none.
fn readable(chosen: &Chosen) -> Option<Credential> {
    credential(&chosen.resolved)
        .inspect_err(|error| {
            tracing::warn!(
                login = %chosen.login.id,
                agent = %chosen.agent,
                %error,
                "Could not read the login's usage; routing on the snapshot it had"
            );
        })
        .ok()
}

/// The reading of `login`'s usage a refresh waits on: the one under way or started within `ttl`,
/// unless `anew`, else the one `start` starts. Readings past their TTL are let go of first.
fn board(login: Uuid, ttl: Duration, anew: bool, start: impl FnOnce() -> Flight) -> Flight {
    FLIGHTS.retain(|_, flight| flight.current(ttl));
    match FLIGHTS.entry(login) {
        Entry::Occupied(entry) if !anew && entry.get().current(ttl) => entry.get().clone(),
        entry => {
            let flight = start();
            entry.insert(flight.clone());
            flight
        }
    }
}

/// Reads `login`'s usage and stores it, unless another server stored a fresh reading meanwhile.
async fn fly(
    state: AppState,
    login: Uuid,
    agent: AgentKind,
    credential: Credential,
) -> Option<Snapshot> {
    let prior = match agent_logins::get(state.db(), login).await {
        Ok(None) => return None,
        Ok(Some(row)) => row.snapshot(),
        Err(error) => {
            tracing::warn!(%login, %error, "Could not read the login's stored usage");
            None
        }
    };
    if let Some(fresh) = prior
        .as_ref()
        .filter(|snapshot| !stale(Some(snapshot.fetched_at), Utc::now(), ttl(&state)))
    {
        return Some(fresh.clone());
    }
    land(state, login, agent, credential, prior).await
}

/// Reads `login`'s usage from its agent and stores it over `prior`, the snapshot stored when the
/// reading began.
async fn land(
    state: AppState,
    login: Uuid,
    agent: AgentKind,
    credential: Credential,
    prior: Option<Snapshot>,
) -> Option<Snapshot> {
    match fetch(&state.config().agents, agent, &credential).await {
        Ok(snapshot) => match store(&state, login, prior.as_ref(), snapshot.clone()).await {
            Ok(stored) => stored,
            Err(error) => {
                tracing::warn!(%login, %error, "Could not store the login's usage");
                Some(snapshot)
            }
        },
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

/// Stores `reading` over the snapshot `login` has now, which `prior` was when the reading began,
/// returning the snapshot the login is left with: a reading another server began later stays,
/// and so does every window a turn observed meanwhile. `None` when the login is gone.
async fn store(
    state: &AppState,
    login: Uuid,
    prior: Option<&Snapshot>,
    reading: Snapshot,
) -> Result<Option<Snapshot>, sqlx::Error> {
    let _held = credential::hold(login).await;
    let mut transaction = state.db().begin().await?;
    let Some(row) = agent_logins::lock(&mut transaction, login).await? else {
        return Ok(None);
    };
    let snapshot = match row.snapshot() {
        Some(stored) if stored.fetched_at > reading.fetched_at => return Ok(Some(stored)),
        Some(stored) => reading.under(prior, &stored),
        None => reading,
    };
    agent_logins::observe(&mut *transaction, login, &snapshot).await?;
    transaction.commit().await?;
    Ok(Some(snapshot))
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

    #[tokio::test]
    async fn a_reading_past_its_ttl_is_let_go_of_once_another_login_is_read() {
        let (spent, current) = (Uuid::new_v4(), Uuid::new_v4());
        board(spent, Duration::ZERO, false, || {
            Flight::start(async { None })
        })
        .landed()
        .await;

        board(current, Duration::ZERO, false, || {
            Flight::start(std::future::pending())
        });

        assert!(
            !FLIGHTS.contains_key(&spent),
            "a login's last reading was kept forever"
        );
        assert!(FLIGHTS.contains_key(&current));
    }

    #[tokio::test]
    async fn a_forced_reading_replaces_the_one_a_refresh_would_share() {
        let login = Uuid::new_v4();
        let ttl = Duration::from_secs(60);
        let shared = Snapshot::new(Vec::new(), at("2026-09-23T06:10:00Z"));
        let forced = Snapshot::new(Vec::new(), at("2026-09-23T06:20:00Z"));
        board(login, ttl, false, {
            let shared = shared.clone();
            move || Flight::start(async move { Some(shared) })
        })
        .landed()
        .await;

        let again = board(login, ttl, false, || Flight::start(async { None }))
            .landed()
            .await;
        let anew = board(login, ttl, true, {
            let forced = forced.clone();
            move || Flight::start(async move { Some(forced) })
        })
        .landed()
        .await;

        assert_eq!(again, Some(shared));
        assert_eq!(anew, Some(forced));
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
