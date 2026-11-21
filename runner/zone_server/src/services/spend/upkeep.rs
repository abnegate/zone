//! Keeps LiteLLM's keys in step with Zone's: registers again what LiteLLM lost, and deletes the
//! keys an earlier encryption key derived.

use std::collections::HashMap;
use std::time::Instant;

use futures::{StreamExt, TryStreamExt, stream};

use super::attribution;
use super::client::Client;
use super::error::Error;
use super::key::Key;
use super::registry::{self, Entry};
use super::subject::Subject;
use crate::db::spend::keys;
use crate::state::AppState;

const CONCURRENCY: usize = 8;
const REVOCATION_BATCH: usize = 100;

/// Registers again each key this process registered that LiteLLM no longer has, and returns
/// how many it repaired.
pub async fn verify(state: &AppState, client: &Client) -> Result<u64, Error> {
    let tokens = registry::tokens(client.host());
    if tokens.is_empty() {
        return Ok(0);
    }
    let subjects: HashMap<String, Subject> = keys::active(state.db())
        .await?
        .iter()
        .map(|row| (row.token.clone(), Subject::from(row)))
        .collect();
    stream::iter(tokens)
        .map(|token| repair(state, client, &subjects, token))
        .buffer_unordered(CONCURRENCY)
        .try_fold(0, |repaired, fixed| async move {
            Ok(repaired + u64::from(fixed))
        })
        .await
}

async fn repair(
    state: &AppState,
    client: &Client,
    subjects: &HashMap<String, Subject>,
    token: String,
) -> Result<bool, Error> {
    if client.exists(&token).await? {
        return Ok(false);
    }
    let host = client.host();
    registry::forget(host, &token);
    let Some(subject) = subjects.get(&token) else {
        return Ok(false);
    };
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

/// Deletes from LiteLLM every active key the current encryption key no longer derives, and marks
/// it revoked. Its row stays, so spend logged under it before now still attributes.
pub async fn revoke(state: &AppState, client: &Client) -> Result<u64, Error> {
    let pool = state.db();
    let stale: Vec<String> = keys::active(pool)
        .await?
        .into_iter()
        .filter(|row| {
            Key::derive(state.master_key(), row.workspace_id, row.user_id).token != row.token
        })
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
