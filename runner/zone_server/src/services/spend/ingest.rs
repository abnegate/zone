//! Copies the spend LiteLLM logged under Zone's keys into `spend_records`.

use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgPool;
use zone_core::llm;

use super::client::{Client, PAGE_SIZE};
use super::error::Error;
use super::log::Log;
use crate::db::audit::lock::Lock;
use crate::db::spend::records::{self, Record};
use crate::db::spend::{cursor, keys};
use crate::state::AppState;

/// Held for a whole run, so one instance ingests at a time.
pub const SPEND_LOCK: i64 = 0x7A6F_6E65_5350_4E44;

/// How far behind the cursor each run reads again. LiteLLM logs a request once it finishes,
/// under the time it started, so a request may appear this long after it began.
pub const OVERLAP: Duration = Duration::from_secs(llm::TIMEOUT.as_secs() + 5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ingested {
    /// Another instance was ingesting.
    Skipped,
    /// No key has been derived, so nothing could have been spent under one.
    Idle,
    /// `rows` were read from LiteLLM, and `stored` of them were Zone's.
    Done { rows: u64, stored: u64 },
}

pub async fn run(state: &AppState, client: &Client) -> Result<Ingested, Error> {
    let pool = state.db();
    let Some(lock) = Lock::attempt(pool, SPEND_LOCK).await? else {
        return Ok(Ingested::Skipped);
    };
    let ingested = ingest(pool, client, Utc::now()).await;
    if let Err(error) = lock.release().await {
        tracing::warn!(%error, "Could not release the spend ingest lock; closing its connection frees it");
    }
    ingested
}

async fn ingest(pool: &PgPool, client: &Client, end: DateTime<Utc>) -> Result<Ingested, Error> {
    let Some(earliest) = keys::earliest(pool).await? else {
        return Ok(Ingested::Idle);
    };
    let start = start(cursor::read(pool).await?, earliest);
    let (rows, stored) = page_through(client, start, end, |batch, through| async move {
        let stored = records::upsert(pool, &batch).await?;
        if let Some(through) = through {
            cursor::advance(pool, through).await?;
        }
        Ok(stored)
    })
    .await?;
    tracing::debug!(rows, stored, %start, %end, "Ingested LiteLLM spend");
    Ok(Ingested::Done { rows, stored })
}

/// Where a run reads from: [`OVERLAP`] before the cursor, or before the first key was derived
/// when that is later or nothing has been ingested yet.
pub fn start(cursor: Option<DateTime<Utc>>, earliest: DateTime<Utc>) -> DateTime<Utc> {
    cursor.map_or(earliest, |cursor| cursor.max(earliest))
        - TimeDelta::from_std(OVERLAP).expect("the overlap fits a delta")
}

/// Reads every page of the logs from `start` through `end`, handing each page's records to
/// `store` with how far the run has read: a full page's last start, which a failure on a later
/// page keeps, or `end` once a page comes back short. Rows come oldest first, and the next run
/// reads [`OVERLAP`] behind it again, so a row sharing that start on the next page isn't lost.
/// Returns the rows read and what `store` stored.
pub(crate) async fn page_through<Store, Stored>(
    client: &Client,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    mut store: Store,
) -> Result<(u64, u64), Error>
where
    Store: FnMut(Vec<Record>, Option<DateTime<Utc>>) -> Stored,
    Stored: Future<Output = Result<u64, Error>>,
{
    let mut rows = 0;
    let mut stored = 0;
    let mut page = 1;
    loop {
        let fetched = client.logs(start, end, page).await?;
        let count = fetched.rows();
        rows += count as u64;
        let logs = fetched.logs();
        let finished = count < PAGE_SIZE;
        let through = if finished {
            Some(end)
        } else {
            logs.last().map(|log| log.start_time.min(end))
        };
        stored += store(logs.iter().filter_map(Log::record).collect(), through).await?;
        if finished {
            return Ok((rows, stored));
        }
        page += 1;
    }
}
