//! The spend sweep: ingests what LiteLLM logged, keeps its keys in step, then settles every
//! budget for the month and sends the notices its crossings owe.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use abnegate_notify::Fanout;
use zone_enterprise::spend::Month;

use super::client::Client;
use super::error::Error;
use super::standing::Standing;
use super::{ingest, notice, upkeep};
use crate::db::audit::lock::Lock;
use crate::db::spend::budgets;
use crate::state::AppState;

pub const INTERVAL_SECONDS: u64 = 60;

/// How often the keys this process registered are checked against LiteLLM's, and stale ones
/// revoked.
pub const VERIFY_EVERY: Duration = Duration::from_secs(15 * 60);

/// Held while budgets are settled and their notices sent, so one instance sends each notice.
pub const NOTICE_LOCK: i64 = 0x7A6F_6E65_4255_4447;

#[derive(Debug, Default)]
pub struct Sweep {
    last_verified: Mutex<Option<Instant>>,
}

impl Sweep {
    /// Does nothing while spend isn't enforced or the instance has no LiteLLM. A failed ingest
    /// still settles the budgets and sends their notices before it is reported.
    pub async fn run(&self, state: &AppState, fanout: &Fanout) -> Result<(), Error> {
        if !Standing::read(state).await.enforced {
            return Ok(());
        }
        let Some(client) = super::client(state) else {
            return Ok(());
        };
        let ingested = ingest::run(state, &client).await;
        if self.due() {
            self.upkeep(state, &client).await;
        }
        settle(state, fanout, Month::current()).await?;
        ingested.map(|_| ())
    }

    pub(super) fn due(&self) -> bool {
        self.verified()
            .is_none_or(|verified| verified.elapsed() >= VERIFY_EVERY)
    }

    pub(super) fn verified_at(&self, at: Instant) {
        *self
            .last_verified
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(at);
    }

    fn verified(&self) -> Option<Instant> {
        *self
            .last_verified
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    async fn upkeep(&self, state: &AppState, client: &Client) {
        let verified = upkeep::verify(state, client).await;
        let revoked = upkeep::revoke(state, client).await;
        match (verified, revoked) {
            (Ok(repaired), Ok(revoked)) => {
                if repaired > 0 || revoked > 0 {
                    tracing::info!(repaired, revoked, "Brought LiteLLM's spend keys in step");
                }
                self.verified_at(Instant::now());
            }
            (verified, revoked) => {
                for error in [verified.err(), revoked.err()].into_iter().flatten() {
                    tracing::warn!(%error, "Could not bring LiteLLM's spend keys in step; trying again on the next sweep");
                }
            }
        }
    }
}

/// Settles every budget for `month` and sends each crossing's notice, unless another instance is
/// already doing so. Returns how many notices were sent.
pub async fn settle(state: &AppState, fanout: &Fanout, month: Month) -> Result<u64, Error> {
    let pool = state.db();
    let Some(lock) = Lock::attempt(pool, NOTICE_LOCK).await? else {
        return Ok(0);
    };
    let announced = announce(state, fanout, month).await;
    if let Err(error) = lock.release().await {
        tracing::warn!(%error, "Could not release the budget notice lock; closing its connection frees it");
    }
    announced
}

async fn announce(state: &AppState, fanout: &Fanout, month: Month) -> Result<u64, Error> {
    let mut announced = 0;
    for crossing in budgets::settle(state.db(), month).await? {
        match notice::announce(state, fanout, &crossing, month).await {
            Ok(true) => announced += 1,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%error, budget = %crossing.row.id, "Could not send a budget notice; it is sent again on the next sweep");
            }
        }
    }
    Ok(announced)
}
