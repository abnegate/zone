use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

/// A read's hold on its key, released when dropped, so a read that fails,
/// returns early or is cancelled never leaves its key behind
#[derive(Debug)]
pub(crate) struct Claim<Key: Eq + Hash> {
    keys: Arc<Mutex<HashSet<Key>>>,
    key: Key,
}

impl<Key: Eq + Hash> Claim<Key> {
    pub(super) fn new(keys: Arc<Mutex<HashSet<Key>>>, key: Key) -> Self {
        Self { keys, key }
    }
}

impl<Key: Eq + Hash> Drop for Claim<Key> {
    fn drop(&mut self) {
        self.keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}
