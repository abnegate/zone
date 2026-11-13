use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use once_cell::sync::Lazy;
use tokio::sync::watch;
use uuid::Uuid;

static REQUESTS: Lazy<DashMap<Uuid, Vec<watch::Sender<bool>>>> = Lazy::new(DashMap::new);

/// A run's line for being told to stop, registered before the run is claimed.
///
/// Dropping it withdraws only this registration: one a worker that lost the
/// claim made beside the winner's leaves the winner reachable, and a stop
/// requested after the run ended reaches nothing rather than a later run's
/// signal.
pub(super) struct Signal {
    run: Uuid,
    sender: watch::Sender<bool>,
}

impl Signal {
    /// Resolve once a stop is requested, including one requested before this
    /// was first polled.
    pub(super) async fn requested(&self) {
        let mut receiver = self.sender.subscribe();
        let _ = receiver.wait_for(|stopped| *stopped).await;
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        if let Entry::Occupied(mut registered) = REQUESTS.entry(self.run) {
            registered
                .get_mut()
                .retain(|sender| !sender.same_channel(&self.sender));
            if registered.get().is_empty() {
                registered.remove();
            }
        }
    }
}

pub(super) fn register(run: Uuid) -> Signal {
    let (sender, _) = watch::channel(false);
    REQUESTS.entry(run).or_default().push(sender.clone());
    Signal { run, sender }
}

/// Tell the worker executing `run` to stop. Returns whether one was listening.
pub(crate) fn request(run: Uuid) -> bool {
    let Some(senders) = REQUESTS.get(&run) else {
        return false;
    };
    for sender in senders.iter() {
        sender.send_replace(true);
    }
    !senders.is_empty()
}

#[cfg(test)]
pub(super) fn listening(run: Uuid) -> bool {
    REQUESTS.contains_key(&run)
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::time::Duration;

    use super::*;

    const RESOLVED_WITHIN: Duration = Duration::from_secs(1);

    #[test]
    fn a_request_for_an_unregistered_run_reaches_nobody() {
        assert!(!request(Uuid::new_v4()));
    }

    #[tokio::test]
    async fn a_request_resolves_the_waiting_signal() {
        let run = Uuid::new_v4();
        let signal = register(run);
        let waiting = async { signal.requested().await };
        let requesting = async {
            tokio::task::yield_now().await;
            assert!(request(run), "the registration was live");
        };
        tokio::time::timeout(RESOLVED_WITHIN, async { tokio::join!(waiting, requesting) })
            .await
            .expect("the request resolved the signal");
    }

    #[tokio::test]
    async fn a_request_made_before_the_signal_is_polled_still_resolves_it() {
        let run = Uuid::new_v4();
        let signal = register(run);
        assert!(request(run));
        tokio::time::timeout(RESOLVED_WITHIN, signal.requested())
            .await
            .expect("an earlier request is not lost");
    }

    #[test]
    fn a_run_listens_only_while_its_signal_lives() {
        let run = Uuid::new_v4();
        assert!(!listening(run));
        let signal = register(run);
        assert!(listening(run));
        drop(signal);
        assert!(!listening(run));
    }

    #[test]
    fn a_dropped_signal_is_no_longer_reachable() {
        let run = Uuid::new_v4();
        drop(register(run));
        assert!(!request(run));
    }

    #[tokio::test]
    async fn dropping_a_superseded_signal_leaves_the_newer_registration() {
        let run = Uuid::new_v4();
        let older = register(run);
        let newer = register(run);
        drop(older);
        assert!(request(run), "the newer registration survived");
        tokio::time::timeout(RESOLVED_WITHIN, newer.requested())
            .await
            .expect("the newer signal heard the request");
        drop(newer);
        assert!(!request(run));
    }

    #[tokio::test]
    async fn a_registration_that_lost_the_claim_leaves_the_winners_reachable() {
        let run = Uuid::new_v4();
        let winner = register(run);
        let loser = register(run);
        drop(loser);
        assert!(request(run), "the winner's registration survived");
        tokio::time::timeout(RESOLVED_WITHIN, winner.requested())
            .await
            .expect("the winner heard the request");
        drop(winner);
        assert!(!request(run));
    }

    #[test]
    fn an_unrequested_signal_stays_pending() {
        let run = Uuid::new_v4();
        let signal = register(run);
        let mut requested = Box::pin(signal.requested());
        let waker = futures::task::noop_waker();
        let mut context = std::task::Context::from_waker(&waker);
        assert!(requested.as_mut().poll(&mut context).is_pending());
    }
}
