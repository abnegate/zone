use sqlx::PgExecutor;
use uuid::Uuid;

use super::identity::Identity;
use crate::db::DbResult;

pub const SUBJECT_KEY: &str = "sso_identities_provider_id_subject_key";
pub const USER_KEY: &str = "sso_identities_provider_id_user_id_key";

const COLUMNS: &str = "id, provider_id, user_id, subject, email, created_at, last_signed_in_at";

pub async fn find(
    executor: impl PgExecutor<'_>,
    provider_id: Uuid,
    subject: &str,
) -> DbResult<Option<Identity>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM sso_identities WHERE provider_id = $1 AND subject = $2"
    )))
    .bind(provider_id)
    .bind(subject)
    .fetch_optional(executor)
    .await
}

pub async fn linked(executor: impl PgExecutor<'_>, user_id: Uuid) -> DbResult<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sso_identities WHERE user_id = $1)")
        .bind(user_id)
        .fetch_one(executor)
        .await
}

pub async fn link(
    executor: impl PgExecutor<'_>,
    provider_id: Uuid,
    user_id: Uuid,
    subject: &str,
    email: &str,
) -> DbResult<Identity> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO sso_identities (provider_id, user_id, subject, email) \
         VALUES ($1, $2, $3, $4) RETURNING {COLUMNS}"
    )))
    .bind(provider_id)
    .bind(user_id)
    .bind(subject)
    .bind(email)
    .fetch_one(executor)
    .await
}

pub async fn touch(executor: impl PgExecutor<'_>, id: Uuid, email: &str) -> DbResult<()> {
    let result = sqlx::query(
        "UPDATE sso_identities SET email = $2, last_signed_in_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(email)
    .execute(executor)
    .await?;
    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}

pub async fn clear(executor: impl PgExecutor<'_>, provider_id: Uuid) -> DbResult<u64> {
    let result = sqlx::query("DELETE FROM sso_identities WHERE provider_id = $1")
        .bind(provider_id)
        .execute(executor)
        .await?;
    Ok(result.rows_affected())
}
