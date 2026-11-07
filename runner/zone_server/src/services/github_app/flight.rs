use std::hash::Hash;
use std::ops::Deref;
use std::sync::Arc;

use super::flights::Flights;

pub(super) struct Flight<'a, Key: Copy + Eq + Hash, Outcome> {
    flights: &'a Flights<Key, Outcome>,
    key: Key,
    outcome: Option<Arc<Outcome>>,
}

impl<'a, Key: Copy + Eq + Hash, Outcome> Flight<'a, Key, Outcome> {
    pub(super) fn new(flights: &'a Flights<Key, Outcome>, key: Key, outcome: Arc<Outcome>) -> Self {
        Self {
            flights,
            key,
            outcome: Some(outcome),
        }
    }
}

impl<Key: Copy + Eq + Hash, Outcome> Deref for Flight<'_, Key, Outcome> {
    type Target = Outcome;

    fn deref(&self) -> &Outcome {
        self.outcome
            .as_deref()
            .expect("a flight holds its outcome until it is dropped")
    }
}

impl<Key: Copy + Eq + Hash, Outcome> Drop for Flight<'_, Key, Outcome> {
    fn drop(&mut self) {
        if let Some(outcome) = self.outcome.take() {
            self.flights.land(self.key, outcome);
        }
    }
}
