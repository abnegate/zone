use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::workspace_members::WorkspaceRole;

/// Seats `user` in `workspace` as `role`, joining them to its organization as
/// a member first unless they already belong: a workspace grants nothing to
/// someone outside its organization.
pub async fn seat(pool: &PgPool, workspace: Uuid, user: Uuid, role: WorkspaceRole) {
    sqlx::query(
        "WITH joined AS ( \
           INSERT INTO organization_members (organization_id, user_id, role) \
           SELECT organization_id, $2, 'member' FROM workspaces WHERE id = $1 \
           ON CONFLICT DO NOTHING) \
         INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)",
    )
    .bind(workspace)
    .bind(user)
    .bind(role.as_str())
    .execute(pool)
    .await
    .expect("a workspace membership");
}
