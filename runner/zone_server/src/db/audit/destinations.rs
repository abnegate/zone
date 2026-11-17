mod changes;
mod error;
mod new_destination;
mod record;

pub use changes::DestinationChanges;
pub use error::{DestinationError, DestinationResult};
pub use new_destination::NewDestination;
pub use record::DestinationRecord;

use abnegate_secret::{MasterKey, SecretValue, encrypt_value};
use sqlx::{PgConnection, PgExecutor, Postgres, QueryBuilder};
use uuid::Uuid;

use super::position::Position;
use super::scope::Scope;
use crate::db::DbResult;

pub const DUPLICATE_KEY: &str = "audit_destinations_name_idx";

const COLUMNS: &str = "id, organization_id, kind, name, enabled, settings, secrets AS sealed_secrets, created_by, created_at, updated_at";

const HORIZON: &str = "SELECT ((pg_snapshot_xmin(pg_current_snapshot()))::text)::bigint - 1";

fn seal(secrets: &SecretValue, key: &MasterKey) -> DestinationResult<String> {
    encrypt_value(secrets, key).map_err(DestinationError::Unsealable)
}

fn classify(error: sqlx::Error) -> DestinationError {
    match error.as_database_error() {
        Some(database)
            if database.is_unique_violation() && database.constraint() == Some(DUPLICATE_KEY) =>
        {
            DestinationError::Duplicate
        }
        _ => DestinationError::Database(error),
    }
}

fn select() -> QueryBuilder<Postgres> {
    QueryBuilder::new(format!("SELECT {COLUMNS} FROM audit_destinations WHERE "))
}

fn push_scope(query: &mut QueryBuilder<Postgres>, scope: Scope) {
    match scope.organization_id() {
        Some(organization_id) => query.push("organization_id = ").push_bind(organization_id),
        None => query.push("organization_id IS NULL"),
    };
}

fn push_identity(query: &mut QueryBuilder<Postgres>, scope: Scope, id: Uuid) {
    query.push("id = ").push_bind(id).push(" AND ");
    push_scope(query, scope);
}

pub async fn list(executor: impl PgExecutor<'_>, scope: Scope) -> DbResult<Vec<DestinationRecord>> {
    let mut query = select();
    push_scope(&mut query, scope);
    query.push(" ORDER BY created_at, id");
    query.build_query_as().fetch_all(executor).await
}

pub async fn get(
    executor: impl PgExecutor<'_>,
    scope: Scope,
    id: Uuid,
) -> DbResult<Option<DestinationRecord>> {
    let mut query = select();
    push_identity(&mut query, scope, id);
    query.build_query_as().fetch_optional(executor).await
}

pub async fn find(executor: impl PgExecutor<'_>, id: Uuid) -> DbResult<Option<DestinationRecord>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM audit_destinations WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(executor)
    .await
}

pub async fn lock(
    connection: &mut PgConnection,
    scope: Scope,
    id: Uuid,
) -> DbResult<Option<DestinationRecord>> {
    let mut query = select();
    push_identity(&mut query, scope, id);
    query.push(" FOR UPDATE");
    query.build_query_as().fetch_optional(connection).await
}

/// Inserts the destination with its delivery cursor at the snapshot horizon, so it streams
/// what is written from now on. The horizon is read before the insert: a transaction's own
/// xid holds the horizon back once it has written.
pub async fn create(
    connection: &mut PgConnection,
    key: &MasterKey,
    new: &NewDestination,
) -> DestinationResult<DestinationRecord> {
    let sealed = seal(&new.secrets, key)?;
    let start = Position {
        transaction_id: sqlx::query_scalar(HORIZON)
            .fetch_one(&mut *connection)
            .await?,
        sequence: i64::MAX,
    };
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "WITH destination AS ( \
             INSERT INTO audit_destinations (organization_id, kind, name, enabled, settings, secrets, created_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {COLUMNS} \
         ), delivery AS ( \
             INSERT INTO audit_deliveries (destination_id, transaction_id, sequence) \
             SELECT id, $8, $9 FROM destination \
         ) \
         SELECT * FROM destination"
    )))
    .bind(new.scope.organization_id())
    .bind(&new.kind)
    .bind(&new.name)
    .bind(new.enabled)
    .bind(&new.settings)
    .bind(sealed)
    .bind(new.created_by)
    .bind(start.transaction_id)
    .bind(start.sequence)
    .fetch_one(connection)
    .await
    .map_err(classify)
}

pub async fn update(
    executor: impl PgExecutor<'_>,
    key: &MasterKey,
    scope: Scope,
    id: Uuid,
    changes: &DestinationChanges,
) -> DestinationResult<Option<DestinationRecord>> {
    if changes.is_empty() {
        return Ok(get(executor, scope, id).await?);
    }
    let sealed = changes
        .secrets
        .as_ref()
        .map(|secrets| seal(secrets, key))
        .transpose()?;

    let mut query = QueryBuilder::<Postgres>::new("UPDATE audit_destinations SET ");
    let mut assignments = query.separated(", ");
    if let Some(name) = &changes.name {
        assignments.push("name = ").push_bind_unseparated(name);
    }
    if let Some(enabled) = changes.enabled {
        assignments
            .push("enabled = ")
            .push_bind_unseparated(enabled);
    }
    if let Some(settings) = &changes.settings {
        assignments
            .push("settings = ")
            .push_bind_unseparated(settings);
    }
    if let Some(sealed) = sealed {
        assignments.push("secrets = ").push_bind_unseparated(sealed);
    }
    assignments.push("updated_at = now()");
    query.push(" WHERE ");
    push_identity(&mut query, scope, id);
    query.push(" RETURNING ").push(COLUMNS);

    query
        .build_query_as()
        .fetch_optional(executor)
        .await
        .map_err(classify)
}

pub async fn delete(executor: impl PgExecutor<'_>, scope: Scope, id: Uuid) -> DbResult<bool> {
    let mut query = QueryBuilder::<Postgres>::new("DELETE FROM audit_destinations WHERE ");
    push_identity(&mut query, scope, id);
    let result = query.build().execute(executor).await?;
    Ok(result.rows_affected() == 1)
}
