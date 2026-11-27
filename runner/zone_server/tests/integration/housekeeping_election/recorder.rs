use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use abnegate_notify::{Channel, Error, Notification, Notifier};
use chrono::NaiveDateTime;

/// A notification channel that records which replica sent what, at the test's clock, after
/// refusing with each scripted failure in turn.
#[derive(Clone)]
pub struct Recorder {
    replica: usize,
    clock: Arc<Mutex<NaiveDateTime>>,
    deliveries: Arc<Mutex<Vec<(usize, NaiveDateTime)>>>,
    failures: Arc<Mutex<VecDeque<Error>>>,
}

impl Recorder {
    pub fn new(
        replica: usize,
        clock: &Arc<Mutex<NaiveDateTime>>,
        deliveries: &Arc<Mutex<Vec<(usize, NaiveDateTime)>>>,
    ) -> Self {
        Self {
            replica,
            clock: Arc::clone(clock),
            deliveries: Arc::clone(deliveries),
            failures: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    #[must_use]
    pub fn failing(self, failure: Error) -> Self {
        self.failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(failure);
        self
    }
}

#[async_trait::async_trait]
impl Notifier for Recorder {
    fn channel(&self) -> Channel {
        Channel::SLACK
    }

    async fn deliver(&self, _notification: &Notification) -> Result<(), Error> {
        let failure = self
            .failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        if let Some(failure) = failure {
            return Err(failure);
        }
        let at = *self.clock.lock().unwrap_or_else(PoisonError::into_inner);
        self.deliveries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((self.replica, at));
        Ok(())
    }
}
