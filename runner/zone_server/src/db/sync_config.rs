//! Sync configuration database queries

use chrono::NaiveDateTime;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

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
    pub sync_direction: String,
    pub last_external_state: Option<JsonValue>,
    pub created_at: Option<NaiveDateTime>,
}

/// Sync event row from database
#[derive(Debug, Clone)]
pub struct SyncEventRow {
    pub id: Uuid,
    pub sync_config_id: Uuid,
    pub synced_item_id: Option<Uuid>,
    pub event_type: String,
    pub direction: String,
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
    WebhookReceived,
    SyncError,
}

impl SyncEventType {
    pub const ALL: [Self; 5] = [
        Self::Create,
        Self::Update,
        Self::Close,
        Self::WebhookReceived,
        Self::SyncError,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Close => "close",
            Self::WebhookReceived => "webhook_received",
            Self::SyncError => "sync_error",
        }
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

    Ok(row.map(|r| SyncedItemRow {
        id: r.id,
        sync_config_id: r.sync_config_id,
        task_id: r.task_id,
        external_id: r.external_id,
        external_url: r.external_url,
        last_synced_at: r.last_synced_at,
        sync_direction: r.sync_direction,
        last_external_state: r.last_external_state,
        created_at: r.created_at,
    }))
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

    Ok(row.map(|r| SyncedItemRow {
        id: r.id,
        sync_config_id: r.sync_config_id,
        task_id: r.task_id,
        external_id: r.external_id,
        external_url: r.external_url,
        last_synced_at: r.last_synced_at,
        sync_direction: r.sync_direction,
        last_external_state: r.last_external_state,
        created_at: r.created_at,
    }))
}

/// Create a synced item
pub async fn create_synced_item(
    pool: &PgPool,
    sync_config_id: Uuid,
    task_id: Uuid,
    external_id: &str,
    external_url: Option<&str>,
    sync_direction: &str,
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
        sync_direction,
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
        sync_direction: row.sync_direction,
        last_external_state: row.last_external_state,
        created_at: row.created_at,
    })
}

/// Update synced item
pub async fn update_synced_item(
    pool: &PgPool,
    id: Uuid,
    last_external_state: Option<JsonValue>,
) -> DbResult<Option<SyncedItemRow>> {
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
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| SyncedItemRow {
        id: r.id,
        sync_config_id: r.sync_config_id,
        task_id: r.task_id,
        external_id: r.external_id,
        external_url: r.external_url,
        last_synced_at: r.last_synced_at,
        sync_direction: r.sync_direction,
        last_external_state: r.last_external_state,
        created_at: r.created_at,
    }))
}

/// Delete synced item
pub async fn delete_synced_item(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!("DELETE FROM synced_items WHERE id = $1", id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Create a sync event
pub async fn create_sync_event(
    pool: &PgPool,
    sync_config_id: Uuid,
    synced_item_id: Option<Uuid>,
    event_type: SyncEventType,
    direction: SyncEventDirection,
    payload: Option<JsonValue>,
    error_message: Option<&str>,
) -> DbResult<SyncEventRow> {
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
    .fetch_one(pool)
    .await?;

    Ok(SyncEventRow {
        id: row.id,
        sync_config_id: row.sync_config_id,
        synced_item_id: row.synced_item_id,
        event_type: row.event_type,
        direction: row.direction,
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

    Ok(rows
        .into_iter()
        .map(|r| SyncEventRow {
            id: r.id,
            sync_config_id: r.sync_config_id,
            synced_item_id: r.synced_item_id,
            event_type: r.event_type,
            direction: r.direction,
            payload: r.payload,
            error_message: r.error_message,
            created_at: r.created_at,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_type_position(event_type: SyncEventType) -> usize {
        match event_type {
            SyncEventType::Create => 0,
            SyncEventType::Update => 1,
            SyncEventType::Close => 2,
            SyncEventType::WebhookReceived => 3,
            SyncEventType::SyncError => 4,
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
            (0..5).collect::<Vec<_>>(),
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
}
