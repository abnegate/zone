//! Sends a request through the instance's LiteLLM under the key derived for whoever it is spent
//! for, so LiteLLM's spend logs say whose spend it was.

use std::time::Instant;

use super::client::Client;
use super::error::Error;
use super::key::Key;
use super::registry::{self, Entry};
use super::standing::Standing;
use super::subject::Subject;
use crate::db::spend::keys;
use crate::services::endpoint::Endpoint;
use crate::state::AppState;

/// `endpoint` under `subject`'s key when it is the instance's LiteLLM under the master key and
/// `standing` enforces spend. Otherwise, and whenever the key can't be registered, `endpoint` as
/// it is: the request still goes, uncounted. Requests that need the same unregistered key wait
/// for one registration.
pub async fn attribute(
    state: &AppState,
    endpoint: Endpoint,
    subject: &Subject,
    standing: &Standing,
) -> Endpoint {
    if !standing.enforced || !endpoint.attributable(state.config()) {
        return endpoint;
    }
    let Some(client) = super::client(state) else {
        return endpoint;
    };
    let key = Key::derive(state.master_key(), subject.workspace, subject.user);
    let host = client.host();
    if let Some(entry) = registry::settled(host, &key.token) {
        return under(endpoint, key, entry);
    }
    let _registering = registry::hold(host, &key.token).await;
    if let Some(entry) = registry::settled(host, &key.token) {
        return under(endpoint, key, entry);
    }
    let entry = match register(state, &client, &key, subject).await {
        Ok(()) => Entry::Registered,
        Err(error) => {
            tracing::warn!(
                %error,
                workspace = %subject.workspace,
                "Could not register a spend key with LiteLLM, so requests are sent under the master key and not counted until it is retried"
            );
            Entry::Failed(Instant::now())
        }
    };
    registry::mark(host, &key.token, entry);
    under(endpoint, key, entry)
}

fn under(endpoint: Endpoint, key: Key, entry: Entry) -> Endpoint {
    match entry {
        Entry::Registered => endpoint.with_key(key.secret),
        Entry::Failed(_) => endpoint,
    }
}

/// Registers `key` with LiteLLM and records whose spend it carries.
pub(crate) async fn register(
    state: &AppState,
    client: &Client,
    key: &Key,
    subject: &Subject,
) -> Result<(), Error> {
    client.register(key, subject).await?;
    keys::insert(
        state.db(),
        &key.token,
        subject.organization,
        subject.workspace,
        subject.user,
    )
    .await?;
    Ok(())
}
