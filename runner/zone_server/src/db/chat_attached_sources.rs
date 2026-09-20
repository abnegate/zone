//! The sources a chat is pinned to.
//!
//! Chosen by the person before a turn rather than recorded after one, which is
//! what separates this from `chat_sources`: that table proves what a chat has
//! already retrieved and may cite, this one narrows what its next retrieval
//! looks at. A chat with nothing attached searches the whole workspace.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Attached {
    pub id: Uuid,
    pub name: String,
    pub source_type: String,
    pub attached_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReplaceError {
    /// Sources that are not active in the chat's workspace. Naming them lets
    /// the caller say which, and refusing the whole write keeps a partial
    /// attachment from standing in for the one that was asked for.
    #[error("{} source(s) are not in this chat's workspace", .0.len())]
    Foreign(Vec<Uuid>),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// The attached sources that are still active, newest attachment last.
pub async fn list(pool: &PgPool, chat_id: Uuid) -> DbResult<Vec<Attached>> {
    sqlx::query_as::<_, Attached>(
        "SELECT source.id, source.name, source.source_type, attached.attached_at \
         FROM chat_attached_sources attached \
         JOIN sources source ON source.id = attached.source_id \
         WHERE attached.chat_id = $1 AND source.is_active = TRUE \
         ORDER BY attached.attached_at, source.name",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await
}

/// The ids alone, for a retrieval that only needs to know where to look.
pub async fn ids(pool: &PgPool, chat_id: Uuid) -> DbResult<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT attached.source_id \
         FROM chat_attached_sources attached \
         JOIN sources source ON source.id = attached.source_id \
         WHERE attached.chat_id = $1 AND source.is_active = TRUE",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await
}

/// What a retrieval for `chat` is confined to: the attached ids when there
/// are any, and nothing when the chat searches the whole workspace. A task run
/// has no chat and is never confined. A registry that cannot be read is logged
/// and treated as empty, since the searches that follow would fail on the
/// same database anyway.
pub async fn scope(pool: &PgPool, chat: Option<Uuid>) -> Option<Vec<Uuid>> {
    let chat = chat?;
    match ids(pool, chat).await {
        Ok(ids) if ids.is_empty() => None,
        Ok(ids) => Some(ids),
        Err(error) => {
            tracing::warn!(%error, chat_id = %chat, "Attached sources could not be read");
            None
        }
    }
}

/// Make `source_ids` the chat's whole attachment: rows not named are removed,
/// rows already present are kept with their original time.
pub async fn replace(
    pool: &PgPool,
    chat_id: Uuid,
    workspace_id: Uuid,
    source_ids: &[Uuid],
) -> Result<Vec<Attached>, ReplaceError> {
    let mut requested = source_ids.to_vec();
    requested.sort_unstable();
    requested.dedup();

    let mut transaction = pool.begin().await?;
    let known: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM sources WHERE id = ANY($1) AND workspace_id = $2 AND is_active = TRUE",
    )
    .bind(&requested)
    .bind(workspace_id)
    .fetch_all(&mut *transaction)
    .await?;
    let foreign: Vec<Uuid> = requested
        .iter()
        .copied()
        .filter(|id| !known.contains(id))
        .collect();
    if !foreign.is_empty() {
        return Err(ReplaceError::Foreign(foreign));
    }

    sqlx::query(
        "DELETE FROM chat_attached_sources WHERE chat_id = $1 AND NOT (source_id = ANY($2))",
    )
    .bind(chat_id)
    .bind(&requested)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO chat_attached_sources (chat_id, source_id) \
         SELECT $1, source_id FROM UNNEST($2::uuid[]) AS source_id \
         ON CONFLICT (chat_id, source_id) DO NOTHING",
    )
    .bind(chat_id)
    .bind(&requested)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(list(pool, chat_id).await?)
}
