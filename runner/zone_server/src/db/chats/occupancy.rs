//! The context window a coding agent last reported running a chat's turn in.

use sqlx::{Executor, Postgres};
use uuid::Uuid;
use zone_core::llm::AgentKind;
use zone_core::llm::provider::Occupancy;

use crate::db::DbResult;

/// What the agent reported, for the model the chat asked it for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    pub agent: AgentKind,
    pub model: String,
    pub occupancy: Occupancy,
}

/// The chat's latest report, or `None` when no agent turn reported one.
pub async fn get<'e, E>(executor: E, chat_id: Uuid) -> DbResult<Option<Reported>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: Option<(String, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT agent, model, window_tokens, used_tokens FROM chat_agent_contexts WHERE chat_id = $1",
    )
    .bind(chat_id)
    .fetch_optional(executor)
    .await?;
    Ok(row.and_then(|(agent, model, window, used)| {
        Some(Reported {
            agent: AgentKind::named(&agent)?,
            model,
            occupancy: Occupancy::reported(
                u64::try_from(window).ok(),
                used.and_then(|used| u64::try_from(used).ok()),
            )?,
        })
    }))
}

/// Records `reported` as the chat's latest report.
pub async fn set<'e, E>(executor: E, chat_id: Uuid, reported: &Reported) -> DbResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "INSERT INTO chat_agent_contexts (chat_id, agent, model, window_tokens, used_tokens) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (chat_id) DO UPDATE SET agent = EXCLUDED.agent, model = EXCLUDED.model, \
         window_tokens = EXCLUDED.window_tokens, used_tokens = EXCLUDED.used_tokens, updated_at = NOW()",
    )
    .bind(chat_id)
    .bind(reported.agent.as_str())
    .bind(&reported.model)
    .bind(i64::try_from(reported.occupancy.window).unwrap_or(i64::MAX))
    .bind(
        reported
            .occupancy
            .used
            .map(|used| i64::try_from(used).unwrap_or(i64::MAX)),
    )
    .execute(executor)
    .await?;
    Ok(())
}
