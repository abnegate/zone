//! A chat's turn moved off a login that can no longer run it, onto another of its organization's,
//! within the same answer.

mod cause;
mod model;
mod notice;
mod reason;
mod switch;

pub use cause::Cause;
pub use model::Model;
pub use notice::Notice;
pub use reason::Reason;
pub use switch::Switch;

use std::path::{Path, PathBuf};
use std::time::Duration;

use abnegate_llm::Message;
use chrono::Utc;
use uuid::Uuid;
use zone_chat::history::NewEntry;
use zone_core::context::Entry;
use zone_core::llm::{AgentKind, LlmBackend, Session};

use super::session::RunContext;
use crate::config::Config;
use crate::db::agent_logins::{self, AgentLoginRow};
use crate::db::chats::ChatSession;
use crate::services::agent::sessions;
use crate::services::backend::{self, Continuation, Resolved};
use crate::services::login::identity::LoginIdentity;
use crate::services::login::router;
use crate::services::login::usage::Availability;
use crate::state::AppState;

/// What a turn that moved to another login is told once it gets there, after what was written.
pub const CONTINUE: &str =
    "Continue the answer from where it stopped, without repeating what was already written.";

/// The id of [`CONTINUE`]'s entry, which is sent and never written to the chat.
const INSTRUCTION: &str = "handover:continue";

/// The logins a chat's turn has run on, and the one it runs on now.
pub struct Handover {
    organization: Uuid,
    current: LoginIdentity,
    /// The logins this turn left, none of which it runs on again.
    tried: Vec<Uuid>,
    /// Whether a subscription limit took one of them out.
    limited: bool,
    model: Model,
    /// The move the turn made before it started, not yet told.
    opening: Option<Notice>,
    /// Whether the turn moves only onto logins of the agent it runs on.
    strict: bool,
    /// The directory the chat's project starts it in, on whichever login it runs.
    directory: Option<PathBuf>,
}

impl Handover {
    /// A turn of one of `organization`'s chats starting on `current`, its model chosen from
    /// `model`. A turn that left `previous`'s login before it started told of it by `opening`,
    /// and never runs on that login again unless it left only to run on the configured agent.
    pub fn new(
        organization: Uuid,
        current: LoginIdentity,
        model: Model,
        previous: Option<&ChatSession>,
        opening: Option<Notice>,
    ) -> Self {
        let unusable = opening
            .as_ref()
            .is_some_and(|opening| opening.reason != Reason::Configured);
        let tried = previous
            .and_then(|previous| previous.login)
            .filter(|login| unusable && *login != current.id)
            .into_iter()
            .collect();
        Self {
            organization,
            current,
            tried,
            limited: false,
            model,
            opening,
            strict: false,
            directory: None,
        }
    }

    /// This handover keeping the turn in `directory`, the one its project starts it in.
    pub fn in_directory(self, directory: Option<PathBuf>) -> Self {
        Self { directory, ..self }
    }

    /// This handover moving the turn only onto logins of the agent it runs on, for a chat that
    /// chose its agent.
    pub fn strict(self) -> Self {
        Self {
            strict: true,
            ..self
        }
    }

    /// The login the turn runs on now.
    pub fn login(&self) -> Uuid {
        self.current.id
    }

    /// The move the turn made before it started, once.
    pub fn opened(&mut self) -> Option<Notice> {
        self.opening.take()
    }

    /// Moves the turn off the login it runs on for `cause`, onto the login the router picks next
    /// among those the turn has not run on, the current agent's first, or only the current
    /// agent's when [`Self::strict`]. Session `session` is
    /// carried along when the new login runs the same agent and that agent resumes a session
    /// carried in from another home; otherwise the turn starts a fresh session there. Either way
    /// the new backend is bounded by `timeout`.
    ///
    /// When no login is left, the error to end the turn with: the organization's limit when
    /// a subscription limit is what left none, and `None` when the turn's own failure says
    /// it best.
    pub async fn next(
        &mut self,
        state: &AppState,
        cause: &Cause,
        session: Option<&str>,
        timeout: Duration,
    ) -> Result<Switch, Option<backend::Error>> {
        let from = self.current.clone();
        let running = self.model.on(from.agent);
        if let Some(limit) = &cause.limit {
            router::mark_limited(state, from.id, limit, &running).await;
        }
        self.limited |= cause.reason == Reason::Limit;
        if !self.tried.contains(&from.id) {
            self.tried.push(from.id);
        }
        let chosen = match router::pick(
            state,
            self.organization,
            from.agent,
            &self.tried,
            None,
            Some(&running),
            self.strict,
        )
        .await
        {
            Ok(chosen) => chosen,
            Err(router::Error::Exhausted { resets_at }) if self.limited || resets_at.is_some() => {
                return Err(Some(backend::Error::Limited {
                    agent: from.agent,
                    resets_at,
                }));
            }
            Err(error) => {
                tracing::info!(login = %from.id, %error, "No other login can take the turn over");
                return Err(None);
            }
        };
        let config = state.config();
        let carried = chosen.agent == from.agent
            && sessions::portable(chosen.agent)
            && session.is_some_and(|id| {
                carry(
                    config,
                    self.organization,
                    chosen.agent,
                    (from.id, chosen.login.id),
                    self.directory.as_deref(),
                    id,
                )
            });
        let session = match session.filter(|_| carried) {
            Some(id) => Some(Session {
                id: id.to_string(),
                resume: true,
            }),
            None => Continuation::fresh(chosen.agent),
        };
        let resolved =
            backend::on_login(config, self.organization, &chosen, session).map_err(Some)?;
        let Some(to) = resolved.login else {
            return Err(None);
        };
        let running = match &self.directory {
            Some(directory) => backend::in_directory(resolved.backend, directory),
            None => resolved.backend,
        };
        if let Err(error) = agent_logins::touch(state.db(), to.id, Utc::now()).await {
            tracing::warn!(login = %to.id, %error, "Could not record that a turn moved onto the login");
        }
        let model = (to.agent != from.agent).then(|| self.model.on(to.agent));
        self.current = to.clone();
        Ok(Switch {
            backend: backend::bounded(running, timeout),
            from,
            to,
            reason: cause.reason,
            resets_at: cause.limit.as_ref().and_then(|limit| limit.resets_at),
            carried,
            model,
        })
    }
}

/// The move a chat's turn makes before it starts, off `previous`'s login onto `resolved`'s, which
/// the router picked because `previous`'s can no longer run it on `model`, or runs another agent
/// than `configured` while `configured` can run it again. The session moves with it when both run
/// the same agent and its file can be carried, and `resolved` then resumes it.
pub async fn opening(
    state: &AppState,
    organization: Uuid,
    configured: AgentKind,
    previous: Option<&ChatSession>,
    resolved: &mut Resolved,
    model: Option<&str>,
    directory: Option<&Path>,
) -> Option<Notice> {
    let previous = previous?;
    let left = previous.login?;
    let to = resolved.login.clone()?;
    if left == to.id {
        return None;
    }
    let row = match agent_logins::get(state.db(), left).await {
        Ok(row) => row?,
        Err(error) => {
            tracing::warn!(login = %left, %error, "Could not read the login the chat left");
            return None;
        }
    };
    let from = identity(&row)?;
    let now = Utc::now();
    let unfunded = model.is_some_and(|model| router::unfunded::cooling(model, now).contains(&left));
    let (reason, resets_at) = match from.agent != configured && to.agent == configured {
        true => (Reason::Configured, None),
        false => standing(&row, unfunded, now),
    };
    let carried = previous.agent == to.agent
        && from.agent == to.agent
        && sessions::portable(to.agent)
        && carry(
            state.config(),
            organization,
            to.agent,
            (left, to.id),
            directory,
            &previous.id,
        );
    if carried {
        resolved.backend = resumed(
            std::mem::replace(&mut resolved.backend, LlmBackend::Http),
            Session {
                id: previous.id.clone(),
                resume: true,
            },
        );
    }
    Some(Notice {
        from: from.label,
        to: to.label,
        from_agent: from.agent,
        agent: to.agent,
        reason,
        resets_at,
        carried,
        at: 0,
        calls: 0,
    })
}

/// The text a turn wrote on a login before it left, as the assistant entry the chat keeps of it.
/// Nothing when it wrote none.
pub fn partial(text: &str) -> Option<NewEntry> {
    (!text.trim().is_empty()).then(|| NewEntry {
        id: Uuid::new_v4().to_string(),
        message: (&Message::assistant(text)).into(),
        mutations: Vec::new(),
    })
}

/// What a fresh session on the next login is sent: `whole`, the transcript with what the turn
/// wrote so far, then the instruction to go on from there.
pub fn replayed(whole: &RunContext) -> RunContext {
    let mut context = whole.clone();
    context.entries.push(instruction());
    context
}

/// What a carried session is sent: the instruction alone, since the session holds the rest.
pub fn carried(whole: &RunContext) -> RunContext {
    RunContext {
        entries: vec![instruction()],
        summary: None,
        policy: whole.policy.clone(),
        reason: whole.reason.clone(),
        incomplete: whole.incomplete,
        artifacts: whole.artifacts.clone(),
        vision: whole.vision,
        reported: whole.reported,
    }
}

/// [`CONTINUE`], from the user so it follows the answer rather than opening the transcript with
/// the instructions, and consumed already so it is never written to the chat.
fn instruction() -> Entry {
    Entry {
        id: INSTRUCTION.into(),
        message: Message::user(CONTINUE),
        preserve: true,
        consumed: true,
    }
}

/// Moves the session `resolved` is about to resume to where its agent will look for it from the
/// directory it runs in, since the chat may have started somewhere else before. One that cannot
/// be moved is left for the agent to refuse, and the turn then replays the chat.
pub fn settle(config: &Config, organization: Uuid, resolved: &Resolved) {
    let (Some(login), LlmBackend::Cli { agent, settings }) = (&resolved.login, &resolved.backend)
    else {
        return;
    };
    let (Some(session), Some(work)) = (&settings.session, &settings.working_directory) else {
        return;
    };
    if !session.resume {
        return;
    }
    let home = config.agents.login_home(organization, *agent, login.id);
    if let Err(error) = sessions::relocate(&home, *agent, work, &session.id) {
        tracing::info!(%error, "The agent session cannot be moved to the chat's directory");
    }
}

/// Copies session `id`'s file from login `from`'s home into login `to`'s, both `agent`'s logins
/// of `organization`, to where `agent` looks for it from `directory`, or from the organization's
/// working directory when the chat has none of its own. False when it cannot, and the turn
/// replays instead.
fn carry(
    config: &Config,
    organization: Uuid,
    agent: AgentKind,
    (from, to): (Uuid, Uuid),
    directory: Option<&Path>,
    id: &str,
) -> bool {
    let home = match config.agents.create_login_home(organization, agent, to) {
        Ok(home) => home,
        Err(error) => {
            tracing::warn!(login = %to, %error, "Could not prepare the login's home for a carried session");
            return false;
        }
    };
    match sessions::carry(
        &config.agents.login_home(organization, agent, from),
        &home,
        agent,
        &directory.map_or_else(
            || config.agents.work(organization, agent),
            Path::to_path_buf,
        ),
        id,
    ) {
        Ok(carried) => {
            tracing::debug!(path = %carried.path.display(), bytes = carried.bytes, "Carried the agent session to the next login");
            true
        }
        Err(error) => {
            tracing::info!(%error, "The agent session cannot be carried; replaying the chat instead");
            false
        }
    }
}

/// `backend` resuming `session` in place of the one it was given.
fn resumed(backend: LlmBackend, session: Session) -> LlmBackend {
    match backend {
        LlmBackend::Cli { agent, settings } => {
            LlmBackend::cli(agent, settings.with_session(session))
        }
        LlmBackend::Http => LlmBackend::Http,
    }
}

/// `row` as the chat names it.
fn identity(row: &AgentLoginRow) -> Option<LoginIdentity> {
    let agent = AgentKind::named(&row.agent)?;
    Some(LoginIdentity {
        id: row.id,
        agent,
        label: row
            .label
            .clone()
            .or_else(|| row.account.clone())
            .unwrap_or_else(|| agent.to_string()),
    })
}

/// Why the router passed over `row`, which it keeps a chat on while it can run: a limit it
/// reached, and when that resets, else the chat's model it refused lately for want of usage
/// credits, when it is `unfunded`, else a sign-in that can no longer be used.
fn standing(
    row: &AgentLoginRow,
    unfunded: bool,
    now: chrono::DateTime<Utc>,
) -> (Reason, Option<chrono::DateTime<Utc>>) {
    if let Some(until) = row.exhausted_until.filter(|until| *until > now) {
        return (Reason::Limit, Some(until));
    }
    match row.snapshot() {
        Some(snapshot) if snapshot.headroom.is_some_and(|headroom| headroom <= 0.0) => {
            let resets_at = match snapshot.availability() {
                Availability::At(until) if until > now => Some(until),
                Availability::At(_) | Availability::Now | Availability::Unknown => None,
            };
            (Reason::Limit, resets_at)
        }
        _ if unfunded => (Reason::Credits, None),
        _ => (Reason::SignedOut, None),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use abnegate_llm::Limit;
    use abnegate_secret::SecretValue;
    use chrono::{DateTime, SecondsFormat, TimeDelta, TimeZone};
    use sqlx::PgPool;
    use tempfile::TempDir;
    use zone_core::context::project;
    use zone_core::llm::CliSettings;
    use zone_core::llm::provider::render;

    use super::*;
    use crate::config::AgentConfig;
    use crate::db::agent_logins::Insert;
    use crate::db::organizations;
    use crate::services::chat::session::{Generation, Pinned};
    use crate::services::login::claude::Tokens;
    use crate::services::login::router::Chosen;
    use crate::services::stages::Preferences;

    const SESSION: &str = "6f1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    const LIMITED: &str = "Stream error: claude: You've hit your session limit · resets 5pm";
    const CLAUDE_REFUSAL: &str = "Stream error: claude: No conversation found with session ID: 6f1";
    const PARTIAL: &str = "The Nile, ";
    const TRANSCRIPT: &[u8] = b"{\"type\":\"user\",\"message\":\"Name three rivers.\"}\n";
    const CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
    const CODEX_HOME: &str = "CODEX_HOME";

    fn whole() -> RunContext {
        RunContext::from_messages(vec![
            Message::system("Be terse."),
            Message::user("Name three rivers."),
        ])
    }

    fn sent(context: &RunContext) -> String {
        render(&project(&context.entries, context.summary.as_ref()).expect("a projectable context"))
    }

    fn resets() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2027, 9, 23, 6, 10, 0)
            .single()
            .expect("a reset time")
    }

    fn limit(credits: bool) -> Cause {
        let mut limit = Limit::new(LIMITED).with_credits(credits);
        limit.resets_at = (!credits).then(resets);
        Cause::limited(limit)
    }

    fn identity(agent: AgentKind, label: &str) -> LoginIdentity {
        LoginIdentity {
            id: Uuid::new_v4(),
            agent,
            label: label.to_string(),
        }
    }

    fn cli(backend: &LlmBackend) -> (AgentKind, &CliSettings) {
        match backend {
            LlmBackend::Cli { agent, settings } => (*agent, settings),
            LlmBackend::Http => panic!("a handover runs on a coding agent"),
        }
    }

    #[test]
    fn a_replayed_resume_carries_the_partial_answer_persisted_and_the_instruction_unpersisted() {
        let mut whole = whole();
        let written = partial(PARTIAL).expect("the part written before the switch");
        whole.append(&written);

        let mut context = replayed(&whole);

        assert_eq!(written.message.role, abnegate_llm::Role::Assistant);
        assert_eq!(written.message.content.as_deref(), Some(PARTIAL));
        assert_eq!(
            sent(&context),
            format!(
                "System:\nBe terse.\n\nUser:\nName three rivers.\n\nAssistant:\n{}\n\nUser:\n{CONTINUE}",
                PARTIAL.trim()
            )
        );
        assert_eq!(
            context.consume(),
            vec![written.id.clone()],
            "only the partial answer is written to the chat"
        );
        assert_eq!(
            whole.entries.len(),
            3,
            "the instruction stays out of the transcript"
        );
        assert!(
            partial("  \n").is_none(),
            "a switch before any text keeps nothing"
        );
    }

    #[test]
    fn a_carried_session_sends_only_the_instruction() {
        let mut whole = whole();
        whole.append(&partial(PARTIAL).expect("a partial answer"));

        let mut context = carried(&whole);

        assert_eq!(sent(&context), format!("User:\n{CONTINUE}"));
        assert!(
            context.consume().is_empty(),
            "the instruction is never written to the chat"
        );
        assert_eq!(context.policy.reserved, whole.policy.reserved);
        assert_eq!(context.vision, whole.vision);
    }

    #[test]
    fn a_carried_session_the_cli_refuses_falls_back_to_a_replay_with_the_partial() {
        let from = identity(AgentKind::Claude, "a@example.com");
        let to = identity(AgentKind::Claude, "b@example.com");
        let resumed = Session {
            id: SESSION.into(),
            resume: true,
        };
        let generation = Generation {
            resolved: Resolved {
                backend: LlmBackend::cli(
                    AgentKind::Claude,
                    CliSettings::default().with_session(resumed.clone()),
                ),
                login: Some(from.clone()),
            },
            session: Some(ChatSession {
                login: Some(from.id),
                id: SESSION.into(),
                agent: AgentKind::Claude,
                entry: 4,
                prompt: Some("a".repeat(64)),
            }),
            directory: None,
        };
        let mut pinned = Pinned::of(&generation).expect("a session on a login");
        pinned.sees("b".repeat(64), 6);
        pinned.keep(whole());
        let written = partial(PARTIAL).expect("a partial answer");
        pinned.extend(&written);

        pinned.switch(to.clone(), Some(resumed.clone()));

        let recorded = pinned.record(7).expect("the carried session");
        assert_eq!(recorded.login, Some(to.id));
        assert_eq!(recorded.id, SESSION);
        assert_eq!(
            recorded.prompt,
            Some("b".repeat(64)),
            "a carried session keeps its prompt"
        );
        assert!(pinned.refused(CLAUDE_REFUSAL));
        assert!(!pinned.refused(LIMITED));

        let (transcript, backend) = pinned
            .restart(LlmBackend::cli(
                AgentKind::Claude,
                CliSettings::default().with_session(resumed),
            ))
            .expect("a replay of the carried session");
        let context = replayed(&transcript);

        assert!(
            sent(&context).ends_with(&format!(
                "User:\nName three rivers.\n\nAssistant:\n{}\n\nUser:\n{CONTINUE}",
                PARTIAL.trim()
            )),
            "{}",
            sent(&context)
        );
        let fresh = cli(&backend)
            .1
            .session
            .clone()
            .expect("a fresh claude session");
        assert!(!fresh.resume);
        assert_ne!(fresh.id, SESSION);
        let recorded = pinned.record(8).expect("the fresh session");
        assert_eq!(recorded.login, Some(to.id));
        assert_eq!(recorded.id, fresh.id);
        assert!(
            !pinned.refused(CLAUDE_REFUSAL),
            "a replay is refused only once"
        );
    }

    #[test]
    fn a_replayed_switch_is_never_taken_for_a_refused_resume() {
        let from = identity(AgentKind::Claude, "a@example.com");
        let generation = Generation {
            resolved: Resolved {
                backend: LlmBackend::cli(AgentKind::Claude, CliSettings::default()),
                login: Some(from),
            },
            session: None,
            directory: None,
        };
        let mut pinned = Pinned::of(&generation).expect("a session on a login");
        pinned.keep(whole());

        pinned.switch(
            identity(AgentKind::Codex, "c@example.com"),
            Continuation::fresh(AgentKind::Codex),
        );

        assert_eq!(pinned.resumed(), None);
        assert!(!pinned.refused(CLAUDE_REFUSAL));
        assert_eq!(
            pinned.record(3),
            None,
            "codex names its thread once it runs"
        );
    }

    struct Fixture {
        pool: PgPool,
        organization: Uuid,
        agents: TempDir,
        state: AppState,
    }

    impl Fixture {
        async fn new() -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
            )
            .await
            .expect("the test database");
            let organization = organizations::create_organization(
                &pool,
                "Handover",
                &Uuid::new_v4().to_string(),
                None,
            )
            .await
            .expect("an organization")
            .id;
            let agents = TempDir::new().expect("an agent state root");
            let state = AppState::new(
                Config {
                    agents: AgentConfig {
                        state: agents.path().to_path_buf(),
                        ..crate::state::test_agents()
                    },
                    ..crate::state::test_config()
                },
                pool.clone(),
                None,
            );
            Self {
                pool,
                organization,
                agents,
                state,
            }
        }

        async fn claude(&self, label: &str) -> LoginIdentity {
            let tokens = Tokens {
                access: SecretValue::new(format!("fake-claude-access-token-{label}")),
                refresh: None,
                expires_at: Utc::now() + TimeDelta::days(30),
                issued_at: None,
                scope: "user:inference".to_string(),
                subscription: None,
            };
            let sealed = tokens
                .seal(self.state.encryption_key())
                .expect("tokens to seal");
            self.store(AgentKind::Claude, Some(&sealed), label).await
        }

        async fn codex(&self, label: &str) -> LoginIdentity {
            self.store(AgentKind::Codex, None, label).await
        }

        async fn store(
            &self,
            agent: AgentKind,
            credential: Option<&str>,
            label: &str,
        ) -> LoginIdentity {
            let row = agent_logins::insert(
                &self.pool,
                &Insert {
                    organization_id: self.organization,
                    agent: agent.as_str(),
                    account: None,
                    credential,
                    label: Some(label),
                    expires_at: None,
                },
            )
            .await
            .expect("a stored login");
            LoginIdentity {
                id: row.id,
                agent,
                label: label.to_string(),
            }
        }

        fn handover(&self, current: &LoginIdentity) -> Handover {
            Handover::new(
                self.organization,
                current.clone(),
                Model {
                    requested: AgentKind::Claude.models()[0].to_string(),
                    preferences: Preferences {
                        fast: Some(AgentKind::Codex.models()[0].to_string()),
                        ..Preferences::default()
                    },
                    message: "Name three rivers.".to_string(),
                    image: false,
                    agentic: true,
                },
                None,
                None,
            )
        }

        fn home(&self, login: &LoginIdentity) -> PathBuf {
            self.state
                .config()
                .agents
                .login_home(self.organization, login.agent, login.id)
        }

        fn work(&self, agent: AgentKind) -> PathBuf {
            self.state.config().agents.work(self.organization, agent)
        }

        /// Writes claude session `id`'s file into `login`'s home, as a turn on it leaves it.
        /// A session of `login` in the folder claude keeps for turns run in `directory`.
        fn session_in(&self, login: &LoginIdentity, id: &str, directory: &Path) -> PathBuf {
            let home = self
                .state
                .config()
                .agents
                .create_login_home(self.organization, login.agent, login.id)
                .expect("the login's home");
            let directory = fs::canonicalize(directory).expect("the directory");
            let path = home
                .join("projects")
                .join(sessions::sanitized(&directory))
                .join(format!("{id}.jsonl"));
            fs::create_dir_all(path.parent().expect("a project folder")).expect("the folder");
            fs::write(&path, b"{}\n").expect("the session file");
            path
        }

        fn session(&self, login: &LoginIdentity, id: &str) {
            let home = self
                .state
                .config()
                .agents
                .create_login_home(self.organization, login.agent, login.id)
                .expect("the login's home");
            let work = fs::canonicalize(self.work(login.agent)).expect("the working directory");
            let path = home
                .join("projects")
                .join(sessions::sanitized(&work))
                .join(format!("{id}.jsonl"));
            fs::create_dir_all(path.parent().expect("a project folder")).expect("the folder");
            fs::write(path, TRANSCRIPT).expect("the session file");
        }

        fn carried(&self, login: &LoginIdentity, id: &str) -> bool {
            sessions::locate(&self.home(login), login.agent, &self.work(login.agent), id).is_some()
        }

        async fn exhausted_until(&self, login: &LoginIdentity) -> Option<DateTime<Utc>> {
            agent_logins::get(&self.pool, login.id)
                .await
                .expect("the login to be readable")
                .expect("the login to be there")
                .exhausted_until
        }

        async fn remove(self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the organization to be removed");
            drop(self.agents);
        }
    }

    const TIMEOUT: Duration = Duration::from_secs(90);

    #[tokio::test]
    async fn a_switch_to_another_agent_replays_and_repicks_the_model() {
        let fixture = Fixture::new().await;
        let claude = fixture.claude("a@example.com").await;
        let codex = fixture.codex("c@example.com").await;
        fixture.session(&claude, SESSION);
        let mut handover = fixture.handover(&claude);

        let switch = handover
            .next(&fixture.state, &limit(false), Some(SESSION), TIMEOUT)
            .await;
        let exhausted = fixture.exhausted_until(&claude).await;
        let codex_work = fixture.work(AgentKind::Codex);
        let codex_home = fixture.home(&codex);
        fixture.remove().await;

        let switch = switch.expect("codex takes the turn over");
        let (agent, settings) = cli(&switch.backend);
        assert_eq!(agent, AgentKind::Codex);
        assert_eq!(switch.to, codex);
        assert_eq!(switch.from, claude);
        assert!(
            !switch.carried,
            "a claude session is never carried to codex"
        );
        assert_eq!(switch.session(), None, "codex names its own thread");
        assert_eq!(
            switch.model.as_deref(),
            Some(AgentKind::Codex.models()[0]),
            "the model is picked again from codex's catalog"
        );
        assert_eq!(
            settings.working_directory.as_deref(),
            Some(codex_work.as_path())
        );
        assert_eq!(
            settings.variables.get(CODEX_HOME).map(PathBuf::from),
            Some(codex_home)
        );
        assert_eq!(settings.timeout, TIMEOUT);
        assert_eq!(
            exhausted,
            Some(resets()),
            "the limited login rests until its reset"
        );
        assert_eq!(handover.login(), codex.id);
        assert_eq!(
            serde_json::to_value(switch.notice(12, 3)).expect("a notice"),
            serde_json::json!({
                "from": "a@example.com",
                "to": "c@example.com",
                "from_agent": "claude",
                "agent": "codex",
                "reason": "limit",
                "resets_at": "2027-09-23T06:10:00Z",
                "carried": false,
                "at": 12,
                "calls": 3,
            })
        );
    }

    #[tokio::test]
    async fn a_strict_handover_ends_the_turn_rather_than_move_to_another_agent() {
        let fixture = Fixture::new().await;
        let claude = fixture.claude("a@example.com").await;
        let codex = fixture.codex("c@example.com").await;
        fixture.session(&claude, SESSION);
        let mut limited = fixture.handover(&claude).strict();
        let mut signed_out = fixture.handover(&claude).strict();

        let refused = signed_out
            .next(
                &fixture.state,
                &Cause::signed_out("Not logged in".into()),
                Some(SESSION),
                TIMEOUT,
            )
            .await;
        let spent = limited
            .next(&fixture.state, &limit(false), Some(SESSION), TIMEOUT)
            .await;
        fixture.remove().await;

        assert!(
            matches!(
                spent,
                Err(Some(backend::Error::Limited {
                    agent: AgentKind::Claude,
                    ..
                }))
            ),
            "a limited turn on a chosen agent moved instead of ending with the limit"
        );
        assert!(
            matches!(refused, Err(None)),
            "a signed-out turn on a chosen agent moved instead of ending in its own words"
        );
        for handover in [limited, signed_out] {
            assert_eq!(handover.login(), claude.id);
            assert_ne!(handover.login(), codex.id, "the turn moved to codex");
        }
    }

    #[tokio::test]
    async fn a_switch_on_the_same_agent_carries_the_session_file_and_resumes_it() {
        let fixture = Fixture::new().await;
        let from = fixture.claude("a@example.com").await;
        let to = fixture.claude("b@example.com").await;
        fixture.session(&from, SESSION);
        let mut handover = fixture.handover(&from);

        let switch = handover
            .next(&fixture.state, &limit(false), Some(SESSION), TIMEOUT)
            .await;
        let landed = fixture.carried(&to, SESSION);
        let home = fixture.home(&to);
        let work = fixture.work(AgentKind::Claude);
        fixture.remove().await;

        let switch = switch.expect("the other claude login takes the turn over");
        let (agent, settings) = cli(&switch.backend);
        assert_eq!(agent, AgentKind::Claude);
        assert_eq!(switch.to, to);
        assert!(switch.carried);
        assert!(landed, "the session file is in the next login's home");
        assert_eq!(
            switch.session(),
            Some(Session {
                id: SESSION.into(),
                resume: true
            })
        );
        assert_eq!(switch.model, None, "the same agent keeps the turn's model");
        assert_eq!(
            settings.variables.get(CLAUDE_CONFIG_DIR).map(PathBuf::from),
            Some(home)
        );
        assert_eq!(settings.working_directory.as_deref(), Some(work.as_path()));
    }

    #[tokio::test]
    async fn a_switch_stays_in_the_chats_directory_and_carries_its_session_there() {
        let fixture = Fixture::new().await;
        let from = fixture.claude("a@example.com").await;
        let to = fixture.claude("b@example.com").await;
        let project = TempDir::new().expect("a project folder");
        let path = fixture.session_in(&from, SESSION, project.path());
        let mut handover = fixture
            .handover(&from)
            .in_directory(Some(project.path().to_path_buf()));

        let switch = handover
            .next(&fixture.state, &limit(false), Some(SESSION), TIMEOUT)
            .await;
        let landed = fixture
            .home(&to)
            .join(path.strip_prefix(fixture.home(&from)).expect("in its home"))
            .is_file();
        fixture.remove().await;

        let switch = switch.expect("the other claude login takes the turn over");
        let (_, settings) = cli(&switch.backend);
        assert!(switch.carried);
        assert_eq!(
            settings.working_directory.as_deref(),
            Some(project.path()),
            "the next login runs where the chat's tools run"
        );
        assert!(
            landed,
            "the next login looks for the session in the chat's directory's folder"
        );
    }

    #[tokio::test]
    async fn a_session_is_settled_where_its_agent_will_look_from_the_chats_directory() {
        let fixture = Fixture::new().await;
        let login = fixture.claude("a@example.com").await;
        fixture.session(&login, SESSION);
        let left = fixture
            .home(&login)
            .join("projects")
            .join(sessions::sanitized(
                &fs::canonicalize(fixture.work(AgentKind::Claude)).expect("the working directory"),
            ));
        let project = TempDir::new().expect("a project folder");
        let chosen: Chosen = router::pick(
            &fixture.state,
            fixture.organization,
            AgentKind::Claude,
            &[],
            Some(login.id),
            None,
            false,
        )
        .await
        .expect("the login");
        let mut resolved = backend::on_login(
            fixture.state.config(),
            fixture.organization,
            &chosen,
            Some(Session {
                id: SESSION.into(),
                resume: true,
            }),
        )
        .expect("a backend");
        resolved.backend = backend::in_directory(resolved.backend, project.path());

        settle(fixture.state.config(), fixture.organization, &resolved);
        let found = sessions::locate(
            &fixture.home(&login),
            AgentKind::Claude,
            project.path(),
            SESSION,
        );
        fixture.remove().await;

        assert!(found.is_some(), "the agent would not find its session");
        assert!(!left.join(format!("{SESSION}.jsonl")).exists());
    }

    #[tokio::test]
    async fn a_switch_without_a_session_file_starts_a_fresh_session() {
        let fixture = Fixture::new().await;
        let from = fixture.claude("a@example.com").await;
        let to = fixture.claude("b@example.com").await;
        let mut handover = fixture.handover(&from);

        let switch = handover
            .next(&fixture.state, &limit(false), Some(SESSION), TIMEOUT)
            .await;
        fixture.remove().await;

        let switch = switch.expect("the other claude login takes the turn over");
        assert_eq!(switch.to, to);
        assert!(!switch.carried);
        let fresh = switch.session().expect("claude is pinned to a fresh id");
        assert!(!fresh.resume);
        assert_ne!(fresh.id, SESSION);
    }

    #[tokio::test]
    async fn a_handover_never_runs_the_same_login_twice_in_one_turn() {
        let fixture = Fixture::new().await;
        let first = fixture.claude("a@example.com").await;
        let second = fixture.claude("b@example.com").await;
        let third = fixture.claude("c@example.com").await;
        let mut handover = fixture.handover(&first);

        let one = handover
            .next(
                &fixture.state,
                &Cause::signed_out("Not logged in".into()),
                None,
                TIMEOUT,
            )
            .await
            .map(|switch| switch.to.id);
        let two = handover
            .next(
                &fixture.state,
                &Cause::signed_out("Not logged in".into()),
                None,
                TIMEOUT,
            )
            .await
            .map(|switch| switch.to.id);
        let three = handover
            .next(
                &fixture.state,
                &Cause::signed_out("Not logged in".into()),
                None,
                TIMEOUT,
            )
            .await
            .map(|switch| switch.to.id);
        fixture.remove().await;

        let mut took = vec![one.expect("a second login"), two.expect("a third login")];
        took.sort();
        let mut others = vec![second.id, third.id];
        others.sort();
        assert_eq!(
            took, others,
            "each login ran once and the first never again"
        );
        assert!(
            matches!(three, Err(None)),
            "with every login tried, a sign-in failure ends the turn in its own words"
        );
    }

    #[tokio::test]
    async fn a_turn_with_no_login_left_ends_with_the_organizations_limit() {
        let fixture = Fixture::new().await;
        let only = fixture.claude("a@example.com").await;
        let mut handover = fixture.handover(&only);

        let ended = handover
            .next(&fixture.state, &limit(false), Some(SESSION), TIMEOUT)
            .await;
        fixture.remove().await;

        let Err(Some(error)) = ended else {
            panic!("a turn with no login left was handed over");
        };
        assert!(matches!(error, backend::Error::Limited { .. }), "{error}");
        assert_eq!(
            error.to_string(),
            format!(
                "Every sign-in of this organization has reached its usage limit; the earliest resets at {}.",
                resets().to_rfc3339_opts(SecondsFormat::Secs, true)
            )
        );
    }

    #[tokio::test]
    async fn a_credits_limit_with_no_login_left_ends_in_its_own_words() {
        let fixture = Fixture::new().await;
        let only = fixture.claude("a@example.com").await;
        let mut handover = fixture.handover(&only);

        let ended = handover
            .next(&fixture.state, &limit(true), None, TIMEOUT)
            .await;
        let exhausted = fixture.exhausted_until(&only).await;
        fixture.remove().await;

        assert!(
            matches!(ended, Err(None)),
            "credits ended as a subscription limit"
        );
        assert_eq!(
            exhausted, None,
            "a credits limit leaves the login for other chats"
        );
    }

    #[tokio::test]
    async fn a_chat_whose_login_is_spent_moves_and_carries_its_session_before_the_turn() {
        let fixture = Fixture::new().await;
        let from = fixture.claude("a@example.com").await;
        let to = fixture.claude("b@example.com").await;
        fixture.session(&from, SESSION);
        agent_logins::exhaust(&fixture.pool, from.id, resets())
            .await
            .expect("the login exhausted");
        let previous = ChatSession {
            login: Some(from.id),
            id: SESSION.into(),
            agent: AgentKind::Claude,
            entry: 3,
            prompt: Some("a".repeat(64)),
        };
        let chosen: Chosen = router::pick(
            &fixture.state,
            fixture.organization,
            AgentKind::Claude,
            &[],
            Some(from.id),
            None,
            false,
        )
        .await
        .expect("the other login");
        let mut resolved = backend::on_login(
            fixture.state.config(),
            fixture.organization,
            &chosen,
            Continuation::Chat(Some(&previous)).session(&chosen),
        )
        .expect("a backend on the other login");

        let notice = opening(
            &fixture.state,
            fixture.organization,
            AgentKind::Claude,
            Some(&previous),
            &mut resolved,
            None,
            None,
        )
        .await;
        let unmoved = opening(
            &fixture.state,
            fixture.organization,
            AgentKind::Claude,
            Some(&ChatSession {
                login: Some(to.id),
                ..previous.clone()
            }),
            &mut resolved.clone(),
            None,
            None,
        )
        .await;
        let landed = fixture.carried(&to, SESSION);
        let handover = Handover::new(
            fixture.organization,
            to.clone(),
            fixture.handover(&to).model,
            Some(&previous),
            notice.clone(),
        );
        fixture.remove().await;

        assert_eq!(
            notice,
            Some(Notice {
                from: "a@example.com".into(),
                to: "b@example.com".into(),
                from_agent: AgentKind::Claude,
                agent: AgentKind::Claude,
                reason: Reason::Limit,
                resets_at: Some(resets()),
                carried: true,
                at: 0,
                calls: 0,
            })
        );
        assert!(landed, "the session file moved with the chat");
        assert_eq!(
            cli(&resolved.backend).1.session,
            Some(Session {
                id: SESSION.into(),
                resume: true
            }),
            "the turn resumes the carried session"
        );
        assert_eq!(unmoved, None, "a chat kept on its login is told nothing");
        assert_eq!(
            handover.tried,
            vec![from.id],
            "the login left is not tried again"
        );
    }

    #[tokio::test]
    async fn a_chat_on_another_agent_moves_back_to_the_configured_agent_before_the_turn() {
        let fixture = Fixture::new().await;
        let from = fixture.codex("codex@example.com").await;
        let to = fixture.claude("claude@example.com").await;
        let previous = ChatSession {
            login: Some(from.id),
            id: SESSION.into(),
            agent: AgentKind::Codex,
            entry: 3,
            prompt: Some("a".repeat(64)),
        };
        let chosen: Chosen = router::pick(
            &fixture.state,
            fixture.organization,
            AgentKind::Claude,
            &[],
            Some(from.id),
            None,
            false,
        )
        .await
        .expect("the configured agent's login");
        let mut resolved = backend::on_login(
            fixture.state.config(),
            fixture.organization,
            &chosen,
            Continuation::Chat(Some(&previous)).session(&chosen),
        )
        .expect("a backend on the configured agent's login");

        let notice = opening(
            &fixture.state,
            fixture.organization,
            AgentKind::Claude,
            Some(&previous),
            &mut resolved,
            None,
            None,
        )
        .await;
        fixture.remove().await;

        assert_eq!(chosen.login.id, to.id);
        assert_eq!(
            notice,
            Some(Notice {
                from: "codex@example.com".into(),
                to: "claude@example.com".into(),
                from_agent: AgentKind::Codex,
                agent: AgentKind::Claude,
                reason: Reason::Configured,
                resets_at: None,
                carried: false,
                at: 0,
                calls: 0,
            })
        );
    }

    #[tokio::test]
    async fn a_chat_moved_back_to_the_configured_agent_can_hand_its_turn_back_to_the_login_it_left()
    {
        let fixture = Fixture::new().await;
        let fallback = fixture.codex("codex@example.com").await;
        let configured = fixture.claude("claude@example.com").await;
        let previous = ChatSession {
            login: Some(fallback.id),
            id: SESSION.into(),
            agent: AgentKind::Codex,
            entry: 3,
            prompt: Some("a".repeat(64)),
        };
        let chosen = router::pick(
            &fixture.state,
            fixture.organization,
            AgentKind::Claude,
            &[],
            Some(fallback.id),
            None,
            false,
        )
        .await
        .expect("the configured agent's login");
        let mut resolved = backend::on_login(
            fixture.state.config(),
            fixture.organization,
            &chosen,
            Continuation::Chat(Some(&previous)).session(&chosen),
        )
        .expect("a backend on the configured agent's login");
        let notice = opening(
            &fixture.state,
            fixture.organization,
            AgentKind::Claude,
            Some(&previous),
            &mut resolved,
            None,
            None,
        )
        .await;
        let mut handover = Handover::new(
            fixture.organization,
            configured.clone(),
            fixture.handover(&configured).model,
            Some(&previous),
            notice.clone(),
        );

        let switch = handover
            .next(&fixture.state, &limit(false), None, TIMEOUT)
            .await;
        fixture.remove().await;

        assert_eq!(chosen.login.id, configured.id);
        assert_eq!(notice.map(|notice| notice.reason), Some(Reason::Configured));
        let switch = switch.unwrap_or_else(|ended| {
            panic!(
                "the login the chat left only for the configured agent was barred from the turn: {:?}",
                ended.map(|error| error.to_string())
            )
        });
        assert_eq!(switch.from, configured);
        assert_eq!(switch.to, fallback);
        assert_eq!(switch.reason, Reason::Limit);
    }

    #[tokio::test]
    async fn a_chat_on_a_login_that_refused_its_model_for_credits_moves_before_the_turn_and_says_why()
     {
        let fixture = Fixture::new().await;
        let refused = fixture.claude("a@example.com").await;
        let other = fixture.claude("b@example.com").await;
        let model = fixture.handover(&refused).model.on(AgentKind::Claude);
        router::mark_limited(
            &fixture.state,
            refused.id,
            &limit(true).limit.expect("a credits limit"),
            &model,
        )
        .await;
        let previous = ChatSession {
            login: Some(refused.id),
            id: SESSION.into(),
            agent: AgentKind::Claude,
            entry: 3,
            prompt: Some("a".repeat(64)),
        };
        let chosen = router::pick(
            &fixture.state,
            fixture.organization,
            AgentKind::Claude,
            &[],
            Some(refused.id),
            Some(&model),
            false,
        )
        .await
        .expect("the other login");
        let mut resolved = backend::on_login(
            fixture.state.config(),
            fixture.organization,
            &chosen,
            Continuation::Chat(Some(&previous)).session(&chosen),
        )
        .expect("a backend on the other login");

        let notice = opening(
            &fixture.state,
            fixture.organization,
            AgentKind::Claude,
            Some(&previous),
            &mut resolved,
            Some(&model),
            None,
        )
        .await;
        let exhausted = fixture.exhausted_until(&refused).await;
        fixture.remove().await;

        assert_eq!(chosen.login.id, other.id);
        assert_eq!(
            notice.map(|notice| (notice.reason, notice.resets_at)),
            Some((Reason::Credits, None))
        );
        assert_eq!(exhausted, None);
    }
}
