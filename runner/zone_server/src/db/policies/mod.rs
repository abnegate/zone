pub mod layers;
pub mod stored;

pub use layers::Layers;
pub use stored::Stored;

use serde_json::Value;
use sqlx::{PgExecutor, Postgres, Transaction};
use uuid::Uuid;

use crate::db::DbResult;

pub async fn organization(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
) -> DbResult<Option<Stored>> {
    sqlx::query_as(
        "SELECT rules, updated_by, updated_at FROM organization_policies \
         WHERE organization_id = $1",
    )
    .bind(organization_id)
    .fetch_optional(executor)
    .await
}

pub async fn workspace(
    executor: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> DbResult<Option<Stored>> {
    sqlx::query_as(
        "SELECT rules, updated_by, updated_at FROM workspace_policies WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(executor)
    .await
}

pub async fn layers(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    workspace_id: Option<Uuid>,
) -> DbResult<Layers> {
    sqlx::query_as(
        "SELECT \
         (SELECT rules FROM organization_policies WHERE organization_id = $1) AS organization, \
         (SELECT rules FROM workspace_policies WHERE workspace_id = $2) AS workspace",
    )
    .bind(organization_id)
    .bind(workspace_id)
    .fetch_one(executor)
    .await
}

pub async fn lock_organization(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
) -> DbResult<Option<Stored>> {
    sqlx::query_as(
        "SELECT rules, updated_by, updated_at FROM organization_policies \
         WHERE organization_id = $1 FOR UPDATE",
    )
    .bind(organization_id)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn lock_workspace(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
) -> DbResult<Option<Stored>> {
    sqlx::query_as(
        "SELECT rules, updated_by, updated_at FROM workspace_policies \
         WHERE workspace_id = $1 FOR UPDATE",
    )
    .bind(workspace_id)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn save_organization(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    rules: &Value,
    actor: Uuid,
) -> DbResult<Stored> {
    sqlx::query_as(
        "INSERT INTO organization_policies (organization_id, rules, updated_by) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (organization_id) DO UPDATE \
         SET rules = EXCLUDED.rules, updated_by = EXCLUDED.updated_by, updated_at = now() \
         RETURNING rules, updated_by, updated_at",
    )
    .bind(organization_id)
    .bind(rules)
    .bind(actor)
    .fetch_one(&mut **transaction)
    .await
}

pub async fn save_workspace(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    rules: &Value,
    actor: Uuid,
) -> DbResult<Stored> {
    sqlx::query_as(
        "INSERT INTO workspace_policies (workspace_id, rules, updated_by) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (workspace_id) DO UPDATE \
         SET rules = EXCLUDED.rules, updated_by = EXCLUDED.updated_by, updated_at = now() \
         RETURNING rules, updated_by, updated_at",
    )
    .bind(workspace_id)
    .bind(rules)
    .bind(actor)
    .fetch_one(&mut **transaction)
    .await
}
