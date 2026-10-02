//! Which of an organization's logins a session runs under, and what each turn learns about their
//! usage.

mod chosen;
mod error;
mod standing;

pub use chosen::Chosen;
pub use error::Error;

use std::cmp::Ordering;

use chrono::{DateTime, TimeDelta, Utc};
use futures::future::join_all;
use uuid::Uuid;
use zone_core::llm::{AgentKind, Limit, Window};

use super::credential;
use super::usage::{self, Availability, Snapshot};
use crate::db::agent_logins::{self, AgentLoginRow};
use crate::state::AppState;
use standing::Standing;

/// How long a login a limit refused rests when nothing says when the limit resets.
const REST: TimeDelta = TimeDelta::minutes(5);

/// The login a session of `organization` starts on.
///
/// `sticky`, the login the session already runs on, is kept while it is signed in, not tried
/// this turn and not exhausted, however much headroom another login has. Otherwise every login
/// but those in `exclude` is resolved, its usage brought up to date, and ranked: `preferred`'s
/// logins before every other agent's, and within an agent the most headroom first, then those
/// whose usage is unknown, then the least recently used, then by label. A login that is exhausted
/// or cannot be resolved is never picked.
pub async fn pick(
    state: &AppState,
    organization: Uuid,
    preferred: AgentKind,
    exclude: &[Uuid],
    sticky: Option<Uuid>,
) -> Result<Chosen, Error> {
    let now = Utc::now();
    let logins = agent_logins::list(state.db(), organization).await?;
    if logins.is_empty() {
        return Err(Error::None);
    }
    let mut failure = Failure::new(preferred);

    let mut unresolved = None;
    if let Some(login) = sticky
        .filter(|sticky| !exclude.contains(sticky))
        .and_then(|sticky| logins.iter().find(|login| login.id == sticky))
        && let Some(agent) = AgentKind::named(&login.agent)
        && Standing::of(login, login.snapshot().as_ref(), now).usable()
    {
        match credential::resolve(state, login).await {
            Ok(resolved) => {
                return Ok(Chosen {
                    login: login.clone(),
                    agent,
                    resolved,
                    snapshot: login.snapshot(),
                });
            }
            Err(error) => {
                failure.record(agent, login, error)?;
                unresolved = Some(login.id);
            }
        }
    }

    let mut spent = Vec::new();
    let mut open = Vec::new();
    for login in &logins {
        let Some(agent) = AgentKind::named(&login.agent) else {
            continue;
        };
        if exclude.contains(&login.id) {
            spent.push(standing::marked(login, now));
        } else if let Some(until) = standing::marked(login, now) {
            spent.push(Some(until));
        } else if unresolved != Some(login.id) {
            open.push((agent, login));
        }
    }

    let resolutions = join_all(open.into_iter().map(|(agent, login)| async move {
        (agent, login, credential::resolve(state, login).await)
    }))
    .await;
    let mut candidates = Vec::new();
    for (agent, login, resolution) in resolutions {
        match resolution {
            Ok(resolved) => candidates.push(Chosen {
                login: login.clone(),
                agent,
                resolved,
                snapshot: login.snapshot(),
            }),
            Err(error) => failure.record(agent, login, error)?,
        }
    }

    usage::refresh(state, &mut candidates).await;
    let mut ranked = Vec::with_capacity(candidates.len());
    for chosen in candidates {
        match Standing::of(&chosen.login, chosen.snapshot.as_ref(), now) {
            Standing::Exhausted { until } => spent.push(until),
            standing => ranked.push((standing, chosen)),
        }
    }
    ranked.sort_by(|left, right| order(preferred, left, right));
    if let Some((_, chosen)) = ranked.into_iter().next() {
        return Ok(chosen);
    }
    if !spent.is_empty() {
        return Err(Error::Exhausted {
            resets_at: spent.into_iter().flatten().min(),
        });
    }
    Err(failure.into_error())
}

/// Records that a turn on `login` hit `limit`.
///
/// A limit on paid credits is not the subscription's: the login stays a candidate for every other
/// session, and the caller leaves it out for the rest of the turn. Any other limit exhausts the
/// login until the limit's own reset, else until the login's last spent window resets, else for
/// [`REST`].
pub async fn mark_limited(state: &AppState, login: Uuid, limit: &Limit) {
    let snapshot = match &limit.window {
        Some(window) => observed(state, login, window).await,
        None => agent_logins::get(state.db(), login)
            .await
            .map(|row| row.and_then(|row| row.snapshot())),
    };
    if limit.credits {
        return;
    }
    let snapshot = snapshot.unwrap_or_else(|error| {
        tracing::warn!(%login, %error, "Could not read the login's usage; resting it a while");
        None
    });
    let until = until(limit, snapshot.as_ref(), Utc::now());
    if let Err(error) = agent_logins::exhaust(state.db(), login, until).await {
        tracing::warn!(%login, %error, %until, "Could not mark the login exhausted");
    }
}

/// Records `window`, as a turn on `login` reported it, over the window of the same name in the
/// login's snapshot, without reading the agent's usage.
pub async fn observe(state: &AppState, login: Uuid, window: &Window) {
    if let Err(error) = observed(state, login, window).await {
        tracing::warn!(%login, %error, window = %window.name, "Could not record the login's usage");
    }
}

/// Brings `login`'s usage up to date once a turn on it has ended.
///
/// Stub until usage is read from each agent's service: the stored snapshot is read again.
pub async fn settle(state: &AppState, login: Uuid) {
    match agent_logins::get(state.db(), login).await {
        Ok(row) => tracing::debug!(
            %login,
            snapshot = ?row.and_then(|row| row.snapshot()),
            "Settled the login's usage"
        ),
        Err(error) => tracing::warn!(%login, %error, "Could not read the login's usage"),
    }
}

async fn observed(
    state: &AppState,
    login: Uuid,
    window: &Window,
) -> Result<Option<Snapshot>, sqlx::Error> {
    let mut transaction = state.db().begin().await?;
    let Some(row) = agent_logins::lock(&mut transaction, login).await? else {
        return Ok(None);
    };
    let snapshot = row
        .snapshot()
        .unwrap_or_else(|| Snapshot {
            windows: Vec::new(),
            headroom: None,
            fetched_at: Utc::now(),
        })
        .observed(window.clone());
    agent_logins::observe(&mut *transaction, login, &snapshot).await?;
    transaction.commit().await?;
    Ok(Some(snapshot))
}

/// When a login `limit` refused can run again.
fn until(limit: &Limit, snapshot: Option<&Snapshot>, now: DateTime<Utc>) -> DateTime<Utc> {
    let spent = snapshot.and_then(|snapshot| match snapshot.availability() {
        Availability::At(at) => Some(at),
        Availability::Now | Availability::Unknown => None,
    });
    limit
        .resets_at
        .filter(|at| *at > now)
        .or_else(|| spent.filter(|at| *at > now))
        .unwrap_or(now + REST)
}

fn order(
    preferred: AgentKind,
    (left, left_chosen): &(Standing, Chosen),
    (right, right_chosen): &(Standing, Chosen),
) -> Ordering {
    group(preferred, left_chosen.agent)
        .cmp(&group(preferred, right_chosen.agent))
        .then_with(|| left.rank(*right))
        .then_with(|| {
            left_chosen
                .login
                .last_used_at
                .cmp(&right_chosen.login.last_used_at)
        })
        .then_with(|| left_chosen.login.label.cmp(&right_chosen.login.label))
}

/// Where `agent`'s logins rank: the preferred agent's first, then each other agent's in turn.
fn group(preferred: AgentKind, agent: AgentKind) -> usize {
    if agent == preferred {
        return 0;
    }
    1 + AgentKind::ALL
        .iter()
        .position(|known| *known == agent)
        .unwrap_or(AgentKind::ALL.len())
}

/// Why the logins that could not be resolved were left out: the first of the preferred agent's,
/// else the first of any.
struct Failure {
    preferred: AgentKind,
    first: Option<(AgentKind, credential::Error)>,
}

impl Failure {
    fn new(preferred: AgentKind) -> Self {
        Self {
            preferred,
            first: None,
        }
    }

    /// Leaves `login` out for `error`, unless the store itself failed. A login signed out
    /// meanwhile is gone, and says nothing about the others.
    fn record(
        &mut self,
        agent: AgentKind,
        login: &AgentLoginRow,
        error: credential::Error,
    ) -> Result<(), Error> {
        match error {
            credential::Error::Database(source) => Err(Error::Database(source)),
            credential::Error::Deleted => Ok(()),
            error => {
                tracing::warn!(
                    organization = %login.organization_id,
                    login = %login.id,
                    %agent,
                    %error,
                    "A login cannot be used; routing around it"
                );
                let replaces = match &self.first {
                    None => true,
                    Some((first, _)) => *first != self.preferred && agent == self.preferred,
                };
                if replaces {
                    self.first = Some((agent, error));
                }
                Ok(())
            }
        }
    }

    fn into_error(self) -> Error {
        match self.first {
            Some((agent, source)) => Error::Unresolved { agent, source },
            None => Error::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use abnegate_secret::SecretValue;
    use sqlx::PgPool;
    use tempfile::TempDir;
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::config::{AgentConfig, Config};
    use crate::db::agent_logins::Insert;
    use crate::services::login::claude::Tokens;
    use crate::services::login::credential::Login;

    const UNREACHABLE: &str = "http://127.0.0.1:1/v1/oauth/token";
    const FIVE_HOURS: &str = "5h";
    const SEVEN_DAYS: &str = "7d";

    struct Scene {
        pool: PgPool,
        state: AppState,
        organization: Uuid,
        _agents: TempDir,
    }

    impl Scene {
        async fn new() -> Self {
            Self::reading(UNREACHABLE.to_string()).await
        }

        /// A scene whose agents' usage endpoints are `usage`.
        async fn reading(usage: String) -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
            )
            .await
            .expect("the test database");
            let organization = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO organizations (id, name, slug) VALUES ($1, 'Login routing', $1::text)",
            )
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization to route");
            let agents = TempDir::new().expect("an agent state root");
            let state = AppState::new(
                Config {
                    agents: AgentConfig {
                        state: agents.path().to_path_buf(),
                        claude_token_url: UNREACHABLE.to_string(),
                        claude_api_url: usage.clone(),
                        codex_api_url: usage,
                        host_login: false,
                        ..AgentConfig::default()
                    },
                    ..crate::state::test_config()
                },
                pool.clone(),
                None,
            );
            Self {
                pool,
                state,
                organization,
                _agents: agents,
            }
        }

        /// A login of `agent` labelled `label`, with `headroom` percent left of its five-hour
        /// window when its usage has been read.
        async fn login(&self, agent: AgentKind, label: &str, headroom: Option<f64>) -> Uuid {
            let credential = match agent {
                AgentKind::Claude => Some(self.sealed(Utc::now() + TimeDelta::hours(8))),
                AgentKind::Codex => None,
            };
            let login = self.insert(agent, label, credential.as_deref()).await;
            if let Some(headroom) = headroom {
                self.read(login, headroom, Utc::now() + TimeDelta::hours(2))
                    .await;
            }
            login
        }

        async fn insert(&self, agent: AgentKind, label: &str, credential: Option<&str>) -> Uuid {
            agent_logins::insert(
                &self.pool,
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

        fn sealed(&self, expires_at: DateTime<Utc>) -> String {
            Tokens {
                access: SecretValue::new(format!("access-{}", Uuid::new_v4())),
                refresh: None,
                expires_at,
                issued_at: None,
                scope: "user:inference".to_string(),
                subscription: None,
            }
            .seal(self.state.encryption_key())
            .expect("tokens to seal")
        }

        /// Stores a reading of `login`'s usage with `headroom` percent left of its five-hour
        /// window, which resets at `resets_at`.
        async fn read(&self, login: Uuid, headroom: f64, resets_at: DateTime<Utc>) {
            let snapshot = Snapshot {
                windows: vec![window(FIVE_HOURS, 100.0 - headroom, Some(resets_at))],
                headroom: Some(headroom),
                fetched_at: Utc::now(),
            };
            agent_logins::observe(&self.pool, login, &snapshot)
                .await
                .expect("the login's usage to be stored");
        }

        async fn exhaust(&self, login: Uuid, until: DateTime<Utc>) {
            sqlx::query("UPDATE agent_logins SET exhausted_until = $2 WHERE id = $1")
                .bind(login)
                .bind(until)
                .execute(&self.pool)
                .await
                .expect("the login to be exhausted");
        }

        async fn stored(&self, login: Uuid) -> AgentLoginRow {
            agent_logins::get(&self.pool, login)
                .await
                .expect("the login to be readable")
                .expect("the login to be there")
        }

        async fn pick(
            &self,
            preferred: AgentKind,
            exclude: &[Uuid],
            sticky: Option<Uuid>,
        ) -> Result<Chosen, Error> {
            pick(&self.state, self.organization, preferred, exclude, sticky).await
        }

        async fn picked(
            &self,
            preferred: AgentKind,
            exclude: &[Uuid],
            sticky: Option<Uuid>,
        ) -> Uuid {
            match self.pick(preferred, exclude, sticky).await {
                Ok(chosen) => chosen.login.id,
                Err(error) => panic!("expected a login, got {error:?}"),
            }
        }

        async fn remove(&self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the organization to be removed");
        }
    }

    fn window(name: &str, used_percent: f64, resets_at: Option<DateTime<Utc>>) -> Window {
        Window {
            name: name.to_string(),
            used_percent: Some(used_percent),
            used: None,
            limit: None,
            resets_at,
        }
    }

    fn limit(credits: bool, resets_at: Option<DateTime<Utc>>) -> Limit {
        Limit {
            message: "You've hit your session limit".to_string(),
            resets_at,
            credits,
            window: None,
        }
    }

    #[tokio::test]
    async fn the_login_with_the_most_headroom_starts_the_session() {
        let scene = Scene::new().await;
        let low = scene.login(AgentKind::Claude, "low", Some(20.0)).await;
        let high = scene.login(AgentKind::Claude, "high", Some(70.0)).await;
        let unread = scene.login(AgentKind::Claude, "unread", None).await;

        let first = scene.picked(AgentKind::Claude, &[], None).await;
        let second = scene.picked(AgentKind::Claude, &[high], None).await;
        let third = scene.picked(AgentKind::Claude, &[high, low], None).await;
        let chosen = scene
            .pick(AgentKind::Claude, &[], None)
            .await
            .expect("a login");
        scene.remove().await;

        assert_eq!(first, high);
        assert_eq!(second, low, "a login whose usage is known ranks first");
        assert_eq!(third, unread);
        assert_eq!(chosen.agent, AgentKind::Claude);
        assert_eq!(
            chosen.snapshot.and_then(|snapshot| snapshot.headroom),
            Some(70.0)
        );
        assert!(matches!(chosen.resolved, Login::Claude { .. }));
    }

    #[tokio::test]
    async fn a_sticky_login_keeps_the_chat_while_it_has_headroom() {
        let scene = Scene::new().await;
        scene.login(AgentKind::Claude, "roomy", Some(90.0)).await;
        let sticky = scene.login(AgentKind::Claude, "tight", Some(10.0)).await;

        let picked = scene.picked(AgentKind::Claude, &[], Some(sticky)).await;
        scene.remove().await;

        assert_eq!(picked, sticky, "headroom alone moved a running chat");
    }

    #[tokio::test]
    async fn a_sticky_login_that_another_chat_exhausted_is_left_at_the_next_turn() {
        let scene = Scene::new().await;
        let sticky = scene.login(AgentKind::Claude, "sticky", Some(60.0)).await;
        let other = scene.login(AgentKind::Claude, "other", Some(30.0)).await;
        mark_limited(
            &scene.state,
            sticky,
            &limit(false, Some(Utc::now() + TimeDelta::hours(1))),
        )
        .await;

        let picked = scene.picked(AgentKind::Claude, &[], Some(sticky)).await;
        scene.remove().await;

        assert_eq!(picked, other);
    }

    #[tokio::test]
    async fn a_sticky_login_its_own_usage_says_is_spent_names_its_reset_when_none_remains() {
        let scene = Scene::new().await;
        let resets_at = Utc::now() + TimeDelta::hours(2);
        let sticky = scene.login(AgentKind::Claude, "sticky", None).await;
        scene.read(sticky, 0.0, resets_at).await;

        let alone = scene.pick(AgentKind::Claude, &[], Some(sticky)).await;
        let other = scene.login(AgentKind::Claude, "other", Some(30.0)).await;
        let picked = scene.picked(AgentKind::Claude, &[], Some(sticky)).await;
        scene.remove().await;

        match alone {
            Err(Error::Exhausted {
                resets_at: Some(at),
            }) => assert_eq!(at.timestamp_micros(), resets_at.timestamp_micros()),
            other => panic!("expected the spent sticky login to be named, got {other:?}"),
        }
        assert_eq!(picked, other);
    }

    #[tokio::test]
    async fn the_configured_agents_logins_come_before_another_agents_even_with_less_headroom() {
        let scene = Scene::new().await;
        let claude = scene.login(AgentKind::Claude, "claude", Some(10.0)).await;
        let codex = scene.login(AgentKind::Codex, "codex", Some(90.0)).await;

        let on_claude = scene
            .pick(AgentKind::Claude, &[], None)
            .await
            .expect("a login");
        let on_codex = scene
            .pick(AgentKind::Codex, &[], None)
            .await
            .expect("a login");
        scene.remove().await;

        assert_eq!(
            (on_claude.login.id, on_claude.agent),
            (claude, AgentKind::Claude)
        );
        assert_eq!(
            (on_codex.login.id, on_codex.agent),
            (codex, AgentKind::Codex)
        );
        assert!(matches!(on_codex.resolved, Login::Codex { .. }));
    }

    #[tokio::test]
    async fn another_agents_login_serves_when_the_configured_agent_is_spent() {
        let scene = Scene::new().await;
        let claude = scene.login(AgentKind::Claude, "claude", Some(40.0)).await;
        let codex = scene.login(AgentKind::Codex, "codex", None).await;
        scene
            .exhaust(claude, Utc::now() + TimeDelta::hours(1))
            .await;

        let chosen = scene
            .pick(AgentKind::Claude, &[], Some(claude))
            .await
            .expect("another agent's login");
        scene.remove().await;

        assert_eq!((chosen.login.id, chosen.agent), (codex, AgentKind::Codex));
    }

    #[tokio::test]
    async fn an_exhausted_login_is_skipped_until_its_reset_and_the_reset_is_named_when_none_remains()
     {
        let scene = Scene::new().await;
        let later = Utc::now() + TimeDelta::hours(3);
        let sooner = Utc::now() + TimeDelta::hours(1);
        let marked = scene.login(AgentKind::Claude, "marked", Some(50.0)).await;
        let spent = scene.login(AgentKind::Claude, "spent", None).await;
        mark_limited(&scene.state, marked, &limit(false, Some(later))).await;
        scene.read(spent, 0.0, sooner).await;

        let none_left = scene.pick(AgentKind::Claude, &[], None).await;
        scene
            .exhaust(marked, Utc::now() - TimeDelta::seconds(1))
            .await;
        let reset = scene.picked(AgentKind::Claude, &[], None).await;
        scene.remove().await;

        match none_left {
            Err(Error::Exhausted {
                resets_at: Some(resets_at),
            }) => assert_eq!(
                resets_at.timestamp_micros(),
                sooner.timestamp_micros(),
                "the earliest reset is named"
            ),
            other => panic!("expected every login to be exhausted, got {other:?}"),
        }
        assert_eq!(reset, marked);
    }

    #[tokio::test]
    async fn a_login_tried_this_turn_is_not_picked_again() {
        let scene = Scene::new().await;
        let tried = scene.login(AgentKind::Claude, "tried", Some(90.0)).await;
        let other = scene.login(AgentKind::Claude, "other", Some(10.0)).await;

        let next = scene.picked(AgentKind::Claude, &[tried], Some(tried)).await;
        let none = scene.pick(AgentKind::Claude, &[tried, other], None).await;
        scene.remove().await;

        assert_eq!(next, other);
        assert!(
            matches!(none, Err(Error::Exhausted { resets_at: None })),
            "{none:?}"
        );
    }

    #[tokio::test]
    async fn a_login_whose_token_cannot_be_renewed_is_never_picked() {
        let scene = Scene::new().await;
        let expired = scene
            .insert(
                AgentKind::Claude,
                "expired",
                Some(&scene.sealed(Utc::now() - TimeDelta::minutes(1))),
            )
            .await;
        scene
            .read(expired, 95.0, Utc::now() + TimeDelta::hours(1))
            .await;

        let alone = scene.pick(AgentKind::Claude, &[], Some(expired)).await;
        let current = scene.login(AgentKind::Claude, "current", Some(5.0)).await;
        let picked = scene.picked(AgentKind::Claude, &[], Some(expired)).await;
        scene.remove().await;

        assert!(
            matches!(
                alone,
                Err(Error::Unresolved {
                    agent: AgentKind::Claude,
                    source: credential::Error::Expired,
                })
            ),
            "{alone:?}"
        );
        assert_eq!(picked, current);
    }

    #[tokio::test]
    async fn a_credits_limit_excludes_the_login_for_the_turn_but_does_not_exhaust_it() {
        let scene = Scene::new().await;
        let unfunded = scene.login(AgentKind::Claude, "unfunded", Some(80.0)).await;
        let other = scene.login(AgentKind::Claude, "other", Some(20.0)).await;

        mark_limited(
            &scene.state,
            unfunded,
            &limit(true, Some(Utc::now() + TimeDelta::hours(1))),
        )
        .await;
        let stored = scene.stored(unfunded).await;
        let this_turn = scene
            .picked(AgentKind::Claude, &[unfunded], Some(unfunded))
            .await;
        let next_session = scene.picked(AgentKind::Claude, &[], None).await;
        scene.remove().await;

        assert_eq!(stored.exhausted_until, None);
        assert_eq!(this_turn, other);
        assert_eq!(next_session, unfunded);
    }

    #[tokio::test]
    async fn observing_a_window_from_the_stream_updates_the_snapshot_without_a_fetch() {
        let usage = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&usage)
            .await;
        let scene = Scene::reading(usage.uri()).await;
        let read = scene.login(AgentKind::Claude, "read", Some(38.0)).await;
        let unread = scene.login(AgentKind::Codex, "unread", None).await;
        let before = scene
            .stored(read)
            .await
            .snapshot()
            .expect("the login's usage was read");
        let resets_at = Utc::now() + TimeDelta::days(5);

        observe(
            &scene.state,
            read,
            &window(SEVEN_DAYS, 31.0, Some(resets_at)),
        )
        .await;
        observe(
            &scene.state,
            read,
            &window(FIVE_HOURS, 95.0, before.windows[0].resets_at),
        )
        .await;
        observe(&scene.state, unread, &window(FIVE_HOURS, 12.0, None)).await;
        let read = scene.stored(read).await.snapshot().expect("a snapshot");
        let unread = scene.stored(unread).await.snapshot().expect("a snapshot");
        let picked = scene
            .pick(AgentKind::Claude, &[], None)
            .await
            .expect("a login");
        scene.remove().await;

        assert_eq!(
            read.windows
                .iter()
                .map(|window| (window.name.as_str(), window.used_percent))
                .collect::<Vec<_>>(),
            [(FIVE_HOURS, Some(95.0)), (SEVEN_DAYS, Some(31.0))]
        );
        let headroom = read.headroom.expect("a headroom");
        assert!((headroom - 5.0).abs() < f64::EPSILON, "{headroom}");
        assert_eq!(
            read.fetched_at.timestamp_micros(),
            before.fetched_at.timestamp_micros(),
            "an observed window is not a fresh reading of the others"
        );
        assert_eq!(unread.headroom, Some(88.0));
        assert_eq!(picked.agent, AgentKind::Claude);
        usage.verify().await;
    }

    #[test]
    fn a_limit_exhausts_until_its_own_reset_else_the_snapshots_else_a_rest() {
        let now = Utc::now();
        let own = now + TimeDelta::hours(1);
        let spent = now + TimeDelta::hours(4);
        let snapshot = Snapshot {
            windows: vec![window(FIVE_HOURS, 100.0, Some(spent))],
            headroom: Some(0.0),
            fetched_at: now,
        };
        let passed = Snapshot {
            windows: vec![window(FIVE_HOURS, 100.0, Some(now - TimeDelta::hours(1)))],
            ..snapshot.clone()
        };

        assert_eq!(until(&limit(false, Some(own)), Some(&snapshot), now), own);
        assert_eq!(until(&limit(false, None), Some(&snapshot), now), spent);
        assert_eq!(
            until(
                &limit(false, Some(now - TimeDelta::minutes(1))),
                Some(&snapshot),
                now
            ),
            spent,
            "a reset that has passed says nothing"
        );
        assert_eq!(until(&limit(false, None), Some(&passed), now), now + REST);
        assert_eq!(until(&limit(false, None), None, now), now + REST);
    }
}
