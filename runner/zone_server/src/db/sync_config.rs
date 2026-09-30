//! Sync configuration database queries

use std::str::FromStr;

use chrono::NaiveDateTime;
use serde_json::Value as JsonValue;
use sqlx::{Acquire, PgConnection, PgPool};
use uuid::Uuid;

use super::{DbResult, tasks};

/// Sync configuration row from database
#[derive(Debug, Clone)]
pub struct SyncConfigRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub provider: String,
    pub enabled: bool,
    pub config: JsonValue,
    pub webhook_secret_encrypted: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Synced item row from database
#[derive(Debug, Clone)]
pub struct SyncedItemRow {
    pub id: Uuid,
    pub sync_config_id: Uuid,
    pub task_id: Uuid,
    pub external_id: String,
    pub external_url: Option<String>,
    pub last_synced_at: Option<NaiveDateTime>,
    pub sync_direction: SyncDirection,
    pub last_external_state: Option<JsonValue>,
    pub created_at: Option<NaiveDateTime>,
}

/// Sync event row from database
#[derive(Debug, Clone)]
pub struct SyncEventRow {
    pub id: Uuid,
    pub sync_config_id: Uuid,
    pub synced_item_id: Option<Uuid>,
    pub event_type: SyncEventType,
    pub direction: SyncEventDirection,
    pub payload: Option<JsonValue>,
    pub error_message: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

/// What a `sync_events` row records; each value is one the table's
/// `event_type` CHECK constraint accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncEventType {
    Create,
    Update,
    Close,
    /// An external issue deleted, so its task no longer follows it
    Unlink,
    WebhookReceived,
    SyncError,
}

impl SyncEventType {
    pub const ALL: [Self; 6] = [
        Self::Create,
        Self::Update,
        Self::Close,
        Self::Unlink,
        Self::WebhookReceived,
        Self::SyncError,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Close => "close",
            Self::Unlink => "unlink",
            Self::WebhookReceived => "webhook_received",
            Self::SyncError => "sync_error",
        }
    }
}

impl FromStr for SyncEventType {
    type Err = UnknownSyncValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|variant| variant.as_str() == value)
            .ok_or_else(|| UnknownSyncValue::new("sync event type", value))
    }
}

/// Which way a logged sync event travelled; each value is one the
/// `sync_events.direction` CHECK constraint accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncEventDirection {
    Inbound,
    Outbound,
}

impl SyncEventDirection {
    pub const ALL: [Self; 2] = [Self::Inbound, Self::Outbound];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }
}

impl FromStr for SyncEventDirection {
    type Err = UnknownSyncValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|variant| variant.as_str() == value)
            .ok_or_else(|| UnknownSyncValue::new("sync event direction", value))
    }
}

/// Which way a synced item may move; each value is one the
/// `synced_items.sync_direction` CHECK constraint accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    Inbound,
    Outbound,
    Bidirectional,
}

impl SyncDirection {
    pub const ALL: [Self; 3] = [Self::Inbound, Self::Outbound, Self::Bidirectional];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
            Self::Bidirectional => "bidirectional",
        }
    }
}

impl FromStr for SyncDirection {
    type Err = UnknownSyncValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|variant| variant.as_str() == value)
            .ok_or_else(|| UnknownSyncValue::new("sync direction", value))
    }
}

/// A stored sync value that none of its enum's variants spell.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{value:?} is not a known {kind}")]
pub struct UnknownSyncValue {
    pub kind: &'static str,
    pub value: String,
}

impl UnknownSyncValue {
    fn new(kind: &'static str, value: &str) -> Self {
        Self {
            kind,
            value: value.to_string(),
        }
    }
}

fn decode<T>(column: &str, value: &str) -> DbResult<T>
where
    T: FromStr<Err = UnknownSyncValue>,
{
    value.parse().map_err(|error| sqlx::Error::ColumnDecode {
        index: column.to_string(),
        source: Box::new(error),
    })
}

/// Get sync config by ID
pub async fn get_sync_config(pool: &PgPool, id: Uuid) -> DbResult<Option<SyncConfigRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, project_id, provider, enabled, config, webhook_secret_encrypted,
               created_at, updated_at
        FROM sync_configs
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| SyncConfigRow {
        id: r.id,
        project_id: r.project_id,
        provider: r.provider,
        enabled: r.enabled,
        config: r.config,
        webhook_secret_encrypted: r.webhook_secret_encrypted,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Get sync config by project and provider
pub async fn get_sync_config_by_project_provider(
    pool: &PgPool,
    project_id: Uuid,
    provider: &str,
) -> DbResult<Option<SyncConfigRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, project_id, provider, enabled, config, webhook_secret_encrypted,
               created_at, updated_at
        FROM sync_configs
        WHERE project_id = $1 AND provider = $2
        "#,
        project_id,
        provider
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| SyncConfigRow {
        id: r.id,
        project_id: r.project_id,
        provider: r.provider,
        enabled: r.enabled,
        config: r.config,
        webhook_secret_encrypted: r.webhook_secret_encrypted,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// List sync configs for a project
pub async fn list_sync_configs(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<SyncConfigRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, project_id, provider, enabled, config, webhook_secret_encrypted,
               created_at, updated_at
        FROM sync_configs
        WHERE project_id = $1
        ORDER BY created_at DESC
        "#,
        project_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| SyncConfigRow {
            id: r.id,
            project_id: r.project_id,
            provider: r.provider,
            enabled: r.enabled,
            config: r.config,
            webhook_secret_encrypted: r.webhook_secret_encrypted,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Create a new sync config
pub async fn create_sync_config(
    pool: &PgPool,
    project_id: Uuid,
    provider: &str,
    enabled: bool,
    config: JsonValue,
    webhook_secret_encrypted: Option<&str>,
) -> DbResult<SyncConfigRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO sync_configs (project_id, provider, enabled, config, webhook_secret_encrypted)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, project_id, provider, enabled, config, webhook_secret_encrypted,
                  created_at, updated_at
        "#,
        project_id,
        provider,
        enabled,
        config,
        webhook_secret_encrypted
    )
    .fetch_one(pool)
    .await?;

    Ok(SyncConfigRow {
        id: row.id,
        project_id: row.project_id,
        provider: row.provider,
        enabled: row.enabled,
        config: row.config,
        webhook_secret_encrypted: row.webhook_secret_encrypted,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Update sync config
pub async fn update_sync_config(
    pool: &PgPool,
    id: Uuid,
    enabled: Option<bool>,
    config: Option<JsonValue>,
    webhook_secret_encrypted: Option<&str>,
) -> DbResult<Option<SyncConfigRow>> {
    let row = sqlx::query!(
        r#"
        UPDATE sync_configs
        SET enabled = COALESCE($2, enabled),
            config = COALESCE($3, config),
            webhook_secret_encrypted = COALESCE($4, webhook_secret_encrypted)
        WHERE id = $1
        RETURNING id, project_id, provider, enabled, config, webhook_secret_encrypted,
                  created_at, updated_at
        "#,
        id,
        enabled,
        config,
        webhook_secret_encrypted
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| SyncConfigRow {
        id: r.id,
        project_id: r.project_id,
        provider: r.provider,
        enabled: r.enabled,
        config: r.config,
        webhook_secret_encrypted: r.webhook_secret_encrypted,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Delete sync config
pub async fn delete_sync_config(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!("DELETE FROM sync_configs WHERE id = $1", id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Get synced item by task
pub async fn get_synced_item_by_task(
    pool: &PgPool,
    sync_config_id: Uuid,
    task_id: Uuid,
) -> DbResult<Option<SyncedItemRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, sync_config_id, task_id, external_id, external_url,
               last_synced_at, sync_direction, last_external_state, created_at
        FROM synced_items
        WHERE sync_config_id = $1 AND task_id = $2
        "#,
        sync_config_id,
        task_id
    )
    .fetch_optional(pool)
    .await?;

    row.map(|r| {
        Ok(SyncedItemRow {
            id: r.id,
            sync_config_id: r.sync_config_id,
            task_id: r.task_id,
            external_id: r.external_id,
            external_url: r.external_url,
            last_synced_at: r.last_synced_at,
            sync_direction: decode("sync_direction", &r.sync_direction)?,
            last_external_state: r.last_external_state,
            created_at: r.created_at,
        })
    })
    .transpose()
}

/// Get synced item by external ID
pub async fn get_synced_item_by_external_id(
    pool: &PgPool,
    sync_config_id: Uuid,
    external_id: &str,
) -> DbResult<Option<SyncedItemRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, sync_config_id, task_id, external_id, external_url,
               last_synced_at, sync_direction, last_external_state, created_at
        FROM synced_items
        WHERE sync_config_id = $1 AND external_id = $2
        "#,
        sync_config_id,
        external_id
    )
    .fetch_optional(pool)
    .await?;

    row.map(|r| {
        Ok(SyncedItemRow {
            id: r.id,
            sync_config_id: r.sync_config_id,
            task_id: r.task_id,
            external_id: r.external_id,
            external_url: r.external_url,
            last_synced_at: r.last_synced_at,
            sync_direction: decode("sync_direction", &r.sync_direction)?,
            last_external_state: r.last_external_state,
            created_at: r.created_at,
        })
    })
    .transpose()
}

/// Create a synced item
pub async fn create_synced_item(
    pool: &PgPool,
    sync_config_id: Uuid,
    task_id: Uuid,
    external_id: &str,
    external_url: Option<&str>,
    sync_direction: SyncDirection,
    last_external_state: Option<JsonValue>,
) -> DbResult<SyncedItemRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO synced_items (sync_config_id, task_id, external_id, external_url,
                                  sync_direction, last_external_state)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, sync_config_id, task_id, external_id, external_url,
                  last_synced_at, sync_direction, last_external_state, created_at
        "#,
        sync_config_id,
        task_id,
        external_id,
        external_url,
        sync_direction.as_str(),
        last_external_state
    )
    .fetch_one(pool)
    .await?;

    Ok(SyncedItemRow {
        id: row.id,
        sync_config_id: row.sync_config_id,
        task_id: row.task_id,
        external_id: row.external_id,
        external_url: row.external_url,
        last_synced_at: row.last_synced_at,
        sync_direction: decode("sync_direction", &row.sync_direction)?,
        last_external_state: row.last_external_state,
        created_at: row.created_at,
    })
}

/// An external issue to turn into a task of `project_id` and link to it.
pub struct NewSyncedTask<'a> {
    pub sync_config_id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub title: &'a str,
    pub description: &'a str,
    pub external_id: &'a str,
    pub external_url: Option<&'a str>,
    pub sync_direction: SyncDirection,
    pub last_external_state: Option<JsonValue>,
}

/// Create a non-agentic task for an external issue and link the two inside
/// the caller's transaction, or nothing when another delivery linked that
/// issue first.
pub async fn create_synced_task(
    connection: &mut PgConnection,
    input: NewSyncedTask<'_>,
) -> DbResult<Option<SyncedItemRow>> {
    let mut transaction = connection.begin().await?;
    let task = tasks::create_in(
        &mut transaction,
        &tasks::Create {
            workspace_id: input.workspace_id,
            project_ids: &[input.project_id],
            title: input.title,
            description: input.description,
            acceptance_criteria: None,
            priority: None,
            is_agentic: false,
            require_plan_approval: false,
            source_id: None,
            created_by: None,
        },
    )
    .await?;
    let row = sqlx::query!(
        r#"
        INSERT INTO synced_items (sync_config_id, task_id, external_id, external_url,
                                  sync_direction, last_external_state)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (sync_config_id, external_id) DO NOTHING
        RETURNING id, sync_config_id, task_id, external_id, external_url,
                  last_synced_at, sync_direction, last_external_state, created_at
        "#,
        input.sync_config_id,
        task.id,
        input.external_id,
        input.external_url,
        input.sync_direction.as_str(),
        input.last_external_state
    )
    .fetch_optional(&mut *transaction)
    .await?;

    let Some(row) = row else {
        transaction.rollback().await?;
        return Ok(None);
    };
    let item = SyncedItemRow {
        id: row.id,
        sync_config_id: row.sync_config_id,
        task_id: row.task_id,
        external_id: row.external_id,
        external_url: row.external_url,
        last_synced_at: row.last_synced_at,
        sync_direction: decode("sync_direction", &row.sync_direction)?,
        last_external_state: row.last_external_state,
        created_at: row.created_at,
    };
    transaction.commit().await?;
    Ok(Some(item))
}

/// The synced item linking `external_id`, locked until the caller's
/// transaction ends so deliveries for one issue apply one at a time.
pub async fn lock_synced_item_by_external_id(
    connection: &mut PgConnection,
    sync_config_id: Uuid,
    external_id: &str,
) -> DbResult<Option<SyncedItemRow>> {
    let record: Option<SyncedItemRecord> = sqlx::query_as(
        r#"
        SELECT id, sync_config_id, task_id, external_id, external_url,
               last_synced_at, sync_direction, last_external_state, created_at
        FROM synced_items
        WHERE sync_config_id = $1 AND external_id = $2
        FOR UPDATE
        "#,
    )
    .bind(sync_config_id)
    .bind(external_id)
    .fetch_optional(&mut *connection)
    .await?;

    record.map(SyncedItemRecord::into_row).transpose()
}

#[derive(sqlx::FromRow)]
struct SyncedItemRecord {
    id: Uuid,
    sync_config_id: Uuid,
    task_id: Uuid,
    external_id: String,
    external_url: Option<String>,
    last_synced_at: Option<NaiveDateTime>,
    sync_direction: String,
    last_external_state: Option<JsonValue>,
    created_at: Option<NaiveDateTime>,
}

impl SyncedItemRecord {
    fn into_row(self) -> DbResult<SyncedItemRow> {
        Ok(SyncedItemRow {
            id: self.id,
            sync_config_id: self.sync_config_id,
            task_id: self.task_id,
            external_id: self.external_id,
            external_url: self.external_url,
            last_synced_at: self.last_synced_at,
            sync_direction: decode("sync_direction", &self.sync_direction)?,
            last_external_state: self.last_external_state,
            created_at: self.created_at,
        })
    }
}

/// Record that a sync received the delivery `delivery_id`; `false` when it
/// already had, so the delivery is a retry or a replay to skip.
pub async fn record_delivery(
    connection: &mut PgConnection,
    sync_config_id: Uuid,
    delivery_id: &str,
) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        INSERT INTO sync_deliveries (sync_config_id, delivery_id)
        VALUES ($1, $2)
        ON CONFLICT (sync_config_id, delivery_id) DO NOTHING
        "#,
    )
    .bind(sync_config_id)
    .bind(delivery_id)
    .execute(&mut *connection)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Update synced item
pub async fn update_synced_item<'connection, E>(
    executor: E,
    id: Uuid,
    last_external_state: Option<JsonValue>,
) -> DbResult<Option<SyncedItemRow>>
where
    E: sqlx::Executor<'connection, Database = sqlx::Postgres>,
{
    let row = sqlx::query!(
        r#"
        UPDATE synced_items
        SET last_synced_at = NOW(),
            last_external_state = COALESCE($2, last_external_state)
        WHERE id = $1
        RETURNING id, sync_config_id, task_id, external_id, external_url,
                  last_synced_at, sync_direction, last_external_state, created_at
        "#,
        id,
        last_external_state
    )
    .fetch_optional(executor)
    .await?;

    row.map(|r| {
        Ok(SyncedItemRow {
            id: r.id,
            sync_config_id: r.sync_config_id,
            task_id: r.task_id,
            external_id: r.external_id,
            external_url: r.external_url,
            last_synced_at: r.last_synced_at,
            sync_direction: decode("sync_direction", &r.sync_direction)?,
            last_external_state: r.last_external_state,
            created_at: r.created_at,
        })
    })
    .transpose()
}

/// Delete synced item
pub async fn delete_synced_item<'connection, E>(executor: E, id: Uuid) -> DbResult<bool>
where
    E: sqlx::Executor<'connection, Database = sqlx::Postgres>,
{
    let result = sqlx::query!("DELETE FROM synced_items WHERE id = $1", id)
        .execute(executor)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Create a sync event
pub async fn create_sync_event<'connection, E>(
    executor: E,
    sync_config_id: Uuid,
    synced_item_id: Option<Uuid>,
    event_type: SyncEventType,
    direction: SyncEventDirection,
    payload: Option<JsonValue>,
    error_message: Option<&str>,
) -> DbResult<SyncEventRow>
where
    E: sqlx::Executor<'connection, Database = sqlx::Postgres>,
{
    let row = sqlx::query!(
        r#"
        INSERT INTO sync_events (sync_config_id, synced_item_id, event_type, direction, payload, error_message)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, sync_config_id, synced_item_id, event_type, direction, payload, error_message, created_at
        "#,
        sync_config_id,
        synced_item_id,
        event_type.as_str(),
        direction.as_str(),
        payload,
        error_message
    )
    .fetch_one(executor)
    .await?;

    Ok(SyncEventRow {
        id: row.id,
        sync_config_id: row.sync_config_id,
        synced_item_id: row.synced_item_id,
        event_type: decode("event_type", &row.event_type)?,
        direction: decode("direction", &row.direction)?,
        payload: row.payload,
        error_message: row.error_message,
        created_at: row.created_at,
    })
}

/// List sync events for a config
pub async fn list_sync_events(
    pool: &PgPool,
    sync_config_id: Uuid,
    limit: i64,
) -> DbResult<Vec<SyncEventRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, sync_config_id, synced_item_id, event_type, direction, payload, error_message, created_at
        FROM sync_events
        WHERE sync_config_id = $1
        ORDER BY created_at DESC
        LIMIT $2
        "#,
        sync_config_id,
        limit
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            Ok(SyncEventRow {
                id: r.id,
                sync_config_id: r.sync_config_id,
                synced_item_id: r.synced_item_id,
                event_type: decode("event_type", &r.event_type)?,
                direction: decode("direction", &r.direction)?,
                payload: r.payload,
                error_message: r.error_message,
                created_at: r.created_at,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_type_position(event_type: SyncEventType) -> usize {
        match event_type {
            SyncEventType::Create => 0,
            SyncEventType::Update => 1,
            SyncEventType::Close => 2,
            SyncEventType::Unlink => 3,
            SyncEventType::WebhookReceived => 4,
            SyncEventType::SyncError => 5,
        }
    }

    fn direction_position(direction: SyncEventDirection) -> usize {
        match direction {
            SyncEventDirection::Inbound => 0,
            SyncEventDirection::Outbound => 1,
        }
    }

    #[test]
    fn all_lists_every_sync_event_type_once_in_order() {
        let positions: Vec<usize> = SyncEventType::ALL
            .into_iter()
            .map(event_type_position)
            .collect();
        assert_eq!(
            positions,
            (0..6).collect::<Vec<_>>(),
            "a new SyncEventType must join ALL, which the sync_events drift test inserts"
        );
    }

    #[test]
    fn all_lists_every_sync_event_direction_once_in_order() {
        let positions: Vec<usize> = SyncEventDirection::ALL
            .into_iter()
            .map(direction_position)
            .collect();
        assert_eq!(
            positions,
            (0..2).collect::<Vec<_>>(),
            "a new SyncEventDirection must join ALL, which the sync_events drift test inserts"
        );
    }

    fn sync_direction_position(direction: SyncDirection) -> usize {
        match direction {
            SyncDirection::Inbound => 0,
            SyncDirection::Outbound => 1,
            SyncDirection::Bidirectional => 2,
        }
    }

    #[test]
    fn all_lists_every_sync_direction_once_in_order() {
        let positions: Vec<usize> = SyncDirection::ALL
            .into_iter()
            .map(sync_direction_position)
            .collect();
        assert_eq!(
            positions,
            (0..3).collect::<Vec<_>>(),
            "a new SyncDirection must join ALL, which the synced_items drift test inserts"
        );
    }

    #[test]
    fn every_sync_value_parses_back_from_its_stored_spelling() {
        for event_type in SyncEventType::ALL {
            assert_eq!(event_type.as_str().parse(), Ok(event_type));
        }
        for direction in SyncEventDirection::ALL {
            assert_eq!(direction.as_str().parse(), Ok(direction));
        }
        for direction in SyncDirection::ALL {
            assert_eq!(direction.as_str().parse(), Ok(direction));
        }
    }

    #[test]
    fn an_unknown_sync_value_names_its_kind_and_spelling() {
        assert_eq!(
            "issue_closed".parse::<SyncEventType>(),
            Err(UnknownSyncValue::new("sync event type", "issue_closed"))
        );
        assert_eq!(
            "bidirectional".parse::<SyncEventDirection>(),
            Err(UnknownSyncValue::new(
                "sync event direction",
                "bidirectional"
            ))
        );
        assert_eq!(
            "sideways".parse::<SyncDirection>(),
            Err(UnknownSyncValue::new("sync direction", "sideways"))
        );
    }

    #[test]
    fn decoding_an_unknown_sync_value_reports_its_column() {
        let error = decode::<SyncDirection>("sync_direction", "sideways").unwrap_err();
        assert!(
            matches!(&error, sqlx::Error::ColumnDecode { index, .. } if index == "sync_direction"),
            "{error}"
        );
    }
}
