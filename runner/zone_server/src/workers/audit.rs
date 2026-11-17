//! Audit event streaming. Each tick rebases positions left behind by a logical restore, then
//! drains every due destination, a bounded number at a time, under Postgres leases shared by all
//! server instances.
use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use futures::future::join_all;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use uuid::Uuid;

use crate::db::audit::events;
use crate::services::audit::delivery::{Cadence, Open, Outcome, deliver};
use crate::state::AppState;

pub fn spawn(state: AppState, open: Arc<dyn Open>) -> JoinHandle<()> {
    let cadence = Cadence::default();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(cadence.tick);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(error) = drain(&state, cadence, open.as_ref()).await {
                tracing::warn!(%error, "Audit delivery tick failed; retrying next tick");
            }
        }
    })
}

/// One tick: rebases first, and an error there aborts the tick, because a delivery read against
/// positions a restore left above the horizon could skip rows. Then up to `cadence.concurrency`
/// deliveries run at once until nothing is due. A slot that claims a destination already found
/// caught up this tick stops, so a destination released as already due can't spin the tick.
/// Returns how many events were delivered.
pub async fn drain(
    state: &AppState,
    cadence: Cadence,
    open: &dyn Open,
) -> Result<usize, sqlx::Error> {
    events::rebase(state.db()).await?;
    let caught = Mutex::new(HashSet::new());
    let slots = (0..cadence.concurrency.max(1)).map(|_| slot(state, cadence, open, &caught));
    join_all(slots)
        .await
        .into_iter()
        .try_fold(0, |total, delivered| Ok(total + delivered?))
}

async fn slot(
    state: &AppState,
    cadence: Cadence,
    open: &dyn Open,
    caught: &Mutex<HashSet<Uuid>>,
) -> Result<usize, sqlx::Error> {
    let mut delivered = 0;
    loop {
        match deliver(state, cadence, open).await? {
            Outcome::Delivered(count) => delivered += count,
            Outcome::Paused(_) | Outcome::Unclaimed => return Ok(delivered),
            Outcome::Idle(destination) | Outcome::Waiting(destination) => {
                let first = caught
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(destination);
                if !first {
                    tracing::debug!(
                        destination_id = %destination,
                        "Audit destination came round again caught up; ending the slot this tick"
                    );
                    return Ok(delivered);
                }
            }
            Outcome::Failed(_) | Outcome::Lost => {}
        }
    }
}
