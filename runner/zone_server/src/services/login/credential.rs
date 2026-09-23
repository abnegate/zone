//! The sign-in Zone keeps for an organization's agent, ready for a turn to run with.

use std::path::PathBuf;
use std::sync::LazyLock;

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
use zone_core::llm::AgentKind;
use zone_core::secret::SecretValue;

use super::claude;
use super::locks::Locks;
use crate::config::Config;
use crate::db::agent_logins::{self, AgentLoginRow};
use crate::state::AppState;

/// The longest a task attempt runs, and so the least time a Claude token handed to a turn has to
/// have left.
const MARGIN: TimeDelta = TimeDelta::hours(1);

const UNOPENED: &str = "the stored sign-in could not be opened";

static RENEWALS: LazyLock<Locks> = LazyLock::new(Locks::default);

#[derive(Debug)]
pub enum Login {
    Claude { token: SecretValue },
    Codex { home: PathBuf },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("it has expired, and there is no refresh token to renew it with")]
    Expired,
    #[error("{0}")]
    Renewal(String),
    #[error("{0}")]
    Unreadable(String),
    #[error("could not read the sign-in: {0}")]
    Database(#[from] sqlx::Error),
}

/// The organization's own sign-in for `agent`, renewed first when it is about to expire, or
/// `None` when Zone keeps none for it.
pub async fn resolve(
    state: &AppState,
    organization: Uuid,
    agent: AgentKind,
) -> Result<Option<Login>, Error> {
    match agent {
        AgentKind::Claude => {
            let endpoint = claude::token_endpoint(&state.config().agents.claude_token_url)
                .map_err(|error| Error::Renewal(error.to_string()))?;
            resolve_claude(state, organization, &claude::Client::new(endpoint)).await
        }
        AgentKind::Codex => {
            let login = agent_logins::get(state.db(), organization, agent.as_str()).await?;
            Ok(login.map(|_| Login::Codex {
                home: state.config().agents.home(organization, agent),
            }))
        }
    }
}

/// [`resolve`] for claude, renewing through `client`.
///
/// Only one renewal runs per login, across processes: it holds the row's lock, and a turn
/// that waited for the lock finds the renewed tokens and runs with those. Turns on one server
/// queue for the renewal before they take a connection.
pub(crate) async fn resolve_claude(
    state: &AppState,
    organization: Uuid,
    client: &claude::Client,
) -> Result<Option<Login>, Error> {
    let agent = AgentKind::Claude.as_str();
    let Some(login) = agent_logins::get(state.db(), organization, agent).await? else {
        return Ok(None);
    };
    let margin = margin(state.config());
    let tokens = open(state, &login)?;
    if !renewable(&tokens, Utc::now(), margin) {
        return current(tokens).map(Some);
    }

    let _renewing = RENEWALS.lock(organization).await;
    let mut transaction = state.db().begin().await?;
    let Some(login) = agent_logins::lock(&mut transaction, organization, agent).await? else {
        return Ok(None);
    };
    let tokens = open(state, &login)?;
    if !renewable(&tokens, Utc::now(), margin) {
        return current(tokens).map(Some);
    }
    match client.refresh(&tokens).await {
        Ok(renewed) => {
            keep(state, transaction, organization, login.id, &renewed).await;
            Ok(Some(Login::Claude {
                token: renewed.access,
            }))
        }
        Err(error) => {
            tracing::warn!(
                %organization,
                %error,
                "Could not renew the organization's Claude sign-in"
            );
            current(tokens)
                .map(Some)
                .map_err(|_| Error::Renewal(error.to_string()))
        }
    }
}

/// Stores `renewed` over the login `id` it renews. A write that fails is tried once more on a
/// fresh connection, and the turn runs on `renewed` either way.
async fn keep(
    state: &AppState,
    mut transaction: Transaction<'_, Postgres>,
    organization: Uuid,
    id: Uuid,
    renewed: &claude::Tokens,
) {
    let sealed = match renewed.seal(state.encryption_key()) {
        Ok(sealed) => sealed,
        Err(error) => {
            tracing::error!(%organization, %error, "Could not seal the renewed Claude sign-in");
            return;
        }
    };
    let expires_at = Some(renewed.expires_at);
    let stored = match agent_logins::renew(&mut *transaction, id, &sealed, expires_at).await {
        Ok(()) => transaction.commit().await,
        Err(error) => {
            if let Err(error) = transaction.rollback().await {
                tracing::warn!(%organization, %error, "Could not roll back a failed renewal");
            }
            Err(error)
        }
    };
    let Err(error) = stored else {
        return;
    };
    tracing::error!(
        %organization,
        %error,
        "Could not store the renewed Claude sign-in; trying once more"
    );
    if let Err(error) = agent_logins::renew(state.db(), id, &sealed, expires_at).await {
        tracing::error!(
            %organization,
            %error,
            "Could not store the renewed Claude sign-in; this turn runs on it unstored"
        );
    }
}

fn renewable(tokens: &claude::Tokens, now: DateTime<Utc>, margin: TimeDelta) -> bool {
    tokens.refresh.is_some() && tokens.expiring(now, margin)
}

/// How long a Claude token has to last to be handed to a turn: the longest turn it may be handed
/// to, a task attempt or a chat, whichever the operator allows longer.
fn margin(config: &Config) -> TimeDelta {
    TimeDelta::from_std(config.chat.timeout)
        .unwrap_or(TimeDelta::MAX)
        .max(MARGIN)
}

/// The access token as it is, while it has not expired.
fn current(tokens: claude::Tokens) -> Result<Login, Error> {
    if tokens.expires_at > Utc::now() {
        Ok(Login::Claude {
            token: tokens.access,
        })
    } else {
        Err(Error::Expired)
    }
}

fn open(state: &AppState, login: &AgentLoginRow) -> Result<claude::Tokens, Error> {
    let sealed = login
        .credential
        .as_ref()
        .ok_or_else(|| Error::Unreadable(UNOPENED.to_string()))?;
    claude::Tokens::open(state.encryption_key(), sealed.expose())
        .map_err(|_| Error::Unreadable(UNOPENED.to_string()))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::time::Duration;

    use futures::future::join_all;
    use serde_json::json;
    use sqlx::PgPool;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::config::{AgentConfig, Config};
    use crate::db::agent_logins::Upsert;
    use crate::services::chat::session::Settings;

    const TOKEN_PATH: &str = "/v1/oauth/token";
    const UNREACHABLE: &str = "http://127.0.0.1:1/v1/oauth/token";
    const ACCESS: &str = "fake-access-token";
    const REFRESH: &str = "fake-refresh-token";
    const RENEWED: &str = "fake-renewed-access-token";
    const LIFETIME: i64 = 28_800;
    const TURNS: usize = 5;
    /// A pool small enough for turns waiting on one renewal to exhaust it, were each to hold a
    /// connection while it waits.
    const CONNECTIONS: u32 = 3;
    const ACQUIRE: Duration = Duration::from_secs(2);
    const HUNG: Duration = Duration::from_secs(4);
    const SETTLE: Duration = Duration::from_secs(1);
    /// How long Claude takes to answer a renewal: long enough to drop a connection meanwhile.
    const SLOW: Duration = Duration::from_secs(2);
    const PAUSE: Duration = Duration::from_millis(50);
    const ATTEMPTS: usize = 100;

    struct Fixture {
        state: AppState,
        /// The fixture's own connections, which stay usable when a test breaks the server's.
        pool: PgPool,
        organization: Uuid,
        _agents: TempDir,
    }

    fn database() -> String {
        std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL")
    }

    async fn connect() -> PgPool {
        PgPool::connect(&database())
            .await
            .expect("the test database")
    }

    /// A pool whose connections carry `name`, so a test can find them and drop them.
    async fn named(name: &str) -> PgPool {
        let options = PgConnectOptions::from_str(&database())
            .expect("the test database's URL")
            .application_name(name);
        PgPoolOptions::new()
            .connect_with(options)
            .await
            .expect("the test database")
    }

    /// Drops the connection of the pool `name` that holds a transaction open, as a renewal does
    /// while it waits for Claude.
    async fn drop_renewal(pool: &PgPool, name: &str) {
        for _ in 0..ATTEMPTS {
            let dropped: Option<bool> = sqlx::query_scalar(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
                 WHERE application_name = $1 AND state = 'idle in transaction'",
            )
            .bind(name)
            .fetch_optional(pool)
            .await
            .expect("the server's connections");
            if dropped == Some(true) {
                return;
            }
            tokio::time::sleep(PAUSE).await;
        }
        panic!("the renewal never held a transaction open");
    }

    impl Fixture {
        async fn new(token_url: String) -> Self {
            Self::on(connect().await, token_url).await
        }

        /// A fixture whose server runs on `server`.
        async fn on(server: PgPool, token_url: String) -> Self {
            Self::chatting(server, token_url, Settings::default()).await
        }

        /// A fixture whose server runs on `server` and gives its chats `chat`.
        async fn chatting(server: PgPool, token_url: String, chat: Settings) -> Self {
            let pool = connect().await;
            let organization = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Agent credentials', $1::text)",
            )
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization to sign agents in for");
            let agents = TempDir::new().expect("an agent state root");
            let state = AppState::new(
                Config {
                    agents: AgentConfig {
                        state: agents.path().to_path_buf(),
                        claude_token_url: token_url,
                        ..AgentConfig::default()
                    },
                    chat,
                    ..crate::state::test_config()
                },
                server,
                None,
            );
            Self {
                state,
                pool,
                organization,
                _agents: agents,
            }
        }

        async fn answered_by(server: &MockServer) -> Self {
            Self::new(format!("{}{TOKEN_PATH}", server.uri())).await
        }

        async fn sign_in(&self, tokens: &claude::Tokens) {
            let sealed = tokens
                .seal(self.state.encryption_key())
                .expect("tokens to seal");
            self.store(AgentKind::Claude, Some(&sealed), Some(tokens.expires_at))
                .await;
        }

        async fn store(
            &self,
            agent: AgentKind,
            credential: Option<&str>,
            expires_at: Option<DateTime<Utc>>,
        ) {
            agent_logins::upsert(
                &self.pool,
                &Upsert {
                    organization_id: self.organization,
                    agent: agent.as_str(),
                    credential,
                    label: None,
                    expires_at,
                },
            )
            .await
            .expect("a stored login");
        }

        async fn stored(&self) -> (claude::Tokens, Option<DateTime<Utc>>) {
            let login =
                agent_logins::get(&self.pool, self.organization, AgentKind::Claude.as_str())
                    .await
                    .expect("the login to be readable")
                    .expect("a claude login");
            let sealed = login.credential.expect("a sealed credential");
            let tokens = claude::Tokens::open(self.state.encryption_key(), sealed.expose())
                .expect("tokens Zone sealed");
            (tokens, login.expires_at)
        }

        async fn resolve(&self, agent: AgentKind) -> Result<Option<Login>, Error> {
            resolve(&self.state, self.organization, agent).await
        }

        async fn remove(&self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the organization to be removed");
        }
    }

    fn tokens(expires_at: DateTime<Utc>, refresh: Option<&str>) -> claude::Tokens {
        claude::Tokens {
            access: SecretValue::new(ACCESS),
            refresh: refresh.map(SecretValue::new),
            expires_at,
            scope: "user:inference".to_string(),
            subscription: Some("max".to_string()),
        }
    }

    async fn token_endpoint(response: ResponseTemplate, requests: u64) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(response)
            .expect(requests)
            .mount(&server)
            .await;
        server
    }

    fn renewed() -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({"access_token": RENEWED, "expires_in": LIFETIME}))
    }

    fn refused() -> ResponseTemplate {
        ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"}))
    }

    fn token(login: Result<Option<Login>, Error>) -> String {
        match login {
            Ok(Some(Login::Claude { token })) => token.expose().to_string(),
            other => panic!("expected a claude token, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_claude_login_far_from_expiry_runs_as_it_is() {
        let server = token_endpoint(renewed(), 0).await;
        let fixture = Fixture::answered_by(&server).await;
        fixture
            .sign_in(&tokens(Utc::now() + TimeDelta::hours(2), Some(REFRESH)))
            .await;

        let login = fixture.resolve(AgentKind::Claude).await;
        fixture.remove().await;

        assert_eq!(token(login), ACCESS);
        server.verify().await;
    }

    #[tokio::test]
    async fn a_claude_login_that_would_expire_during_the_longest_turn_is_renewed_first() {
        let server = token_endpoint(renewed(), 1).await;
        let fixture = Fixture::answered_by(&server).await;
        fixture
            .sign_in(&tokens(Utc::now() + TimeDelta::minutes(50), Some(REFRESH)))
            .await;

        let login = fixture.resolve(AgentKind::Claude).await;
        fixture.remove().await;

        assert_eq!(
            token(login),
            RENEWED,
            "a token with 50 minutes left was handed to a turn that may run an hour"
        );
        server.verify().await;
    }

    #[test]
    fn a_token_lasts_the_longer_of_a_task_attempt_and_a_chat() {
        let chatting = |seconds| Config {
            chat: Settings {
                timeout: Duration::from_secs(seconds),
                ..Settings::default()
            },
            ..crate::state::test_config()
        };

        assert_eq!(margin(&chatting(30 * 60)), MARGIN);
        assert_eq!(margin(&chatting(60 * 60)), MARGIN);
        assert_eq!(margin(&chatting(2 * 60 * 60)), TimeDelta::hours(2));
    }

    /// An operator may give a chat longer than a task attempt's hour, and a
    /// token handed to that chat has to last it too.
    #[tokio::test]
    async fn a_claude_login_that_would_expire_during_a_longer_chat_is_renewed_first() {
        let server = token_endpoint(renewed(), 1).await;
        let fixture = Fixture::chatting(
            connect().await,
            format!("{}{TOKEN_PATH}", server.uri()),
            Settings {
                timeout: Duration::from_secs(2 * 60 * 60),
                ..Settings::default()
            },
        )
        .await;
        fixture
            .sign_in(&tokens(Utc::now() + TimeDelta::minutes(90), Some(REFRESH)))
            .await;

        let login = fixture.resolve(AgentKind::Claude).await;
        fixture.remove().await;

        assert_eq!(
            token(login),
            RENEWED,
            "a token with 90 minutes left was handed to a chat that may run two hours"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn turns_waiting_on_a_hung_renewal_hold_no_connections_while_they_wait() {
        let server = token_endpoint(renewed().set_delay(HUNG), 1).await;
        let pool = PgPoolOptions::new()
            .max_connections(CONNECTIONS)
            .acquire_timeout(ACQUIRE)
            .connect(&database())
            .await
            .expect("the test database");
        let fixture = Fixture::on(pool.clone(), format!("{}{TOKEN_PATH}", server.uri())).await;
        fixture
            .sign_in(&tokens(Utc::now() + TimeDelta::minutes(1), Some(REFRESH)))
            .await;

        let turns = join_all((0..TURNS).map(|_| fixture.resolve(AgentKind::Claude)));
        let other_work = async {
            tokio::time::sleep(SETTLE).await;
            sqlx::query("SELECT 1").execute(&pool).await
        };
        let (logins, other_work) = tokio::join!(turns, other_work);
        fixture.remove().await;

        assert!(
            other_work.is_ok(),
            "turns waiting on one renewal took every connection: {other_work:?}"
        );
        for login in logins {
            assert_eq!(token(login), RENEWED);
        }
        server.verify().await;
    }

    #[tokio::test]
    async fn a_renewal_that_cannot_be_stored_still_runs_the_turn_and_is_stored_once_more() {
        let server = token_endpoint(renewed().set_delay(SLOW), 1).await;
        let name = format!("zone-renewal-{}", Uuid::new_v4().simple());
        let fixture =
            Fixture::on(named(&name).await, format!("{}{TOKEN_PATH}", server.uri())).await;
        fixture
            .sign_in(&tokens(Utc::now() + TimeDelta::minutes(1), Some(REFRESH)))
            .await;

        let (login, ()) = tokio::join!(
            fixture.resolve(AgentKind::Claude),
            drop_renewal(&fixture.pool, &name)
        );
        let (stored, _) = fixture.stored().await;
        fixture.remove().await;

        assert_eq!(token(login), RENEWED);
        assert_eq!(
            stored.access.expose(),
            RENEWED,
            "the renewal Claude granted was never stored"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn a_renewal_that_is_never_stored_still_runs_the_turn() {
        let server = token_endpoint(renewed().set_delay(SLOW), 1).await;
        let name = format!("zone-renewal-{}", Uuid::new_v4().simple());
        let fixture =
            Fixture::on(named(&name).await, format!("{}{TOKEN_PATH}", server.uri())).await;
        let current = tokens(Utc::now() + TimeDelta::minutes(1), Some(REFRESH));
        fixture.sign_in(&current).await;

        let (login, ()) = tokio::join!(fixture.resolve(AgentKind::Claude), async {
            drop_renewal(&fixture.pool, &name).await;
            fixture.state.db().close().await;
        });
        let (stored, _) = fixture.stored().await;
        fixture.remove().await;

        assert_eq!(token(login), RENEWED);
        assert_eq!(
            stored, current,
            "a renewal that could not be stored changed the login"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn an_expiring_claude_login_is_renewed_once_however_many_turns_ask_at_once() {
        let server = token_endpoint(renewed().set_delay(Duration::from_millis(300)), 1).await;
        let fixture = Fixture::answered_by(&server).await;
        fixture
            .sign_in(&tokens(Utc::now() + TimeDelta::minutes(1), Some(REFRESH)))
            .await;
        let before = Utc::now();

        let logins = join_all((0..TURNS).map(|_| fixture.resolve(AgentKind::Claude))).await;
        let (stored, expires_at) = fixture.stored().await;
        fixture.remove().await;

        for login in logins {
            assert_eq!(token(login), RENEWED);
        }
        server.verify().await;
        assert_eq!(stored.access.expose(), RENEWED);
        assert_eq!(
            stored.refresh.as_ref().map(SecretValue::expose),
            Some(REFRESH),
            "Claude sent no new refresh token, so the old one stays"
        );
        assert!(
            stored.expires_at >= before + TimeDelta::seconds(LIFETIME),
            "{}",
            stored.expires_at
        );
        let recorded = expires_at.expect("the renewal records its expiry");
        assert!(
            (recorded - stored.expires_at).num_milliseconds() == 0,
            "the row says {recorded}, the tokens {}",
            stored.expires_at
        );
    }

    #[tokio::test]
    async fn a_failed_renewal_before_expiry_keeps_the_current_token() {
        let server = token_endpoint(refused(), 1).await;
        let fixture = Fixture::answered_by(&server).await;
        let current = tokens(Utc::now() + TimeDelta::minutes(2), Some(REFRESH));
        fixture.sign_in(&current).await;

        let login = fixture.resolve(AgentKind::Claude).await;
        let (stored, _) = fixture.stored().await;
        fixture.remove().await;

        assert_eq!(token(login), ACCESS);
        assert_eq!(
            stored, current,
            "a renewal that failed must leave the stored sign-in as it was"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn a_failed_renewal_after_expiry_asks_for_a_new_sign_in() {
        let server = token_endpoint(refused(), 1).await;
        let fixture = Fixture::answered_by(&server).await;
        fixture
            .sign_in(&tokens(Utc::now() - TimeDelta::minutes(1), Some(REFRESH)))
            .await;

        let login = fixture.resolve(AgentKind::Claude).await;
        fixture.remove().await;

        assert!(
            matches!(&login, Err(Error::Renewal(message)) if message.contains("invalid_grant")),
            "{login:?}"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn an_expired_claude_login_with_nothing_to_renew_it_with_has_expired() {
        let server = token_endpoint(renewed(), 0).await;
        let fixture = Fixture::answered_by(&server).await;
        fixture
            .sign_in(&tokens(Utc::now() - TimeDelta::minutes(1), None))
            .await;
        let expiring = Fixture::answered_by(&server).await;
        expiring
            .sign_in(&tokens(Utc::now() + TimeDelta::minutes(1), None))
            .await;

        let expired = fixture.resolve(AgentKind::Claude).await;
        let unrenewable = expiring.resolve(AgentKind::Claude).await;
        fixture.remove().await;
        expiring.remove().await;

        assert!(matches!(expired, Err(Error::Expired)), "{expired:?}");
        assert_eq!(
            token(unrenewable),
            ACCESS,
            "a token with no way to renew it still runs until it expires"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn a_claude_login_zone_cannot_open_is_unreadable() {
        let fixture = Fixture::new(UNREACHABLE.to_string()).await;
        fixture
            .store(AgentKind::Claude, Some("not-a-sealed-credential"), None)
            .await;

        let login = fixture.resolve(AgentKind::Claude).await;
        fixture.remove().await;

        assert!(matches!(login, Err(Error::Unreadable(_))), "{login:?}");
    }

    #[tokio::test]
    async fn an_organization_zone_keeps_no_login_for_has_none() {
        let fixture = Fixture::new(UNREACHABLE.to_string()).await;

        let claude = fixture.resolve(AgentKind::Claude).await;
        let codex = fixture.resolve(AgentKind::Codex).await;
        fixture.remove().await;

        assert!(matches!(claude, Ok(None)), "{claude:?}");
        assert!(matches!(codex, Ok(None)), "{codex:?}");
    }

    #[tokio::test]
    async fn a_codex_login_is_the_organizations_codex_home() {
        let fixture = Fixture::new(UNREACHABLE.to_string()).await;
        fixture.store(AgentKind::Codex, None, None).await;

        let login = fixture.resolve(AgentKind::Codex).await;
        fixture.remove().await;

        let expected = fixture
            .state
            .config()
            .agents
            .home(fixture.organization, AgentKind::Codex);
        match login {
            Ok(Some(Login::Codex { home })) => assert_eq!(home, expected),
            other => panic!("expected the codex home, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_claude_login_renews_through_the_client_it_is_given() {
        let server = token_endpoint(renewed(), 1).await;
        let fixture = Fixture::new(UNREACHABLE.to_string()).await;
        fixture
            .sign_in(&tokens(Utc::now() + TimeDelta::minutes(1), Some(REFRESH)))
            .await;
        let client = claude::Client::new(
            claude::token_endpoint(&format!("{}{TOKEN_PATH}", server.uri()))
                .expect("a loopback token endpoint"),
        );

        let login = resolve_claude(&fixture.state, fixture.organization, &client).await;
        fixture.remove().await;

        assert_eq!(token(login), RENEWED);
        server.verify().await;
    }
}
