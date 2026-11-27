//! Background indexing worker for automatic source indexing
//!
//! Automatically indexes sources when they are created or updated.

use uuid::Uuid;

use crate::db::context_gatherings;
use crate::state::AppState;
use crate::workers::gathering;

const FAILED: &str = "failed";
const SEMAPHORE_CLOSED: &str = "The indexing semaphore closed before the index started";

/// Index a source in the background, once this replica has an indexing permit free.
///
/// Called when a source is created or updated. `is_update` re-indexes what is already there.
pub fn spawn_index_source(
    state: AppState,
    source_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    is_update: bool,
) {
    tokio::spawn(async move {
        let Ok(_permit) = state.index_semaphore().acquire().await else {
            tracing::error!(%source_id, "Failed to acquire the indexing semaphore");
            return;
        };
        let gathering_id = match context_gatherings::create_gathering(
            state.db(),
            user_id,
            workspace_id,
            &[source_id],
        )
        .await
        {
            Ok(id) => id,
            Err(error) => {
                tracing::error!(%source_id, %error, "Failed to create a gathering for the source");
                return;
            }
        };
        index(&state, gathering_id, workspace_id, source_id, is_update).await;
    });
}

/// Re-index a source whose gathering a resync pass already claimed with
/// [`context_gatherings::claim_index`], once this replica has an indexing permit free.
pub fn spawn_claimed_index(
    state: AppState,
    gathering_id: Uuid,
    source_id: Uuid,
    workspace_id: Uuid,
) {
    tokio::spawn(async move {
        let Ok(_permit) = state.index_semaphore().acquire().await else {
            tracing::error!(%source_id, "Failed to acquire the indexing semaphore");
            if let Err(error) = context_gatherings::update_gathering_status(
                state.db(),
                gathering_id,
                FAILED,
                Some(SEMAPHORE_CLOSED),
            )
            .await
            {
                tracing::error!(%source_id, %error, "Failed to fail the claimed gathering");
            }
            return;
        };
        index(&state, gathering_id, workspace_id, source_id, true).await;
    });
}

async fn index(
    state: &AppState,
    gathering_id: Uuid,
    workspace_id: Uuid,
    source_id: Uuid,
    is_update: bool,
) {
    tracing::info!(
        %source_id,
        "Starting background {} of the source",
        if is_update { "re-index" } else { "index" }
    );
    gathering::execute_gathering(
        state,
        gathering_id,
        workspace_id,
        vec![source_id],
        is_update,
        true,
    )
    .await;
    tracing::info!(%source_id, "Background indexing of the source finished");
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
