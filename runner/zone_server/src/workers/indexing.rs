//! Background indexing worker for automatic source indexing
//!
//! Automatically indexes sources when they are created or updated.

use uuid::Uuid;

use crate::db::DbResult;
use crate::db::context_gatherings::{self, FAILED};
use crate::state::AppState;
use crate::workers::gathering;

const SEMAPHORE_CLOSED: &str = "The indexing semaphore closed before the index started";

/// Index a source in the background unless an index of it is already under way on any replica.
/// The index is claimed before this replica waits for a permit, so it holds the source while it
/// waits. `is_update` re-indexes what is already there. Returns `None` when one was under way.
pub async fn claim(
    state: &AppState,
    source_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    is_update: bool,
) -> DbResult<Option<Uuid>> {
    let claimed =
        context_gatherings::claim_index(state.db(), source_id, workspace_id, user_id).await?;
    if let Some(gathering_id) = claimed {
        spawn(
            state.clone(),
            gathering_id,
            source_id,
            workspace_id,
            is_update,
        );
    }
    Ok(claimed)
}

/// Re-index a source whose config or credentials changed, superseding an index of it that is
/// still queued. Returns `None` when the source is gone.
pub async fn queue(
    state: &AppState,
    source_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<Uuid>> {
    let queued =
        context_gatherings::queue_index(state.db(), source_id, workspace_id, user_id).await?;
    if let Some(gathering_id) = queued {
        spawn(state.clone(), gathering_id, source_id, workspace_id, true);
    }
    Ok(queued)
}

/// Run a queued index once this replica has an indexing permit free.
fn spawn(
    state: AppState,
    gathering_id: Uuid,
    source_id: Uuid,
    workspace_id: Uuid,
    is_update: bool,
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
                tracing::error!(%source_id, %error, "Failed to fail the queued gathering");
            }
            return;
        };
        index(&state, gathering_id, workspace_id, source_id, is_update).await;
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
pub fn config_changed(previous: &serde_json::Value, current: &serde_json::Value) -> bool {
    previous != current
}

/// Check if a source needs re-indexing based on credential changes
pub fn credentials_changed(previous: Option<&str>, current: Option<&str>) -> bool {
    previous != current
}
