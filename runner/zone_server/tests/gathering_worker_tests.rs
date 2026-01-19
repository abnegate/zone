//! Tests for context gathering worker
//!
//! These tests verify that the gathering worker:
//! - Processes sources using ContextService
//! - Persists events to database
//! - Updates gathering status on completion/failure

use sqlx::PgPool;
use std::sync::Arc;
use tokio::time::{Duration, sleep, timeout};
use uuid::Uuid;
use zone_context::adapters::{AdapterRegistry, TextAdapter};
use zone_context::context::ContextService;
use zone_context::embeddings::providers::MockEmbeddingService;
use zone_context::stream::{GatheringCallback, GatheringEvent};
use zone_server::db::{context_gatherings, gathering_events, sources};
use zone_server::state::AppState;
use zone_server::workers::gathering;

mod common;

/// Test helper to create a test source
async fn create_test_source(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    source_type: &str,
) -> Uuid {
    let config = serde_json::json!({
        "content": "Test content for gathering"
    });

    sources::create_source(
        pool,
        workspace_id,
        name,
        source_type,
        config,
        None,
        None,
        None,
    )
    .await
    .expect("Failed to create test source")
    .id
}

#[tokio::test]
async fn test_execute_gathering_updates_status_to_running() {
    // Given: A test workspace and gathering
    let pool = common::create_test_pool().await;
    let (_org_id, workspace_id, user_id) = common::setup_test_data(&pool).await;

    let source_name = format!("Test Source {}", Uuid::new_v4());
    let source_id = create_test_source(&pool, workspace_id, &source_name, "text").await;
    let gathering_id =
        context_gatherings::create_gathering(&pool, user_id, workspace_id, &[source_id])
            .await
            .expect("Failed to create gathering");

    // Create app state with context services
    let config = common::create_test_config();
    let adapter_registry = Arc::new({
        let mut registry = AdapterRegistry::new();
        registry.register(TextAdapter::new());
        registry
    });
    let embedding_service = Arc::new(MockEmbeddingService::new(384));
    let context_service = Arc::new(ContextService::new(
        pool.clone(),
        adapter_registry.clone(),
        embedding_service.clone(),
    ));

    let state = AppState::new_with_services(
        config,
        pool.clone(),
        None,
        adapter_registry,
        embedding_service,
        context_service,
    );

    // When: Execute gathering
    gathering::execute_gathering(&state, gathering_id, workspace_id, vec![source_id], false).await;

    // Then: Status should be updated to completed or failed (not pending)
    let gathering = context_gatherings::get_gathering(&pool, gathering_id)
        .await
        .expect("Failed to get gathering")
        .expect("Gathering not found");

    assert_ne!(
        gathering.status, "pending",
        "Status should be updated from pending"
    );
    assert!(
        gathering.status == "completed" || gathering.status == "failed",
        "Status should be completed or failed, got: {}",
        gathering.status
    );
}

#[tokio::test]
async fn test_execute_gathering_persists_events() {
    // Given: A test workspace and gathering with text source
    let pool = common::create_test_pool().await;
    let (_org_id, workspace_id, user_id) = common::setup_test_data(&pool).await;

    let source_name = format!("Test Source {}", Uuid::new_v4());
    let source_id = create_test_source(&pool, workspace_id, &source_name, "text").await;
    let gathering_id =
        context_gatherings::create_gathering(&pool, user_id, workspace_id, &[source_id])
            .await
            .expect("Failed to create gathering");

    // Create app state with context services
    let config = common::create_test_config();
    let adapter_registry = Arc::new({
        let mut registry = AdapterRegistry::new();
        registry.register(TextAdapter::new());
        registry
    });
    let embedding_service = Arc::new(MockEmbeddingService::new(384));
    let context_service = Arc::new(ContextService::new(
        pool.clone(),
        adapter_registry.clone(),
        embedding_service.clone(),
    ));

    let state = AppState::new_with_services(
        config,
        pool.clone(),
        None,
        adapter_registry,
        embedding_service,
        context_service,
    );

    // When: Execute gathering
    gathering::execute_gathering(&state, gathering_id, workspace_id, vec![source_id], false).await;

    // Then: Events should be persisted (poll until started + terminal events show up)
    let events = timeout(Duration::from_secs(2), async {
        loop {
            let events = gathering_events::get_events_since(&pool, gathering_id, None, None)
                .await
                .expect("Failed to get events");
            let has_started = events.iter().any(|e| e.event_type == "started");
            let has_terminal = events
                .iter()
                .any(|e| e.event_type == "completed" || e.event_type == "failed");
            if has_started && has_terminal {
                break events;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Timed out waiting for gathering events");

    assert!(
        !events.is_empty(),
        "Should have persisted at least one event"
    );
}

#[tokio::test]
async fn test_execute_gathering_handles_missing_source() {
    // Given: A gathering with non-existent source ID
    let pool = common::create_test_pool().await;
    let (_org_id, workspace_id, user_id) = common::setup_test_data(&pool).await;

    let non_existent_source = Uuid::new_v4();
    let gathering_id =
        context_gatherings::create_gathering(&pool, user_id, workspace_id, &[non_existent_source])
            .await
            .expect("Failed to create gathering");

    // Create app state
    let config = common::create_test_config();
    let adapter_registry = Arc::new({
        let mut registry = AdapterRegistry::new();
        registry.register(TextAdapter::new());
        registry
    });
    let embedding_service = Arc::new(MockEmbeddingService::new(384));
    let context_service = Arc::new(ContextService::new(
        pool.clone(),
        adapter_registry.clone(),
        embedding_service.clone(),
    ));

    let state = AppState::new_with_services(
        config,
        pool.clone(),
        None,
        adapter_registry,
        embedding_service,
        context_service,
    );

    // When: Execute gathering with non-existent source
    gathering::execute_gathering(
        &state,
        gathering_id,
        workspace_id,
        vec![non_existent_source],
        false,
    )
    .await;

    // Then: Status should be failed
    let gathering = context_gatherings::get_gathering(&pool, gathering_id)
        .await
        .expect("Failed to get gathering")
        .expect("Gathering not found");

    assert_eq!(
        gathering.status, "failed",
        "Status should be failed for missing source"
    );
}

#[tokio::test]
async fn test_execute_gathering_with_multiple_sources() {
    // Given: Multiple text sources
    let pool = common::create_test_pool().await;
    let (_org_id, workspace_id, user_id) = common::setup_test_data(&pool).await;

    let source1_name = format!("Source 1 {}", Uuid::new_v4());
    let source2_name = format!("Source 2 {}", Uuid::new_v4());
    let source1_id = create_test_source(&pool, workspace_id, &source1_name, "text").await;
    let source2_id = create_test_source(&pool, workspace_id, &source2_name, "text").await;

    let gathering_id = context_gatherings::create_gathering(
        &pool,
        user_id,
        workspace_id,
        &[source1_id, source2_id],
    )
    .await
    .expect("Failed to create gathering");

    // Create app state
    let config = common::create_test_config();
    let adapter_registry = Arc::new({
        let mut registry = AdapterRegistry::new();
        registry.register(TextAdapter::new());
        registry
    });
    let embedding_service = Arc::new(MockEmbeddingService::new(384));
    let context_service = Arc::new(ContextService::new(
        pool.clone(),
        adapter_registry.clone(),
        embedding_service.clone(),
    ));

    let state = AppState::new_with_services(
        config,
        pool.clone(),
        None,
        adapter_registry,
        embedding_service,
        context_service,
    );

    // When: Execute gathering with multiple sources
    gathering::execute_gathering(
        &state,
        gathering_id,
        workspace_id,
        vec![source1_id, source2_id],
        false,
    )
    .await;

    // Then: Should have events for both sources (poll until persisted)
    let events = timeout(Duration::from_secs(2), async {
        loop {
            let events = gathering_events::get_events_since(&pool, gathering_id, None, None)
                .await
                .expect("Failed to get events");
            let source_started_count = events
                .iter()
                .filter(|e| e.event_type == "source_started")
                .count();
            if source_started_count >= 2 {
                break events;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Timed out waiting for source_started events");

    let source_started_count = events
        .iter()
        .filter(|e| e.event_type == "source_started")
        .count();

    // Should have at least started events for sources
    assert!(
        source_started_count >= 2,
        "Should have source_started events for both sources"
    );
}

#[tokio::test]
async fn test_database_callback_persists_events() {
    // Given: A gathering and a callback
    let pool = common::create_test_pool().await;
    let (_org_id, workspace_id, user_id) = common::setup_test_data(&pool).await;

    let gathering_id = context_gatherings::create_gathering(&pool, user_id, workspace_id, &[])
        .await
        .expect("Failed to create gathering");

    let callback = gathering::DatabaseCallback::new(pool.clone(), gathering_id);

    // When: Emit events
    callback.on_event(GatheringEvent::Started {
        gathering_id,
        source_count: 1,
        timestamp: chrono::Utc::now(),
    });

    // Give async task time to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Then: Event should be persisted
    let events = gathering_events::get_events_since(&pool, gathering_id, None, None)
        .await
        .expect("Failed to get events");

    assert!(!events.is_empty(), "Should have persisted event");
    assert_eq!(events[0].event_type, "started");
}

#[tokio::test]
async fn test_database_callback_handles_all_event_types() {
    // Given: A gathering and callback
    let pool = common::create_test_pool().await;
    let (_org_id, workspace_id, user_id) = common::setup_test_data(&pool).await;

    let gathering_id = context_gatherings::create_gathering(&pool, user_id, workspace_id, &[])
        .await
        .expect("Failed to create gathering");

    let callback = gathering::DatabaseCallback::new(pool.clone(), gathering_id);

    // When: Emit various event types
    let source_id = Uuid::new_v4();

    callback.on_event(GatheringEvent::Started {
        gathering_id,
        source_count: 1,
        timestamp: chrono::Utc::now(),
    });

    callback.on_event(GatheringEvent::SourceStarted {
        gathering_id,
        source_id,
        source_name: "Test".to_string(),
        source_type: "text".to_string(),
    });

    callback.on_event(GatheringEvent::SourceProgress {
        gathering_id,
        source_id,
        items_fetched: 5,
        estimated_total: Some(10),
        tokens_fetched: 100,
    });

    callback.on_event(GatheringEvent::SourceCompleted {
        gathering_id,
        source_id,
        items_count: 10,
        token_count: 200,
        duration_ms: 1000,
    });

    callback.on_event(GatheringEvent::Completed {
        gathering_id,
        total_items: 10,
        total_tokens: 200,
        duration_ms: 1000,
        timestamp: chrono::Utc::now(),
    });

    let events = timeout(Duration::from_secs(2), async {
        loop {
            let events = gathering_events::get_events_since(&pool, gathering_id, None, None)
                .await
                .expect("Failed to get events");
            if events.len() >= 5 {
                break events;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Timed out waiting for events");

    // Then: All events should be persisted with correct types
    assert!(events.len() >= 5, "Should have persisted all events");

    let event_types: Vec<String> = events.iter().map(|e| e.event_type.clone()).collect();
    assert!(event_types.contains(&"started".to_string()));
    assert!(event_types.contains(&"source_started".to_string()));
    assert!(event_types.contains(&"source_progress".to_string()));
    assert!(event_types.contains(&"source_completed".to_string()));
    assert!(event_types.contains(&"completed".to_string()));
}
