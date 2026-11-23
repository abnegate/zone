use sqlx::PgConnection;
use uuid::Uuid;

use crate::db::DbResult;

#[derive(Debug, sqlx::FromRow)]
pub struct Seat {
    pub role: String,
    pub custom_role_id: Option<Uuid>,
    pub is_active: bool,
}

impl Seat {
    pub async fn organization(
        connection: &mut PgConnection,
        organization_id: Uuid,
        user_id: Uuid,
    ) -> DbResult<Option<Self>> {
        sqlx::query_as(
            "SELECT role, custom_role_id, is_active FROM organization_members \
             WHERE organization_id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(organization_id)
        .bind(user_id)
        .fetch_optional(connection)
        .await
    }

    pub async fn workspace(
        connection: &mut PgConnection,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> DbResult<Option<Self>> {
        sqlx::query_as(
            "SELECT role, custom_role_id, is_active FROM workspace_members \
             WHERE workspace_id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(connection)
        .await
    }
}
