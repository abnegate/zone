//! The sign-in Zone keeps for an organization's agent, ready for a turn to run with.

use std::path::PathBuf;
use std::sync::LazyLock;

use chrono::{DateTime, TimeDelta, Utc};
use uuid::Uuid;
use zone_core::llm::AgentKind;
use zone_core::secret::SecretValue;

use super::claude;
use super::locks::Locks;
use crate::db::agent_logins::{self, AgentLoginRow};
use crate::state::AppState;

/// A Claude token is renewed once it would run out within the longest turn it is handed to.
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
    let tokens = open(state, &login)?;
    if !renewable(&tokens, Utc::now()) {
        return current(tokens).map(Some);
    }

    let _renewing = RENEWALS.lock(organization).await;
    let mut transaction = state.db().begin().await?;
    let Some(login) = agent_logins::lock(&mut transaction, organization, agent).await? else {
        return Ok(None);
    };
    let tokens = open(state, &login)?;
    if !renewable(&tokens, Utc::now()) {
        return current(tokens).map(Some);
    }
    match client.refresh(&tokens).await {
        Ok(renewed) => {
            let sealed = renewed
                .seal(state.encryption_key())
                .map_err(|error| Error::Renewal(error.to_string()))?;
            agent_logins::renew(
                &mut *transaction,
                login.id,
                &sealed,
                Some(renewed.expires_at),
            )
            .await?;
            transaction.commit().await?;
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

fn renewable(tokens: &claude::Tokens, now: DateTime<Utc>) -> bool {
    tokens.refresh.is_some() && tokens.expiring(now, MARGIN)
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
    use std::time::Duration;

    use futures::future::join_all;
    use serde_json::json;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::config::{AgentConfig, Config};
    use crate::db::agent_logins::Upsert;

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

    struct Fixture {
        state: AppState,
        organization: Uuid,
        _agents: TempDir,
    }

    fn database() -> String {
        std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL")
    }

    impl Fixture {
        async fn new(token_url: String) -> Self {
            let pool = PgPool::connect(&database())
                .await
                .expect("the test database");
            Self::on(pool, token_url).await
        }

        async fn on(pool: PgPool, token_url: String) -> Self {
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
                    ..crate::state::test_config()
                },
                pool,
                None,
            );
            Self {
                state,
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
                self.state.db(),
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
            let login = agent_logins::get(
                self.state.db(),
                self.organization,
                AgentKind::Claude.as_str(),
            )
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
                .execute(self.state.db())
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
