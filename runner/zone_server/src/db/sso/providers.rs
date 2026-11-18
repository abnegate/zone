use abnegate_secret::{MasterKey, SecretValue, encrypt_value};
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgExecutor, Postgres, QueryBuilder};
use uuid::Uuid;
use zone_enterprise::sso::Protocol;

use super::listing::Listing;
use super::new_provider::NewProvider;
use super::provider::ProviderRecord;
use super::provider_changes::ProviderChanges;
use super::provider_error::{ProviderError, ProviderResult};
use crate::db::DbResult;

pub const DUPLICATE_KEY: &str = "sso_providers_issuer_client_id_key";

const COLUMNS: &str = "id, protocol, display_name, issuer, client_id, client_secret, allowed_domains, require_verified_email, enabled, created_at, updated_at";

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    protocol: String,
    display_name: String,
    issuer: String,
    client_id: String,
    client_secret: String,
    allowed_domains: Vec<String>,
    require_verified_email: bool,
    enabled: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Row {
    fn record(self) -> ProviderResult<ProviderRecord> {
        let protocol = Protocol::parse(&self.protocol)
            .ok_or_else(|| ProviderError::Protocol(self.id, self.protocol.clone()))?;
        Ok(ProviderRecord {
            id: self.id,
            protocol,
            display_name: self.display_name,
            issuer: self.issuer,
            client_id: self.client_id,
            sealed_secret: self.client_secret,
            allowed_domains: self.allowed_domains,
            require_verified_email: self.require_verified_email,
            enabled: self.enabled,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn seal(secret: &SecretValue, key: &MasterKey) -> ProviderResult<String> {
    encrypt_value(secret, key).map_err(ProviderError::Unsealable)
}

fn classify(error: sqlx::Error) -> ProviderError {
    match error.as_database_error() {
        Some(database)
            if database.is_unique_violation() && database.constraint() == Some(DUPLICATE_KEY) =>
        {
            ProviderError::Duplicate
        }
        _ => ProviderError::Database(error),
    }
}

pub async fn list(executor: impl PgExecutor<'_>) -> ProviderResult<Vec<ProviderRecord>> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM sso_providers ORDER BY created_at, id"
    )))
    .fetch_all(executor)
    .await?;
    rows.into_iter().map(Row::record).collect()
}

pub async fn list_enabled(executor: impl PgExecutor<'_>) -> DbResult<Vec<Listing>> {
    sqlx::query_as(
        "SELECT id, display_name FROM sso_providers WHERE enabled ORDER BY created_at, id",
    )
    .fetch_all(executor)
    .await
}

pub async fn get(
    executor: impl PgExecutor<'_>,
    id: Uuid,
) -> ProviderResult<Option<ProviderRecord>> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM sso_providers WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(executor)
    .await?;
    row.map(Row::record).transpose()
}

pub async fn lock(
    connection: &mut PgConnection,
    id: Uuid,
) -> ProviderResult<Option<ProviderRecord>> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM sso_providers WHERE id = $1 FOR UPDATE"
    )))
    .bind(id)
    .fetch_optional(connection)
    .await?;
    row.map(Row::record).transpose()
}

pub async fn create(
    executor: impl PgExecutor<'_>,
    key: &MasterKey,
    new: &NewProvider,
) -> ProviderResult<ProviderRecord> {
    let sealed = seal(&new.client_secret, key)?;
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO sso_providers \
           (protocol, display_name, issuer, client_id, client_secret, allowed_domains, require_verified_email, enabled) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {COLUMNS}"
    )))
    .bind(new.protocol.as_str())
    .bind(&new.display_name)
    .bind(&new.issuer)
    .bind(&new.client_id)
    .bind(sealed)
    .bind(&new.allowed_domains)
    .bind(new.require_verified_email)
    .bind(new.enabled)
    .fetch_one(executor)
    .await
    .map_err(classify)?;
    row.record()
}

pub async fn update(
    executor: impl PgExecutor<'_>,
    key: &MasterKey,
    id: Uuid,
    changes: &ProviderChanges,
) -> ProviderResult<Option<ProviderRecord>> {
    if changes.is_empty() {
        return get(executor, id).await;
    }
    let sealed = changes
        .client_secret
        .as_ref()
        .map(|secret| seal(secret, key))
        .transpose()?;

    let mut query = QueryBuilder::<Postgres>::new("UPDATE sso_providers SET ");
    let mut assignments = query.separated(", ");
    if let Some(display_name) = &changes.display_name {
        assignments
            .push("display_name = ")
            .push_bind_unseparated(display_name);
    }
    if let Some(issuer) = &changes.issuer {
        assignments.push("issuer = ").push_bind_unseparated(issuer);
    }
    if let Some(client_id) = &changes.client_id {
        assignments
            .push("client_id = ")
            .push_bind_unseparated(client_id);
    }
    if let Some(sealed) = sealed {
        assignments
            .push("client_secret = ")
            .push_bind_unseparated(sealed);
    }
    if let Some(allowed_domains) = &changes.allowed_domains {
        assignments
            .push("allowed_domains = ")
            .push_bind_unseparated(allowed_domains);
    }
    if let Some(require_verified_email) = changes.require_verified_email {
        assignments
            .push("require_verified_email = ")
            .push_bind_unseparated(require_verified_email);
    }
    if let Some(enabled) = changes.enabled {
        assignments
            .push("enabled = ")
            .push_bind_unseparated(enabled);
    }
    assignments.push("updated_at = now()");
    query
        .push(" WHERE id = ")
        .push_bind(id)
        .push(" RETURNING ")
        .push(COLUMNS);

    let row: Option<Row> = query
        .build_query_as()
        .fetch_optional(executor)
        .await
        .map_err(classify)?;
    row.map(Row::record).transpose()
}

pub async fn delete(executor: impl PgExecutor<'_>, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query("DELETE FROM sso_providers WHERE id = $1")
        .bind(id)
        .execute(executor)
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn count_enabled(executor: impl PgExecutor<'_>) -> DbResult<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM sso_providers WHERE enabled")
        .fetch_one(executor)
        .await
}
