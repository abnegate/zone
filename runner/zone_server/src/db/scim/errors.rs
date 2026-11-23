use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

use super::error_record::ErrorRecord;
use super::new_error::NewError;
use crate::db::DbResult;

pub const RECENT: i64 = 20;
pub const RETENTION_DAYS: i32 = 30;

const STATUS_COLUMN: &str = "status";
const COLUMNS: &str =
    "id, organization_id, token_id, method, path, status, scim_type, detail, occurred_at";

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    organization_id: Uuid,
    token_id: Option<Uuid>,
    method: String,
    path: String,
    status: i16,
    scim_type: Option<String>,
    detail: String,
    occurred_at: DateTime<Utc>,
}

impl TryFrom<Row> for ErrorRecord {
    type Error = sqlx::Error;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        let status = u16::try_from(row.status).map_err(|error| sqlx::Error::ColumnDecode {
            index: STATUS_COLUMN.to_owned(),
            source: Box::new(error),
        })?;
        Ok(Self {
            id: row.id,
            organization_id: row.organization_id,
            token_id: row.token_id,
            method: row.method,
            path: row.path,
            status,
            scim_type: row.scim_type,
            detail: row.detail,
            occurred_at: row.occurred_at,
        })
    }
}

pub async fn record(executor: impl PgExecutor<'_>, new: &NewError<'_>) -> DbResult<ErrorRecord> {
    let status = i16::try_from(new.status).map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        r#"
        WITH pruned AS (
            DELETE FROM scim_errors
            WHERE organization_id = $1 AND occurred_at < now() - make_interval(days => $8)
        )
        INSERT INTO scim_errors (organization_id, token_id, method, path, status, scim_type, detail)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING {COLUMNS}
        "#
    )))
    .bind(new.organization_id)
    .bind(new.token_id)
    .bind(new.method)
    .bind(new.path)
    .bind(status)
    .bind(new.scim_type)
    .bind(new.detail)
    .bind(RETENTION_DAYS)
    .fetch_one(executor)
    .await?;
    row.try_into()
}

pub async fn recent(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
) -> DbResult<Vec<ErrorRecord>> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM scim_errors WHERE organization_id = $1 \
         ORDER BY occurred_at DESC, id DESC LIMIT $2"
    )))
    .bind(organization_id)
    .bind(RECENT)
    .fetch_all(executor)
    .await?;
    rows.into_iter().map(ErrorRecord::try_from).collect()
}
