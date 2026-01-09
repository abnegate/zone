//! Context gathering worker
//!
//! Executes context gathering operations in the background, persisting
//! events to the database for WebSocket streaming.

use sqlx::PgPool;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;
use uuid::Uuid;
use zone_context::content::FetchConfig;
use zone_context::stream::{GatheringCallback, GatheringEvent};
use zone_core::Source;

use crate::db::{context_gatherings, gathering_events, sources};
use crate::state::AppState;

// Max concurrent gatherings to prevent resource exhaustion
const MAX_CONCURRENT_GATHERINGS: usize = 10;

// Timeout for gathering operations (1 hour)
const GATHERING_TIMEOUT_SECS: u64 = 3600;

// Global semaphore to limit concurrent gatherings
static GATHERING_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn get_semaphore() -> &'static Arc<Semaphore> {
    GATHERING_SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_GATHERINGS)))
}

/// Callback that persists gathering events to the database
///
/// Events are persisted asynchronously in spawned tasks to avoid blocking
/// the gathering pipeline. Events may arrive slightly out of order, but
/// timestamps ensure correct ordering when queried.
pub struct DatabaseCallback {
    pool: PgPool,
    gathering_id: Uuid,
}

impl DatabaseCallback {
    /// Create a new database callback
    pub fn new(pool: PgPool, gathering_id: Uuid) -> Self {
        Self { pool, gathering_id }
    }
}

impl GatheringCallback for DatabaseCallback {
    fn on_event(&self, event: GatheringEvent) {
        let pool = self.pool.clone();
        let gathering_id = self.gathering_id;

        // Spawn async task to persist event without blocking
        tokio::spawn(async move {
            // Map event variant to event_type string
            let event_type = match &event {
                GatheringEvent::Started { .. } => "started",
                GatheringEvent::SourceStarted { .. } => "source_started",
                GatheringEvent::SourceProgress { .. } => "source_progress",
                GatheringEvent::SourceCompleted { .. } => "source_completed",
                GatheringEvent::SourceError { .. } => "source_error",
                GatheringEvent::AnalysisStarted { .. } => "analysis_started",
                GatheringEvent::AnalysisProgress { .. } => "analysis_progress",
                GatheringEvent::EmbeddingProgress { .. } => "embedding_progress",
                GatheringEvent::Completed { .. } => "completed",
                GatheringEvent::Failed { .. } => "failed",
            };

            // Serialize event as JSON payload
            let payload = serde_json::to_value(&event).unwrap_or_default();

            // Persist to database
            if let Err(e) =
                gathering_events::persist_event(&pool, gathering_id, event_type, &payload).await
            {
                tracing::error!("Failed to persist gathering event: {}", e);
            }
        });
    }
}

/// Execute context gathering for a set of sources
///
/// This function runs the complete gathering pipeline:
/// 1. Acquires semaphore permit to limit concurrent gatherings
/// 2. Updates gathering status to "running"
/// 3. Fetches sources from database (with workspace_id authorization)
/// 4. Executes ContextService.gather() with DatabaseCallback (with timeout)
/// 5. Updates status to "completed" or "failed"
///
/// All events are persisted to the database via DatabaseCallback for
/// WebSocket streaming to clients.
///
/// CRITICAL: workspace_id is required to prevent authorization bypass
pub async fn execute_gathering(
    state: &AppState,
    gathering_id: Uuid,
    workspace_id: Uuid,
    source_ids: Vec<Uuid>,
    force_refresh: bool,
) {
    // Acquire semaphore permit to limit concurrent gatherings
    let _permit = match get_semaphore().acquire().await {
        Ok(p) => p,
        Err(_) => {
            tracing::error!("Gathering semaphore closed for gathering {}", gathering_id);
            if let Err(e) = context_gatherings::update_gathering_status(
                state.db(),
                gathering_id,
                "failed",
                Some("System overload - semaphore closed"),
            )
            .await
            {
                tracing::error!(
                    "CRITICAL: Failed to update gathering {} status: {}",
                    gathering_id,
                    e
                );
            }
            return;
        }
    };

    tracing::info!(
        "Starting gathering execution: gathering_id={}, workspace_id={}, sources={}, force_refresh={}",
        gathering_id,
        workspace_id,
        source_ids.len(),
        force_refresh
    );

    // Update status to running
    if let Err(e) = context_gatherings::update_status(state.db(), gathering_id, "running").await {
        tracing::error!(
            "CRITICAL: Failed to update gathering {} status to running: {}",
            gathering_id,
            e
        );
        // Continue anyway - the gathering can still run even if status update fails
    }

    // Get context service
    let context_service = match state.context_service() {
        Some(svc) => svc,
        None => {
            tracing::error!("Context service not available");
            if let Err(e) = context_gatherings::update_gathering_status(
                state.db(),
                gathering_id,
                "failed",
                Some("Context service not available"),
            )
            .await
            {
                tracing::error!(
                    "CRITICAL: Failed to update gathering {} status: {}",
                    gathering_id,
                    e
                );
            }
            return;
        }
    };

    // Fetch sources from database with workspace_id filter for authorization
    let db_sources = match sources::get_sources_by_ids(state.db(), &source_ids, workspace_id).await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to fetch sources from database: {}", e);
            if let Err(e) = context_gatherings::update_gathering_status(
                state.db(),
                gathering_id,
                "failed",
                Some(&format!("Failed to fetch sources: {}", e)),
            )
            .await
            {
                tracing::error!(
                    "CRITICAL: Failed to update gathering {} status: {}",
                    gathering_id,
                    e
                );
            }
            return;
        }
    };

    // Convert database sources to zone_core::Source
    let sources: Vec<Source> = db_sources
        .into_iter()
        .filter_map(|db_source| {
            // Parse source_type string to enum
            let source_type = match db_source.source_type.as_str() {
                "text" => zone_core::SourceType::Text,
                "filesystem" => zone_core::SourceType::Filesystem,
                "github" => zone_core::SourceType::GitHub,
                "gitlab" => zone_core::SourceType::GitLab,
                "google_calendar" => zone_core::SourceType::GoogleCalendar,
                "google_mail" => zone_core::SourceType::GoogleMail,
                "notion" => zone_core::SourceType::Notion,
                "slack" => zone_core::SourceType::Slack,
                "web" => zone_core::SourceType::Web,
                _ => {
                    tracing::warn!("Unknown source type: {}", db_source.source_type);
                    return None;
                }
            };

            let category = source_type.category();

            // Decrypt credentials and inject into config
            let mut config = db_source.config.clone();
            if let Some(encrypted_creds) = &db_source.credentials_encrypted {
                match crate::crypto::decrypt(state.encryption_key(), encrypted_creds) {
                    Ok(decrypted) => {
                        // Inject decrypted credentials as "token" field in config
                        // This works for GitHub, GitLab, and other adapters that expect a token
                        if let Some(config_obj) = config.as_object_mut() {
                            config_obj.insert("token".to_string(), serde_json::Value::String(decrypted));
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to decrypt credentials for source {}: {}. Source will be skipped.",
                            db_source.id,
                            e
                        );
                        return None;
                    }
                }
            }

            Some(Source {
                id: db_source.id,
                name: db_source.name,
                source_type,
                category,
                config,
                is_active: db_source.is_active.unwrap_or(true),
                last_synced_at: db_source.last_verified_at.map(|dt| dt.and_utc()),
                created_at: db_source.created_at.map(|dt| dt.and_utc()).unwrap_or_else(chrono::Utc::now),
                updated_at: db_source.updated_at.map(|dt| dt.and_utc()).unwrap_or_else(chrono::Utc::now),
            })
        })
        .collect();

    if sources.is_empty() {
        tracing::warn!("No valid sources to gather from");
        if let Err(e) = context_gatherings::update_gathering_status(
            state.db(),
            gathering_id,
            "failed",
            Some("No valid sources to gather from"),
        )
        .await
        {
            tracing::error!(
                "CRITICAL: Failed to update gathering {} status: {}",
                gathering_id,
                e
            );
        }
        return;
    }

    tracing::info!("Prepared {} sources for gathering", sources.len());

    // Create database callback for event persistence
    let callback = DatabaseCallback::new(state.db().clone(), gathering_id);

    // Configure fetch behavior
    let fetch_config = FetchConfig {
        max_tokens: 100_000,
        token_budget: 100_000, // Default budget
        allow_metadata_only: true,
        since: if force_refresh {
            None
        } else {
            Some(chrono::Utc::now() - chrono::Duration::days(30))
        },
        include_patterns: vec![],
        exclude_patterns: vec![],
    };

    // Execute gathering with timeout
    let gathering_future = context_service.gather(&sources, fetch_config, &callback);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(GATHERING_TIMEOUT_SECS),
        gathering_future,
    )
    .await;

    match result {
        Ok(Ok(gathering_result)) => {
            // Gathering completed successfully
            tracing::info!(
                "Gathering {} completed: sources={}, items={}, embeddings={}, duration={}ms",
                gathering_id,
                gathering_result.sources_processed,
                gathering_result.items_gathered,
                gathering_result.embeddings_created,
                gathering_result.duration_ms
            );

            if !gathering_result.errors.is_empty() {
                tracing::warn!(
                    "Gathering {} had {} errors",
                    gathering_id,
                    gathering_result.errors.len()
                );
            }

            if let Err(e) =
                context_gatherings::update_status(state.db(), gathering_id, "completed").await
            {
                tracing::error!(
                    "CRITICAL: Failed to update gathering {} status to completed: {}",
                    gathering_id,
                    e
                );
            }
        }
        Ok(Err(e)) => {
            // Gathering failed with error
            tracing::error!("Gathering {} failed: {}", gathering_id, e);
            if let Err(e) = context_gatherings::update_gathering_status(
                state.db(),
                gathering_id,
                "failed",
                Some(&e.to_string()),
            )
            .await
            {
                tracing::error!(
                    "CRITICAL: Failed to update gathering {} status: {}",
                    gathering_id,
                    e
                );
            }
        }
        Err(_) => {
            // Gathering timed out
            tracing::error!(
                "Gathering {} timed out after {} seconds",
                gathering_id,
                GATHERING_TIMEOUT_SECS
            );
            if let Err(e) = context_gatherings::update_gathering_status(
                state.db(),
                gathering_id,
                "failed",
                Some(&format!(
                    "Gathering timed out after {} seconds",
                    GATHERING_TIMEOUT_SECS
                )),
            )
            .await
            {
                tracing::error!(
                    "CRITICAL: Failed to update gathering {} status: {}",
                    gathering_id,
                    e
                );
            }
        }
    }
}

// Integration tests are in zone_server/tests/gathering_worker_tests.rs
