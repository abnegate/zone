use std::path::PathBuf;
use std::sync::Arc;

use zone_core::llm::LlmBackend;
use zone_enterprise::policy::Effective;

use crate::db::chats::ChatSession;
use crate::db::tasks::Origin;
use crate::services::backend::Resolved;

/// A turn a model answers: the backend it was routed to, the agent session the chat's turns ran
/// in before it, the directory its project starts it in, when it has one, what opened it, and
/// the organization's agent policy its tools are held to, when one is enforced.
#[derive(Debug, Clone)]
pub struct Generation {
    pub resolved: Resolved,
    pub session: Option<ChatSession>,
    pub directory: Option<PathBuf>,
    pub origin: Origin,
    pub policy: Option<Arc<Effective>>,
}

impl Generation {
    /// A turn on `backend`, run under no login and in no session of the chat's.
    pub fn unrouted(backend: LlmBackend) -> Self {
        Self {
            resolved: Resolved::unrouted(backend),
            session: None,
            directory: None,
            origin: Origin::Member,
            policy: None,
        }
    }
}
