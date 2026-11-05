//! The subagents each chat's coding agent started for itself.

use chrono::{DateTime, Utc};
use sqlx::types::Json;
use sqlx::{Executor, Postgres};
use uuid::Uuid;
use zone_core::llm::{SubagentStatus, SubagentStep};

use super::DbResult;

macro_rules! columns {
    () => {
        "id, chat_id, message_id, external_id, description, kind, model, background, status, \
         tool_uses, duration_ms, tokens, last_tool, summary, error, steps, started_at, \
         finished_at, updated_at"
    };
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct SubagentRow {
    pub id: Uuid,
    pub chat_id: Uuid,
    /// The turn that started it, which is also its assistant message's id.
    pub message_id: Option<Uuid>,
    /// The agent's own name for it: Claude's Agent call id, or codex's thread id.
    pub external_id: String,
    pub description: String,
    pub kind: Option<String>,
    pub model: Option<String>,
    pub background: bool,
    /// One of [`SubagentStatus`]'s names; the table's check refuses any other.
    pub status: String,
    pub tool_uses: Option<i64>,
    pub duration_ms: Option<i64>,
    pub tokens: Option<i64>,
    pub last_tool: Option<String>,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub steps: Json<Vec<SubagentStep>>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// What one update says about a chat's subagent. Every `None` leaves the
/// stored value as it is, or the column's default when the row is new.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubagentChange<'a> {
    pub chat_id: Uuid,
    pub message_id: Option<Uuid>,
    pub external_id: &'a str,
    pub description: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub model: Option<&'a str>,
    pub background: Option<bool>,
    pub status: Option<SubagentStatus>,
    pub tool_uses: Option<i64>,
    pub duration_ms: Option<i64>,
    pub tokens: Option<i64>,
    pub last_tool: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub error: Option<&'a str>,
    pub steps: Option<&'a [SubagentStep]>,
}

/// Records what `change` says about one subagent, creating its row the first
/// time the chat hears of it.
///
/// The turn that first named it keeps it. A finished subagent never runs
/// again, and `stopped`, which is all a sweep or a shutdown knows, never hides
/// the completion or failure it already reported; a reported outcome may still
/// replace a `stopped` one.
pub async fn record<'e, E>(executor: E, change: &SubagentChange<'_>) -> DbResult<SubagentRow>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "INSERT INTO chat_subagents AS s (
             chat_id, message_id, external_id, description, kind, model, background, status,
             tool_uses, duration_ms, tokens, last_tool, summary, error, steps, finished_at
         )
         VALUES (
             $1, $2, $3, COALESCE($4, ''), $5, $6, COALESCE($7, false), COALESCE($8, 'running'),
             $9, $10, $11, $12, $13, $14, COALESCE($15, '[]'::jsonb),
             CASE WHEN COALESCE($8, 'running') <> 'running' THEN now() END
         )
         ON CONFLICT (chat_id, external_id) DO UPDATE SET
             message_id = COALESCE(s.message_id, $2),
             description = COALESCE($4, s.description),
             kind = COALESCE($5, s.kind),
             model = COALESCE($6, s.model),
             background = COALESCE($7, s.background),
             status = CASE
                 WHEN $8 IS NULL THEN s.status
                 WHEN s.status = 'running' THEN $8
                 WHEN $8 IN ('running', 'stopped') THEN s.status
                 ELSE $8
             END,
             tool_uses = COALESCE($9, s.tool_uses),
             duration_ms = COALESCE($10, s.duration_ms),
             tokens = COALESCE($11, s.tokens),
             last_tool = COALESCE($12, s.last_tool),
             summary = COALESCE($13, s.summary),
             error = COALESCE($14, s.error),
             steps = COALESCE($15, s.steps),
             finished_at = COALESCE(
                 s.finished_at,
                 CASE WHEN COALESCE($8, s.status) <> 'running' THEN now() END
             ),
             updated_at = now()
         RETURNING ",
        columns!()
    ))
    .bind(change.chat_id)
    .bind(change.message_id)
    .bind(change.external_id)
    .bind(change.description)
    .bind(change.kind)
    .bind(change.model)
    .bind(change.background)
    .bind(change.status.map(SubagentStatus::as_str))
    .bind(change.tool_uses)
    .bind(change.duration_ms)
    .bind(change.tokens)
    .bind(change.last_tool)
    .bind(change.summary)
    .bind(change.error)
    .bind(change.steps.map(Json))
    .fetch_one(executor)
    .await
}

/// Every subagent `chat_id` started, oldest first.
pub async fn list<'e, E>(executor: E, chat_id: Uuid) -> DbResult<Vec<SubagentRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM chat_subagents WHERE chat_id = $1 ORDER BY started_at, id"
    ))
    .bind(chat_id)
    .fetch_all(executor)
    .await
}

/// Stops the subagents turn `message_id` left running when it ended, and
/// returns them oldest first.
pub async fn stop<'e, E>(executor: E, chat_id: Uuid, message_id: Uuid) -> DbResult<Vec<SubagentRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "WITH stopped AS (
             UPDATE chat_subagents
                SET status = 'stopped', finished_at = COALESCE(finished_at, now()), updated_at = now()
              WHERE chat_id = $1 AND message_id = $2 AND status = 'running'
             RETURNING *
         )
         SELECT ",
        columns!(),
        " FROM stopped ORDER BY started_at, id"
    ))
    .bind(chat_id)
    .bind(message_id)
    .fetch_all(executor)
    .await
}

/// Stops the subagents any turn of `chat_id` other than `message_id` left
/// running, and returns them oldest first.
///
/// A turn whose server died keeps its `chat_turns` row running until the
/// chat's next turn settles it, so [`abandon`] cannot see its subagents at
/// boot. The next turn calls this as it starts instead.
pub async fn stop_others<'e, E>(
    executor: E,
    chat_id: Uuid,
    message_id: Uuid,
) -> DbResult<Vec<SubagentRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "WITH stopped AS (
             UPDATE chat_subagents
                SET status = 'stopped', finished_at = COALESCE(finished_at, now()), updated_at = now()
              WHERE chat_id = $1 AND message_id IS DISTINCT FROM $2 AND status = 'running'
             RETURNING *
         )
         SELECT ",
        columns!(),
        " FROM stopped ORDER BY started_at, id"
    ))
    .bind(chat_id)
    .bind(message_id)
    .fetch_all(executor)
    .await
}

/// Stops every subagent left running by a turn that is no longer running, and
/// returns how many it stopped. The server calls it once as it boots, when no
/// turn of its own can still be reporting on them.
pub async fn abandon<'e, E>(executor: E) -> DbResult<u64>
where
    E: Executor<'e, Database = Postgres>,
{
    Ok(sqlx::query(
        "UPDATE chat_subagents s
            SET status = 'stopped', finished_at = COALESCE(s.finished_at, now()), updated_at = now()
          WHERE s.status = 'running'
            AND NOT EXISTS (
                SELECT 1 FROM chat_turns t
                 WHERE t.chat_id = s.chat_id AND t.id = s.message_id AND t.status = 'running')",
    )
    .execute(executor)
    .await?
    .rows_affected())
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;
    use zone_core::llm::StepStatus;

    use super::*;
    use crate::db::chats::create_chat;

    async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database accepts connections")
    }

    async fn chat(pool: &PgPool) -> Uuid {
        create_chat(pool, None, "Subagents", "sonnet", false, true)
            .await
            .expect("a chat")
            .id
    }

    fn named(chat_id: Uuid, external_id: &str) -> SubagentChange<'_> {
        SubagentChange {
            chat_id,
            external_id,
            ..SubagentChange::default()
        }
    }

    fn step(id: &str, status: StepStatus) -> SubagentStep {
        SubagentStep {
            id: id.to_string(),
            name: "Bash".to_string(),
            subject: Some("cargo test".to_string()),
            status,
        }
    }

    async fn running(pool: &PgPool, chat_id: Uuid, external_id: &str, turn: Option<Uuid>) {
        record(
            pool,
            &SubagentChange {
                message_id: turn,
                ..named(chat_id, external_id)
            },
        )
        .await
        .expect("a running subagent");
    }

    async fn statuses(pool: &PgPool, chat_id: Uuid) -> Vec<(String, String)> {
        list(pool, chat_id)
            .await
            .expect("the list")
            .into_iter()
            .map(|row| (row.external_id, row.status))
            .collect()
    }

    #[tokio::test]
    async fn a_new_subagent_takes_the_columns_defaults() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;

        let row = record(&pool, &named(chat_id, "toolu_01Agent"))
            .await
            .expect("a subagent");

        assert_eq!(row.chat_id, chat_id);
        assert_eq!(row.message_id, None);
        assert_eq!(row.external_id, "toolu_01Agent");
        assert_eq!(row.description, "");
        assert!(!row.background);
        assert_eq!(row.status, SubagentStatus::Running.as_str());
        assert_eq!(row.steps.0, []);
        assert_eq!(row.finished_at, None);
        assert_eq!(
            (
                row.kind,
                row.model,
                row.tool_uses,
                row.duration_ms,
                row.tokens
            ),
            (None, None, None, None, None)
        );
        assert_eq!((row.last_tool, row.summary, row.error), (None, None, None));
    }

    #[tokio::test]
    async fn an_update_changes_only_what_it_names() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;
        let turn = Uuid::new_v4();
        let steps = [step("toolu_01Step", StepStatus::Running)];
        let first = record(
            &pool,
            &SubagentChange {
                message_id: Some(turn),
                description: Some("Ask for a review"),
                kind: Some("general-purpose"),
                model: Some("sonnet"),
                background: Some(true),
                tool_uses: Some(3),
                steps: Some(&steps),
                ..named(chat_id, "toolu_01Agent")
            },
        )
        .await
        .expect("the first update");

        let second = record(
            &pool,
            &SubagentChange {
                tokens: Some(18_000),
                last_tool: Some("Bash"),
                ..named(chat_id, "toolu_01Agent")
            },
        )
        .await
        .expect("the second update");

        assert_eq!(second.id, first.id);
        assert_eq!(second.message_id, Some(turn));
        assert_eq!(second.description, "Ask for a review");
        assert_eq!(second.kind.as_deref(), Some("general-purpose"));
        assert_eq!(second.model.as_deref(), Some("sonnet"));
        assert!(second.background);
        assert_eq!(second.tool_uses, Some(3));
        assert_eq!(second.tokens, Some(18_000));
        assert_eq!(second.last_tool.as_deref(), Some("Bash"));
        assert_eq!(second.steps.0, steps);
        assert_eq!(second.status, SubagentStatus::Running.as_str());
        assert!(second.updated_at >= first.updated_at);
        assert_eq!(list(&pool, chat_id).await.expect("the list"), [second]);
    }

    #[tokio::test]
    async fn the_turn_that_first_named_a_subagent_keeps_it() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;
        let first = Uuid::new_v4();
        running(&pool, chat_id, "thread-1", Some(first)).await;

        let row = record(
            &pool,
            &SubagentChange {
                message_id: Some(Uuid::new_v4()),
                ..named(chat_id, "thread-1")
            },
        )
        .await
        .expect("a later update");

        assert_eq!(row.message_id, Some(first));
    }

    #[tokio::test]
    async fn a_turn_names_a_subagent_first_heard_of_without_one() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;
        let turn = Uuid::new_v4();
        running(&pool, chat_id, "thread-1", None).await;

        let row = record(
            &pool,
            &SubagentChange {
                message_id: Some(turn),
                ..named(chat_id, "thread-1")
            },
        )
        .await
        .expect("a later update");

        assert_eq!(row.message_id, Some(turn));
    }

    #[tokio::test]
    async fn a_finished_subagent_never_runs_again_and_finishes_once() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;
        running(&pool, chat_id, "toolu_01Agent", None).await;
        let completed = record(
            &pool,
            &SubagentChange {
                status: Some(SubagentStatus::Completed),
                summary: Some("Looks good"),
                ..named(chat_id, "toolu_01Agent")
            },
        )
        .await
        .expect("the completion");
        let finished = completed.finished_at.expect("a finish time");

        for status in [SubagentStatus::Running, SubagentStatus::Stopped] {
            let row = record(
                &pool,
                &SubagentChange {
                    status: Some(status),
                    ..named(chat_id, "toolu_01Agent")
                },
            )
            .await
            .expect("a late update");
            assert_eq!(row.status, SubagentStatus::Completed.as_str(), "{status:?}");
            assert_eq!(row.finished_at, Some(finished), "{status:?}");
        }
        assert_eq!(completed.summary.as_deref(), Some("Looks good"));
    }

    #[tokio::test]
    async fn a_reported_outcome_replaces_a_stop_but_keeps_its_finish_time() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;
        let stopped = record(
            &pool,
            &SubagentChange {
                status: Some(SubagentStatus::Stopped),
                ..named(chat_id, "thread-1")
            },
        )
        .await
        .expect("a stopped subagent");

        let failed = record(
            &pool,
            &SubagentChange {
                status: Some(SubagentStatus::Failed),
                error: Some("exit 1"),
                ..named(chat_id, "thread-1")
            },
        )
        .await
        .expect("its failure");

        assert_eq!(failed.status, SubagentStatus::Failed.as_str());
        assert_eq!(failed.error.as_deref(), Some("exit 1"));
        assert!(stopped.finished_at.is_some());
        assert_eq!(failed.finished_at, stopped.finished_at);
    }

    #[tokio::test]
    async fn a_subagent_first_heard_of_finished_is_finished_now() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;

        let row = record(
            &pool,
            &SubagentChange {
                status: Some(SubagentStatus::Completed),
                ..named(chat_id, "thread-1")
            },
        )
        .await
        .expect("a finished subagent");

        assert_eq!(row.status, SubagentStatus::Completed.as_str());
        assert!(row.finished_at.is_some());
    }

    #[tokio::test]
    async fn the_same_name_in_another_chat_is_another_subagent() {
        let pool = pool().await;
        let first = chat(&pool).await;
        let second = chat(&pool).await;

        let one = record(&pool, &named(first, "toolu_01Agent"))
            .await
            .expect("one");
        let other = record(&pool, &named(second, "toolu_01Agent"))
            .await
            .expect("the other");

        assert_ne!(one.id, other.id);
        assert_eq!(list(&pool, first).await.expect("the list"), [one]);
    }

    #[tokio::test]
    async fn a_chat_lists_its_subagents_oldest_first() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;
        running(&pool, chat_id, "newer", None).await;
        running(&pool, chat_id, "older", None).await;
        sqlx::query(
            "UPDATE chat_subagents SET started_at = started_at - INTERVAL '1 minute'
             WHERE chat_id = $1 AND external_id = 'older'",
        )
        .bind(chat_id)
        .execute(&pool)
        .await
        .expect("an older subagent");

        let names: Vec<String> = list(&pool, chat_id)
            .await
            .expect("the list")
            .into_iter()
            .map(|row| row.external_id)
            .collect();

        assert_eq!(names, ["older", "newer"]);
    }

    #[tokio::test]
    async fn stopping_a_turn_stops_only_its_running_subagents() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;
        let other = chat(&pool).await;
        let turn = Uuid::new_v4();
        let earlier = Uuid::new_v4();
        running(&pool, chat_id, "mine", Some(turn)).await;
        running(&pool, chat_id, "earlier", Some(earlier)).await;
        running(&pool, chat_id, "unnamed", None).await;
        running(&pool, other, "elsewhere", Some(turn)).await;
        record(
            &pool,
            &SubagentChange {
                message_id: Some(turn),
                status: Some(SubagentStatus::Completed),
                ..named(chat_id, "done")
            },
        )
        .await
        .expect("a completed subagent");

        let stopped = stop(&pool, chat_id, turn).await.expect("the stop");

        assert_eq!(stopped.len(), 1, "{stopped:?}");
        assert_eq!(stopped[0].external_id, "mine");
        assert_eq!(stopped[0].status, SubagentStatus::Stopped.as_str());
        assert!(stopped[0].finished_at.is_some());
        let mut seen = statuses(&pool, chat_id).await;
        seen.sort();
        assert_eq!(
            seen,
            [
                ("done".to_string(), "completed".to_string()),
                ("earlier".to_string(), "running".to_string()),
                ("mine".to_string(), "stopped".to_string()),
                ("unnamed".to_string(), "running".to_string()),
            ]
        );
        assert_eq!(
            statuses(&pool, other).await,
            [("elsewhere".to_string(), "running".to_string())]
        );
    }

    #[tokio::test]
    async fn starting_a_turn_stops_the_running_subagents_of_every_other_turn() {
        let pool = pool().await;
        let chat_id = chat(&pool).await;
        let other = chat(&pool).await;
        let turn = Uuid::new_v4();
        let earlier = Uuid::new_v4();
        running(&pool, chat_id, "mine", Some(turn)).await;
        running(&pool, chat_id, "earlier", Some(earlier)).await;
        running(&pool, chat_id, "unnamed", None).await;
        running(&pool, other, "elsewhere", Some(earlier)).await;
        record(
            &pool,
            &SubagentChange {
                message_id: Some(earlier),
                status: Some(SubagentStatus::Failed),
                ..named(chat_id, "failed")
            },
        )
        .await
        .expect("a failed subagent");

        let stopped = stop_others(&pool, chat_id, turn).await.expect("the sweep");

        let mut names: Vec<&str> = stopped.iter().map(|row| row.external_id.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["earlier", "unnamed"]);
        assert!(
            stopped
                .iter()
                .all(|row| row.status == SubagentStatus::Stopped.as_str()
                    && row.finished_at.is_some())
        );
        let mut seen = statuses(&pool, chat_id).await;
        seen.sort();
        assert_eq!(
            seen,
            [
                ("earlier".to_string(), "stopped".to_string()),
                ("failed".to_string(), "failed".to_string()),
                ("mine".to_string(), "running".to_string()),
                ("unnamed".to_string(), "stopped".to_string()),
            ]
        );
        assert_eq!(
            statuses(&pool, other).await,
            [("elsewhere".to_string(), "running".to_string())]
        );
    }
}
