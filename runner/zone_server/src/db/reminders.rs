//! Durable reminders delivered atomically to their originating chat, and the
//! recurring automations they become when one carries a rule for its own next
//! firing.
use super::{DbResult, actions};
use crate::services::schedule::{self, MAX_LIFETIME, Recurrence};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// How a due automation is meant to be read when it fires.
///
/// One mode, because one is what the worker dispatches. `deliver_next` sends
/// the reminder's content at the stated time and does nothing else, which is
/// exactly `exact_schedule` and is the wrong contract for either of the others:
/// a `flexible_schedule` would fire at a time it never promised to keep, and a
/// `condition_watch` would redeliver fixed content and call it monitoring.
/// Accepting a mode and then running it under a different contract is worse
/// than refusing it, so the list widens when the worker learns the mode, and
/// the database constraint widens with it.
const TIMING_MODES: [&str; 1] = ["exact_schedule"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reminder {
    pub content: String,
    pub due_at: DateTime<Utc>,
    /// An RFC 5545 rule, in the subset `services::schedule` accepts. Absent is
    /// what a one-shot is, which is every reminder that exists today.
    #[serde(default)]
    pub rrule: Option<String>,
    /// What the future self is to do, as an imperative. Stored now; nothing
    /// executes it yet, and the worker still delivers `content`.
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub timing_mode: Option<String>,
}

/// Reads and checks the automation half of a create.
///
/// Returns the rule and the mode to store. Each refusal says which clause was
/// wrong and what is accepted: the caller is a model turning a sentence into a
/// schedule, and a refusal it cannot act on costs the person another turn.
fn automation(input: &Reminder) -> Result<(Option<Recurrence>, &'static str), sqlx::Error> {
    let mode = match input.timing_mode.as_deref() {
        None => "exact_schedule",
        Some(given) => *TIMING_MODES
            .iter()
            .find(|mode| **mode == given)
            .ok_or_else(|| {
                actions::invalid(&format!(
                    "\"{given}\" is not a timing mode this build runs. Only exact_schedule is \
                     dispatched; flexible_schedule and condition_watch need a firing that can run \
                     a turn, which does not exist yet."
                ))
            })?,
    };

    let rule = match input
        .rrule
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        Some(rrule) => Some(Recurrence::parse(rrule).map_err(|why| actions::invalid(&why))?),
        None => None,
    };

    Ok((rule, mode))
}

pub async fn create(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    chat_id: Uuid,
    input: Reminder,
) -> DbResult<Value> {
    if input.content.trim().is_empty() || input.due_at <= Utc::now() {
        return Err(actions::invalid(
            "Provide nonblank content and a future RFC3339 due_at with an explicit UTC offset",
        ));
    }
    let (rule, mode) = automation(&input)?;
    // The first firing is the anchor: a recurrence is measured from a fixed
    // point, because due_at moves on every firing and measuring from it would
    // let "every second Monday" drift a period each time one landed late.
    let anchor = rule.as_ref().map(|_| input.due_at);
    // A schedule nobody renews is a schedule nobody wanted, so a recurring one
    // stops on its own after a week unless it is asked for again.
    let expires = rule.as_ref().map(|_| Utc::now() + MAX_LIFETIME);

    let mut transaction = pool.begin().await?;
    actions::authorize(&mut transaction, workspace_id, user_id, true).await?;
    actions::chat(&mut transaction, workspace_id, chat_id).await?;
    let result = sqlx::query_scalar(
        "INSERT INTO reminders (workspace_id, created_by, chat_id, content, due_at, rrule, \
         prompt, timing_mode, anchor_at, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING to_jsonb(reminders.*)",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(chat_id)
    .bind(input.content)
    .bind(input.due_at)
    .bind(
        input
            .rrule
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty()),
    )
    .bind(
        input
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty()),
    )
    .bind(mode)
    .bind(anchor)
    .bind(expires)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(result)
}

pub async fn list(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> DbResult<Value> {
    let mut transaction = pool.begin().await?;
    actions::authorize(&mut transaction, workspace_id, user_id, false).await?;
    let result = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(reminders.*) ORDER BY due_at, id), '[]'::jsonb) FROM reminders WHERE workspace_id = $1 AND created_by = $2")
        .bind(workspace_id).bind(user_id).fetch_one(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(result)
}

pub async fn cancel(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    reminder_id: Uuid,
) -> DbResult<Value> {
    let mut transaction = pool.begin().await?;
    actions::authorize(&mut transaction, workspace_id, user_id, true).await?;
    let result = sqlx::query_scalar("UPDATE reminders SET status = 'cancelled', completed_at = NOW() WHERE id = $1 AND workspace_id = $2 AND created_by = $3 AND status = 'pending' RETURNING to_jsonb(reminders.*)")
        .bind(reminder_id).bind(workspace_id).bind(user_id).fetch_optional(&mut *transaction).await?;
    transaction.commit().await?;
    result.ok_or_else(|| actions::invalid("Pending reminder not found"))
}

/// One claimed row, as the dispatcher needs to read it.
#[derive(sqlx::FromRow)]
struct Due {
    id: Uuid,
    workspace_id: Uuid,
    created_by: Uuid,
    chat_id: Uuid,
    content: String,
    rrule: Option<String>,
    anchor_at: Option<DateTime<Utc>>,
    due_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    fired_count: i32,
}

/// Where a fired automation goes next, or `None` when its rule is spent and
/// the row is finished.
///
/// The search starts from now rather than from the firing that just happened,
/// so a schedule slept through — the server was down, the tick was late — fires
/// next at its next real occurrence instead of replaying the ones it missed.
///
/// The offset from `jitter` is added to every computed occurrence and never to
/// the first firing, which is the time the person actually named. Everyone who
/// asks for "every morning" is given nine o'clock, so without it a fleet of
/// schedules arrives in one burst and queues behind itself.
fn reschedule(
    id: Uuid,
    rrule: Option<&str>,
    anchor: Option<DateTime<Utc>>,
    due_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    fired: i32,
) -> Option<DateTime<Utc>> {
    let now = Utc::now();
    if expires_at.is_some_and(|expires| expires <= now) {
        return None;
    }
    let rule = Recurrence::parse(rrule?).ok()?;
    let fired = u32::try_from(fired).unwrap_or(u32::MAX);
    // The row still holds the count from before this firing -- the increment
    // shares the transaction that is delivering it -- so the delivery in hand
    // is added here. Without it COUNT=1 schedules a second firing, and every
    // finite rule delivers one more than it was asked for.
    let next = rule.next_after(anchor.unwrap_or(due_at), now, fired.saturating_add(1))?
        + schedule::jitter(id, rule.period());
    // A firing past the lifetime is not scheduled at all, rather than scheduled
    // and then refused when it arrives.
    if expires_at.is_some_and(|expires| next > expires) {
        return None;
    }
    Some(next)
}

/// The claim, message, and terminal state share a transaction across all workers.
pub async fn deliver_next(pool: &PgPool) -> DbResult<bool> {
    let mut transaction = pool.begin().await?;
    let next = sqlx::query_as::<_, Due>(
        "SELECT id, workspace_id, created_by, chat_id, content, rrule, anchor_at, due_at, \
         expires_at, fired_count FROM reminders WHERE status = 'pending' AND due_at <= NOW() \
         ORDER BY due_at, id LIMIT 1 FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(Due {
        id,
        workspace_id,
        created_by: user_id,
        chat_id,
        content,
        rrule,
        anchor_at,
        due_at,
        expires_at,
        fired_count,
    }) = next
    else {
        return Ok(false);
    };

    // A schedule whose lifetime ran out before this claim ends without
    // delivering. The row went past its week while it sat due -- the server was
    // down, or the tick was late -- and the firing it is holding is one nobody
    // renewed, so sending it would be one unasked-for message on the way out.
    // Checked here rather than in `reschedule`, which runs after the insert.
    if expires_at.is_some_and(|expires| expires <= Utc::now()) {
        sqlx::query("UPDATE reminders SET status = 'expired', completed_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        return Ok(true);
    }

    let mut message = None;
    if let Err(error) = actions::authorize(&mut transaction, workspace_id, user_id, true).await {
        if !matches!(error, sqlx::Error::Protocol(_)) {
            return Err(error);
        }
        sqlx::query(
            "UPDATE reminders SET status = 'cancelled', completed_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    } else {
        actions::chat(&mut transaction, workspace_id, chat_id).await?;
        let message_id = Uuid::new_v4();
        message = Some(sqlx::query_scalar::<_, Value>("INSERT INTO messages (id, chat_id, role, content, metadata) VALUES ($1, $2, 'assistant', $3, $4) RETURNING to_jsonb(messages.*)")
            .bind(message_id).bind(chat_id).bind(content).bind(json!({"source": "reminder", "reminder_id": id, "actor_id": user_id})).fetch_one(&mut *transaction).await?);
        sqlx::query("UPDATE chats SET updated_at = NOW() WHERE id = $1")
            .bind(chat_id)
            .execute(&mut *transaction)
            .await?;
        // A one-shot is finished; an automation either moves to its next
        // occurrence and stays pending, or has run out of rule and is
        // `expired` -- which is a different ending from `delivered`, and a
        // different one again from a schedule somebody cancelled.
        match reschedule(
            id,
            rrule.as_deref(),
            anchor_at,
            due_at,
            expires_at,
            fired_count,
        ) {
            Some(next) if rrule.is_some() => {
                sqlx::query(
                    "UPDATE reminders SET due_at = $2, fired_count = fired_count + 1, \
                     last_fired_at = NOW(), message_id = $3 WHERE id = $1",
                )
                .bind(id)
                .bind(next)
                .bind(message_id)
                .execute(&mut *transaction)
                .await?;
            }
            _ => {
                let ended = if rrule.is_some() {
                    "expired"
                } else {
                    "delivered"
                };
                sqlx::query(
                    "UPDATE reminders SET status = $2, completed_at = NOW(), \
                     fired_count = fired_count + 1, last_fired_at = NOW(), message_id = $3 \
                     WHERE id = $1",
                )
                .bind(id)
                .bind(ended)
                .bind(message_id)
                .execute(&mut *transaction)
                .await?;
            }
        }
    }
    transaction.commit().await?;
    if let Some(message) = message {
        actions::publish(chat_id, message);
    }
    Ok(true)
}
