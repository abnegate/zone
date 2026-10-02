use uuid::Uuid;
use zone_core::llm::{AgentKind, Session};

use crate::db::chats::ChatSession;
use crate::services::login::router::Chosen;

/// Which of a coding agent's own sessions a routed backend runs in.
#[derive(Debug, Clone, Copy, Default)]
pub enum Continuation<'a> {
    /// No session pinned: the agent picks its own, as a task run or a classifier does.
    #[default]
    Unpinned,
    /// A chat's turn: the chat's session resumed while it is on the login and agent picked, or
    /// a fresh one.
    Chat(Option<&'a ChatSession>),
}

impl Continuation<'_> {
    /// The session `chosen` runs this turn in.
    ///
    /// A session lives in the home of the login that started it, so one left on another login
    /// or agent is not resumed.
    pub fn session(self, chosen: &Chosen) -> Option<Session> {
        match self {
            Self::Unpinned => None,
            Self::Chat(Some(previous))
                if previous.agent == chosen.agent && previous.login == Some(chosen.login.id) =>
            {
                Some(Session {
                    id: previous.id.clone(),
                    resume: true,
                })
            }
            Self::Chat(_) => Self::fresh(chosen.agent),
        }
    }

    /// A new session for `agent`: pinned to an id of zone's for claude, while codex names its
    /// own thread and announces it.
    pub fn fresh(agent: AgentKind) -> Option<Session> {
        match agent {
            AgentKind::Claude => Some(Session {
                id: Uuid::new_v4().to_string(),
                resume: false,
            }),
            AgentKind::Codex => None,
        }
    }
}
