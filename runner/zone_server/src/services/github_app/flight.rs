use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use abnegate_secret::SecretValue;
use abnegate_vcs::pull_request::InstallationId;
use tokio::sync::OnceCell;

use super::error::Error;

#[derive(Default)]
pub(super) struct Outcome {
    minted: OnceCell<Result<SecretValue, Error>>,
    confirmed: OnceCell<Error>,
}

#[derive(Default)]
pub(super) struct Flights(Mutex<HashMap<InstallationId, Arc<Outcome>>>);

impl Flights {
    fn lock(&self) -> MutexGuard<'_, HashMap<InstallationId, Arc<Outcome>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn board(&self, installation: InstallationId) -> Flight<'_> {
        let outcome = self.lock().entry(installation).or_default().clone();
        Flight {
            flights: self,
            installation,
            outcome: Some(outcome),
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.lock().len()
    }
}

pub(super) struct Flight<'a> {
    flights: &'a Flights,
    installation: InstallationId,
    outcome: Option<Arc<Outcome>>,
}

impl Flight<'_> {
    fn outcome(&self) -> &Outcome {
        self.outcome
            .as_deref()
            .expect("a flight holds its outcome until it is dropped")
    }

    pub(super) async fn minted<F, Mint>(&self, mint: F) -> Result<SecretValue, Error>
    where
        F: FnOnce() -> Mint,
        Mint: Future<Output = Result<SecretValue, Error>>,
    {
        self.outcome()
            .minted
            .get_or_init(mint)
            .await
            .as_ref()
            .cloned()
            .map_err(Error::duplicate)
    }

    pub(super) async fn confirmed<F, Confirm>(&self, confirm: F) -> Error
    where
        F: FnOnce() -> Confirm,
        Confirm: Future<Output = Error>,
    {
        self.outcome()
            .confirmed
            .get_or_init(confirm)
            .await
            .duplicate()
    }
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        let Some(outcome) = self.outcome.take() else {
            return;
        };
        // Every clone and release of an outcome happens under this lock, so
        // its strong count is exact here.
        let mut flights = self.flights.lock();
        let last = flights
            .get(&self.installation)
            .is_some_and(|boarded| Arc::ptr_eq(boarded, &outcome))
            && Arc::strong_count(&outcome) == 2;
        if last {
            flights.remove(&self.installation);
        }
        drop(outcome);
    }
}
