use chrono::{DateTime, Utc};
use zone_core::llm::AgentKind;

use crate::services::login::credential;

/// Why no login of an organization can run a session.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The organization holds no login of any agent.
    #[error("the organization holds no sign-in")]
    None,
    /// Every login left has reached a usage limit, or was tried this turn; `resets_at` is the
    /// earliest any of them says it resets.
    #[error("every sign-in has reached its usage limit")]
    Exhausted { resets_at: Option<DateTime<Utc>> },
    /// No login could be made ready for a turn: `source` is why for one of `agent`'s, the
    /// configured agent's when it has one.
    #[error("the {agent} sign-in cannot be used: {source}")]
    Unresolved {
        agent: AgentKind,
        #[source]
        source: credential::Error,
    },
    #[error("could not read the organization's sign-ins: {0}")]
    Database(#[from] sqlx::Error),
}
