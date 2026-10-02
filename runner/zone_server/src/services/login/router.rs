//! Which of an organization's logins a session runs under, and what each turn learns about their
//! usage.

mod chosen;
mod error;
mod failure;
mod observed;
mod standing;
pub mod unfunded;

pub use chosen::Chosen;
pub use error::Error;
pub use observed::Observed;

use std::cmp::Ordering;

use chrono::{DateTime, TimeDelta, Utc};
use futures::future::join_all;
use uuid::Uuid;
use zone_core::llm::{AgentKind, Limit, Window};

use super::credential;
use super::usage::{self, Availability, Snapshot};
use crate::db::agent_logins;
use crate::state::AppState;
use failure::Failure;
use standing::Standing;

/// How long a login a limit refused rests when nothing says when the limit resets.
const REST: TimeDelta = TimeDelta::minutes(5);

/// The login a session of `organization` starts on.
///
/// `sticky`, the login the session already runs on, is kept while it is signed in, not tried
/// this turn and not exhausted, however much headroom another login has, as long as it runs
/// `preferred`, the configured agent, or no login of `preferred` can run the session. Otherwise
/// every login but those in `exclude` is resolved, its usage brought up to date, and ranked:
/// `preferred`'s logins before every other agent's, and within an agent the most headroom first,
/// then those whose usage is unknown, then the least recently used, then by label. A login that
/// is exhausted or cannot be resolved is never picked.
///
/// A login that refused `model`, the one the session runs on `preferred`, for want of usage
/// credits within [`unfunded::COOL_DOWN`] cannot run the session: it ranks after every other
/// login, and is picked only when no other can be, even as `sticky`.
pub async fn pick(
    state: &AppState,
    organization: Uuid,
    preferred: AgentKind,
    exclude: &[Uuid],
    sticky: Option<Uuid>,
    model: Option<&str>,
) -> Result<Chosen, Error> {
    let now = Utc::now();
    let logins = agent_logins::list(state.db(), organization).await?;
    if logins.is_empty() {
        return Err(Error::None);
    }
    let mut failure = Failure::new(preferred);
    let sticky = sticky.filter(|sticky| !exclude.contains(sticky));
    let cooling = model
        .map(|model| unfunded::cooling(model, now))
        .unwrap_or_default();

    let mut unresolved = None;
    if let Some(login) = sticky.and_then(|sticky| logins.iter().find(|login| login.id == sticky))
        && AgentKind::named(&login.agent) == Some(preferred)
        && !cooling.contains(&login.id)
        && Standing::of(login, login.snapshot().as_ref(), now).usable()
    {
        match credential::resolve(state, login).await {
            Ok(resolved) => {
                return Ok(Chosen {
                    login: login.clone(),
                    agent: preferred,
                    resolved,
                    snapshot: login.snapshot(),
                });
            }
            Err(error) => {
                failure.record(preferred, login, error)?;
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
    let cooled = |(_, chosen): &(Standing, Chosen)| cooling.contains(&chosen.login.id);
    ranked.sort_by(|left, right| {
        cooled(left)
            .cmp(&cooled(right))
            .then_with(|| order(preferred, left, right))
    });
    let kept = match ranked.first() {
        Some((_, best)) if best.agent != preferred => ranked
            .iter()
            .position(|entry| Some(entry.1.login.id) == sticky && !cooled(entry))
            .unwrap_or(0),
        _ => 0,
    };
    if kept < ranked.len() {
        return Ok(ranked.swap_remove(kept).1);
    }
    if !spent.is_empty() {
        return Err(Error::Exhausted {
            resets_at: spent.into_iter().flatten().min(),
        });
    }
    Err(failure.into_error())
}

/// Records that a turn on `login`, running `model`, hit `limit`.
///
/// A limit on paid credits is not the subscription's, and refuses only the model: the login stays
/// a candidate for every session on another model, the caller leaves it out for the rest of the
/// turn, and sessions on `model` pass it over for [`unfunded::COOL_DOWN`]. Any other limit
/// exhausts the login until the limit's own reset, else until the login's last spent window
/// resets, else, when a reading of the agent's usage made now names no spent window either, for
/// [`REST`].
pub async fn mark_limited(state: &AppState, login: Uuid, limit: &Limit, model: &str) {
    let snapshot = match &limit.window {
        Some(window) => observed(state, login, std::slice::from_ref(window)).await,
        None => agent_logins::get(state.db(), login)
            .await
            .map(|row| row.and_then(|row| row.snapshot())),
    };
    if limit.credits {
        unfunded::record(login, model, Utc::now());
        return;
    }
    let snapshot = snapshot.unwrap_or_else(|error| {
        tracing::warn!(%login, %error, "Could not read the login's usage; reading it again");
        None
    });
    let now = Utc::now();
    let until = match reset(limit, snapshot.as_ref(), now) {
        Some(reset) => reset,
        None => self::until(limit, reread(state, login).await.as_ref(), now),
    };
    let _held = credential::hold(login).await;
    if let Err(error) = agent_logins::exhaust(state.db(), login, until).await {
        tracing::warn!(%login, %error, %until, "Could not mark the login exhausted");
    }
}

/// Records `windows`, as a turn on `login` reported them, over the windows of the same names in
/// the login's snapshot in one write, without reading the agent's usage.
pub async fn observe(state: &AppState, login: Uuid, windows: &[Window]) {
    if windows.is_empty() {
        return;
    }
    if let Err(error) = observed(state, login, windows).await {
        tracing::warn!(%login, %error, "Could not record the login's usage");
    }
}

/// Brings `login`'s usage up to date once a turn on it has ended, as [`usage::refresh`] does: read
/// again from the agent when the stored snapshot is older than the TTL, and kept as it is when it
/// cannot be read.
///
/// A codex login is resolved into its own home first, which takes the organization's device lock.
pub async fn settle(state: &AppState, login: Uuid) {
    if let Some(chosen) = chosen(state, login).await {
        usage::refresh(state, &mut [chosen]).await;
    }
}

/// `login`'s usage read from its agent now, however recently it was read, or `None` when it
/// cannot be.
async fn reread(state: &AppState, login: Uuid) -> Option<Snapshot> {
    usage::reread(state, &chosen(state, login).await?).await
}

/// `login`, resolved for a reading of its usage, or `None`, said why, when it cannot be.
async fn chosen(state: &AppState, login: Uuid) -> Option<Chosen> {
    let row = match agent_logins::get(state.db(), login).await {
        Ok(row) => row?,
        Err(error) => {
            tracing::warn!(%login, %error, "Could not read the login to read its usage");
            return None;
        }
    };
    let agent = AgentKind::named(&row.agent)?;
    let resolved = match credential::resolve(state, &row).await {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::warn!(%login, %agent, %error, "Could not read the login's usage");
            return None;
        }
    };
    let snapshot = row.snapshot();
    Some(Chosen {
        login: row,
        agent,
        resolved,
        snapshot,
    })
}

/// Writes `windows` over `login`'s snapshot. A login whose usage was never read starts from a
/// snapshot that is already due a reading, since the windows a turn reports say nothing of the
/// others.
async fn observed(
    state: &AppState,
    login: Uuid,
    windows: &[Window],
) -> Result<Option<Snapshot>, sqlx::Error> {
    let _held = credential::hold(login).await;
    let mut transaction = state.db().begin().await?;
    let Some(row) = agent_logins::lock(&mut transaction, login).await? else {
        return Ok(None);
    };
    let unread = || Snapshot::new(Vec::new(), usage::unread_at(state, Utc::now()));
    let snapshot = windows
        .iter()
        .cloned()
        .fold(row.snapshot().unwrap_or_else(unread), Snapshot::observed);
    agent_logins::observe(&mut *transaction, login, &snapshot).await?;
    transaction.commit().await?;
    Ok(Some(snapshot))
}

/// When a login `limit` refused can run again, else after [`REST`].
fn until(limit: &Limit, snapshot: Option<&Snapshot>, now: DateTime<Utc>) -> DateTime<Utc> {
    reset(limit, snapshot, now).unwrap_or(now + REST)
}

/// When a login `limit` refused can run again, by the limit's own reset, else by the last of the
/// spent windows in `snapshot`; `None` when neither says.
fn reset(limit: &Limit, snapshot: Option<&Snapshot>, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let spent = snapshot.and_then(|snapshot| match snapshot.availability() {
        Availability::At(at) => Some(at),
        Availability::Now | Availability::Unknown => None,
    });
    limit
        .resets_at
        .filter(|at| *at > now)
        .or_else(|| spent.filter(|at| *at > now))
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use abnegate_secret::SecretValue;
    use chrono::SubsecRound;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tempfile::TempDir;
    use wiremock::matchers::{any, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::config::{AgentConfig, Config};
    use crate::db::agent_logins::{AgentLoginRow, Insert};
    use crate::services::login::claude::Tokens;
    use crate::services::login::credential::Login;

    const UNREACHABLE: &str = "http://127.0.0.1:1/v1/oauth/token";
    const FIVE_HOURS: &str = Window::FIVE_HOURS;
    const SEVEN_DAYS: &str = Window::SEVEN_DAYS;
    const CLAUDE_USAGE: &str = "/api/oauth/usage";
    const CODEX_USAGE: &str = "/backend-api/wham/usage";
    const READING: &str = r#"{"five_hour":{"utilization":62.0,"resets_at":"2026-09-23T06:10:00Z"},"seven_day":{"utilization":31.0,"resets_at":"2026-09-28T04:00:00Z"}}"#;
    const READ_HEADROOM: f64 = 38.0;
    const MODEL: &str = "opus";
    const OTHER_MODEL: &str = "sonnet";

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
            Self::pooled(usage, PgPoolOptions::new()).await
        }

        /// [`Scene::reading`], on a pool made with `options`.
        async fn pooled(usage: String, options: PgPoolOptions) -> Self {
            let pool = options
                .connect(&std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"))
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

        /// Signs the codex CLI in inside `login`'s home, as a codex sign-in leaves it.
        fn signed_in(&self, login: Uuid) {
            let home = self
                .state
                .config()
                .agents
                .create_login_home(self.organization, AgentKind::Codex, login)
                .expect("the login's home");
            std::fs::write(
                home.join("auth.json"),
                r#"{"tokens":{"access_token":"codex-access","refresh_token":"codex-refresh","account_id":"acct-42"}}"#,
            )
            .expect("the codex sign-in");
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
            self.read_at(login, headroom, resets_at, Utc::now()).await;
        }

        /// [`Scene::read`], as read at `fetched_at`.
        async fn read_at(
            &self,
            login: Uuid,
            headroom: f64,
            resets_at: DateTime<Utc>,
            fetched_at: DateTime<Utc>,
        ) {
            let snapshot = Snapshot {
                windows: vec![window(FIVE_HOURS, 100.0 - headroom, Some(resets_at))],
                headroom: Some(headroom),
                fetched_at,
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
            pick(
                &self.state,
                self.organization,
                preferred,
                exclude,
                sticky,
                None,
            )
            .await
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

        /// [`Scene::picked`] for a session that runs `model` on `preferred`.
        async fn running(
            &self,
            model: &str,
            preferred: AgentKind,
            exclude: &[Uuid],
            sticky: Option<Uuid>,
        ) -> Uuid {
            match pick(
                &self.state,
                self.organization,
                preferred,
                exclude,
                sticky,
                Some(model),
            )
            .await
            {
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
            MODEL,
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
        mark_limited(&scene.state, marked, &limit(false, Some(later)), MODEL).await;
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
            MODEL,
        )
        .await;
        let stored = scene.stored(unfunded).await;
        let this_turn = scene
            .picked(AgentKind::Claude, &[unfunded], Some(unfunded))
            .await;
        let next_session = scene.picked(AgentKind::Claude, &[], None).await;
        let another_model = scene
            .running(OTHER_MODEL, AgentKind::Claude, &[], Some(unfunded))
            .await;
        let same_model = scene
            .running(MODEL, AgentKind::Claude, &[], Some(unfunded))
            .await;
        scene.remove().await;

        assert_eq!(stored.exhausted_until, None);
        assert_eq!(this_turn, other);
        assert_eq!(next_session, unfunded);
        assert_eq!(
            another_model, unfunded,
            "a credits refusal is the model's alone"
        );
        assert_eq!(
            same_model, other,
            "a session on the refused model is not sent back to the login that refused it"
        );
    }

    #[tokio::test]
    async fn a_chat_a_credits_refusal_moved_to_another_agent_stays_there_until_the_cool_down_passes()
     {
        let scene = Scene::new().await;
        let refused = scene.login(AgentKind::Claude, "refused", Some(80.0)).await;
        let fallback = scene.login(AgentKind::Codex, "fallback", None).await;
        mark_limited(&scene.state, refused, &limit(true, None), MODEL).await;

        let stays = scene
            .running(MODEL, AgentKind::Claude, &[], Some(fallback))
            .await;
        let starts = scene.running(MODEL, AgentKind::Claude, &[], None).await;
        let alone = scene
            .running(MODEL, AgentKind::Claude, &[fallback], None)
            .await;
        unfunded::record(
            refused,
            MODEL,
            Utc::now() - unfunded::COOL_DOWN - TimeDelta::seconds(1),
        );
        let returns = scene
            .running(MODEL, AgentKind::Claude, &[], Some(fallback))
            .await;
        let stored = scene.stored(refused).await;
        scene.remove().await;

        assert_eq!(
            stays, fallback,
            "the chat went back to the login that refused its model"
        );
        assert_eq!(starts, fallback, "a new session on the model avoids it too");
        assert_eq!(
            alone, refused,
            "a login that refused the model still runs it when no other can"
        );
        assert_eq!(returns, refused, "the cool-down passed");
        assert_eq!(stored.exhausted_until, None);
    }

    #[tokio::test]
    async fn a_chat_on_a_login_that_refused_its_model_moves_to_another_agent_that_can_run_it() {
        let scene = Scene::new().await;
        let refused = scene.login(AgentKind::Claude, "refused", Some(80.0)).await;
        mark_limited(&scene.state, refused, &limit(true, None), MODEL).await;

        let alone = scene
            .running(MODEL, AgentKind::Claude, &[], Some(refused))
            .await;
        let fallback = scene.login(AgentKind::Codex, "fallback", None).await;
        let moves = scene
            .running(MODEL, AgentKind::Claude, &[], Some(refused))
            .await;
        scene.remove().await;

        assert_eq!(
            moves, fallback,
            "the chat stayed on the login that refused its model while another agent could run it"
        );
        assert_eq!(
            alone, refused,
            "a login that refused the model still keeps the chat when no other can run it"
        );
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
            &[
                window(SEVEN_DAYS, 31.0, Some(resets_at)),
                window(FIVE_HOURS, 95.0, before.windows[0].resets_at),
            ],
        )
        .await;
        observe(&scene.state, unread, &[window(FIVE_HOURS, 12.0, None)]).await;
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

    /// The windows claude's stream reports in `infos`, as the agent's reader reads them.
    fn streamed(infos: &[serde_json::Value]) -> Vec<Window> {
        let mut reader = AgentKind::Claude.reader();
        let mut events = Vec::new();
        for info in infos {
            let line = serde_json::json!({"type": "rate_limit_event", "rate_limit_info": info});
            reader.interpret(&line.to_string(), &mut events);
        }
        events
            .into_iter()
            .filter_map(|event| match event {
                zone_core::llm::provider::AgentEvent::Window(window) => Some(window),
                _ => None,
            })
            .collect()
    }

    fn used(snapshot: &Snapshot) -> Vec<(&str, Option<f64>)> {
        snapshot
            .windows
            .iter()
            .map(|window| (window.name.as_str(), window.used_percent))
            .collect()
    }

    #[tokio::test]
    async fn a_window_the_stream_reports_lands_on_the_window_the_usage_endpoint_named() {
        let usage = answering(Duration::ZERO).await;
        let scene = Scene::reading(usage.uri()).await;
        let login = scene.login(AgentKind::Claude, "read", None).await;
        scene.picked(AgentKind::Claude, &[], None).await;

        observe(
            &scene.state,
            login,
            &streamed(&[
                serde_json::json!({"status": "allowed", "rateLimitType": "five_hour", "utilization": 0.9, "isUsingOverage": false}),
                serde_json::json!({"status": "allowed_warning", "rateLimitType": "seven_day", "utilization": 0.4, "isUsingOverage": false}),
            ]),
        )
        .await;
        let stored = scene.stored(login).await.snapshot().expect("a snapshot");
        scene.remove().await;

        assert_eq!(
            used(&stored),
            [(FIVE_HOURS, Some(90.0)), (SEVEN_DAYS, Some(40.0))],
            "one window of each name, whoever reported it"
        );
        let headroom = stored.headroom.expect("a headroom");
        assert!((headroom - 10.0).abs() < 1e-9, "{headroom}");
        assert_eq!(readings(&usage).await, 1);
    }

    #[tokio::test]
    async fn a_snapshot_a_streamed_window_starts_is_due_a_reading() {
        let usage = answering(Duration::ZERO).await;
        let scene = Scene::reading(usage.uri()).await;
        let login = scene.login(AgentKind::Claude, "unread", None).await;

        observe(&scene.state, login, &[window(FIVE_HOURS, 90.0, None)]).await;
        let observed = scene.stored(login).await.snapshot().expect("a snapshot");
        let chosen = scene.pick(AgentKind::Claude, &[], None).await;
        let stored = scene.stored(login).await.snapshot().expect("a snapshot");
        scene.remove().await;

        assert_eq!(observed.headroom, Some(10.0));
        assert_eq!(
            readings(&usage).await,
            1,
            "a window from the stream held the reading of the others off for a whole TTL"
        );
        assert_eq!(headroom(&chosen), Some(READ_HEADROOM));
        assert_eq!(
            used(&stored),
            [(FIVE_HOURS, Some(62.0)), (SEVEN_DAYS, Some(31.0))]
        );
    }

    #[tokio::test]
    async fn a_usage_write_waits_out_a_renewal_without_holding_a_connection() {
        let scene = Scene::pooled(
            UNREACHABLE.to_string(),
            PgPoolOptions::new()
                .max_connections(3)
                .acquire_timeout(Duration::from_secs(2)),
        )
        .await;
        let login = scene.login(AgentKind::Claude, "renewing", None).await;
        let renewing = credential::hold(login).await;
        let mut renewal = scene.pool.begin().await.expect("a renewal's transaction");
        agent_logins::lock(&mut renewal, login)
            .await
            .expect("the renewal's row lock")
            .expect("the login");

        let writes = (0..4)
            .map(|used| {
                let state = scene.state.clone();
                tokio::spawn(async move {
                    observe(&state, login, &[window(FIVE_HOURS, f64::from(used), None)]).await;
                })
            })
            .collect::<Vec<_>>();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let meanwhile = sqlx::query("SELECT 1").execute(&scene.pool).await;
        renewal.commit().await.expect("the renewal to end");
        drop(renewing);
        for write in writes {
            write.await.expect("the write to finish");
        }
        let stored = scene.stored(login).await.snapshot().expect("a snapshot");
        scene.remove().await;

        assert!(
            meanwhile.is_ok(),
            "usage writes waiting on the renewal drained the pool: {meanwhile:?}"
        );
        assert_eq!(stored.windows.len(), 1);
    }

    #[tokio::test]
    async fn changing_the_configured_agent_moves_a_chat_off_its_usable_login() {
        let scene = Scene::new().await;
        let claude = scene.login(AgentKind::Claude, "claude", Some(90.0)).await;
        let codex = scene.login(AgentKind::Codex, "codex", None).await;

        let moved = scene
            .pick(AgentKind::Codex, &[], Some(claude))
            .await
            .expect("a login");
        let kept = scene
            .pick(AgentKind::Codex, &[], Some(codex))
            .await
            .expect("a login");
        scene.remove().await;

        assert_eq!((moved.login.id, moved.agent), (codex, AgentKind::Codex));
        assert_eq!(kept.login.id, codex);
    }

    #[tokio::test]
    async fn a_chat_on_another_agent_returns_once_the_configured_agent_can_run_it() {
        let scene = Scene::new().await;
        let claude = scene.login(AgentKind::Claude, "claude", Some(40.0)).await;
        let codex = scene.login(AgentKind::Codex, "codex", None).await;
        let other = scene.login(AgentKind::Codex, "other", Some(90.0)).await;
        scene
            .exhaust(claude, Utc::now() + TimeDelta::hours(1))
            .await;

        let stays = scene.picked(AgentKind::Claude, &[], Some(codex)).await;
        scene
            .exhaust(claude, Utc::now() - TimeDelta::seconds(1))
            .await;
        let returns = scene.picked(AgentKind::Claude, &[], Some(codex)).await;
        scene.remove().await;

        assert_eq!(
            stays, codex,
            "headroom alone moved a chat between one agent's logins"
        );
        assert_ne!(stays, other);
        assert_eq!(returns, claude);
    }

    #[tokio::test]
    async fn a_limit_that_names_no_reset_reads_the_usage_before_resting_the_login() {
        let resets_at = (Utc::now() + TimeDelta::hours(2)).trunc_subsecs(0);
        let usage = MockServer::start().await;
        Mock::given(path(CODEX_USAGE))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                r#"{{"plan_type":"pro","rate_limit":{{"primary_window":{{"used_percent":100,"limit_window_seconds":18000,"reset_at":{}}}}}}}"#,
                resets_at.timestamp()
            )))
            .expect(1)
            .mount(&usage)
            .await;
        let scene = Scene::reading(usage.uri()).await;
        let limited = scene.login(AgentKind::Codex, "limited", None).await;
        scene.signed_in(limited);
        scene
            .read_at(
                limited,
                40.0,
                Utc::now() + TimeDelta::hours(1),
                Utc::now() - TimeDelta::hours(1),
            )
            .await;

        mark_limited(&scene.state, limited, &limit(false, None), MODEL).await;
        let stored = scene.stored(limited).await;
        scene.remove().await;

        assert_eq!(
            stored.exhausted_until,
            Some(resets_at),
            "a codex limit rested the login only {REST} on a stale snapshot"
        );
        assert_eq!(
            stored.snapshot().map(|snapshot| snapshot.availability()),
            Some(Availability::At(resets_at))
        );
        usage.verify().await;
    }

    #[tokio::test]
    async fn a_slow_reading_keeps_a_window_a_turn_observed_while_it_ran() {
        let usage = answering(Duration::from_millis(800)).await;
        let scene = Scene::reading(usage.uri()).await;
        let login = scene.login(AgentKind::Claude, "busy", None).await;
        let resets_at = Utc::now() + TimeDelta::hours(2);
        scene
            .read_at(login, 70.0, resets_at, Utc::now() - TimeDelta::hours(1))
            .await;
        let started = Utc::now();

        let (_, ()) = tokio::join!(scene.pick(AgentKind::Claude, &[], None), async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            observe(
                &scene.state,
                login,
                &[window(FIVE_HOURS, 95.0, Some(resets_at))],
            )
            .await;
        });
        let stored = scene.stored(login).await.snapshot().expect("a snapshot");
        scene.remove().await;

        assert_eq!(
            used(&stored),
            [(FIVE_HOURS, Some(95.0)), (SEVEN_DAYS, Some(31.0))],
            "the reading wrote its older five-hour window over the one the turn observed"
        );
        assert!(stored.fetched_at >= started);
        assert_eq!(readings(&usage).await, 1);
    }

    #[tokio::test]
    async fn a_reading_never_overwrites_one_that_began_after_it() {
        let usage = answering(Duration::from_millis(800)).await;
        let scene = Scene::reading(usage.uri()).await;
        let login = scene.login(AgentKind::Claude, "shared", None).await;
        let resets_at = Utc::now() + TimeDelta::hours(2);
        scene
            .read_at(login, 70.0, resets_at, Utc::now() - TimeDelta::hours(1))
            .await;

        let (chosen, ()) = tokio::join!(scene.pick(AgentKind::Claude, &[], None), async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            scene.read(login, 15.0, resets_at).await;
        });
        let stored = scene.stored(login).await.snapshot().expect("a snapshot");
        scene.remove().await;

        assert_eq!(stored.headroom, Some(15.0));
        assert_eq!(headroom(&chosen), Some(15.0));
    }

    /// A usage endpoint that answers every reading with [`READING`], once `delay` has passed.
    async fn answering(delay: Duration) -> MockServer {
        let usage = MockServer::start().await;
        Mock::given(path(CLAUDE_USAGE))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(READING)
                    .set_delay(delay),
            )
            .named("the Claude usage endpoint")
            .mount(&usage)
            .await;
        usage
    }

    async fn readings(usage: &MockServer) -> usize {
        usage
            .received_requests()
            .await
            .expect("the requests to be recorded")
            .iter()
            .filter(|request| request.url.path() == CLAUDE_USAGE)
            .count()
    }

    fn headroom(chosen: &Result<Chosen, Error>) -> Option<f64> {
        match chosen {
            Ok(chosen) => chosen
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.headroom),
            Err(error) => panic!("expected a login, got {error:?}"),
        }
    }

    #[tokio::test]
    async fn a_refused_usage_token_leaves_the_snapshot_unknown_and_the_login_a_candidate() {
        let usage = MockServer::start().await;
        Mock::given(path(CLAUDE_USAGE))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&usage)
            .await;
        let scene = Scene::reading(usage.uri()).await;
        let refused = scene.login(AgentKind::Claude, "refused", None).await;

        let chosen = scene.pick(AgentKind::Claude, &[], None).await;
        let stored = scene.stored(refused).await;
        scene.remove().await;

        let chosen = chosen.expect("the refused login to stay a candidate");
        assert_eq!(chosen.login.id, refused);
        assert_eq!(chosen.snapshot, None);
        assert_eq!(stored.snapshot(), None);
        usage.verify().await;
    }

    #[tokio::test]
    async fn a_slow_usage_endpoint_keeps_the_stored_snapshot() {
        let usage = answering(Duration::from_secs(30)).await;
        let scene = Scene::reading(usage.uri()).await;
        let slow = scene.login(AgentKind::Claude, "slow", None).await;
        let read_at = Utc::now() - TimeDelta::hours(1);
        scene
            .read_at(slow, 70.0, Utc::now() + TimeDelta::hours(2), read_at)
            .await;
        let started = Instant::now();

        let chosen = scene.pick(AgentKind::Claude, &[], None).await;
        let waited = started.elapsed();
        let stored = scene.stored(slow).await.snapshot().expect("a snapshot");
        scene.remove().await;

        assert_eq!(headroom(&chosen), Some(70.0));
        assert_eq!(
            stored.fetched_at.timestamp_micros(),
            read_at.timestamp_micros()
        );
        assert!(waited < Duration::from_secs(15), "waited {waited:?}");
        assert_eq!(readings(&usage).await, 1);
    }

    #[tokio::test]
    async fn concurrent_refreshes_of_one_login_fetch_once() {
        let usage = answering(Duration::from_millis(300)).await;
        let scene = Scene::reading(usage.uri()).await;
        let shared = scene.login(AgentKind::Claude, "shared", None).await;

        let (first, second) = tokio::join!(
            scene.pick(AgentKind::Claude, &[], None),
            scene.pick(AgentKind::Claude, &[], None)
        );
        let third = scene.pick(AgentKind::Claude, &[], None).await;
        let stored = scene.stored(shared).await.snapshot();
        scene.remove().await;

        for chosen in [&first, &second, &third] {
            assert_eq!(headroom(chosen), Some(READ_HEADROOM));
        }
        assert_eq!(
            stored.and_then(|snapshot| snapshot.headroom),
            Some(READ_HEADROOM)
        );
        assert_eq!(readings(&usage).await, 1);
    }

    #[tokio::test]
    async fn a_fresh_snapshot_is_not_fetched_again_and_a_stale_one_is() {
        let usage = answering(Duration::ZERO).await;
        let scene = Scene::reading(usage.uri()).await;
        let fresh = scene.login(AgentKind::Claude, "fresh", Some(70.0)).await;
        let stale = scene.login(AgentKind::Claude, "stale", None).await;
        scene
            .read_at(
                stale,
                90.0,
                Utc::now() + TimeDelta::hours(2),
                Utc::now() - TimeDelta::hours(1),
            )
            .await;
        let before = Utc::now();

        let picked = scene.picked(AgentKind::Claude, &[], None).await;
        let kept = scene.stored(fresh).await.snapshot().expect("a snapshot");
        let reread = scene.stored(stale).await.snapshot().expect("a snapshot");
        scene.remove().await;

        assert_eq!(kept.headroom, Some(70.0));
        assert!(kept.fetched_at < before);
        assert_eq!(reread.headroom, Some(READ_HEADROOM));
        assert!(reread.fetched_at >= before);
        assert_eq!(
            picked, fresh,
            "the stale login was ranked on its new reading"
        );
        assert_eq!(readings(&usage).await, 1);
    }

    #[tokio::test]
    async fn settling_a_turn_reads_a_stale_logins_usage_and_leaves_a_fresh_one() {
        let usage = answering(Duration::ZERO).await;
        let scene = Scene::reading(usage.uri()).await;
        let fresh = scene.login(AgentKind::Claude, "fresh", Some(70.0)).await;
        let stale = scene.login(AgentKind::Claude, "stale", None).await;
        scene
            .read_at(
                stale,
                90.0,
                Utc::now() + TimeDelta::hours(2),
                Utc::now() - TimeDelta::hours(1),
            )
            .await;

        settle(&scene.state, stale).await;
        settle(&scene.state, fresh).await;
        settle(&scene.state, Uuid::new_v4()).await;
        let fresh = scene.stored(fresh).await.snapshot().expect("a snapshot");
        let stale = scene.stored(stale).await.snapshot().expect("a snapshot");
        scene.remove().await;

        assert_eq!(fresh.headroom, Some(70.0));
        assert_eq!(stale.headroom, Some(READ_HEADROOM));
        assert_eq!(readings(&usage).await, 1);
    }

    #[tokio::test]
    async fn a_usage_endpoint_that_fails_is_not_asked_again_within_the_ttl() {
        let usage = MockServer::start().await;
        Mock::given(path(CLAUDE_USAGE))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&usage)
            .await;
        let scene = Scene::reading(usage.uri()).await;
        let failing = scene.login(AgentKind::Claude, "failing", None).await;

        let first = scene.pick(AgentKind::Claude, &[], None).await;
        let second = scene.pick(AgentKind::Claude, &[], None).await;
        scene.remove().await;

        for chosen in [first, second] {
            let chosen = chosen.expect("the login to stay a candidate");
            assert_eq!((chosen.login.id, chosen.snapshot), (failing, None));
        }
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
