use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

use super::predicate::scoped;
use super::scope::Scope;
use crate::db::DbResult;

const COLUMNS: &str =
    "organization_id, days, updated_by, created_at, updated_at, swept_at, deleted_count";

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct RetentionRecord {
    pub organization_id: Option<Uuid>,
    pub days: i32,
    pub updated_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub swept_at: Option<DateTime<Utc>>,
    pub deleted_count: i64,
}

impl RetentionRecord {
    pub fn scope(&self) -> Scope {
        Scope::from(self.organization_id)
    }
}

pub async fn get(executor: impl PgExecutor<'_>, scope: Scope) -> DbResult<Option<RetentionRecord>> {
    let predicate = scoped("organization_id", scope, 1);
    sqlx::query_as::<_, RetentionRecord>(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM audit_retentions WHERE {predicate}"
    )))
    .bind(scope.organization_id())
    .fetch_optional(executor)
    .await
}

pub async fn set(
    executor: impl PgExecutor<'_>,
    scope: Scope,
    days: i32,
    user: Uuid,
) -> DbResult<RetentionRecord> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO audit_retentions (organization_id, days, updated_by) VALUES ($1, $2, $3) \
         ON CONFLICT ((COALESCE(organization_id, '00000000-0000-0000-0000-000000000000'::uuid))) \
         DO UPDATE SET days = EXCLUDED.days, updated_by = EXCLUDED.updated_by, updated_at = now() \
         RETURNING {COLUMNS}"
    )))
    .bind(scope.organization_id())
    .bind(days)
    .bind(user)
    .fetch_one(executor)
    .await
}

pub async fn clear(
    executor: impl PgExecutor<'_>,
    scope: Scope,
) -> DbResult<Option<RetentionRecord>> {
    let predicate = scoped("organization_id", scope, 1);
    sqlx::query_as::<_, RetentionRecord>(sqlx::AssertSqlSafe(format!(
        "DELETE FROM audit_retentions WHERE {predicate} RETURNING {COLUMNS}"
    )))
    .bind(scope.organization_id())
    .fetch_optional(executor)
    .await
}

pub async fn all(executor: impl PgExecutor<'_>) -> DbResult<Vec<RetentionRecord>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM audit_retentions ORDER BY organization_id NULLS FIRST"
    )))
    .fetch_all(executor)
    .await
}

pub async fn swept(executor: impl PgExecutor<'_>, scope: Scope, deleted: i64) -> DbResult<()> {
    let predicate = scoped("organization_id", scope, 2);
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE audit_retentions SET swept_at = now(), deleted_count = $1 WHERE {predicate}"
    )))
    .bind(deleted)
    .bind(scope.organization_id())
    .execute(executor)
    .await?;
    Ok(())
}
