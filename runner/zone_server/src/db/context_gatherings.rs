//! Context gathering database queries
//!
//! Provides persistence for context gathering operations

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Context gathering row from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ContextGatheringRow {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub source_ids: Option<Vec<Uuid>>,
    pub status: String,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub created_at: NaiveDateTime,
}

/// Create a new context gathering record
pub async fn create_gathering(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    source_ids: &[Uuid],
) -> DbResult<Uuid> {
    let id = sqlx::query_scalar(
        r#"
        INSERT INTO context_gatherings (user_id, workspace_id, source_ids, status, started_at)
        VALUES ($1, $2, $3, 'pending', NOW())
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(source_ids)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Get a gathering by ID
pub async fn get_gathering(pool: &PgPool, id: Uuid) -> DbResult<Option<ContextGatheringRow>> {
    sqlx::query_as::<_, ContextGatheringRow>(
        r#"
        SELECT id, user_id, workspace_id, source_ids, status, started_at, completed_at,
               error_message, created_at
        FROM context_gatherings
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Update gathering status
pub async fn update_gathering_status(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    error_message: Option<&str>,
) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        UPDATE context_gatherings
        SET status = $2,
            completed_at = CASE WHEN $2 IN ('completed', 'failed') THEN NOW() ELSE completed_at END,
            error_message = $3
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(status)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Update gathering status (convenience function without error message)
pub async fn update_status(pool: &PgPool, id: Uuid, status: &str) -> DbResult<bool> {
    update_gathering_status(pool, id, status, None).await
}

/// List gatherings for a workspace
pub async fn list_gatherings(
    pool: &PgPool,
    workspace_id: Uuid,
    limit: i64,
) -> DbResult<Vec<ContextGatheringRow>> {
    sqlx::query_as::<_, ContextGatheringRow>(
        r#"
        SELECT id, user_id, workspace_id, source_ids, status, started_at, completed_at,
               error_message, created_at
        FROM context_gatherings
        WHERE workspace_id = $1
        ORDER BY created_at DESC
        LIMIT $2
        "#,
    )
    .bind(workspace_id)
    .bind(limit)
    .fetch_all(pool)
    .await
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
    async fn test_create_and_get_gathering() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        let source_ids = vec![Uuid::new_v4(), Uuid::new_v4()];

        let gathering_id = create_gathering(&pool, user_id, workspace_id, &source_ids)
            .await
            .expect("Failed to create gathering");

        let retrieved = get_gathering(&pool, gathering_id)
            .await
            .expect("Failed to get gathering")
            .expect("Gathering not found");

        assert_eq!(retrieved.id, gathering_id);
        assert_eq!(retrieved.user_id, Some(user_id));
        assert_eq!(retrieved.workspace_id, Some(workspace_id));
        assert_eq!(retrieved.source_ids, Some(source_ids));
        assert_eq!(retrieved.status, "pending");
    }

    #[tokio::test]
    async fn test_update_gathering_status() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        let gathering_id = create_gathering(&pool, user_id, workspace_id, &[])
            .await
            .expect("Failed to create gathering");

        let updated = update_gathering_status(&pool, gathering_id, "completed", None)
            .await
            .expect("Failed to update status");

        assert!(updated);

        let retrieved = get_gathering(&pool, gathering_id)
            .await
            .expect("Failed to get gathering")
            .expect("Gathering not found");

        assert_eq!(retrieved.status, "completed");
        assert!(retrieved.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_list_gatherings() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, user_id) = setup_test_data(&pool).await;

        // Create multiple gatherings
        create_gathering(&pool, user_id, workspace_id, &[])
            .await
            .expect("Failed to create gathering");
        create_gathering(&pool, user_id, workspace_id, &[])
            .await
            .expect("Failed to create gathering");

        let gatherings = list_gatherings(&pool, workspace_id, 10)
            .await
            .expect("Failed to list gatherings");

        assert_eq!(gatherings.len(), 2);
    }
}
