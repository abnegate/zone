//! Chat database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::services::character::ChatCharacter;

use super::DbResult;

/// Chat row from database
#[derive(Debug, Clone)]
pub struct ChatRow {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub title: String,
    pub model_name: String,
    pub archived: Option<bool>,
    /// Whether this chat runs the tool-calling agent loop instead of a plain
    /// completion.
    pub agent_enabled: bool,
    /// Whether the agent is restricted to authorized workspace tools. When
    /// false it also gets the host tools that read, write and run commands.
    pub agent_sandboxed: bool,
    /// When true, mutating file and shell tools run without a confirmation.
    pub auto_approve: bool,
    /// Persona for models that expect a character card. Absent on ordinary assistant chats.
    pub character: Option<ChatCharacter>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Message row from database
#[derive(Debug, Clone)]
pub struct MessageRow {
    pub title_claimed: bool,
    pub id: Uuid,
    pub chat_id: Uuid,
    pub role: String,
    pub content: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: Option<NaiveDateTime>,
}

/// Helper macro to map a row to ChatRow
macro_rules! map_chat_row {
    ($r:expr) => {
        ChatRow {
            id: $r.id,
            workspace_id: $r.workspace_id,
            title: $r.title,
            model_name: $r.model_name,
            archived: $r.archived,
            agent_enabled: $r.agent_enabled,
            agent_sandboxed: $r.agent_sandboxed,
            auto_approve: $r.auto_approve,
            character: None,
            created_at: $r.created_at,
            updated_at: $r.updated_at,
        }
    };
}

/// List chats with optional workspace and archived filter
pub async fn list_chats(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
    archived: Option<bool>,
) -> DbResult<Vec<ChatRow>> {
    match (workspace_id, archived) {
        (Some(wid), Some(a)) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, workspace_id, title, model_name, archived, agent_enabled, agent_sandboxed, auto_approve, created_at, updated_at
                FROM chats
                WHERE workspace_id = $1 AND archived = $2
                ORDER BY updated_at DESC
                "#,
                wid,
                a
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_chat_row!(r)).collect())
        }
        (Some(wid), None) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, workspace_id, title, model_name, archived, agent_enabled, agent_sandboxed, auto_approve, created_at, updated_at
                FROM chats
                WHERE workspace_id = $1
                ORDER BY updated_at DESC
                "#,
                wid
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_chat_row!(r)).collect())
        }
        (None, Some(a)) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, workspace_id, title, model_name, archived, agent_enabled, agent_sandboxed, auto_approve, created_at, updated_at
                FROM chats
                WHERE archived = $1
                ORDER BY updated_at DESC
                "#,
                a
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_chat_row!(r)).collect())
        }
        (None, None) => {
            let rows = sqlx::query!(
                r#"
                SELECT id, workspace_id, title, model_name, archived, agent_enabled, agent_sandboxed, auto_approve, created_at, updated_at
                FROM chats
                ORDER BY updated_at DESC
                "#
            )
            .fetch_all(pool)
            .await?;
            Ok(rows.into_iter().map(|r| map_chat_row!(r)).collect())
        }
    }
}

/// Get chat by ID
pub async fn get_chat(pool: &PgPool, id: Uuid) -> DbResult<Option<ChatRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, workspace_id, title, model_name, archived, agent_enabled, agent_sandboxed, auto_approve, created_at, updated_at
        FROM chats
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    let Some(r) = row else {
        return Ok(None);
    };
    let mut chat = ChatRow {
        id: r.id,
        workspace_id: r.workspace_id,
        title: r.title,
        model_name: r.model_name,
        archived: r.archived,
        agent_enabled: r.agent_enabled,
        agent_sandboxed: r.agent_sandboxed,
        auto_approve: r.auto_approve,
        character: None,
        created_at: r.created_at,
        updated_at: r.updated_at,
    };
    chat.character = get_chat_character(pool, chat.id).await?;
    Ok(Some(chat))
}

/// Load a chat's character card. Missing or unreadable JSON is treated as none.
pub async fn get_chat_character(pool: &PgPool, id: Uuid) -> DbResult<Option<ChatCharacter>> {
    let value: Option<Option<serde_json::Value>> =
        sqlx::query_scalar("SELECT character FROM chats WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(value
        .flatten()
        .and_then(|value| serde_json::from_value(value).ok()))
}

/// Replace or clear the character card on a chat, then return the fresh row.
pub async fn set_chat_character(
    pool: &PgPool,
    id: Uuid,
    character: Option<&ChatCharacter>,
) -> DbResult<Option<ChatRow>> {
    let value = character
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
    let updated = sqlx::query("UPDATE chats SET character = $2, updated_at = NOW() WHERE id = $1")
        .bind(id)
        .bind(value)
        .execute(pool)
        .await?
        .rows_affected();
    if updated == 0 {
        return Ok(None);
    }
    get_chat(pool, id).await
}

/// Create a new chat
pub async fn create_chat(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
    title: &str,
    model_name: &str,
    agent_enabled: bool,
    agent_sandboxed: bool,
) -> DbResult<ChatRow> {
    create_chat_with_title(
        pool,
        workspace_id,
        title,
        model_name,
        (agent_enabled, agent_sandboxed),
        false,
        false,
    )
    .await
}

/// Create a chat with explicitly opted-in automatic naming.
pub async fn create_chat_with_title(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
    title: &str,
    model_name: &str,
    agent: (bool, bool),
    automatic_title: bool,
    auto_approve: bool,
) -> DbResult<ChatRow> {
    let (agent_enabled, agent_sandboxed) = agent;
    let mut transaction = pool.begin().await?;
    let row = sqlx::query!(
        r#"
        INSERT INTO chats (workspace_id, title, model_name, agent_enabled, agent_sandboxed, auto_approve)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, workspace_id, title, model_name, archived, agent_enabled, agent_sandboxed, auto_approve, created_at, updated_at
        "#,
        workspace_id,
        title,
        model_name,
        agent_enabled,
        agent_sandboxed,
        auto_approve
    )
    .fetch_one(&mut *transaction)
    .await?;

    if automatic_title {
        sqlx::query("UPDATE chats SET automatic_title = TRUE WHERE id = $1")
            .bind(row.id)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;

    Ok(ChatRow {
        id: row.id,
        workspace_id: row.workspace_id,
        title: row.title,
        model_name: row.model_name,
        archived: row.archived,
        agent_enabled: row.agent_enabled,
        agent_sandboxed: row.agent_sandboxed,
        auto_approve: row.auto_approve,
        character: None,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Update a chat's title and/or agent mode. `None` leaves a field untouched.
pub async fn update_chat(
    pool: &PgPool,
    id: Uuid,
    title: Option<&str>,
    agent_enabled: Option<bool>,
    agent_sandboxed: Option<bool>,
    auto_approve: Option<bool>,
) -> DbResult<Option<ChatRow>> {
    let mut transaction = pool.begin().await?;
    if title.is_some() {
        sqlx::query("UPDATE chats SET automatic_title = FALSE WHERE id = $1")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
    }
    let row = sqlx::query!(
        r#"
        UPDATE chats
        SET title = COALESCE($2, title),
            agent_enabled = COALESCE($3, agent_enabled),
            agent_sandboxed = COALESCE($4, agent_sandboxed),
            auto_approve = COALESCE($5, auto_approve),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, title, model_name, archived, agent_enabled, agent_sandboxed, auto_approve, created_at, updated_at
        "#,
        id,
        title,
        agent_enabled,
        agent_sandboxed,
        auto_approve
    )
    .fetch_optional(&mut *transaction)
    .await?;

    transaction.commit().await?;

    match row {
        Some(r) => get_chat(pool, r.id).await,
        None => Ok(None),
    }
}

/// Save a generated title only while its original claim still owns the title.
pub async fn complete_title(
    pool: &PgPool,
    chat_id: Uuid,
    message_id: Uuid,
    title: &str,
) -> DbResult<bool> {
    Ok(sqlx::query("UPDATE chats SET title = $3, automatic_title = FALSE, updated_at = NOW() WHERE id = $1 AND automatic_title AND title_message_id = $2")
        .bind(chat_id).bind(message_id).bind(title).execute(pool).await?.rows_affected() == 1)
}

/// Delete a chat
pub async fn delete_chat(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!("DELETE FROM chats WHERE id = $1", id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Archive a chat
pub async fn archive_chat(pool: &PgPool, id: Uuid) -> DbResult<Option<ChatRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE chats
        SET archived = true,
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, title, model_name, archived, agent_enabled, agent_sandboxed, auto_approve, created_at, updated_at
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ChatRow {
        id: r.id,
        workspace_id: r.workspace_id,
        title: r.title,
        model_name: r.model_name,
        archived: r.archived,
        agent_enabled: r.agent_enabled,
        agent_sandboxed: r.agent_sandboxed,
        auto_approve: r.auto_approve,
        character: None,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Unarchive a chat
pub async fn unarchive_chat(pool: &PgPool, id: Uuid) -> DbResult<Option<ChatRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE chats
        SET archived = false,
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, title, model_name, archived, agent_enabled, agent_sandboxed, auto_approve, created_at, updated_at
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ChatRow {
        id: r.id,
        workspace_id: r.workspace_id,
        title: r.title,
        model_name: r.model_name,
        archived: r.archived,
        agent_enabled: r.agent_enabled,
        agent_sandboxed: r.agent_sandboxed,
        auto_approve: r.auto_approve,
        character: None,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// List messages for a chat
pub async fn list_messages(pool: &PgPool, chat_id: Uuid) -> DbResult<Vec<MessageRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, chat_id, role, content, metadata, created_at
        FROM messages
        WHERE chat_id = $1
        ORDER BY created_at ASC
        "#,
        chat_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| MessageRow {
            title_claimed: false,
            id: r.id,
            chat_id: r.chat_id,
            role: r.role,
            content: r.content,
            metadata: r.metadata,
            created_at: r.created_at,
        })
        .collect())
}

/// Newest `limit` messages in chronological order, for a model turn.
pub async fn list_recent_messages(
    pool: &PgPool,
    chat_id: Uuid,
    limit: i64,
) -> DbResult<Vec<MessageRow>> {
    let mut messages = list_messages(pool, chat_id).await?;
    let keep = usize::try_from(limit).unwrap_or(0);
    if messages.len() > keep {
        messages.drain(..messages.len() - keep);
    }
    Ok(messages)
}

/// Create a new message
///
/// # Background Embedding Generation
///
/// Message embeddings should be generated asynchronously via a background task
/// to avoid blocking the message creation flow. The embedding can be stored using
/// `message_embeddings::store_message_embedding()`.
///
/// This allows messages to be saved immediately while the embedding is computed
/// in the background, providing a better user experience and enabling the system
/// to handle embedding failures gracefully without affecting message creation.
///
/// Future implementations may use a job queue (e.g., tokio task, dedicated worker,
/// or queue system like BullMQ) to handle embedding generation at scale.
pub async fn create_message(
    pool: &PgPool,
    chat_id: Uuid,
    role: &str,
    content: &str,
    metadata: Option<serde_json::Value>,
) -> DbResult<MessageRow> {
    create_message_with_id(pool, Uuid::new_v4(), chat_id, role, content, metadata).await
}

/// Create a message with a caller-provided ID.
///
/// Streaming protocols can publish this ID before persistence and later use it
/// as the durable owner for message-scoped artifacts.
pub async fn create_message_with_id(
    pool: &PgPool,
    message_id: Uuid,
    chat_id: Uuid,
    role: &str,
    content: &str,
    metadata: Option<serde_json::Value>,
) -> DbResult<MessageRow> {
    let mut transaction = pool.begin().await?;
    // The row lock serializes first-user-message ownership with every insertion
    // and explicit rename, including requests served by different processes.
    sqlx::query("UPDATE chats SET updated_at = NOW() WHERE id = $1")
        .bind(chat_id)
        .execute(&mut *transaction)
        .await?;

    let title_claimed = if role == "user" {
        sqlx::query(
            "UPDATE chats SET title_message_id = $2
             WHERE id = $1 AND automatic_title AND title_message_id IS NULL
             AND NOT EXISTS (SELECT 1 FROM messages WHERE chat_id = $1 AND role = 'user')",
        )
        .bind(chat_id)
        .bind(message_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected()
            == 1
    } else {
        false
    };

    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            String,
            String,
            Option<serde_json::Value>,
            Option<NaiveDateTime>,
        ),
    >(
        r#"
        INSERT INTO messages (id, chat_id, role, content, metadata)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, chat_id, role, content, metadata, created_at
        "#,
    )
    .bind(message_id)
    .bind(chat_id)
    .bind(role)
    .bind(content)
    .bind(metadata)
    .fetch_one(&mut *transaction)
    .await?;

    transaction.commit().await?;

    Ok(MessageRow {
        title_claimed,
        id: row.0,
        chat_id: row.1,
        role: row.2,
        content: row.3,
        metadata: row.4,
        created_at: row.5,
    })
}

/// Create a message with background embedding generation
///
/// This is a wrapper around `create_message` that also triggers
/// background embedding generation for the message content.
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `chat_id` - ID of the chat
/// * `role` - Role of the message (user, assistant, system)
/// * `content` - Message content
/// * `metadata` - Optional metadata JSON
/// * `embedding_service` - Optional embedding service for generating embeddings
///
/// # Returns
/// The created message row
///
/// # Note
/// The embedding generation happens asynchronously in the background.
/// Any embedding errors are logged but do not fail the message creation.
pub async fn create_message_with_embedding(
    pool: &PgPool,
    chat_id: Uuid,
    role: &str,
    content: &str,
    metadata: Option<serde_json::Value>,
    embedding_service: Option<Arc<dyn zone_context::embeddings::EmbeddingService>>,
) -> DbResult<MessageRow> {
    // Create the message first
    let message = create_message(pool, chat_id, role, content, metadata).await?;

    // Trigger background embedding generation if service is available
    if let Some(service) = embedding_service {
        let pool_clone = pool.clone();
        let message_id = message.id;
        let content_owned = content.to_string();

        // Spawn background task for embedding generation with timeout
        tokio::spawn(async move {
            use std::time::Duration;

            // Add 30-second timeout to prevent hanging indefinitely
            match tokio::time::timeout(Duration::from_secs(30), service.embed(&content_owned)).await
            {
                Ok(Ok(embedding)) => {
                    let model = service.model();
                    if let Err(e) = super::message_embeddings::store_message_embedding(
                        &pool_clone,
                        message_id,
                        chat_id,
                        &embedding,
                        model,
                    )
                    .await
                    {
                        tracing::error!(
                            "Failed to store message embedding for message_id={}: {}",
                            message_id,
                            e
                        );
                    } else {
                        tracing::debug!(
                            "Generated and stored embedding for message_id={}",
                            message_id
                        );
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        "Failed to generate embedding for message_id={}: {}",
                        message_id,
                        e
                    );
                }
                Err(_) => {
                    tracing::warn!(
                        "Embedding timeout for message_id={} (exceeded 30s)",
                        message_id
                    );
                }
            }
        });
    }

    Ok(message)
}

/// Delete a message
pub async fn delete_message(pool: &PgPool, chat_id: Uuid, message_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!(
        "DELETE FROM messages WHERE id = $1 AND chat_id = $2",
        message_id,
        chat_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}
