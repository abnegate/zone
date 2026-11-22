//! Keeps LiteLLM's keys in step with Zone's: registers again what LiteLLM lost, and deletes the
//! keys nobody can spend under any more.

use std::collections::HashMap;
use std::time::Instant;

use futures::{StreamExt, stream};

use super::attribution;
use super::client::Client;
use super::error::Error;
use super::key::Key;
use super::registry::{self, Entry};
use super::subject::Subject;
use crate::db::spend::keys::{self, KeyRow};
use crate::state::AppState;

const CONCURRENCY: usize = 8;
const REVOCATION_BATCH: usize = 100;

/// Registers again each key this process registered that LiteLLM no longer has, and returns
/// how many it repaired. A key LiteLLM can't be asked about doesn't stop the others; each is
/// logged, and the run then fails with how many there were.
pub async fn verify(state: &AppState, client: &Client) -> Result<u64, Error> {
    let tokens = registry::tokens(client.host());
    if tokens.is_empty() {
        return Ok(0);
    }
    let subjects: HashMap<String, Subject> = keys::active(state.db())
        .await?
        .iter()
        .filter_map(|row| Some((row.token.clone(), Subject::of(row)?)))
        .collect();
    let checked = tokens.len();
    let (repaired, failed) = stream::iter(tokens)
        .map(|token| repair(state, client, &subjects, token))
        .buffer_unordered(CONCURRENCY)
        .fold((0, 0), |(repaired, failed), outcome| async move {
            match outcome {
                Ok(fixed) => (repaired + u64::from(fixed), failed),
                Err(error) => {
                    tracing::warn!(%error, "Could not check a spend key with LiteLLM");
                    (repaired, failed + 1)
                }
            }
        })
        .await;
    if failed > 0 {
        return Err(Error::Unverified { failed, checked });
    }
    Ok(repaired)
}

async fn repair(
    state: &AppState,
    client: &Client,
    subjects: &HashMap<String, Subject>,
    token: String,
) -> Result<bool, Error> {
    let host = client.host();
    let Some(subject) = subjects.get(&token) else {
        registry::forget(host, &token);
        return Ok(false);
    };
    if client.exists(&token).await? {
        return Ok(false);
    }
    registry::forget(host, &token);
    let key = Key::derive(state.master_key(), subject.workspace, subject.user);
    if key.token != token {
        return Ok(false);
    }
    if let Err(error) = attribution::register(state, client, &key, subject).await {
        registry::mark(host, &token, Entry::Failed(Instant::now()));
        return Err(error);
    }
    registry::mark(host, &token, Entry::Registered);
    tracing::info!(workspace = %subject.workspace, "Registered a spend key LiteLLM had lost");
    Ok(true)
}

/// Deletes from LiteLLM every active key nobody can spend under any more, and marks it revoked:
/// a key the current encryption key no longer derives, and a key whose workspace or member is
/// deleted. Its row stays, so spend logged under it before now still attributes.
pub async fn revoke(state: &AppState, client: &Client) -> Result<u64, Error> {
    let pool = state.db();
    let stale: Vec<String> = keys::active(pool)
        .await?
        .into_iter()
        .filter(|row| stale(state, row))
        .map(|row| row.token)
        .collect();
    for batch in stale.chunks(REVOCATION_BATCH) {
        client.revoke(batch).await?;
        for token in batch {
            keys::revoked(pool, token).await?;
            registry::forget(client.host(), token);
        }
    }
    Ok(stale.len() as u64)
}

fn stale(state: &AppState, row: &KeyRow) -> bool {
    Subject::of(row).is_none_or(|subject| {
        Key::derive(state.master_key(), subject.workspace, subject.user).token != row.token
    })
}
