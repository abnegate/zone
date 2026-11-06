//! The subagents each chat's coding agent started for itself, and the steps
//! each of them took.

mod change;
mod recorded_steps;
mod row;
mod stopped;
#[cfg(test)]
mod tests;

use sqlx::types::Json;
use sqlx::{Executor, Postgres};
use uuid::Uuid;
use zone_core::llm::{StepStatus, SubagentStatus, SubagentStep};

use super::DbResult;
use super::context::RUNNING as TURN_RUNNING;

pub use change::SubagentChange;
pub use recorded_steps::RecordedSteps;
pub use row::SubagentRow;
pub use stopped::Stopped;

/// Every [`SubagentRow`] column of the row aliased `s`.
macro_rules! columns {
    () => {
        "s.id, s.chat_id, s.message_id, s.external_id, s.description, s.kind, s.model, \
         s.background, s.status, s.tool_uses, s.duration_ms, s.tokens, s.last_tool, s.summary, \
         s.error, s.revision, s.started_at, s.finished_at, s.updated_at, \
         (SELECT count(*) FROM chat_subagent_steps p WHERE p.subagent_id = s.id) AS step_count"
    };
}

/// Matches the subagents that are still running. Spelled out rather than bound
/// so the planner can match it to `chat_subagents_running_idx`'s predicate,
/// which a parameter never proves.
macro_rules! running {
    () => {
        "status = 'running'"
    };
}

/// Stops the running subagents `$1`'s turns matching `$condition` left, fails
/// their running steps with them, and selects each as a [`Stopped`], oldest
/// first. `$3` is the stopped subagent status, and `$4` and `$5` the failed and
/// running step statuses.
macro_rules! stop_running {
    ($condition:literal) => {
        concat!(
            "WITH stopped AS (
                 UPDATE chat_subagents
                    SET status = $3::text,
                        finished_at = COALESCE(finished_at, clock_timestamp()),
                        revision = revision + 1,
                        updated_at = clock_timestamp()
                  WHERE chat_id = $1 AND ",
            $condition,
            " AND ",
            running!(),
            "
                 RETURNING *
             ),
             failed AS (
                 UPDATE chat_subagent_steps p
                    SET status = $4::text
                   FROM stopped
                  WHERE p.subagent_id = stopped.id AND p.status = $5::text
                 RETURNING p.*
             )
             SELECT ",
            columns!(),
            ", COALESCE(
                   (SELECT jsonb_agg(
                               jsonb_build_object(
                                   'id', f.step_id, 'name', f.name,
                                   'subject', f.subject, 'status', f.status)
                               ORDER BY f.position)
                      FROM failed f WHERE f.subagent_id = s.id),
                   '[]'::jsonb) AS failed
               FROM stopped s ORDER BY s.started_at, s.id"
        )
    };
}

const STOP: &str = stop_running!("message_id = $2");

const STOP_OTHERS: &str = stop_running!("message_id IS DISTINCT FROM $2");

const ABANDON: &str = concat!(
    "WITH stopped AS (
         UPDATE chat_subagents s
            SET status = $1::text,
                finished_at = COALESCE(s.finished_at, clock_timestamp()),
                revision = s.revision + 1,
                updated_at = clock_timestamp()
          WHERE s.",
    running!(),
    "
            AND NOT EXISTS (
                SELECT 1 FROM chat_turns t
                 WHERE t.chat_id = s.chat_id AND t.id = s.message_id AND t.status = $2::text)
         RETURNING s.id
     ),
     failed AS (
         UPDATE chat_subagent_steps p
            SET status = $3::text
           FROM stopped
          WHERE p.subagent_id = stopped.id AND p.status = $4::text
     )
     SELECT count(*) FROM stopped"
);

/// Records what `change` says about one subagent, creating its row the first
/// time the chat hears of it. Its steps are [`record_steps`]'s to write.
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
             tool_uses, duration_ms, tokens, last_tool, summary, error, finished_at, updated_at
         )
         VALUES (
             $1, $2, $3, COALESCE($4, ''), $5, $6, COALESCE($7, false),
             COALESCE($8::text, $15::text), $9, $10, $11, $12, $13, $14,
             CASE WHEN COALESCE($8::text, $15::text) <> $15::text THEN clock_timestamp() END,
             clock_timestamp()
         )
         ON CONFLICT (chat_id, external_id) DO UPDATE SET
             message_id = COALESCE(s.message_id, $2),
             description = COALESCE($4, s.description),
             kind = COALESCE($5, s.kind),
             model = COALESCE($6, s.model),
             background = COALESCE($7, s.background),
             status = CASE
                 WHEN $8::text IS NULL THEN s.status
                 WHEN s.status = $15::text THEN $8::text
                 WHEN $8::text IN ($15::text, $16::text) THEN s.status
                 ELSE $8::text
             END,
             tool_uses = COALESCE($9, s.tool_uses),
             duration_ms = COALESCE($10, s.duration_ms),
             tokens = COALESCE($11, s.tokens),
             last_tool = COALESCE($12, s.last_tool),
             summary = COALESCE($13, s.summary),
             error = COALESCE($14, s.error),
             finished_at = COALESCE(
                 s.finished_at,
                 CASE WHEN COALESCE($8::text, s.status) <> $15::text THEN clock_timestamp() END
             ),
             revision = s.revision + 1,
             updated_at = clock_timestamp()
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
    .bind(SubagentStatus::Running.as_str())
    .bind(SubagentStatus::Stopped.as_str())
    .fetch_one(executor)
    .await
}

/// Writes `steps` for subagent `subagent_id` and returns those that changed.
///
/// A new step goes after every step the subagent already has. A known step
/// keeps its place, and one that has settled never runs again: an update that
/// says it is running is ignored. When `steps` names one step twice, the later
/// is kept.
pub async fn record_steps<'e, E>(
    executor: E,
    subagent_id: Uuid,
    steps: &[SubagentStep],
) -> DbResult<RecordedSteps>
where
    E: Executor<'e, Database = Postgres>,
{
    if steps.is_empty() {
        return Ok(RecordedSteps::default());
    }
    let ids: Vec<&str> = steps.iter().map(|step| step.id.as_str()).collect();
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    let subjects: Vec<Option<&str>> = steps.iter().map(|step| step.subject.as_deref()).collect();
    let statuses: Vec<&str> = steps.iter().map(|step| step.status.as_str()).collect();
    let rows: Vec<(String, String, Option<String>, String, bool)> = sqlx::query_as(
        "WITH incoming AS (
             SELECT DISTINCT ON (i.step_id) i.step_id, i.name, i.subject, i.status, i.ordinal
               FROM unnest($2::text[], $3::text[], $4::text[], $5::text[])
                    WITH ORDINALITY AS i(step_id, name, subject, status, ordinal)
              ORDER BY i.step_id, i.ordinal DESC
         ),
         last AS (
             SELECT COALESCE(MAX(position), -1) AS position
               FROM chat_subagent_steps WHERE subagent_id = $1
         ),
         placed AS (
             SELECT i.step_id, i.name, i.subject, i.status,
                    COALESCE(
                        known.position,
                        l.position + ROW_NUMBER() OVER (
                            PARTITION BY known.position IS NULL ORDER BY i.ordinal)
                    ) AS position
               FROM incoming i
              CROSS JOIN last l
               LEFT JOIN chat_subagent_steps known
                 ON known.subagent_id = $1 AND known.step_id = i.step_id
         ),
         written AS (
             INSERT INTO chat_subagent_steps AS k (subagent_id, step_id, position, name, subject, status)
             SELECT $1, step_id, position, name, subject, status FROM placed
             ON CONFLICT (subagent_id, step_id) DO UPDATE SET
                 name = EXCLUDED.name,
                 subject = EXCLUDED.subject,
                 status = EXCLUDED.status
              WHERE NOT (k.status <> $6::text AND EXCLUDED.status = $6::text)
                AND (k.name, k.subject, k.status)
                    IS DISTINCT FROM (EXCLUDED.name, EXCLUDED.subject, EXCLUDED.status)
             RETURNING k.step_id, k.name, k.subject, k.status, k.position
         )
         SELECT w.step_id, w.name, w.subject, w.status, w.position > l.position AS added
           FROM written w CROSS JOIN last l
          ORDER BY w.position",
    )
    .bind(subagent_id)
    .bind(ids)
    .bind(names)
    .bind(subjects)
    .bind(statuses)
    .bind(StepStatus::Running.as_str())
    .fetch_all(executor)
    .await?;
    let added = rows.iter().filter(|row| row.4).count();
    Ok(RecordedSteps {
        changed: rows
            .into_iter()
            .map(|(id, name, subject, status, _)| SubagentStep {
                id,
                name,
                subject,
                status: StepStatus::named(&status).unwrap_or(StepStatus::Failed),
            })
            .collect(),
        added: i64::try_from(added).unwrap_or(i64::MAX),
    })
}

/// Every subagent `chat_id` started, oldest first.
pub async fn list<'e, E>(executor: E, chat_id: Uuid) -> DbResult<Vec<SubagentRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM chat_subagents s WHERE s.chat_id = $1 ORDER BY s.started_at, s.id"
    ))
    .bind(chat_id)
    .fetch_all(executor)
    .await
}

/// The steps of `chat_id`'s subagent `subagent_id` in the order it took them,
/// or `None` when the chat has no such subagent.
pub async fn steps<'e, E>(
    executor: E,
    chat_id: Uuid,
    subagent_id: Uuid,
) -> DbResult<Option<Vec<SubagentStep>>>
where
    E: Executor<'e, Database = Postgres>,
{
    let steps: Option<Json<Vec<SubagentStep>>> = sqlx::query_scalar(
        "SELECT COALESCE(
                    (SELECT jsonb_agg(
                                jsonb_build_object(
                                    'id', p.step_id, 'name', p.name,
                                    'subject', p.subject, 'status', p.status)
                                ORDER BY p.position)
                       FROM chat_subagent_steps p WHERE p.subagent_id = s.id),
                    '[]'::jsonb)
           FROM chat_subagents s
          WHERE s.chat_id = $1 AND s.id = $2",
    )
    .bind(chat_id)
    .bind(subagent_id)
    .fetch_optional(executor)
    .await?;
    Ok(steps.map(|steps| steps.0))
}

/// Stops the subagents turn `message_id` left running when it ended, fails
/// their running steps, and returns them oldest first.
pub async fn stop<'e, E>(executor: E, chat_id: Uuid, message_id: Uuid) -> DbResult<Vec<Stopped>>
where
    E: Executor<'e, Database = Postgres>,
{
    stop_matching(executor, STOP, chat_id, message_id).await
}

/// Stops the subagents any turn of `chat_id` other than `message_id` left
/// running, fails their running steps, and returns them oldest first.
///
/// A turn whose server died keeps its `chat_turns` row running until the
/// chat's next turn settles it, so [`abandon`] cannot see its subagents at
/// boot. The next turn calls this before it records a subagent of its own.
///
/// Safe only because a chat runs one turn at a time: a turn holds the chat's
/// `chat_leases` row until it ends, and no other turn can take it while the
/// lease is live. Any running subagent of another turn therefore belongs to a
/// turn that is over. A chat that ever runs turns side by side must narrow
/// this to the turns its lease has fenced off.
pub async fn stop_others<'e, E>(
    executor: E,
    chat_id: Uuid,
    message_id: Uuid,
) -> DbResult<Vec<Stopped>>
where
    E: Executor<'e, Database = Postgres>,
{
    stop_matching(executor, STOP_OTHERS, chat_id, message_id).await
}

async fn stop_matching<'e, E>(
    executor: E,
    statement: &'static str,
    chat_id: Uuid,
    message_id: Uuid,
) -> DbResult<Vec<Stopped>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(statement)
        .bind(chat_id)
        .bind(message_id)
        .bind(SubagentStatus::Stopped.as_str())
        .bind(StepStatus::Failed.as_str())
        .bind(StepStatus::Running.as_str())
        .fetch_all(executor)
        .await
}

/// Stops every subagent left running by a turn that is no longer running,
/// fails their running steps, and returns how many subagents it stopped. The
/// server calls it once as it boots, when no turn of its own can still be
/// reporting on them.
pub async fn abandon<'e, E>(executor: E) -> DbResult<i64>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar(ABANDON)
        .bind(SubagentStatus::Stopped.as_str())
        .bind(TURN_RUNNING)
        .bind(StepStatus::Failed.as_str())
        .bind(StepStatus::Running.as_str())
        .fetch_one(executor)
        .await
}

/// Removes the subagents message `message_id` of `chat_id` started, with
/// their steps, and returns their ids.
pub async fn remove<'e, E>(executor: E, chat_id: Uuid, message_id: Uuid) -> DbResult<Vec<Uuid>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar(
        "DELETE FROM chat_subagents WHERE chat_id = $1 AND message_id = $2
         RETURNING id",
    )
    .bind(chat_id)
    .bind(message_id)
    .fetch_all(executor)
    .await
}
