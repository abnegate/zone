//! One lock per organization, forgotten once nobody holds it or waits for it.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{Mutex, OwnedMutexGuard};
use uuid::Uuid;

#[derive(Default)]
pub struct Locks {
    held: DashMap<Uuid, Arc<Mutex<()>>>,
}

/// The organization's lock, until it is dropped.
pub struct Guard<'a> {
    locks: &'a Locks,
    organization: Uuid,
    guard: Option<OwnedMutexGuard<()>>,
}

impl Locks {
    pub async fn lock(&self, organization: Uuid) -> Guard<'_> {
        let lock = self.held.entry(organization).or_default().value().clone();
        Guard {
            locks: self,
            organization,
            guard: Some(lock.lock_owned().await),
        }
    }

    #[cfg(test)]
    pub fn kept(&self, organization: Uuid) -> bool {
        self.held.contains_key(&organization)
    }
}

impl Drop for Guard<'_> {
    /// Every holder or waiter holds a count of the lock, and a new one takes it under the map's
    /// own lock, so a lock with no count but the map's is idle and can go.
    fn drop(&mut self) {
        drop(self.guard.take());
        self.locks
            .held
            .remove_if(&self.organization, |_, lock| Arc::strong_count(lock) == 1);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::sync::oneshot;
    use tokio::time::timeout;

    use super::*;

    const BRIEFLY: Duration = Duration::from_millis(100);
    const WAIT: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn an_organizations_lock_is_forgotten_once_released() {
        let locks = Locks::default();
        let organization = Uuid::new_v4();

        drop(locks.lock(organization).await);

        assert!(!locks.kept(organization));
    }

    #[tokio::test]
    async fn a_lock_is_kept_while_someone_waits_for_it_and_forgotten_after_them() {
        let locks = Arc::new(Locks::default());
        let organization = Uuid::new_v4();
        let first = locks.lock(organization).await;
        let (acquired, waited) = oneshot::channel();
        let (release, released) = oneshot::channel::<()>();
        let waiter = tokio::spawn({
            let locks = Arc::clone(&locks);
            async move {
                let _second = locks.lock(organization).await;
                let _ = acquired.send(());
                let _ = released.await;
            }
        });
        tokio::time::sleep(BRIEFLY).await;

        drop(first);
        timeout(WAIT, waited)
            .await
            .expect("the waiter to take the lock")
            .expect("the waiter to say so");
        assert!(
            locks.kept(organization),
            "a lock someone holds was forgotten"
        );
        let _ = release.send(());
        timeout(WAIT, waiter)
            .await
            .expect("the waiter to finish")
            .expect("the waiter not to panic");

        assert!(!locks.kept(organization));
    }

    #[tokio::test]
    async fn one_organizations_lock_waits_for_its_holder_and_not_for_anothers() {
        let locks = Locks::default();
        let (organization, other) = (Uuid::new_v4(), Uuid::new_v4());
        let held = locks.lock(organization).await;

        assert!(
            timeout(BRIEFLY, locks.lock(organization)).await.is_err(),
            "the lock was taken twice at once"
        );
        let other = timeout(BRIEFLY, locks.lock(other))
            .await
            .expect("another organization's lock is free");

        drop((held, other));
        timeout(WAIT, locks.lock(organization))
            .await
            .expect("the lock to be free once released");
    }
}
