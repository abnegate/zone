use sqlx::PgPool;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};
use uuid::Uuid;

use crate::db::workspace_members;

/// Cancels a turn once its author can no longer write to its workspace.
///
/// Halting a departed member's turns reaches only the replica that recorded
/// the departure, so a turn running anywhere else finds out from the database.
/// A recheck that fails to reach the database leaves the turn running.
pub(super) struct Access(JoinHandle<()>);

impl Access {
    pub(super) fn watch(
        pool: PgPool,
        workspace_id: Uuid,
        author: Uuid,
        period: Duration,
        cancel: broadcast::Sender<()>,
    ) -> Self {
        Self(tokio::spawn(async move {
            let mut interval = tokio::time::interval_at(Instant::now() + period, period);
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                match workspace_members::can_write(&pool, workspace_id, author).await {
                    Ok(true) => {}
                    Ok(false) => {
                        tracing::info!(
                            %workspace_id,
                            %author,
                            "Ending a turn whose author lost write access to its workspace"
                        );
                        let _ = cancel.send(());
                        return;
                    }
                    Err(error) => {
                        tracing::warn!(%error, %workspace_id, %author, "Could not recheck a turn's access");
                    }
                }
            }
        }))
    }
}

impl Drop for Access {
    fn drop(&mut self) {
        self.0.abort();
    }
}
