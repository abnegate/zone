//! Background indexing worker for automatic source indexing
//!
//! Automatically indexes sources when they are created or updated.

use uuid::Uuid;

use crate::db::context_gatherings;
use crate::state::AppState;

/// Spawn background indexing task for a source
///
/// This is called when a source is created or updated.
/// It queues the source for background indexing.
pub fn spawn_index_source(
    state: AppState,
    source_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    is_update: bool, // true = re-index, false = initial index
) {
    tokio::spawn(async move {
        // Acquire semaphore permit from AppState
        let semaphore = state.index_semaphore().clone();
        let _permit = match semaphore.acquire().await {
            Ok(p) => p,
            Err(_) => {
                tracing::error!(
                    "Failed to acquire indexing semaphore for source {}",
                    source_id
                );
                return;
            }
        };

        tracing::info!(
            "Starting background {} for source {}",
            if is_update { "re-index" } else { "index" },
            source_id
        );

        // Create gathering record
        let gathering_id = match context_gatherings::create_gathering(
            state.db(),
            user_id,
            workspace_id,
            &[source_id],
        )
        .await
        {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Failed to create gathering for source {}: {}", source_id, e);
                return;
            }
        };

        // Execute gathering (reuse existing worker)
        use crate::workers::gathering;

        gathering::execute_gathering(
            &state,
            gathering_id,
            workspace_id,
            vec![source_id],
            is_update, // force_refresh = true for updates
        )
        .await;

        // Note: execute_gathering returns () so we can't check result directly
        // The gathering worker updates the gathering status in the database
        tracing::info!(
            "Background indexing task completed for source {}",
            source_id
        );
    });
}

/// Check if a source needs re-indexing based on config changes
pub fn config_changed(old_config: &serde_json::Value, new_config: &serde_json::Value) -> bool {
    // Deep compare configs
    old_config != new_config
}

/// Check if a source needs re-indexing based on credential changes
pub fn credentials_changed(old_creds: Option<&str>, new_creds: Option<&str>) -> bool {
    old_creds != new_creds
}
