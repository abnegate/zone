//! Stopping the subagents of chat turns whose server died mid-turn.

use crate::db::{DbResult, chat_subagents};
use crate::services::chat::session::LEASE_LIFETIME;
use crate::state::AppState;

/// A dead turn's lease lapses within one lifetime of its last renewal, so its
/// subagents stop within three.
pub const SWEEP_INTERVAL_SECONDS: u64 = 2 * LEASE_LIFETIME.as_secs();

/// Stops every subagent whose turn no longer runs and tells the readers of
/// each one's chat. Returns how many it stopped.
pub async fn run_cycle(state: &AppState) -> DbResult<usize> {
    let stopped = chat_subagents::abandon(state.db()).await?;
    let count = stopped.len();
    crate::ws::chat::announce_stopped_subagents(stopped).await;
    Ok(count)
}
