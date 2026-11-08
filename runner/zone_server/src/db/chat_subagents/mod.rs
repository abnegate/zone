//! The subagents each chat's coding agent started for itself, and the steps
//! each of them took.

mod change;
mod recorded_steps;
mod row;
mod stopped;
#[cfg(test)]
mod tests;

use abnegate_agent_cli::{StepStatus, SubagentStatus, SubagentStep};
use sqlx::types::Json;
use sqlx::{Executor, PgConnection, Postgres};
use uuid::Uuid;

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

/// Matches the rows that are still running. Spelled out rather than bound so
/// the planner can match it to the running partial indexes' predicates, which
/// a parameter never proves.
macro_rules! running {
    () => {
        "status = 'running'"
    };
}

/// Fails the running steps of the subagents a `stopped` CTE settled, and
/// selects each of them as a [`Stopped`], oldest first. `$failed` and
/// `$running` are the placeholders of the failed and running step statuses.
macro_rules! fail_stopped_steps {
    ($failed:literal, $running:literal) => {
        concat!(
            "failed AS (
                 UPDATE chat_subagent_steps p
                    SET status = ",
            $failed,
            "::text
                   FROM stopped
                  WHERE p.subagent_id = stopped.id AND p.status = ",
            $running,
            "::text
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

/// Stops the subagents a `seen` CTE read as running, those still running for
/// the turn they ran for when it read them. `$stopped` is the placeholder of
/// the stopped subagent status.
///
/// Under Read Committed an `UPDATE` rechecks a row changed since its snapshot
/// against the row's newest version but every other table as the snapshot saw
/// it, so a sweep that began before a newer turn took a subagent over would
/// otherwise judge the newer run by turns it cannot see, and stop it.
macro_rules! stop_seen {
    ($stopped:literal) => {
        concat!(
            "stopped AS (
                 UPDATE chat_subagents s
                    SET status = ",
            $stopped,
            "::text,
                        finished_at = COALESCE(s.finished_at, clock_timestamp()),
                        revision = s.revision + 1,
                        updated_at = clock_timestamp()
                   FROM seen
                  WHERE s.id = seen.id AND s.message_id IS NOT DISTINCT FROM seen.message_id
                    AND s.",
            running!(),
            "
                 RETURNING s.*
             ), "
        )
    };
}

/// Stops the running subagents of chat `$1` that match `$condition`, fails
/// their running steps with them, and selects each as a [`Stopped`], oldest
/// first. `$3` is the stopped subagent status, and `$4` and `$5` the failed and
/// running step statuses.
macro_rules! stop_running {
    ($($condition:tt)+) => {
        concat!(
            "WITH seen AS MATERIALIZED (
                 SELECT id, message_id FROM chat_subagents
                  WHERE chat_id = $1 AND ",
            $($condition)+,
            " AND ",
            running!(),
            "
             ), ",
            stop_seen!("$3"),
            fail_stopped_steps!("$4", "$5")
        )
    };
}

const STOP: &str = stop_running!("message_id = $2");

const STOP_OTHERS: &str = stop_running!(
    "message_id IS DISTINCT FROM $2
     AND EXISTS (SELECT 1 FROM chat_turns t WHERE t.chat_id = $1 AND t.id = $2 AND t.",
    running!(),
    ")"
);

const ABANDON: &str = concat!(
    "WITH seen AS MATERIALIZED (
         SELECT s.id, s.message_id FROM chat_subagents s
          WHERE s.",
    running!(),
    "
            AND NOT EXISTS (
                SELECT 1
                  FROM chat_turns t
                  JOIN chat_leases l ON l.chat_id = t.chat_id AND l.fence = t.fence
                 WHERE t.chat_id = s.chat_id AND t.id = s.message_id AND t.status = $2::text
                   AND l.expires_at > clock_timestamp())
     ), ",
    stop_seen!("$1"),
    fail_stopped_steps!("$3", "$4")
);

/// A settled subagent that is running again, as it is only when a turn other
/// than the one that ran it reports it running: the update names the run that
/// turn started rather than the one that ended.
macro_rules! resumed {
    () => {
        "($8::text = $15::text AND s.status <> $15::text
          AND $2::uuid IS NOT NULL AND s.message_id IS DISTINCT FROM $2::uuid)"
    };
}

/// Records what `change` says about one subagent, creating its row the first
/// time the chat hears of it. Its steps are [`record_steps`]'s to write.
///
/// The turn that first named it keeps it. A finished subagent never runs
/// again within that turn, and `stopped`, which is all a sweep or a shutdown
/// knows, never hides the completion or failure it already reported; a
/// reported outcome may still replace a `stopped` one. Only another turn
/// reporting it running starts it again: that turn takes it, and the outcome
/// of the run before is cleared.
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
             message_id = CASE WHEN ",
        resumed!(),
        " THEN $2 ELSE COALESCE(s.message_id, $2) END,
             description = COALESCE($4, s.description),
             kind = COALESCE($5, s.kind),
             model = COALESCE($6, s.model),
             background = COALESCE($7, s.background),
             status = CASE
                 WHEN $8::text IS NULL THEN s.status
                 WHEN s.status = $15::text THEN $8::text
                 WHEN ",
        resumed!(),
        " THEN $8::text
                 WHEN $8::text IN ($15::text, $16::text) THEN s.status
                 ELSE $8::text
             END,
             tool_uses = CASE WHEN ",
        resumed!(),
        " THEN $9 ELSE COALESCE($9, s.tool_uses) END,
             duration_ms = CASE WHEN ",
        resumed!(),
        " THEN $10 ELSE COALESCE($10, s.duration_ms) END,
             tokens = CASE WHEN ",
        resumed!(),
        " THEN $11 ELSE COALESCE($11, s.tokens) END,
             last_tool = CASE WHEN ",
        resumed!(),
        " THEN $12 ELSE COALESCE($12, s.last_tool) END,
             summary = CASE WHEN ",
        resumed!(),
        " THEN $13 ELSE COALESCE($13, s.summary) END,
             error = CASE WHEN ",
        resumed!(),
        " THEN $14 ELSE COALESCE($14, s.error) END,
             started_at = CASE WHEN ",
        resumed!(),
        " THEN clock_timestamp() ELSE s.started_at END,
             finished_at = CASE WHEN ",
        resumed!(),
        " THEN NULL ELSE COALESCE(
                 s.finished_at,
                 CASE WHEN COALESCE($8::text, s.status) <> $15::text THEN clock_timestamp() END
             ) END,
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
/// says it is running is ignored. A step that would run under a subagent that
/// is no longer running is failed instead. When `steps` names one step twice,
/// the later is kept.
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
         agent AS (
             SELECT status FROM chat_subagents WHERE id = $1
         ),
         placed AS (
             SELECT i.step_id, i.name, i.subject,
                    CASE
                        WHEN i.status = $6::text
                         AND a.status <> $7::text
                         AND COALESCE(known.status, $6::text) = $6::text
                        THEN $8::text
                        ELSE i.status
                    END AS status,
                    COALESCE(
                        known.position,
                        l.position + ROW_NUMBER() OVER (
                            PARTITION BY known.position IS NULL ORDER BY i.ordinal)
                    ) AS position
               FROM incoming i
              CROSS JOIN last l
               LEFT JOIN agent a ON true
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
    .bind(SubagentStatus::Running.as_str())
    .bind(StepStatus::Failed.as_str())
    .fetch_all(executor)
    .await?;
    let added = rows.iter().filter(|row| row.4).count();
    Ok(RecordedSteps {
        changed: rows
            .into_iter()
            .map(|(id, name, subject, status, _)| {
                let mut step = SubagentStep::new(
                    id,
                    name,
                    StepStatus::named(&status).unwrap_or(StepStatus::Failed),
                );
                step.subject = subject;
                step
            })
            .collect(),
        added: i64::try_from(added).unwrap_or(i64::MAX),
    })
}

/// Fails the steps subagent `subagent_id` left running and returns them in the
/// order it took them. A subagent that has finished runs no step, so whatever
/// step it never reported on is settled with it.
pub async fn fail_running_steps<'e, E>(
    executor: E,
    subagent_id: Uuid,
) -> DbResult<Vec<SubagentStep>>
where
    E: Executor<'e, Database = Postgres>,
{
    let failed: Json<Vec<SubagentStep>> = sqlx::query_scalar(
        "WITH failed AS (
             UPDATE chat_subagent_steps
                SET status = $2::text
              WHERE subagent_id = $1 AND status = $3::text
             RETURNING step_id, name, subject, status, position
         )
         SELECT COALESCE(
                    jsonb_agg(
                        jsonb_build_object(
                            'id', step_id, 'name', name, 'subject', subject, 'status', status)
                        ORDER BY position),
                    '[]'::jsonb)
           FROM failed",
    )
    .bind(subagent_id)
    .bind(StepStatus::Failed.as_str())
    .bind(StepStatus::Running.as_str())
    .fetch_one(executor)
    .await?;
    Ok(failed.0)
}

/// Whether turn `message_id` of `chat_id` is still there to record subagents
/// for: its turn or its message is. Holds the turn, or the message when it
/// has no turn, until the caller's transaction ends, so deleting the message
/// waits for whatever that transaction writes, and removes it with the rest.
pub async fn hold_turn<'e, E>(executor: E, chat_id: Uuid, message_id: Uuid) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar(
        "SELECT EXISTS (
                    SELECT 1 FROM chat_turns WHERE chat_id = $1 AND id = $2 FOR KEY SHARE)
             OR EXISTS (
                    SELECT 1 FROM messages WHERE chat_id = $1 AND id = $2 FOR KEY SHARE)",
    )
    .bind(chat_id)
    .bind(message_id)
    .fetch_one(executor)
    .await
}

/// Whether message `message_id` of `chat_id` exists. Once every write that
/// holds turn `message_id` or that message through [`hold_turn`] has
/// committed, locks both against any other until the caller's transaction
/// ends, so the message's subagents can be removed and the message deleted
/// with no write landing in between. Call it before locking any of the
/// message's subagents: a write holds the turn, or the message when it has
/// no turn, before it touches a subagent, so seizing both first keeps the
/// order every write takes its locks in.
pub async fn seize_turn(
    connection: &mut PgConnection,
    chat_id: Uuid,
    message_id: Uuid,
) -> DbResult<bool> {
    sqlx::query("SELECT 1 FROM chat_turns WHERE chat_id = $1 AND id = $2 FOR UPDATE")
        .bind(chat_id)
        .bind(message_id)
        .fetch_optional(&mut *connection)
        .await?;
    let message = sqlx::query("SELECT 1 FROM messages WHERE chat_id = $1 AND id = $2 FOR UPDATE")
        .bind(chat_id)
        .bind(message_id)
        .fetch_optional(&mut *connection)
        .await?;
    Ok(message.is_some())
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
/// A turn that is interrupted, or recovered after its server died, stops its
/// own subagents as it settles. A turn that completed can still leave some
/// running when its keeper's final [`stop`] fails. Every turn calls this as it
/// starts, so those stop at the chat's next turn rather than the server's next
/// boot.
///
/// A chat runs one turn at a time: a turn holds the chat's lease until it
/// ends, and the turn that takes the lease next settles it as it begins. So
/// while turn `message_id` is still `running`, any running subagent of
/// another turn belongs to one that is over. Once it is not, a newer turn may
/// own them, and this stops nothing.
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

/// The subagents among `ids` of `chat_id` that are no longer running, oldest
/// first, each with every step it failed.
pub async fn settled<'e, E>(executor: E, chat_id: Uuid, ids: &[Uuid]) -> DbResult<Vec<Stopped>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        ", COALESCE(
               (SELECT jsonb_agg(
                           jsonb_build_object(
                               'id', p.step_id, 'name', p.name,
                               'subject', p.subject, 'status', p.status)
                           ORDER BY p.position)
                  FROM chat_subagent_steps p WHERE p.subagent_id = s.id AND p.status = $3::text),
               '[]'::jsonb) AS failed
           FROM chat_subagents s
          WHERE s.chat_id = $1 AND s.id = ANY($2) AND s.status <> $4::text
          ORDER BY s.started_at, s.id"
    ))
    .bind(chat_id)
    .bind(ids)
    .bind(StepStatus::Failed.as_str())
    .bind(SubagentStatus::Running.as_str())
    .fetch_all(executor)
    .await
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

/// Stops every subagent left running by a turn that no longer runs, fails
/// their running steps, and returns them, oldest first. A turn runs while it
/// is `running` and the chat's lease is still the one it began under, so a
/// turn whose server died is let go of once its lease expires, whether or not
/// the chat ever runs another.
pub async fn abandon<'e, E>(executor: E) -> DbResult<Vec<Stopped>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(ABANDON)
        .bind(SubagentStatus::Stopped.as_str())
        .bind(TURN_RUNNING)
        .bind(StepStatus::Failed.as_str())
        .bind(StepStatus::Running.as_str())
        .fetch_all(executor)
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
