//! Workspace membership database queries
//!
//! Provides authorization functions to check workspace membership and roles.

use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

use super::DbResult;

/// Workspace role hierarchy
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkspaceRole {
    Viewer = 0,
    Member = 1,
    Admin = 2,
    Owner = 3,
}

impl std::str::FromStr for WorkspaceRole {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "viewer" => Ok(Self::Viewer),
            "member" => Ok(Self::Member),
            "admin" => Ok(Self::Admin),
            "owner" => Ok(Self::Owner),
            _ => Err(()),
        }
    }
}

impl WorkspaceRole {
    /// Convert role to string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Member => "member",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }
}

/// Check if user is an active member of workspace
pub async fn is_member(pool: &PgPool, user_id: Uuid, workspace_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query_scalar::<_, Option<bool>>("SELECT check_workspace_membership($1, $2)")
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await?;

    // If the query returns NULL or no row, treat as false (not a member)
    Ok(result.flatten().unwrap_or(false))
}

/// Get user's role in workspace (None if not a member)
pub async fn get_role(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> DbResult<Option<WorkspaceRole>> {
    let role: Option<String> = sqlx::query_scalar("SELECT get_workspace_role($1, $2)")
        .bind(user_id)
        .bind(workspace_id)
        .fetch_one(pool)
        .await?;

    Ok(role.and_then(|r| r.parse().ok()))
}

/// Check if user has at least the specified role
pub async fn has_role_or_higher(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    required_role: WorkspaceRole,
) -> DbResult<bool> {
    match get_role(pool, user_id, workspace_id).await? {
        Some(role) => Ok(role >= required_role),
        None => Ok(false),
    }
}

/// Verify that all source IDs belong to the workspace
pub async fn verify_sources_in_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
    source_ids: &[Uuid],
) -> DbResult<bool> {
    if source_ids.is_empty() {
        return Ok(true);
    }

    // Count how many of the sources belong to the workspace
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM sources
        WHERE id = ANY($1) AND workspace_id = $2
        "#,
    )
    .bind(source_ids)
    .bind(workspace_id)
    .fetch_one(pool)
    .await?;

    Ok(count == source_ids.len() as i64)
}

/// Add a user to a workspace with a specific role
///
/// CRITICAL-7: This function now fails if the member already exists (active or inactive).
/// Use `reactivate_member` to explicitly reactivate an inactive member.
pub async fn add_member(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role: WorkspaceRole,
    invited_by: Option<Uuid>,
) -> DbResult<Uuid> {
    let id = sqlx::query_scalar(
        r#"
        INSERT INTO workspace_members (workspace_id, user_id, role, invited_by)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role.as_str())
    .bind(invited_by)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Reactivate an inactive member (or add if they don't exist)
/// This is explicit about the intent to reactivate removed members
pub async fn reactivate_member<'a, E>(
    executor: E,
    workspace_id: Uuid,
    user_id: Uuid,
    role: WorkspaceRole,
    invited_by: Option<Uuid>,
) -> DbResult<Uuid>
where
    E: Executor<'a, Database = Postgres>,
{
    let id = sqlx::query_scalar(
        r#"
        INSERT INTO workspace_members (workspace_id, user_id, role, invited_by)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (workspace_id, user_id) DO UPDATE
        SET role = EXCLUDED.role,
            is_active = TRUE,
            invited_by = EXCLUDED.invited_by,
            updated_at = NOW()
        RETURNING id
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role.as_str())
    .bind(invited_by)
    .fetch_one(executor)
    .await?;

    Ok(id)
}

/// Remove a user from a workspace (set inactive)
pub async fn remove_member(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        UPDATE workspace_members
        SET is_active = FALSE, updated_at = NOW()
        WHERE workspace_id = $1 AND user_id = $2
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Workspace member row from database
#[derive(Debug, Clone)]
pub struct WorkspaceMemberRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub role: WorkspaceRole,
    pub is_active: bool,
    pub invited_by: Option<Uuid>,
    pub invited_at: Option<chrono::NaiveDateTime>,
    pub accepted_at: Option<chrono::NaiveDateTime>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

/// Get a workspace member by workspace and user ID
pub async fn get_member(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<WorkspaceMemberRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, workspace_id, user_id, role, is_active,
               invited_by, invited_at, accepted_at, created_at, updated_at
        FROM workspace_members
        WHERE workspace_id = $1 AND user_id = $2
        "#,
        workspace_id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let now = chrono::Utc::now().naive_utc();
        WorkspaceMemberRow {
            id: r.id,
            workspace_id: r.workspace_id,
            user_id: r.user_id,
            role: r.role.parse().unwrap_or(WorkspaceRole::Member),
            is_active: r.is_active,
            invited_by: r.invited_by,
            invited_at: r.invited_at,
            accepted_at: r.accepted_at,
            created_at: r.created_at.unwrap_or(now),
            updated_at: r.updated_at.unwrap_or(now),
        }
    }))
}

/// List all members of a workspace
pub async fn list_members(pool: &PgPool, workspace_id: Uuid) -> DbResult<Vec<WorkspaceMemberRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, workspace_id, user_id, role, is_active,
               invited_by, invited_at, accepted_at, created_at, updated_at
        FROM workspace_members
        WHERE workspace_id = $1 AND is_active = TRUE
        ORDER BY created_at ASC
        "#,
        workspace_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let now = chrono::Utc::now().naive_utc();
            WorkspaceMemberRow {
                id: r.id,
                workspace_id: r.workspace_id,
                user_id: r.user_id,
                role: r.role.parse().unwrap_or(WorkspaceRole::Member),
                is_active: r.is_active,
                invited_by: r.invited_by,
                invited_at: r.invited_at,
                accepted_at: r.accepted_at,
                created_at: r.created_at.unwrap_or(now),
                updated_at: r.updated_at.unwrap_or(now),
            }
        })
        .collect())
}

/// List all workspaces a user is a member of
pub async fn list_user_workspaces(
    pool: &PgPool,
    user_id: Uuid,
) -> DbResult<Vec<super::workspaces::WorkspaceRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT w.id, w.organization_id, w.name, w.slug, w.description,
               w.is_active, w.created_at, w.updated_at
        FROM workspaces w
        INNER JOIN workspace_members wm ON w.id = wm.workspace_id
        WHERE wm.user_id = $1 AND wm.is_active = TRUE
        ORDER BY w.created_at DESC
        "#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| super::workspaces::WorkspaceRow {
            id: r.id,
            organization_id: r.organization_id,
            name: r.name,
            slug: r.slug,
            description: r.description,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// List workspaces in a specific organization that a user is a member of
pub async fn list_user_workspaces_in_org(
    pool: &PgPool,
    user_id: Uuid,
    organization_id: Uuid,
) -> DbResult<Vec<super::workspaces::WorkspaceRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT w.id, w.organization_id, w.name, w.slug, w.description,
               w.is_active, w.created_at, w.updated_at
        FROM workspaces w
        INNER JOIN workspace_members wm ON w.id = wm.workspace_id
        WHERE wm.user_id = $1
          AND w.organization_id = $2
          AND wm.is_active = TRUE
        ORDER BY w.created_at DESC
        "#,
        user_id,
        organization_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| super::workspaces::WorkspaceRow {
            id: r.id,
            organization_id: r.organization_id,
            name: r.name,
            slug: r.slug,
            description: r.description,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Update a member's role
pub async fn update_member_role(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role: WorkspaceRole,
) -> DbResult<WorkspaceMemberRow> {
    let row = sqlx::query!(
        r#"
        UPDATE workspace_members
        SET role = $3, updated_at = NOW()
        WHERE workspace_id = $1 AND user_id = $2
        RETURNING id, workspace_id, user_id, role, is_active,
                  invited_by, invited_at, accepted_at, created_at, updated_at
        "#,
        workspace_id,
        user_id,
        role.as_str()
    )
    .fetch_one(pool)
    .await?;

    let now = chrono::Utc::now().naive_utc();
    Ok(WorkspaceMemberRow {
        id: row.id,
        workspace_id: row.workspace_id,
        user_id: row.user_id,
        role: row.role.parse().unwrap_or(WorkspaceRole::Member),
        is_active: row.is_active,
        invited_by: row.invited_by,
        invited_at: row.invited_at,
        accepted_at: row.accepted_at,
        created_at: row.created_at.unwrap_or(now),
        updated_at: row.updated_at.unwrap_or(now),
    })
}

/// Check if user can read workspace resources (viewer or higher)
pub async fn can_read(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    has_role_or_higher(pool, user_id, workspace_id, WorkspaceRole::Viewer).await
}

/// Check if user can write/modify workspace resources (member or higher)
pub async fn can_write(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    has_role_or_higher(pool, user_id, workspace_id, WorkspaceRole::Member).await
}

/// Check if user can administrate workspace (admin or higher)
pub async fn can_admin(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    has_role_or_higher(pool, user_id, workspace_id, WorkspaceRole::Admin).await
}

/// Count the number of active admins (admin or owner) in a workspace
/// Used to prevent removal of the last admin
pub async fn count_admins(pool: &PgPool, workspace_id: Uuid) -> DbResult<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM workspace_members
        WHERE workspace_id = $1
          AND is_active = TRUE
          AND (role = 'admin' OR role = 'owner')
        "#,
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await?;

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::organizations;
    use crate::db::users;
    use crate::db::workspaces;

    async fn create_test_pool() -> PgPool {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@localhost:5432/zone_test".to_string()
        });

        sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to test database")
    }

    async fn setup_test_data(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
        // Create organization
        let org_id = organizations::create_organization(
            pool,
            &format!("Test Org {}", Uuid::new_v4()),
            &format!("test-org-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create organization")
        .id;

        // Create workspace
        let workspace_id = workspaces::create_workspace(
            pool,
            org_id,
            &format!("Test Workspace {}", Uuid::new_v4()),
            &format!("test-ws-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create user
        let user_email = format!("test-{}@example.com", Uuid::new_v4());
        let user_id =
            users::create_user(pool, &user_email, "password_hash", Some("Test User"), false)
                .await
                .expect("Failed to create user")
                .id;

        (org_id, workspace_id, user_id)
    }

    #[tokio::test]
    async fn test_is_member_returns_true_for_active_member() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        // Add user as member
        add_member(&pool, workspace_id, user_id, WorkspaceRole::Member, None)
            .await
            .expect("Failed to add member");

        // Check membership
        let is_member_result = is_member(&pool, user_id, workspace_id)
            .await
            .expect("Failed to check membership");

        assert!(is_member_result, "User should be a member of the workspace");
    }

    #[tokio::test]
    async fn test_is_member_returns_false_for_non_member() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, _user_id) = setup_test_data(&pool).await;

        // Create another user who is not a member
        let non_member_email = format!("non-member-{}@example.com", Uuid::new_v4());
        let non_member_id = users::create_user(
            &pool,
            &non_member_email,
            "password_hash",
            Some("Non Member"),
            false,
        )
        .await
        .expect("Failed to create user")
        .id;

        // Check membership
        let is_member_result = is_member(&pool, non_member_id, workspace_id)
            .await
            .expect("Failed to check membership");

        assert!(!is_member_result, "Non-member should not be a member");
    }

    #[tokio::test]
    async fn test_is_member_returns_false_for_inactive_member() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        // Add user as member
        add_member(&pool, workspace_id, user_id, WorkspaceRole::Member, None)
            .await
            .expect("Failed to add member");

        // Remove member (set inactive)
        remove_member(&pool, workspace_id, user_id)
            .await
            .expect("Failed to remove member");

        // Check membership
        let is_member_result = is_member(&pool, user_id, workspace_id)
            .await
            .expect("Failed to check membership");

        assert!(
            !is_member_result,
            "Inactive member should not be considered a member"
        );
    }

    #[tokio::test]
    async fn test_get_role_returns_correct_role() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        // Add user as admin
        add_member(&pool, workspace_id, user_id, WorkspaceRole::Admin, None)
            .await
            .expect("Failed to add member");

        // Get role
        let role = get_role(&pool, user_id, workspace_id)
            .await
            .expect("Failed to get role");

        assert_eq!(role, Some(WorkspaceRole::Admin), "Role should be Admin");
    }

    #[tokio::test]
    async fn test_get_role_returns_none_for_non_member() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, _user_id) = setup_test_data(&pool).await;

        // Create another user who is not a member
        let non_member_email = format!("non-member-{}@example.com", Uuid::new_v4());
        let non_member_id = users::create_user(
            &pool,
            &non_member_email,
            "password_hash",
            Some("Non Member"),
            false,
        )
        .await
        .expect("Failed to create user")
        .id;

        // Get role
        let role = get_role(&pool, non_member_id, workspace_id)
            .await
            .expect("Failed to get role");

        assert_eq!(role, None, "Non-member should have no role");
    }

    #[tokio::test]
    async fn test_has_role_or_higher_works_correctly() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        // Add user as admin
        add_member(&pool, workspace_id, user_id, WorkspaceRole::Admin, None)
            .await
            .expect("Failed to add member");

        // Admin should satisfy admin requirement
        assert!(
            has_role_or_higher(&pool, user_id, workspace_id, WorkspaceRole::Admin)
                .await
                .expect("Failed to check role"),
            "Admin should have admin role"
        );

        // Admin should satisfy member requirement
        assert!(
            has_role_or_higher(&pool, user_id, workspace_id, WorkspaceRole::Member)
                .await
                .expect("Failed to check role"),
            "Admin should satisfy member requirement"
        );

        // Admin should satisfy viewer requirement
        assert!(
            has_role_or_higher(&pool, user_id, workspace_id, WorkspaceRole::Viewer)
                .await
                .expect("Failed to check role"),
            "Admin should satisfy viewer requirement"
        );

        // Admin should NOT satisfy owner requirement
        assert!(
            !has_role_or_higher(&pool, user_id, workspace_id, WorkspaceRole::Owner)
                .await
                .expect("Failed to check role"),
            "Admin should not satisfy owner requirement"
        );
    }

    #[tokio::test]
    async fn test_verify_sources_in_workspace_returns_true_for_empty_list() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, _user_id) = setup_test_data(&pool).await;

        let result = verify_sources_in_workspace(&pool, workspace_id, &[])
            .await
            .expect("Failed to verify sources");

        assert!(result, "Empty source list should return true");
    }

    // Note: verify_sources_in_workspace tests require workspace_id column on sources table
    // These tests are commented out until the schema is updated
    // TODO: Uncomment when workspace_id is added to sources table

    // #[tokio::test]
    // async fn test_verify_sources_in_workspace_returns_true_for_valid_sources() {
    //     // Test implementation pending workspace_id on sources
    // }

    // #[tokio::test]
    // async fn test_verify_sources_in_workspace_returns_false_for_invalid_sources() {
    //     // Test implementation pending workspace_id on sources
    // }

    #[tokio::test]
    async fn test_add_member_creates_membership() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        let result = add_member(&pool, workspace_id, user_id, WorkspaceRole::Member, None)
            .await
            .expect("Failed to add member");

        assert_ne!(result, Uuid::nil(), "Should return valid UUID");

        // Verify membership was created
        assert!(
            is_member(&pool, user_id, workspace_id)
                .await
                .expect("Failed to check membership"),
            "User should be a member"
        );
    }

    #[tokio::test]
    async fn test_remove_member_deactivates_membership() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        // Add member
        add_member(&pool, workspace_id, user_id, WorkspaceRole::Member, None)
            .await
            .expect("Failed to add member");

        // Remove member
        let removed = remove_member(&pool, workspace_id, user_id)
            .await
            .expect("Failed to remove member");

        assert!(removed, "Should successfully remove member");

        // Verify membership is inactive
        assert!(
            !is_member(&pool, user_id, workspace_id)
                .await
                .expect("Failed to check membership"),
            "User should not be a member after removal"
        );
    }
}
