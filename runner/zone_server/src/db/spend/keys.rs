//! The LiteLLM virtual keys Zone derived, by token, and whose spend each one carries. A revoked
//! key keeps its row, so spend LiteLLM logged under it before the revocation still attributes.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::DbResult;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct KeyRow {
    pub token: String,
    pub organization_id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Records `token` as carrying the spend of `user` in `workspace`, or of its automation when
/// `user` is none. A token derived again after its revocation is active once more.
pub async fn insert(
    pool: &PgPool,
    token: &str,
    organization: Uuid,
    workspace: Uuid,
    user: Option<Uuid>,
) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO spend_keys (token, organization_id, workspace_id, user_id)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (token) DO UPDATE SET revoked_at = NULL
         WHERE spend_keys.revoked_at IS NOT NULL",
    )
    .bind(token)
    .bind(organization)
    .bind(workspace)
    .bind(user)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn active(pool: &PgPool) -> DbResult<Vec<KeyRow>> {
    sqlx::query_as(
        "SELECT token, organization_id, workspace_id, user_id, created_at
         FROM spend_keys WHERE revoked_at IS NULL ORDER BY created_at, token",
    )
    .fetch_all(pool)
    .await
}

pub async fn revoked(pool: &PgPool, token: &str) -> DbResult<()> {
    sqlx::query("UPDATE spend_keys SET revoked_at = now() WHERE token = $1 AND revoked_at IS NULL")
        .bind(token)
        .execute(pool)
        .await?;
    Ok(())
}

/// When the first key was derived, revoked or not: no spend Zone attributes is older.
pub async fn earliest(pool: &PgPool) -> DbResult<Option<DateTime<Utc>>> {
    sqlx::query_scalar("SELECT min(created_at) FROM spend_keys")
        .fetch_one(pool)
        .await
}
