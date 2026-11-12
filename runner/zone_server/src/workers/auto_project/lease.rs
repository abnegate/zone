//! Keeping a project's claim while its drive runs.
//!
//! A drive reviews and repairs until its agents finish, so the lease does not
//! measure the drive: the instance driving renews its claim on an interval,
//! and the lease only has to outlast a missed renewal or two before a dead
//! instance's project is handed to a live one.

use std::future::Future;
use std::time::Duration;

use sqlx::PgPool;
use tokio::task::AbortHandle;
use tokio::time::{Instant, MissedTickBehavior};

use crate::db::auto_projects::{self, Claim};

/// Stops a drive's renewals when the drive is over, however it ended -- a
/// drive that panicked must not leave its claim renewed for ever, or its
/// project is never driven again.
struct Renewal(AbortHandle);

impl Drop for Renewal {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Runs `drive` to completion while renewing `claim` every `renewal`.
///
/// The renewals run as a task of their own rather than racing the drive, so a
/// renewal that fails or finds the claim lost never cancels a drive part way
/// through a merge.
pub async fn holding<Drive: Future>(
    pool: PgPool,
    claim: Claim,
    renewal: Duration,
    drive: Drive,
) -> Drive::Output {
    let _renewal = Renewal(tokio::spawn(heartbeat(pool, claim, renewal)).abort_handle());
    drive.await
}

async fn heartbeat(pool: PgPool, claim: Claim, renewal: Duration) {
    let mut interval = tokio::time::interval_at(Instant::now() + renewal, renewal);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        match auto_projects::renew(&pool, claim).await {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(
                    project_id = %claim.project_id,
                    "A running auto project drive lost its claim; no longer renewing it"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    project_id = %claim.project_id,
                    "Could not renew a running auto project drive's claim; trying again"
                );
            }
        }
    }
}
