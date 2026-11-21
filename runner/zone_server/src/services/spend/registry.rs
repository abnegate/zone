//! Which derived keys this process has registered with which LiteLLM, so a request sends its key
//! without asking LiteLLM first. It is kept in memory alone: a restarted server registers each
//! key again, which also restores keys LiteLLM lost to a restore or a manual delete.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use dashmap::DashMap;

/// How long a key whose registration failed is sent under the master key before it is tried
/// again.
pub const COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Registered,
    Failed(Instant),
}

static ENTRIES: LazyLock<DashMap<(String, String), Entry>> = LazyLock::new(DashMap::new);

pub fn registered(host: &str, token: &str) -> bool {
    matches!(entry(host, token), Some(Entry::Registered))
}

pub fn cooling(host: &str, token: &str) -> bool {
    matches!(entry(host, token), Some(Entry::Failed(at)) if at.elapsed() < COOLDOWN)
}

pub fn mark(host: &str, token: &str, entry: Entry) {
    ENTRIES.insert((host.to_string(), token.to_string()), entry);
}

pub fn forget(host: &str, token: &str) {
    ENTRIES.remove(&(host.to_string(), token.to_string()));
}

/// The tokens registered with `host`.
pub fn tokens(host: &str) -> Vec<String> {
    ENTRIES
        .iter()
        .filter(|entry| entry.key().0 == host && *entry.value() == Entry::Registered)
        .map(|entry| entry.key().1.clone())
        .collect()
}

fn entry(host: &str, token: &str) -> Option<Entry> {
    ENTRIES
        .get(&(host.to_string(), token.to_string()))
        .map(|entry| *entry.value())
}
