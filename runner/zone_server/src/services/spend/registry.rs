//! Which derived keys this process has registered with which LiteLLM, so a request sends its key
//! without asking LiteLLM first. It is kept in memory alone: a restarted server registers each
//! key again, which also restores keys LiteLLM lost to a restore or a manual delete.

use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// How long a key whose registration failed is sent under the master key before it is tried
/// again.
pub const COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Registered,
    Failed(Instant),
}

#[derive(Default)]
struct Slot {
    entry: Option<Entry>,
    registering: Arc<Mutex<()>>,
}

/// Each LiteLLM host's tokens, so a lookup borrows the host and token it is given.
static HOSTS: LazyLock<DashMap<String, DashMap<String, Slot>>> = LazyLock::new(DashMap::new);

pub fn registered(host: &str, token: &str) -> bool {
    settled(host, token) == Some(Entry::Registered)
}

pub fn cooling(host: &str, token: &str) -> bool {
    matches!(settled(host, token), Some(Entry::Failed(_)))
}

/// What is known of `token` on `host` that a request can act on without registering it: that it
/// is registered, or that it failed less than [`COOLDOWN`] ago.
pub fn settled(host: &str, token: &str) -> Option<Entry> {
    let entry = HOSTS.get(host)?.get(token)?.entry?;
    match entry {
        Entry::Failed(at) if at.elapsed() >= COOLDOWN => None,
        entry => Some(entry),
    }
}

/// Waits until no other request is registering `token` on `host`, and keeps the others waiting
/// until the guard drops, so one registration runs at a time for each key.
pub async fn hold(host: &str, token: &str) -> OwnedMutexGuard<()> {
    let registering = change(host, token, |slot| slot.registering.clone());
    registering.lock_owned().await
}

pub fn mark(host: &str, token: &str, entry: Entry) {
    change(host, token, |slot| slot.entry = Some(entry));
}

pub fn forget(host: &str, token: &str) {
    if let Some(tokens) = HOSTS.get(host) {
        tokens.remove(token);
    }
}

/// The tokens registered with `host`.
pub fn tokens(host: &str) -> Vec<String> {
    HOSTS.get(host).map_or_else(Vec::new, |tokens| {
        tokens
            .iter()
            .filter(|slot| slot.entry == Some(Entry::Registered))
            .map(|slot| slot.key().clone())
            .collect()
    })
}

/// Applies `apply` to `token`'s slot on `host`, creating it first when there is none. A map is
/// never written while this still reads it, which would deadlock on its shard.
fn change<R>(host: &str, token: &str, apply: impl FnOnce(&mut Slot) -> R) -> R {
    let tokens = match HOSTS.get(host) {
        Some(tokens) => tokens,
        None => HOSTS.entry(host.to_string()).or_default().downgrade(),
    };
    if let Some(mut slot) = tokens.get_mut(token) {
        return apply(&mut slot);
    }
    apply(&mut tokens.entry(token.to_string()).or_default())
}
