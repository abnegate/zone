//! Chat database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Chat row from database
#[derive(Debug, Clone)]
pub struct ChatRow {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub title: String,
    pub model_name: String,
    pub archived: Option<bool>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Message row from database
#[derive(Debug, Clone)]
pub struct MessageRow {
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
                SELECT id, workspace_id, title, model_name, archived, created_at, updated_at
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
                SELECT id, workspace_id, title, model_name, archived, created_at, updated_at
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
                SELECT id, workspace_id, title, model_name, archived, created_at, updated_at
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
                SELECT id, workspace_id, title, model_name, archived, created_at, updated_at
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
        SELECT id, workspace_id, title, model_name, archived, created_at, updated_at
        FROM chats
        WHERE id = $1
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
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Create a new chat
pub async fn create_chat(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
    title: &str,
    model_name: &str,
) -> DbResult<ChatRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO chats (workspace_id, title, model_name)
        VALUES ($1, $2, $3)
        RETURNING id, workspace_id, title, model_name, archived, created_at, updated_at
        "#,
        workspace_id,
        title,
        model_name
    )
    .fetch_one(pool)
    .await?;

    Ok(ChatRow {
        id: row.id,
        workspace_id: row.workspace_id,
        title: row.title,
        model_name: row.model_name,
        archived: row.archived,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Update chat title
pub async fn update_chat(
    pool: &PgPool,
    id: Uuid,
    title: Option<&str>,
) -> DbResult<Option<ChatRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE chats
        SET title = COALESCE($2, title),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, workspace_id, title, model_name, archived, created_at, updated_at
        "#,
        id,
        title
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ChatRow {
        id: r.id,
        workspace_id: r.workspace_id,
        title: r.title,
        model_name: r.model_name,
        archived: r.archived,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
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
        RETURNING id, workspace_id, title, model_name, archived, created_at, updated_at
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
        RETURNING id, workspace_id, title, model_name, archived, created_at, updated_at
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
            id: r.id,
            chat_id: r.chat_id,
            role: r.role,
            content: r.content,
            metadata: r.metadata,
            created_at: r.created_at,
        })
        .collect())
}

/// Create a new message
pub async fn create_message(
    pool: &PgPool,
    chat_id: Uuid,
    role: &str,
    content: &str,
    metadata: Option<serde_json::Value>,
) -> DbResult<MessageRow> {
    // Also update chat's updated_at
    sqlx::query!("UPDATE chats SET updated_at = NOW() WHERE id = $1", chat_id)
        .execute(pool)
        .await?;

    let row = sqlx::query!(
        r#"
        INSERT INTO messages (chat_id, role, content, metadata)
        VALUES ($1, $2, $3, $4)
        RETURNING id, chat_id, role, content, metadata, created_at
        "#,
        chat_id,
        role,
        content,
        metadata
    )
    .fetch_one(pool)
    .await?;

    Ok(MessageRow {
        id: row.id,
        chat_id: row.chat_id,
        role: row.role,
        content: row.content,
        metadata: row.metadata,
        created_at: row.created_at,
    })
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
