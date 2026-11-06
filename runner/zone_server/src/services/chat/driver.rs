//! The coding agent CLI a chat runs on, which alone knows the window its model has.
//!
//! The instance's LiteLLM deployment knows nothing of the model a CLI runs, and
//! a context a person chose for a self-hosted model does not bound one, so an
//! agent chat's window is only ever what the agent itself last reported.

use abnegate_agent_cli::AgentKind;
use abnegate_agent_cli::Occupancy;
use sqlx::PgPool;
use uuid::Uuid;
use zone_chat::capacity::Capacity;
use zone_core::llm::LlmBackend;

use crate::db::chats::occupancy::{self, Reported};

const UNREPORTED: &str =
    "The coding agent reports its model's context window after its first turn on this model.";

/// The agent a chat's turns run on, and the model the chat asks it for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Driver {
    pub agent: AgentKind,
    pub model: String,
}

impl Driver {
    pub fn new(agent: AgentKind, model: impl Into<String>) -> Self {
        Self {
            agent,
            model: model.into(),
        }
    }

    /// The driver of a turn on `backend`, when a coding agent runs it.
    pub fn on(backend: &LlmBackend, model: impl Into<String>) -> Option<Self> {
        match backend {
            LlmBackend::Cli { agent, .. } => Some(Self::new(*agent, model)),
            LlmBackend::Http => None,
        }
    }

    /// The window the agent last reported for this chat on this model, and
    /// how much of it the chat's last turn filled. Unknown until it reports.
    pub async fn capacity(&self, pool: &PgPool, chat_id: Uuid) -> (Capacity, Option<u64>) {
        let reported = match occupancy::get(pool, chat_id).await {
            Ok(reported) => reported,
            Err(error) => {
                tracing::warn!(%chat_id, %error, "Could not read the context window the chat's agent reported");
                None
            }
        };
        match reported
            .filter(|reported| reported.agent == self.agent && reported.model == self.model)
        {
            Some(reported) => (
                Capacity::reported(&self.model, reported.occupancy.window),
                reported.occupancy.used,
            ),
            None => (Capacity::unknown(&self.model, UNREPORTED), None),
        }
    }

    /// Keeps what the agent reported of this turn for the chat's next.
    pub async fn record(&self, pool: &PgPool, chat_id: Uuid, occupancy: Occupancy) {
        let reported = Reported {
            agent: self.agent,
            model: self.model.clone(),
            occupancy,
        };
        if let Err(error) = occupancy::set(pool, chat_id, &reported).await {
            tracing::warn!(%chat_id, %error, "Could not keep the context window the chat's agent reported");
        }
    }
}
