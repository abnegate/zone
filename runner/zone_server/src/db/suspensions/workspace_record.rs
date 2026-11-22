use sqlx::PgConnection;
use uuid::Uuid;

use crate::db::DbResult;

/// The role and custom role a suspended workspace membership held, read and
/// locked for the caller's transaction.
#[derive(Debug, sqlx::FromRow)]
pub(super) struct WorkspaceRecord {
    pub workspace_id: Uuid,
    pub role: String,
    pub custom_role_id: Option<Uuid>,
}

impl WorkspaceRecord {
    /// Every record of the member in the organization, by ascending workspace.
    pub async fn lock_all(
        connection: &mut PgConnection,
        organization_id: Uuid,
        user_id: Uuid,
    ) -> DbResult<Vec<Self>> {
        sqlx::query_as(
            "SELECT workspace_id, role, custom_role_id FROM workspace_suspensions \
             WHERE organization_id = $1 AND user_id = $2 \
             ORDER BY workspace_id FOR UPDATE",
        )
        .bind(organization_id)
        .bind(user_id)
        .fetch_all(connection)
        .await
    }
}
