use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::flight::Flight;

pub(super) struct Flights<Key, Outcome>(Mutex<HashMap<Key, Arc<Outcome>>>);

impl<Key, Outcome> Default for Flights<Key, Outcome> {
    fn default() -> Self {
        Self(Mutex::default())
    }
}

impl<Key: Copy + Eq + Hash, Outcome: Default> Flights<Key, Outcome> {
    pub(super) fn board(&self, key: Key) -> Flight<'_, Key, Outcome> {
        let outcome = self.lock().entry(key).or_default().clone();
        Flight::new(self, key, outcome)
    }
}

impl<Key: Copy + Eq + Hash, Outcome> Flights<Key, Outcome> {
    fn lock(&self) -> MutexGuard<'_, HashMap<Key, Arc<Outcome>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn land(&self, key: Key, outcome: Arc<Outcome>) {
        // Every clone and release of an outcome happens under this lock, so
        // its strong count is exact here.
        let mut flights = self.lock();
        let last = flights
            .get(&key)
            .is_some_and(|boarded| Arc::ptr_eq(boarded, &outcome))
            && Arc::strong_count(&outcome) == 2;
        if last {
            flights.remove(&key);
        }
        drop(outcome);
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.lock().len()
    }
}
