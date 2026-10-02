use zone_chat::history::NewEntry;
use zone_core::llm::{LlmBackend, Session};

use super::{Generation, RunContext};
use crate::db::chats::ChatSession;
use crate::services::backend::Continuation;
use crate::services::login::identity::LoginIdentity;

/// What a coding agent says, lowercased, when it has no session by the id it was asked to
/// resume: claude's `No conversation found with session ID: <id>`, and codex's
/// `no rollout found for thread id <id>`, or `conversation id` where codex names it so.
const REFUSALS: [&str; 3] = [
    "no conversation found with session id",
    "no rollout found for thread id",
    "no rollout found for conversation id",
];

/// The coding agent session a chat's turn runs in, on one of the organization's logins, and
/// what the chat records of it once the agent has it.
#[derive(Debug, Clone)]
pub struct Pinned {
    login: LoginIdentity,
    /// The session the chat's turns ran in before this one, or the one carried to another login
    /// during it.
    previous: Option<ChatSession>,
    /// The session the agent is given.
    session: Option<Session>,
    /// The id the agent announced for the session it runs in.
    announced: Option<String>,
    /// The stable hash of the system prompt the session has seen once this turn is sent.
    prompt: Option<String>,
    /// The latest `chat_entries.position` the session has seen once this turn is sent.
    entry: i64,
    /// The whole transcript, kept for a resume the agent refuses and for a handover that
    /// replays it on another login.
    replay: Option<RunContext>,
}

impl Pinned {
    /// The session `generation` runs in: one only on a coding agent under a login of the
    /// organization's, never over HTTP or under the instance's or the host's sign-in.
    pub fn of(generation: &Generation) -> Option<Self> {
        let LlmBackend::Cli { settings, .. } = &generation.resolved.backend else {
            return None;
        };
        Some(Self {
            login: generation.resolved.login.clone()?,
            previous: generation.session.clone(),
            session: settings.session.clone(),
            announced: None,
            prompt: None,
            entry: 0,
            replay: None,
        })
    }

    /// The chat's session this turn resumes, when it resumes one.
    pub fn resumed(&self) -> Option<&ChatSession> {
        self.session
            .as_ref()
            .filter(|session| session.resume)
            .and(self.previous.as_ref())
    }

    /// Records that this turn sends the session a system prompt of stable hash `prompt`, and
    /// the chat up to `entry`.
    pub fn sees(&mut self, prompt: String, entry: i64) {
        self.prompt = Some(prompt);
        self.entry = entry;
    }

    pub fn entry(&self) -> i64 {
        self.entry
    }

    /// Keeps `replay`, the whole transcript, for a resume the agent refuses.
    pub fn keep(&mut self, replay: RunContext) {
        self.replay = Some(replay);
    }

    /// The whole transcript this turn would replay, with what it has written since.
    pub fn transcript(&self) -> Option<&RunContext> {
        self.replay.as_ref()
    }

    /// Adds `entry`, written this turn, to the whole transcript kept for a replay.
    pub fn extend(&mut self, entry: &NewEntry) {
        if let Some(replay) = self.replay.as_mut() {
            replay.append(entry);
        }
    }

    /// The id the session is resumed by: the one the agent announced, else the one it was given.
    pub fn id(&self) -> Option<&str> {
        self.announced
            .as_deref()
            .or_else(|| self.session.as_ref().map(|session| session.id.as_str()))
    }

    /// Moves the session onto `login`, in `session`, the one its backend was given there: the
    /// same session resumed when it was carried to that login's home, or a fresh one that will
    /// be sent the whole transcript. The prompt the session has seen is unchanged by either,
    /// since a replay sends it this turn's prompt whole.
    pub fn switch(&mut self, login: LoginIdentity, session: Option<Session>) {
        self.login = login;
        self.session = session;
        self.announced = None;
        self.previous = self
            .session
            .as_ref()
            .filter(|session| session.resume)
            .and_then(|_| self.record(self.entry));
    }

    pub fn announced(&mut self, id: String) {
        self.announced = Some(id);
    }

    /// What the chat records of this session once the agent has seen the chat up to `entry`:
    /// nothing until there is an id to resume it by.
    pub fn record(&self, entry: i64) -> Option<ChatSession> {
        Some(ChatSession {
            login: Some(self.login.id),
            id: self.id()?.to_string(),
            agent: self.login.agent,
            entry,
            prompt: self.prompt.clone(),
        })
    }

    /// Whether `message`, a failure before the agent announced any session, is the agent
    /// refusing to resume one it does not have, while a transcript to replay instead is kept.
    pub fn refused(&self, message: &str) -> bool {
        let message = message.to_lowercase();
        self.resumed().is_some()
            && self.announced.is_none()
            && self.replay.is_some()
            && REFUSALS.iter().any(|refusal| message.contains(refusal))
    }

    /// The whole transcript a refused resume is replayed with instead, once, and `backend` on
    /// a fresh session for it.
    pub fn restart(&mut self, backend: LlmBackend) -> Option<(RunContext, LlmBackend)> {
        self.resumed()?;
        let replay = self.replay.clone()?;
        Some((replay, self.renew(backend)))
    }

    /// `backend` on a fresh session in place of the one it was given.
    pub fn renew(&mut self, backend: LlmBackend) -> LlmBackend {
        self.session = Continuation::fresh(self.login.agent);
        self.announced = None;
        match backend {
            LlmBackend::Cli {
                agent,
                mut settings,
            } => {
                settings.session.clone_from(&self.session);
                LlmBackend::cli(agent, settings)
            }
            LlmBackend::Http => LlmBackend::Http,
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;
    use zone_core::llm::{AgentKind, CliSettings, Message};

    use super::*;
    use crate::services::backend::Resolved;

    const CLAUDE_REFUSAL: &str = "Stream error: claude: No conversation found with session ID: 6f1";
    const CODEX_REFUSAL: &str = "Stream error: codex: Error: thread/resume: thread/resume failed: \
         no rollout found for thread id 019a0000-0000-7000-8000-000000000000 (code -32600)";

    fn login(agent: AgentKind) -> LoginIdentity {
        LoginIdentity {
            id: Uuid::new_v4(),
            agent,
            label: "jake@example.com".into(),
        }
    }

    fn previous(login: &LoginIdentity) -> ChatSession {
        ChatSession {
            login: Some(login.id),
            id: "previous".into(),
            agent: login.agent,
            entry: 4,
            prompt: Some("a".repeat(64)),
        }
    }

    fn generation(login: &LoginIdentity, session: Option<Session>) -> Generation {
        let settings = CliSettings {
            session,
            ..CliSettings::default()
        };
        Generation {
            resolved: Resolved {
                backend: LlmBackend::cli(login.agent, settings),
                login: Some(login.clone()),
            },
            session: Some(previous(login)),
        }
    }

    fn resuming(login: &LoginIdentity) -> Pinned {
        let mut pinned = Pinned::of(&generation(
            login,
            Some(Session {
                id: "previous".into(),
                resume: true,
            }),
        ))
        .expect("a session on a login");
        pinned.sees("b".repeat(64), 6);
        pinned.keep(RunContext::from_messages(vec![Message::user("Hello")]));
        pinned
    }

    fn session(backend: &LlmBackend) -> Option<&Session> {
        match backend {
            LlmBackend::Cli { settings, .. } => settings.session.as_ref(),
            LlmBackend::Http => None,
        }
    }

    #[test]
    fn only_a_coding_agent_on_a_login_keeps_a_session() {
        let claude = login(AgentKind::Claude);
        assert!(Pinned::of(&Generation::unrouted(LlmBackend::Http)).is_none());
        assert!(
            Pinned::of(&Generation::unrouted(LlmBackend::cli(
                AgentKind::Claude,
                CliSettings::default()
            )))
            .is_none(),
            "the host's sign-in kept a session"
        );
        assert!(Pinned::of(&generation(&claude, None)).is_some());
    }

    #[test]
    fn a_session_is_recorded_by_the_id_the_agent_announced() {
        let claude = login(AgentKind::Claude);
        let mut pinned = resuming(&claude);
        assert_eq!(
            pinned.record(7).map(|session| session.id),
            Some("previous".to_string())
        );

        pinned.announced("announced".into());
        let recorded = pinned.record(7).expect("a recorded session");

        assert_eq!(recorded.id, "announced");
        assert_eq!(recorded.login, Some(claude.id));
        assert_eq!(recorded.agent, AgentKind::Claude);
        assert_eq!(recorded.entry, 7);
        assert_eq!(recorded.prompt, Some("b".repeat(64)));
    }

    #[test]
    fn a_codex_thread_is_recorded_only_once_announced() {
        let codex = login(AgentKind::Codex);
        let mut pinned = Pinned::of(&generation(&codex, None)).expect("a session on a login");
        pinned.sees("c".repeat(64), 2);
        assert_eq!(pinned.record(2), None);
        assert_eq!(pinned.resumed(), None);

        pinned.announced("thread".into());

        assert_eq!(
            pinned.record(2).map(|session| session.id),
            Some("thread".to_string())
        );
    }

    #[test]
    fn a_refused_resume_restarts_once_on_a_fresh_session() {
        for (agent, refusal) in [
            (AgentKind::Claude, CLAUDE_REFUSAL),
            (AgentKind::Codex, CODEX_REFUSAL),
        ] {
            let identity = login(agent);
            let mut pinned = resuming(&identity);
            assert!(pinned.refused(refusal), "{agent}: {refusal}");
            assert!(!pinned.refused("Stream error: claude: You've hit your session limit"));

            let backend = generation(&identity, None).resolved.backend;
            let (replay, backend) = pinned.restart(backend).expect("a replay");

            assert_eq!(replay.entries.len(), 1);
            match agent {
                AgentKind::Claude => {
                    let fresh = session(&backend).expect("claude pinned to a fresh id");
                    assert!(!fresh.resume);
                    assert_ne!(fresh.id, "previous");
                    assert_eq!(
                        pinned.record(6).map(|session| session.id),
                        Some(fresh.id.clone())
                    );
                }
                AgentKind::Codex => {
                    assert_eq!(session(&backend), None, "codex names its own thread");
                    assert_eq!(pinned.record(6), None);
                }
            }
            assert_eq!(pinned.resumed(), None);
            assert!(!pinned.refused(refusal), "{agent} restarted a second time");
            assert!(pinned.restart(LlmBackend::Http).is_none());
        }
    }

    #[test]
    fn a_missing_model_command_or_page_is_no_refused_resume() {
        for agent in [AgentKind::Claude, AgentKind::Codex] {
            let pinned = resuming(&login(agent));
            for failure in [
                "Stream error: claude: model not found: claude-opus-9",
                "Stream error: codex: sh: codex: command not found",
                "Stream error: codex: unexpected status 404 Not Found: {\"detail\":\"Not Found\"}",
            ] {
                assert!(!pinned.refused(failure), "{agent}: {failure}");
            }
        }
    }

    #[test]
    fn a_failure_after_the_agent_announced_its_session_is_no_refusal() {
        let mut pinned = resuming(&login(AgentKind::Claude));
        pinned.announced("previous".into());

        assert!(!pinned.refused(CLAUDE_REFUSAL));
    }
}
