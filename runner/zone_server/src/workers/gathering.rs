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

const TOKEN: &str = "token";

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
    index_mode: bool,
) {
    let mut obs = crate::metrics::GatheringObs::new(index_mode);

    // Acquire semaphore permit to limit concurrent gatherings
    let _permit = match get_semaphore().acquire().await {
        Ok(p) => p,
        Err(_) => {
            obs.set_status("semaphore_denied");
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
        "Starting gathering execution: gathering_id={}, workspace_id={}, sources={}, force_refresh={}, index_mode={}",
        gathering_id,
        workspace_id,
        source_ids.len(),
        force_refresh,
        index_mode
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
            obs.set_status("unavailable");
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
            obs.set_status("error");
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

    let mut sources: Vec<Source> = Vec::with_capacity(db_sources.len());
    for db_source in db_sources {
        sources.extend(core_source_from_row(state, db_source).await);
    }

    if sources.is_empty() {
        obs.set_status("no_sources");
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

    let fetch_config = if index_mode {
        FetchConfig::for_source_indexing()
    } else {
        FetchConfig::for_context_gathering(force_refresh)
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
            obs.set_stats(
                gathering_result.embeddings_created as u64,
                gathering_result.items_unchanged as u64,
            );
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

            let searchable =
                gathering_result.embeddings_created > 0 || gathering_result.items_unchanged > 0;
            if index_mode && gathering_result.items_gathered > 0 && !searchable {
                obs.set_status("failed");
                let message = format!(
                    "Indexing produced no searchable chunks ({} items fetched, {} errors)",
                    gathering_result.items_gathered,
                    gathering_result.errors.len()
                );
                tracing::error!("Gathering {} failed: {}", gathering_id, message);
                if let Err(e) = context_gatherings::update_gathering_status(
                    state.db(),
                    gathering_id,
                    "failed",
                    Some(&message),
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

            obs.set_status("completed");
            if let Err(e) =
                context_gatherings::update_status(state.db(), gathering_id, "completed").await
            {
                tracing::error!(
                    "CRITICAL: Failed to update gathering {} status to completed: {}",
                    gathering_id,
                    e
                );
            }
            for source in &sources {
                if let Err(e) =
                    sources::update_verification(state.db(), source.id, workspace_id, None).await
                {
                    tracing::warn!(
                        source_id = %source.id,
                        error = %e,
                        "failed to record last_verified_at after index"
                    );
                }
            }
        }
        Ok(Err(e)) => {
            obs.set_status("failed");
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
            obs.set_status("timeout");
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

/// Convert a database source row into a core `Source`, with its credential as `token`.
pub(crate) async fn core_source_from_row(
    state: &AppState,
    db_source: sources::SourceRow,
) -> Option<Source> {
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
    let mut config = db_source.config.clone();
    match state.github().source(state.db(), &db_source).await {
        Ok(Some(credential)) => {
            if let Some(object) = config.as_object_mut() {
                object.insert(
                    TOKEN.to_string(),
                    serde_json::Value::String(credential.secret().expose().to_string()),
                );
            }
        }
        Ok(None) => {}
        Err(error) => {
            tracing::error!(
                source_id = %db_source.id,
                %error,
                "Could not resolve the source's credential. Source will be skipped."
            );
            return None;
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
        created_at: db_source
            .created_at
            .map(|dt| dt.and_utc())
            .unwrap_or_else(chrono::Utc::now),
        updated_at: db_source
            .updated_at
            .map(|dt| dt.and_utc())
            .unwrap_or_else(chrono::Utc::now),
    })
}

// Integration tests are in zone_server/tests/integration/gathering_worker.rs

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::MockServer;

    use super::*;
    use crate::db::github_apps::tests::{pool, seed_organization, seed_workspace};
    use crate::db::github_installations;
    use crate::db::sources::{GITHUB, NewSource, SourceRow};
    use crate::services::github_app::testing::{installed, issuing, provider};
    use crate::state::test_config;

    const INSTALLATION_ID: u64 = 61_000_001;
    const MINTED_TOKEN: &str = "ghs_throwaway_gathering";
    const STORED_TOKEN: &str = "ghp_throwaway_gathering";

    struct Harness {
        state: AppState,
        github: MockServer,
        organization: Uuid,
        workspace: Uuid,
    }

    impl Harness {
        async fn new() -> Self {
            let pool = pool().await;
            let organization = seed_organization(&pool).await;
            let workspace = seed_workspace(&pool, organization).await;
            let github = MockServer::start().await;
            let state = AppState::new(test_config(), pool, None);
            state.install_github(provider(&github, *state.encryption_key()));
            Self {
                state,
                github,
                organization,
                workspace,
            }
        }

        async fn installation(&self) -> Uuid {
            installed(
                self.state.db(),
                self.state.github(),
                self.organization,
                INSTALLATION_ID,
            )
            .await
        }

        async fn source(&self, credentials: Option<&str>, installation: Option<Uuid>) -> SourceRow {
            let sealed = credentials.map(|token| {
                crate::crypto::encrypt(self.state.encryption_key(), token).expect("seal the token")
            });
            sources::create_source(
                self.state.db(),
                NewSource {
                    workspace_id: self.workspace,
                    name: &format!("Widgets {}", Uuid::new_v4()),
                    source_type: GITHUB,
                    config: json!({ "owner": "acme", "repo": "widgets" }),
                    description: None,
                    url: None,
                    credentials_encrypted: sealed.as_deref(),
                    github_installation_id: installation,
                },
            )
            .await
            .expect("store the source")
        }
    }

    #[tokio::test]
    async fn an_installation_source_is_gathered_with_a_minted_token() {
        let harness = Harness::new().await;
        issuing(&harness.github, INSTALLATION_ID, MINTED_TOKEN, 1).await;
        let installation = harness.installation().await;
        let row = harness.source(None, Some(installation)).await;

        let source = core_source_from_row(&harness.state, row)
            .await
            .expect("an installation source is gathered");

        assert_eq!(source.config[TOKEN], json!(MINTED_TOKEN));
        assert_eq!(source.config["owner"], json!("acme"));
    }

    #[tokio::test]
    async fn a_token_source_is_gathered_with_its_stored_token() {
        let harness = Harness::new().await;
        let row = harness.source(Some(STORED_TOKEN), None).await;

        let source = core_source_from_row(&harness.state, row)
            .await
            .expect("a token source is gathered");

        assert_eq!(source.config[TOKEN], json!(STORED_TOKEN));
    }

    #[tokio::test]
    async fn a_source_without_a_credential_is_gathered_without_a_token() {
        let harness = Harness::new().await;
        let row = harness.source(None, None).await;

        let source = core_source_from_row(&harness.state, row)
            .await
            .expect("a public source is gathered");

        assert!(source.config.get(TOKEN).is_none(), "{}", source.config);
    }

    #[tokio::test]
    async fn a_source_on_a_revoked_installation_is_skipped() {
        let harness = Harness::new().await;
        issuing(&harness.github, INSTALLATION_ID, MINTED_TOKEN, 0).await;
        let installation = harness.installation().await;
        let row = harness.source(None, Some(installation)).await;
        assert!(
            github_installations::mark_revoked(harness.state.db(), installation)
                .await
                .expect("mark the installation revoked")
        );

        assert!(
            core_source_from_row(&harness.state, row).await.is_none(),
            "a revoked installation fails closed"
        );
    }
}
