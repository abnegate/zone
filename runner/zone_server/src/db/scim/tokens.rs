use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

use super::new_token::NewToken;
use super::token_credential::TokenCredential;
use super::token_record::TokenRecord;
use crate::db::DbResult;

const COLUMNS: &str =
    "id, organization_id, name, hint, created_by, created_at, rotated_at, last_used_at";

pub async fn create(executor: impl PgExecutor<'_>, new: &NewToken<'_>) -> DbResult<TokenRecord> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO scim_tokens (organization_id, name, token_hash, hint, created_by) \
         VALUES ($1, $2, $3, $4, $5) RETURNING {COLUMNS}"
    )))
    .bind(new.organization_id)
    .bind(new.name)
    .bind(new.token_hash)
    .bind(new.hint)
    .bind(new.created_by)
    .fetch_one(executor)
    .await
}

pub async fn rotate(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    id: Uuid,
    token_hash: &str,
    hint: &str,
) -> DbResult<Option<TokenRecord>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE scim_tokens SET token_hash = $3, hint = $4, rotated_at = now() \
         WHERE organization_id = $1 AND id = $2 AND revoked_at IS NULL RETURNING {COLUMNS}"
    )))
    .bind(organization_id)
    .bind(id)
    .bind(token_hash)
    .bind(hint)
    .fetch_optional(executor)
    .await
}

pub async fn revoke(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    id: Uuid,
) -> DbResult<Option<TokenRecord>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE scim_tokens SET revoked_at = now() \
         WHERE organization_id = $1 AND id = $2 AND revoked_at IS NULL RETURNING {COLUMNS}"
    )))
    .bind(organization_id)
    .bind(id)
    .fetch_optional(executor)
    .await
}

pub async fn list(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
) -> DbResult<Vec<TokenRecord>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM scim_tokens \
         WHERE organization_id = $1 AND revoked_at IS NULL ORDER BY created_at, id"
    )))
    .bind(organization_id)
    .fetch_all(executor)
    .await
}

pub async fn active(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
) -> DbResult<Vec<TokenCredential>> {
    sqlx::query_as(
        "SELECT id, name, token_hash FROM scim_tokens \
         WHERE organization_id = $1 AND revoked_at IS NULL ORDER BY created_at, id",
    )
    .bind(organization_id)
    .fetch_all(executor)
    .await
}

pub async fn touch(executor: impl PgExecutor<'_>, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query(
        "UPDATE scim_tokens SET last_used_at = now() \
         WHERE id = $1 AND revoked_at IS NULL \
           AND (last_used_at IS NULL OR last_used_at < now() - interval '1 minute')",
    )
    .bind(id)
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn last_used(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
) -> DbResult<Option<DateTime<Utc>>> {
    sqlx::query_scalar("SELECT MAX(last_used_at) FROM scim_tokens WHERE organization_id = $1")
        .bind(organization_id)
        .fetch_one(executor)
        .await
}
