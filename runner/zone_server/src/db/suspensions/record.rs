use sqlx::PgConnection;
use uuid::Uuid;

use crate::db::DbResult;

#[derive(Debug, sqlx::FromRow)]
pub(super) struct Record {
    pub role: String,
    pub custom_role_id: Option<Uuid>,
}

impl Record {
    pub async fn lock(
        connection: &mut PgConnection,
        organization_id: Uuid,
        user_id: Uuid,
    ) -> DbResult<Option<Self>> {
        sqlx::query_as(
            "SELECT role, custom_role_id FROM organization_suspensions \
             WHERE organization_id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(organization_id)
        .bind(user_id)
        .fetch_optional(connection)
        .await
    }
}
