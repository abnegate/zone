//! Keeping a project's claim while its drive runs.
//!
//! A drive reviews and repairs until its agents finish, so the lease does not
//! measure the drive: the instance driving renews its claim on an interval,
//! and the lease only has to outlast a missed renewal or two before a dead
//! instance's project is handed to a live one.
//!
//! A renewal that finds the claim taken over says so through the drive's
//! [`Lease`], and the drive stops at its next boundary between steps. Every
//! write it makes until then is fenced on the claim, so nothing it does after
//! losing the claim reaches the database.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sqlx::PgPool;
use tokio::task::AbortHandle;
use tokio::time::{Instant, MissedTickBehavior};

use crate::db::auto_projects::{self, Claim};

/// What a drive knows of its hold on the project.
#[derive(Debug, Clone)]
pub struct Lease {
    claim: Option<Claim>,
    lost: Arc<AtomicBool>,
}

impl Lease {
    fn held(claim: Claim) -> Self {
        Self {
            claim: Some(claim),
            lost: Arc::default(),
        }
    }

    /// A drive run by a caller that holds the project some other way, whose
    /// writes are not fenced.
    pub fn unclaimed() -> Self {
        Self {
            claim: None,
            lost: Arc::default(),
        }
    }

    /// The claim the drive's writes are fenced on.
    pub fn claim(&self) -> Option<Claim> {
        self.claim
    }

    /// Whether another instance has taken the project over since the drive
    /// began, so the drive must stop at its next boundary.
    pub fn lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }

    pub(super) fn lose(&self) {
        self.lost.store(true, Ordering::SeqCst);
    }
}

/// Stops a drive's renewals when the drive is over, however it ended -- a
/// drive that panicked must not leave its claim renewed for ever, or its
/// project is never driven again.
struct Renewal(AbortHandle);

impl Drop for Renewal {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Runs the drive `start` makes of its lease to completion while renewing
/// `claim` every `renewal`.
///
/// The renewals run as a task of their own rather than racing the drive, so a
/// renewal that fails or finds the claim lost never cancels a drive part way
/// through a merge: it marks the lease lost, and the drive stops itself.
pub async fn holding<Start, Drive>(
    pool: PgPool,
    claim: Claim,
    renewal: Duration,
    start: Start,
) -> Drive::Output
where
    Start: FnOnce(Lease) -> Drive,
    Drive: Future,
{
    let lease = Lease::held(claim);
    let _renewal =
        Renewal(tokio::spawn(heartbeat(pool, lease.clone(), claim, renewal)).abort_handle());
    start(lease).await
}

async fn heartbeat(pool: PgPool, lease: Lease, claim: Claim, renewal: Duration) {
    let mut interval = tokio::time::interval_at(Instant::now() + renewal, renewal);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        match auto_projects::renew(&pool, claim).await {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(
                    project_id = %claim.project_id,
                    "A running auto project drive lost its claim; stopping it at its next step"
                );
                lease.lose();
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
