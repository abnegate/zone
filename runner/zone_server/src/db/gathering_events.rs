//! Gathering event database queries
//!
//! Provides persistence for WebSocket gathering events

use chrono::NaiveDateTime;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// Gathering event row from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GatheringEventRow {
    pub id: Uuid,
    pub gathering_id: Uuid,
    pub event_type: String,
    pub payload: Value,
    pub created_at: NaiveDateTime,
}

/// Persist a gathering event to the database
pub async fn persist_event(
    pool: &PgPool,
    gathering_id: Uuid,
    event_type: &str,
    payload: &Value,
) -> DbResult<Uuid> {
    let id = sqlx::query_scalar(
        r#"
        INSERT INTO gathering_events (gathering_id, event_type, payload)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
    )
    .bind(gathering_id)
    .bind(event_type)
    .bind(payload)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Default limit for event queries
const DEFAULT_EVENT_LIMIT: i64 = 100;

/// Get events for a gathering since a timestamp with pagination
pub async fn get_events_since(
    pool: &PgPool,
    gathering_id: Uuid,
    since: Option<NaiveDateTime>,
    limit: Option<i64>,
) -> DbResult<Vec<GatheringEventRow>> {
    let limit = limit.unwrap_or(DEFAULT_EVENT_LIMIT);

    if let Some(since) = since {
        sqlx::query_as::<_, GatheringEventRow>(
            r#"
            SELECT id, gathering_id, event_type, payload, created_at
            FROM gathering_events
            WHERE gathering_id = $1 AND created_at > $2
            ORDER BY created_at ASC
            LIMIT $3
            "#,
        )
        .bind(gathering_id)
        .bind(since)
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, GatheringEventRow>(
            r#"
            SELECT id, gathering_id, event_type, payload, created_at
            FROM gathering_events
            WHERE gathering_id = $1
            ORDER BY created_at ASC
            LIMIT $2
            "#,
        )
        .bind(gathering_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    }
}

/// Cleanup events older than retention period
/// If gathering_id is provided, only cleanup events for that gathering
pub async fn cleanup_old_events(
    pool: &PgPool,
    retention_hours: i64,
    gathering_id: Option<Uuid>,
) -> DbResult<u64> {
    let result = if let Some(gid) = gathering_id {
        sqlx::query(
            r#"
            DELETE FROM gathering_events
            WHERE created_at < NOW() - INTERVAL '1 hour' * $1
            AND gathering_id = $2
            "#,
        )
        .bind(retention_hours)
        .bind(gid)
        .execute(pool)
        .await?
    } else {
        sqlx::query(
            r#"
            DELETE FROM gathering_events
            WHERE created_at < NOW() - INTERVAL '1 hour' * $1
            "#,
        )
        .bind(retention_hours)
        .execute(pool)
        .await?
    };

    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

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

    async fn create_test_gathering(pool: &PgPool) -> Uuid {
        // Create a test gathering in context_gatherings table
        // For now, we'll use a simple insert
        sqlx::query_scalar(
            r#"
            INSERT INTO context_gatherings (status, started_at)
            VALUES ('pending', NOW())
            RETURNING id
            "#,
        )
        .fetch_one(pool)
        .await
        .expect("Failed to create test gathering")
    }

    #[tokio::test]
    #[ignore] // Requires running PostgreSQL database
    async fn test_persist_event_stores_to_database() {
        let pool = create_test_pool().await;
        let gathering_id = create_test_gathering(&pool).await;

        // Persist an event
        let event_type = "Started";
        let payload = serde_json::json!({"message": "Gathering started"});

        let event_id = persist_event(&pool, gathering_id, event_type, &payload)
            .await
            .expect("Failed to persist event");

        assert_ne!(event_id, Uuid::nil(), "Should return valid event ID");

        // Query the event back
        let events = get_events_since(&pool, gathering_id, None, None)
            .await
            .expect("Failed to get events");

        assert_eq!(events.len(), 1, "Should have one event");
        assert_eq!(events[0].event_type, event_type);
        assert_eq!(events[0].payload, payload);
    }

    #[tokio::test]
    #[ignore] // Requires running PostgreSQL database
    async fn test_get_events_since_returns_new_events_only() {
        let pool = create_test_pool().await;
        let gathering_id = create_test_gathering(&pool).await;

        // Insert first event
        let payload1 = serde_json::json!({"step": 1});
        persist_event(&pool, gathering_id, "Progress", &payload1)
            .await
            .expect("Failed to persist event 1");

        // Wait a tiny bit to ensure timestamp difference
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Get timestamp before second event
        let checkpoint = Utc::now().naive_utc();

        // Wait a tiny bit
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Insert second event
        let payload2 = serde_json::json!({"step": 2});
        persist_event(&pool, gathering_id, "Progress", &payload2)
            .await
            .expect("Failed to persist event 2");

        // Query events since checkpoint
        let events = get_events_since(&pool, gathering_id, Some(checkpoint), None)
            .await
            .expect("Failed to get events");

        // Should only get the second event
        assert_eq!(events.len(), 1, "Should have one new event");
        assert_eq!(events[0].payload, payload2);
    }

    #[tokio::test]
    #[ignore] // Requires running PostgreSQL database
    async fn test_get_events_since_none_returns_all_events() {
        let pool = create_test_pool().await;
        let gathering_id = create_test_gathering(&pool).await;

        // Insert multiple events
        for i in 1..=3 {
            let payload = serde_json::json!({"step": i});
            persist_event(&pool, gathering_id, "Progress", &payload)
                .await
                .expect("Failed to persist event");
        }

        // Query all events
        let events = get_events_since(&pool, gathering_id, None, None)
            .await
            .expect("Failed to get events");

        assert_eq!(events.len(), 3, "Should have all three events");
    }

    #[tokio::test]
    #[ignore] // Requires running PostgreSQL database
    async fn test_cleanup_old_events_removes_stale_data() {
        let pool = create_test_pool().await;
        let gathering_id = create_test_gathering(&pool).await;

        // Insert an event
        let payload = serde_json::json!({"test": "data"});
        persist_event(&pool, gathering_id, "Test", &payload)
            .await
            .expect("Failed to persist event");

        // Manually update the event to be old (simulate old data)
        // Set it to 25 hours ago
        sqlx::query(
            r#"
            UPDATE gathering_events
            SET created_at = NOW() - INTERVAL '25 hours'
            WHERE gathering_id = $1
            "#,
        )
        .bind(gathering_id)
        .execute(&pool)
        .await
        .expect("Failed to update event timestamp");

        // Cleanup events older than 24 hours (filter by gathering_id for test isolation)
        let deleted = cleanup_old_events(&pool, 24, Some(gathering_id))
            .await
            .expect("Failed to cleanup events");

        assert!(deleted <= 1, "Should delete at most one old event");

        // Verify event is gone
        let events = get_events_since(&pool, gathering_id, None, None)
            .await
            .expect("Failed to get events");

        assert_eq!(events.len(), 0, "Should have no events after cleanup");
    }

    #[tokio::test]
    #[ignore] // Requires running PostgreSQL database
    async fn test_cleanup_preserves_recent_events() {
        let pool = create_test_pool().await;
        let gathering_id = create_test_gathering(&pool).await;

        // Insert a recent event
        let payload = serde_json::json!({"test": "data"});
        persist_event(&pool, gathering_id, "Test", &payload)
            .await
            .expect("Failed to persist event");

        // Cleanup events older than 24 hours (filter by gathering_id for test isolation)
        let deleted = cleanup_old_events(&pool, 24, Some(gathering_id))
            .await
            .expect("Failed to cleanup events");

        assert_eq!(deleted, 0, "Should not delete recent events");

        // Verify event still exists
        let events = get_events_since(&pool, gathering_id, None, None)
            .await
            .expect("Failed to get events");

        assert_eq!(events.len(), 1, "Should still have one event");
    }

    #[tokio::test]
    #[ignore] // Requires running PostgreSQL database
    async fn test_cleanup_without_gathering_id_cleans_all() {
        let pool = create_test_pool().await;
        let gathering_id = create_test_gathering(&pool).await;

        // Insert an event
        let payload = serde_json::json!({"test": "global_cleanup"});
        persist_event(&pool, gathering_id, "Test", &payload)
            .await
            .expect("Failed to persist event");

        // Make it old
        sqlx::query(
            r#"
            UPDATE gathering_events
            SET created_at = NOW() - INTERVAL '25 hours'
            WHERE gathering_id = $1
            "#,
        )
        .bind(gathering_id)
        .execute(&pool)
        .await
        .expect("Failed to update event timestamp");

        // Cleanup without gathering_id filter (global cleanup)
        let deleted = cleanup_old_events(&pool, 24, None)
            .await
            .expect("Failed to cleanup events");

        // Should delete at least our event (may delete more from other tests)
        assert!(deleted >= 1, "Should delete at least one old event");
    }
}
