use uuid::Uuid;

use super::Continuation;

/// How a coding agent backend is routed to one of the organization's logins.
#[derive(Debug, Clone, Copy, Default)]
pub struct Routing<'a> {
    /// Logins never to pick: those this turn already tried.
    pub exclude: &'a [Uuid],
    /// The login the session already runs on, kept while it can run.
    pub sticky: Option<Uuid>,
    /// Whether the login picked is recorded as used: only when a session or run starts on it.
    pub touch: bool,
    /// The agent's own session the backend runs in.
    pub continuation: Continuation<'a>,
    /// The model the session runs on the configured agent, which a login that lately refused it
    /// for want of usage credits is passed over for.
    pub model: Option<&'a str>,
}
