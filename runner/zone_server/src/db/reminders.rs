//! Durable reminders delivered atomically to their originating chat.
use super::{DbResult, actions};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reminder {
    pub content: String,
    pub due_at: DateTime<Utc>,
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
    let mut transaction = pool.begin().await?;
    actions::authorize(&mut transaction, workspace_id, user_id, true).await?;
    actions::chat(&mut transaction, workspace_id, chat_id).await?;
    let result = sqlx::query_scalar("INSERT INTO reminders (workspace_id, created_by, chat_id, content, due_at) VALUES ($1, $2, $3, $4, $5) RETURNING to_jsonb(reminders.*)")
        .bind(workspace_id).bind(user_id).bind(chat_id).bind(input.content).bind(input.due_at).fetch_one(&mut *transaction).await?;
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

/// The claim, message, and terminal state share a transaction across all workers.
pub async fn deliver_next(pool: &PgPool) -> DbResult<bool> {
    let mut transaction = pool.begin().await?;
    let next: Option<(Uuid, Uuid, Uuid, Uuid, String)> = sqlx::query_as("SELECT id, workspace_id, created_by, chat_id, content FROM reminders WHERE status = 'pending' AND due_at <= NOW() ORDER BY due_at, id LIMIT 1 FOR UPDATE SKIP LOCKED")
        .fetch_optional(&mut *transaction).await?;
    let Some((id, workspace_id, user_id, chat_id, content)) = next else {
        return Ok(false);
    };
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
        sqlx::query("UPDATE reminders SET status = 'delivered', completed_at = NOW(), message_id = $2 WHERE id = $1").bind(id).bind(message_id).execute(&mut *transaction).await?;
    }
    transaction.commit().await?;
    if let Some(message) = message {
        actions::publish(chat_id, message);
    }
    Ok(true)
}
