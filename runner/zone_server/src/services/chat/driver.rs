//! The coding agent CLI a chat runs on, which alone knows the window its model has.
//!
//! The instance's LiteLLM deployment knows nothing of the model a CLI runs, and
//! a context a person chose for a self-hosted model does not bound one, so an
//! agent chat's window is only ever what the agent itself last reported.

use abnegate_agent::chat::Capacity;
use abnegate_agent_cli::AgentKind;
use abnegate_agent_cli::Occupancy;
use sqlx::PgPool;
use uuid::Uuid;
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

#[cfg(test)]
mod tests {
    use abnegate_agent::context::ContextSource;
    use abnegate_agent_cli::CliSettings;

    use super::*;
    use crate::db::chats::create_chat;

    const MODEL: &str = "sonnet";
    const WINDOW: u64 = 200_000;
    const USED: u64 = 48_000;

    async fn chat() -> (PgPool, Uuid) {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .expect("the test database accepts connections");
        let chat_id = create_chat(&pool, None, "Driver", MODEL, false, true)
            .await
            .expect("a chat")
            .id;
        (pool, chat_id)
    }

    fn known(capacity: &(Capacity, Option<u64>)) -> (Option<u64>, ContextSource, Option<u64>) {
        (capacity.0.limit, capacity.0.source, capacity.1)
    }

    #[test]
    fn only_a_coding_agent_drives_a_chat() {
        assert_eq!(Driver::on(&LlmBackend::Http, MODEL), None);
        for &agent in AgentKind::ALL {
            assert_eq!(
                Driver::on(&LlmBackend::cli(agent, CliSettings::default()), MODEL),
                Some(Driver::new(agent, MODEL))
            );
        }
    }

    #[test]
    fn a_zero_reported_window_is_unknown_with_its_reason() {
        let capacity = Capacity::reported(MODEL, 0);

        assert_eq!(capacity.limit, None);
        assert_eq!(capacity.source, ContextSource::Unknown);
        assert_eq!(
            capacity.reason.as_deref(),
            Some("The deployment's server reported no context window.")
        );
        assert_eq!(capacity.identity, MODEL);
    }

    #[tokio::test]
    async fn a_chats_window_is_unknown_until_its_agent_reports_it() {
        let (pool, chat_id) = chat().await;
        let claude = Driver::new(AgentKind::Claude, MODEL);

        let before = claude.capacity(&pool, chat_id).await;
        claude
            .record(
                &pool,
                chat_id,
                Occupancy::reported(WINDOW, Some(USED)).expect("a reported window"),
            )
            .await;
        let after = claude.capacity(&pool, chat_id).await;

        assert_eq!(known(&before), (None, ContextSource::Unknown, None));
        assert_eq!(before.0.reason.as_deref(), Some(UNREPORTED));
        assert_eq!(
            known(&after),
            (Some(WINDOW), ContextSource::Runtime, Some(USED))
        );
        assert_eq!(after.0.identity, MODEL);
    }

    #[tokio::test]
    async fn a_window_another_agent_or_model_reported_is_not_this_drivers() {
        let (pool, chat_id) = chat().await;
        Driver::new(AgentKind::Claude, MODEL)
            .record(
                &pool,
                chat_id,
                Occupancy::reported(WINDOW, Some(USED)).expect("a reported window"),
            )
            .await;

        for other in [
            Driver::new(AgentKind::Codex, MODEL),
            Driver::new(AgentKind::Claude, "opus"),
        ] {
            assert_eq!(
                known(&other.capacity(&pool, chat_id).await),
                (None, ContextSource::Unknown, None),
                "{other:?}"
            );
        }
    }

    #[tokio::test]
    async fn the_latest_report_replaces_the_last() {
        let (pool, chat_id) = chat().await;
        let claude = Driver::new(AgentKind::Claude, MODEL);
        let codex = Driver::new(AgentKind::Codex, "gpt-5.5");

        claude
            .record(
                &pool,
                chat_id,
                Occupancy::reported(WINDOW, Some(USED)).expect("a reported window"),
            )
            .await;
        codex
            .record(
                &pool,
                chat_id,
                Occupancy::reported(WINDOW * 2, None).expect("a reported window"),
            )
            .await;

        assert_eq!(
            known(&codex.capacity(&pool, chat_id).await),
            (Some(WINDOW * 2), ContextSource::Runtime, None)
        );
        assert_eq!(
            known(&claude.capacity(&pool, chat_id).await),
            (None, ContextSource::Unknown, None),
            "a report belongs to the agent that made it"
        );
    }
}
