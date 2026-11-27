//! Context gathering database queries
//!
//! Provides persistence for context gathering operations

mod latest;

use std::time::Duration;

use chrono::NaiveDateTime;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::DbResult;

pub use latest::Latest;

pub const PENDING: &str = "pending";
pub const RUNNING: &str = "running";

/// How long a gathering may run before it is stopped and failed.
pub const TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// What a gathering past its timeout may still take to record how it ended.
const SETTLING: Duration = Duration::from_secs(10 * 60);

/// How long a gathering can stay `running` before it is taken to have stopped with its replica.
/// Its task lives in that replica alone, so a pod stopped mid-index leaves it `running` for good.
pub const ORPHANED: Duration = TIMEOUT.saturating_add(SETTLING);

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

/// Queue an index of `source_id` as a gathering of its own, unless one is already
/// [under way](Latest::under_way): its latest gathering is running and not orphaned, or pending
/// and younger than `abandoned`. Returns the new gathering, or `None` when another pass got there
/// first.
///
/// Two callers claiming one source at once queue it once: the source's row is locked before
/// its gatherings are read, so the second reads the first's gathering once it is committed.
pub async fn claim_index(
    pool: &PgPool,
    source_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    abandoned: Duration,
) -> DbResult<Option<Uuid>> {
    let mut transaction = pool.begin().await?;
    let source: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM sources WHERE id = $1 FOR NO KEY UPDATE")
            .bind(source_id)
            .fetch_optional(&mut *transaction)
            .await?;
    if source.is_none() {
        return Ok(None);
    }
    if latest(&mut *transaction, source_id)
        .await?
        .is_some_and(|latest| latest.under_way(abandoned))
    {
        return Ok(None);
    }
    let claimed = sqlx::query_scalar(
        r#"
        INSERT INTO context_gatherings
            (user_id, workspace_id, source_ids, status, started_at, created_at)
        VALUES ($2, $3, ARRAY[$1]::uuid[], $4, LOCALTIMESTAMP, statement_timestamp())
        RETURNING id
        "#,
    )
    .bind(source_id)
    .bind(user_id)
    .bind(workspace_id)
    .bind(PENDING)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Some(claimed))
}

/// The newest gathering of `source_id`, if it has ever had one.
pub async fn latest<'e>(
    executor: impl PgExecutor<'e>,
    source_id: Uuid,
) -> DbResult<Option<Latest>> {
    sqlx::query_as::<_, Latest>(
        r#"
        SELECT status, completed_at,
               EXTRACT(EPOCH FROM LOCALTIMESTAMP - COALESCE(started_at, created_at))::float8
                   AS age
        FROM context_gatherings
        WHERE $1 = ANY(source_ids)
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(source_id)
    .fetch_optional(executor)
    .await
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

/// Update gathering status. A gathering that begins running is timed from then, not from when it
/// was queued, so one that waited for a permit is not taken to be [orphaned](ORPHANED) early.
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
            started_at = CASE WHEN $2 = $4 THEN LOCALTIMESTAMP ELSE started_at END,
            completed_at = CASE WHEN $2 IN ('completed', 'failed') THEN NOW() ELSE completed_at END,
            error_message = $3
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(status)
    .bind(error_message)
    .bind(RUNNING)
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
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
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
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
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
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
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
