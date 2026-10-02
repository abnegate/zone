use chrono::{DateTime, Utc};
use zone_core::llm::{LlmBackend, Session};

use super::{Notice, Reason};
use crate::services::login::identity::LoginIdentity;

/// A turn moved onto another login: the backend it continues on, and how it got there.
#[derive(Debug, Clone)]
pub struct Switch {
    /// The new login's agent, in its home and its agent's working directory, bounded by what is
    /// left of the turn.
    pub backend: LlmBackend,
    pub from: LoginIdentity,
    pub to: LoginIdentity,
    pub reason: Reason,
    pub resets_at: Option<DateTime<Utc>>,
    /// Whether the session file was carried into the new login's home and is resumed there.
    pub carried: bool,
    /// The model the turn continues on, when the agent changed and the old one's model is
    /// not the new one's to run.
    pub model: Option<String>,
}

impl Switch {
    /// The agent session the turn continues in.
    pub fn session(&self) -> Option<Session> {
        match &self.backend {
            LlmBackend::Cli { settings, .. } => settings.session.clone(),
            LlmBackend::Http => None,
        }
    }

    /// The notice of this switch, made once `at` scalar values of the answer were written.
    pub fn notice(&self, at: usize) -> Notice {
        Notice {
            from: self.from.label.clone(),
            to: self.to.label.clone(),
            agent: self.to.agent,
            reason: self.reason,
            resets_at: self.resets_at,
            carried: self.carried,
            at,
        }
    }
}
