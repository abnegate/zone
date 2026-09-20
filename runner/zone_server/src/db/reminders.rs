//! Durable reminders delivered atomically to their originating chat, and the
//! recurring automations they become when one carries a rule for its own next
//! firing.
use super::{DbResult, actions};
use crate::services::schedule::{self, MAX_LIFETIME, Recurrence};
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// How a due automation is meant to be read when it fires.
///
/// Two modes, because two are what the worker dispatches. `deliver_next` fires
/// at the stated time and nowhere else, which is exactly `exact_schedule`. A
/// `condition_watch` is that same firing handed the last one's answer and asked
/// what differs, which is a contract the worker now honours because the answer
/// is kept on the row. A `flexible_schedule` is a window to place a firing
/// inside, and placing one needs a reading of what else is on the person's day
/// that nothing here takes, so it stays refused. Accepting a mode and then
/// running it under a different contract is worse than refusing it, so this
/// list widens when the worker learns the mode, and the database constraint
/// widens with it.
const TIMING_MODES: [&str; 2] = ["exact_schedule", "condition_watch"];

/// How much of one firing's answer is kept as the next one's baseline.
///
/// A model's answer has no length anybody promised, and this one is read back
/// into the next firing's prompt, where an unbounded one would crowd out the
/// context it is meant to be compared in. Truncation is safe here where
/// compaction was not: it takes the same leading characters every firing, so
/// two readings stay comparable, and whatever falls past the bound is missed
/// consistently rather than intermittently. The database holds the same bound,
/// so a later writer cannot store a baseline this would not have kept.
pub const OBSERVATION_CAP: usize = 4000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reminder {
    /// The words a firing delivers, and required whether or not one is sent:
    /// alongside a `prompt` they are not delivered at all, and are the
    /// schedule's name instead. A standing job needs a name somebody can
    /// recognise in `list` and act on in `cancel`, and its prompt is an
    /// instruction rather than a name — reading one back to somebody deciding
    /// whether to stop it tells them how the job works rather than what it is.
    pub content: String,
    /// Kept in the offset the caller wrote it in, so a refusal can answer in
    /// the same clock the caller was reading.
    #[serde(deserialize_with = "rfc3339")]
    pub due_at: DateTime<FixedOffset>,
    /// An RFC 5545 rule, in the subset `services::schedule` accepts. Absent is
    /// what a one-shot is.
    #[serde(default)]
    pub rrule: Option<String>,
    /// What the future self is to do, as an imperative. A firing that carries
    /// one runs it as a turn in the chat instead of posting `content`.
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
                    "\"{given}\" is not a timing mode this build runs. exact_schedule fires at \
                     the time you name, and condition_watch fires on a rule and reports what \
                     differs from its last firing. flexible_schedule needs a window to place a \
                     firing in, and nothing here reads what else is on the person's day, so it \
                     is not stored yet."
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

    // A watch needs both halves of a comparison. One firing has nothing to
    // compare against, and fixed content has nothing to compare. Either way it
    // is a reminder wearing a watch's name, and a mode that quietly does less
    // than it says is the thing this whole path exists to avoid.
    if mode == "condition_watch" {
        let prompted = input
            .prompt
            .as_deref()
            .map(str::trim)
            .is_some_and(|prompt| !prompt.is_empty());
        if rule.is_none() || !prompted {
            return Err(actions::invalid(
                "A condition_watch reports what changed since its last firing, so it needs both \
                 halves of that comparison: an rrule, because a schedule that fires once has \
                 nothing to compare against, and a prompt, because fixed content has nothing to \
                 compare. Add whichever is missing, or drop timing_mode if the time is the point \
                 rather than the change.",
            ));
        }
    }

    Ok((rule, mode))
}

fn rfc3339<'de, D: Deserializer<'de>>(deserializer: D) -> Result<DateTime<FixedOffset>, D::Error> {
    let raw = String::deserialize(deserializer)?;
    DateTime::parse_from_rfc3339(raw.trim()).map_err(|_| {
        serde::de::Error::custom(format!(
            "due_at \"{raw}\" is not RFC3339 with an explicit UTC offset (e.g. \
             2026-09-20T19:30:00+12:00)"
        ))
    })
}

/// Both instants in the caller's own offset. A model that wrote 19:23+12:00
/// needs to read that it is 19:29+12:00 now, not that the format was wrong:
/// told the latter it retries the same past instant in every spelling it knows.
fn past_due(due: DateTime<FixedOffset>, now: DateTime<Utc>) -> String {
    let now = now.with_timezone(due.offset());
    let format = if due.date_naive() == now.date_naive() {
        "%H:%M:%S%:z"
    } else {
        "%Y-%m-%dT%H:%M:%S%:z"
    };
    format!(
        "due_at {} is in the past; now is {}",
        due.format(format),
        now.format(format)
    )
}

pub async fn create(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    chat_id: Uuid,
    input: Reminder,
) -> DbResult<Value> {
    if input.content.trim().is_empty() {
        return Err(actions::invalid(
            "content is blank; say what the reminder delivers",
        ));
    }
    let now = Utc::now();
    if input.due_at <= now {
        return Err(actions::invalid(&past_due(input.due_at, now)));
    }
    let (rule, mode) = automation(&input)?;
    // The first firing is the anchor: a recurrence is measured from a fixed
    // point, because due_at moves on every firing and measuring from it would
    // let "every second Monday" drift a period each time one landed late.
    let anchor = rule.as_ref().map(|_| input.due_at.with_timezone(&Utc));
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
    .bind(input.due_at.with_timezone(&Utc))
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
    // A firing that came due and has not run yet goes with the schedule.
    // Stopping a standing job and then being answered by it once more reads as
    // the cancel not having worked.
    if result.is_some() {
        sqlx::query("DELETE FROM reminder_turns WHERE reminder_id = $1")
            .bind(reminder_id)
            .execute(&mut *transaction)
            .await?;
    }
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
    prompt: Option<String>,
}

/// What one claim did.
///
/// A reminder that carries no prompt is finished when its message is stored:
/// the content *is* the delivery. One that carries a prompt has delivered
/// nothing yet — the prompt is a turn for the model to take, and the answer is
/// what the person asked to receive — so the claim writes the turn down as
/// still owed and commits, and the worker runs it from there. Running a
/// generation inside the claim's transaction would hold the row lock for the
/// length of a model call; handing it back in memory instead would lose the
/// firing with the process holding it, and the schedule has already moved past
/// that occurrence by the time the claim commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivered {
    /// Nothing was due.
    Nothing,
    /// A claim was settled: a message was stored, a turn was written down for
    /// the worker to run, a membership was gone, or a schedule had outlived
    /// its lifetime.
    Settled,
}

/// One firing that still owes its chat a turn.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PendingTurn {
    pub id: Uuid,
    pub reminder_id: Uuid,
    pub workspace_id: Uuid,
    pub chat_id: Uuid,
    pub user_id: Uuid,
    pub prompt: String,
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
pub async fn deliver_next(pool: &PgPool) -> DbResult<Delivered> {
    let mut transaction = pool.begin().await?;
    let next = sqlx::query_as::<_, Due>(
        "SELECT id, workspace_id, created_by, chat_id, content, rrule, anchor_at, due_at, \
         expires_at, fired_count, prompt FROM reminders WHERE status = 'pending' AND due_at <= NOW() \
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
        prompt,
    }) = next
    else {
        return Ok(Delivered::Nothing);
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
        return Ok(Delivered::Settled);
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
        // A prompt is a turn, not a message. The worker runs it after this
        // commits, and that turn stores the prompt as the user message and the
        // model's answer beside it -- so storing `content` here as well would
        // put a notice in the chat that the person never asked for.
        let message_id = match prompt {
            Some(prompt) => {
                sqlx::query(
                    "INSERT INTO reminder_turns (reminder_id, workspace_id, chat_id, created_by, \
                     prompt) VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(id)
                .bind(workspace_id)
                .bind(chat_id)
                .bind(user_id)
                .bind(prompt)
                .execute(&mut *transaction)
                .await?;
                None
            }
            None => {
                let message_id = Uuid::new_v4();
                message = Some(sqlx::query_scalar::<_, Value>("INSERT INTO messages (id, chat_id, role, content, metadata) VALUES ($1, $2, 'assistant', $3, $4) RETURNING to_jsonb(messages.*)")
                    .bind(message_id).bind(chat_id).bind(content).bind(json!({"source": "reminder", "reminder_id": id, "actor_id": user_id})).fetch_one(&mut *transaction).await?);
                sqlx::query("UPDATE chats SET updated_at = NOW() WHERE id = $1")
                    .bind(chat_id)
                    .execute(&mut *transaction)
                    .await?;
                Some(message_id)
            }
        };
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
    Ok(Delivered::Settled)
}

/// The key the claim serialises on. Arbitrary and constant; it only has to be
/// the same number in every process, and distinct from the one the dispatch
/// tests take.
const CLAIM_LOCK: i64 = 0x7A6F_6E65_5455_524E;

/// How many times one firing's turn is offered before it is given up on.
///
/// A turn is only ever offered again because whatever took it did not live to
/// say it was done, so a firing that exhausts this is one three attempts have
/// started and none has finished. A fourth is a crash loop rather than a
/// delivery, and a crash loop costs every other schedule its dispatch.
const MAX_TURN_ATTEMPTS: i32 = 3;

/// Takes the oldest firing that still owes a turn, or `None` when none does.
///
/// `lease` is how long a claim is honoured: past it the firing is taken to have
/// died with whatever was running it, and is offered again. The caller sets it
/// because only the caller knows how long a turn can take — the generation
/// timeout, plus however long one may sit queued behind the chat's own
/// semaphore. Too short costs a duplicate turn and too long costs a late one,
/// and of the two only the duplicate is something a person has to read.
pub async fn claim_turn(pool: &PgPool, lease: Duration) -> DbResult<Option<PendingTurn>> {
    let seconds = lease.as_secs_f64();
    let mut transaction = pool.begin().await?;
    // One claim at a time across the fleet, released when this transaction
    // ends. The claim below refuses a firing whose schedule already has one in
    // flight, and that refusal is only sound if no other worker is deciding the
    // same thing at the same moment: `FOR UPDATE SKIP LOCKED` locks the row
    // being taken, not the sibling row the check is about. Cheap to hold —
    // this transaction is two statements and no model call, which is the whole
    // reason the turn is written down and run outside it.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(CLAIM_LOCK)
        .execute(&mut *transaction)
        .await?;
    // Dropped here rather than skipped, so the queue does not silt up with
    // firings nothing will ever run. Named in the log, because a firing nobody
    // receives is worth one line saying which schedule it belonged to.
    let abandoned: Vec<Uuid> = sqlx::query_scalar(
        "DELETE FROM reminder_turns WHERE attempts >= $1 AND claimed_at IS NOT NULL \
         AND claimed_at <= NOW() - make_interval(secs => $2) RETURNING reminder_id",
    )
    .bind(MAX_TURN_ATTEMPTS)
    .bind(seconds)
    .fetch_all(&mut *transaction)
    .await?;
    for reminder_id in abandoned {
        tracing::warn!(
            %reminder_id,
            attempts = MAX_TURN_ATTEMPTS,
            "Reminder turn abandoned after repeated attempts; its firing may never have reached \
             the chat"
        );
    }
    // The attempt bound is repeated here so the rows this can claim and the
    // rows the delete above takes are disjoint sets. Without it a worker could
    // hold a row another worker's delete is waiting on while waiting on a row
    // that worker holds, which is a deadlock either of them could have been
    // written out of.
    //
    // The NOT EXISTS is what keeps one schedule's firings in order. Two of them
    // can be owed at once -- the server was down, and the schedule moved on
    // twice -- and a watch handed both at the same time would read one baseline
    // into both, answer the same question twice over, and write the two answers
    // back in whatever order they finished. Its whole contract is that each
    // firing is compared with the one before it, so they run one at a time: a
    // firing whose schedule already has a turn under a live claim is left for
    // the next sweep, by which time the one ahead of it has recorded what it
    // found. A claim that went stale is not live and does not hold its siblings.
    //
    // Live is decided by the claim alone, with no attempt bound: a turn taken
    // for the third time has reached the limit and is still running, and asking
    // for `attempts < $1` here would read the last attempt any firing gets as
    // though nothing held the schedule at all. The bound belongs on the row
    // being taken, which is a different question from whether another one is
    // already out.
    let claimed = sqlx::query_as::<_, PendingTurn>(
        "UPDATE reminder_turns SET attempts = attempts + 1, claimed_at = NOW() WHERE id = ( \
         SELECT owed.id FROM reminder_turns AS owed WHERE owed.attempts < $1 \
         AND (owed.claimed_at IS NULL OR owed.claimed_at <= NOW() - make_interval(secs => $2)) \
         AND NOT EXISTS ( \
           SELECT 1 FROM reminder_turns AS ahead \
           WHERE ahead.reminder_id = owed.reminder_id AND ahead.id <> owed.id \
           AND ahead.claimed_at IS NOT NULL \
           AND ahead.claimed_at > NOW() - make_interval(secs => $2)) \
         ORDER BY owed.created_at, owed.id LIMIT 1 FOR UPDATE SKIP LOCKED) \
         RETURNING id, reminder_id, workspace_id, chat_id, created_by AS user_id, prompt",
    )
    .bind(MAX_TURN_ATTEMPTS)
    .bind(seconds)
    .fetch_optional(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(claimed)
}

/// Marks one firing's turn as run, whatever the turn made of it.
///
/// A turn that reached the chat and failed there has already said so in the
/// chat, and offering it again would repeat the failure rather than recover
/// from it. The retry in `claim_turn` exists for the one case this cannot
/// reach: a process that did not live to call this.
pub async fn finish_turn(pool: &PgPool, turn_id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM reminder_turns WHERE id = $1")
        .bind(turn_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// What the last firing of this watch found, or `None` when the reminder is
/// not a watch at all.
///
/// The outer `Option` answers whether to compose a comparison into the firing;
/// the inner one is empty on the first firing, which has nothing to compare
/// against yet. Read live at run time rather than snapshotted onto the turn,
/// so two firings that queued up behind a restart compare against each other
/// rather than both against the reading they shared.
pub async fn watch_baseline(pool: &PgPool, reminder_id: Uuid) -> DbResult<Option<Option<String>>> {
    let baseline = sqlx::query_scalar::<_, Option<String>>(
        "SELECT last_observation FROM reminders WHERE id = $1 AND timing_mode = 'condition_watch'",
    )
    .bind(reminder_id)
    .fetch_optional(pool)
    .await?;
    Ok(baseline)
}

/// Keeps what this firing found as the baseline the next one is compared with.
///
/// The answer is read back by the id the turn was given rather than by taking
/// the chat's most recent message, so something the person typed while the turn
/// was running cannot be mistaken for the watch's own reading.
///
/// A firing that stored no answer — it failed before the model replied, or was
/// cancelled — leaves the baseline as it was. Overwriting it with nothing would
/// make the next firing compare against an empty reading and report the world
/// changed when all that happened is that this firing did not run.
pub async fn record_observation(
    pool: &PgPool,
    reminder_id: Uuid,
    message_id: Uuid,
) -> DbResult<()> {
    let answer = sqlx::query_scalar::<_, String>(
        "SELECT content FROM messages WHERE id = $1 AND role = 'assistant'",
    )
    .bind(message_id)
    .fetch_optional(pool)
    .await?
    .filter(|answer| !answer.trim().is_empty());
    let Some(answer) = answer else {
        tracing::warn!(
            %reminder_id,
            "A watch fired but stored no answer; its baseline is left as it was, so the next \
             firing compares against the last reading that worked"
        );
        return Ok(());
    };
    sqlx::query(
        "UPDATE reminders SET last_observation = $2 WHERE id = $1 \
         AND timing_mode = 'condition_watch'",
    )
    .bind(reminder_id)
    .bind(answer.chars().take(OBSERVATION_CAP).collect::<String>())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use sqlx::postgres::PgPoolOptions;

    /// Validation answers before the pool is touched, so a pool nothing listens
    /// behind is enough to read the refusal.
    fn unreachable_pool() -> PgPool {
        PgPoolOptions::new()
            .connect_lazy("postgres://127.0.0.1:1/zone")
            .expect("a lazy pool needs no server")
    }

    fn reminder(due_at: DateTime<FixedOffset>) -> Reminder {
        Reminder {
            content: "Stand up".into(),
            due_at,
            rrule: None,
            prompt: None,
            timing_mode: None,
        }
    }

    fn refusal(error: sqlx::Error) -> String {
        match error {
            sqlx::Error::Protocol(message) => message,
            other => panic!("expected a validation refusal, got {other}"),
        }
    }

    #[tokio::test]
    async fn a_past_due_at_is_named_against_now_in_its_own_offset() {
        let offset = FixedOffset::east_opt(12 * 3600).unwrap();
        let due = (Utc::now() - Duration::minutes(6)).with_timezone(&offset);
        let error = create(
            &unreachable_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            reminder(due),
        )
        .await
        .expect_err("a past due_at is refused");
        let message = refusal(error);
        let expected = format!(
            "due_at {} is in the past; now is ",
            due.format("%H:%M:%S%:z")
        );
        assert!(
            message.starts_with(&expected),
            "the refusal names the time that was asked for: {message}"
        );
        assert!(
            message.ends_with("+12:00"),
            "now is given in the caller's offset: {message}"
        );
        assert!(
            !message.contains("RFC3339"),
            "a well-formed value is not told about the format: {message}"
        );
    }

    #[tokio::test]
    async fn a_past_due_at_on_another_day_carries_its_date() {
        let due = (Utc::now() - Duration::days(2)).fixed_offset();
        let message = refusal(
            create(
                &unreachable_pool(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                reminder(due),
            )
            .await
            .expect_err("a past due_at is refused"),
        );
        assert!(
            message.starts_with(&format!("due_at {}", due.format("%Y-%m-%dT%H:%M:%S%:z"))),
            "{message}"
        );
    }

    #[tokio::test]
    async fn blank_content_is_named_on_its_own() {
        let mut input = reminder((Utc::now() + Duration::hours(1)).fixed_offset());
        input.content = "   ".into();
        let message = refusal(
            create(
                &unreachable_pool(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                input,
            )
            .await
            .expect_err("blank content is refused"),
        );
        assert!(message.starts_with("content is blank"), "{message}");
    }

    #[test]
    fn a_due_at_without_an_offset_is_told_the_format() {
        let error = serde_json::from_value::<Reminder>(
            json!({"content": "check", "due_at": "2026-09-20T19:30:00"}),
        )
        .expect_err("no offset is refused");
        let message = error.to_string();
        assert!(
            message.contains("due_at \"2026-09-20T19:30:00\" is not RFC3339"),
            "{message}"
        );
        assert!(message.contains("2026-09-20T19:30:00+12:00"), "{message}");
    }

    #[test]
    fn a_due_at_keeps_the_offset_it_was_written_in() {
        let input: Reminder = serde_json::from_value(
            json!({"content": "check", "due_at": "2026-09-20T19:30:00+12:00"}),
        )
        .unwrap();
        assert_eq!(input.due_at.offset().local_minus_utc(), 12 * 3600);
        assert_eq!(input.due_at.to_rfc3339(), "2026-09-20T19:30:00+12:00");
    }
}
