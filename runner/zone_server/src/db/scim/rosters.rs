use sqlx::PgExecutor;
use uuid::Uuid;

use super::roster_entry::RosterEntry;
use crate::db::DbResult;

pub async fn list(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
    groups: &[Uuid],
) -> DbResult<Vec<RosterEntry>> {
    sqlx::query_as(
        r#"
        SELECT m.group_id, m.user_id,
            COALESCE(s.display_name, u.display_name, s.user_name, u.email) AS display
        FROM scim_group_members m
        INNER JOIN scim_groups g ON g.id = m.group_id
        INNER JOIN users u ON u.id = m.user_id
        LEFT JOIN scim_users s ON s.organization_id = g.organization_id AND s.user_id = m.user_id
        WHERE g.organization_id = $1 AND m.group_id = ANY($2)
        ORDER BY m.group_id, m.created_at, m.user_id
        "#,
    )
    .bind(organization_id)
    .bind(groups)
    .fetch_all(executor)
    .await
}
