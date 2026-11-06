//! The repositories each installation reached when GitHub was last asked.

mod entry;

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use abnegate_vcs::pull_request::InstalledRepository;
use uuid::Uuid;

use entry::Entry;

pub const TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct Catalog(Mutex<HashMap<Uuid, Entry>>);

impl Catalog {
    fn lock(&self) -> MutexGuard<'_, HashMap<Uuid, Entry>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn get(&self, installation: Uuid, now: Instant) -> Option<Arc<[InstalledRepository]>> {
        self.lock()
            .get(&installation)
            .filter(|entry| entry.fresh(now))
            .map(|entry| Arc::clone(&entry.repositories))
    }

    pub fn insert(
        &self,
        installation: Uuid,
        repositories: Arc<[InstalledRepository]>,
        now: Instant,
    ) {
        let mut entries = self.lock();
        entries.retain(|_, entry| entry.fresh(now));
        entries.insert(
            installation,
            Entry {
                repositories,
                expires_at: now + TTL,
            },
        );
    }

    pub fn forget(&self, installation: Uuid) {
        self.lock().remove(&installation);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
    }
}

impl fmt::Debug for Catalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Catalog")
            .field("installations", &self.lock().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use abnegate_vcs::{RemoteBranchName, Repository};

    use super::*;

    fn repositories(names: &[&str]) -> Arc<[InstalledRepository]> {
        names
            .iter()
            .zip(1..)
            .map(|(name, id)| {
                InstalledRepository::new(
                    id,
                    Repository::new("acme", name).expect("repository"),
                    RemoteBranchName::parse("main").expect("branch"),
                    format!("https://github.com/acme/{name}"),
                )
            })
            .collect()
    }

    fn names(repositories: &[InstalledRepository]) -> Vec<&str> {
        repositories.iter().map(InstalledRepository::name).collect()
    }

    #[test]
    fn a_fresh_entry_is_served() {
        let catalog = Catalog::default();
        let installation = Uuid::new_v4();
        let now = Instant::now();
        catalog.insert(installation, repositories(&["widgets", "gadgets"]), now);

        let served = catalog
            .get(installation, now + TTL - Duration::from_millis(1))
            .expect("the entry is still fresh");

        assert_eq!(names(&served), ["widgets", "gadgets"]);
    }

    #[test]
    fn an_entry_expires_after_the_ttl() {
        let catalog = Catalog::default();
        let installation = Uuid::new_v4();
        let now = Instant::now();
        catalog.insert(installation, repositories(&["widgets"]), now);

        assert!(catalog.get(installation, now + TTL).is_none());
    }

    #[test]
    fn an_installation_never_listed_misses() {
        let catalog = Catalog::default();
        catalog.insert(Uuid::new_v4(), repositories(&["widgets"]), Instant::now());

        assert!(catalog.get(Uuid::new_v4(), Instant::now()).is_none());
    }

    #[test]
    fn a_forgotten_installation_misses_and_spares_the_others() {
        let catalog = Catalog::default();
        let forgotten = Uuid::new_v4();
        let kept = Uuid::new_v4();
        let now = Instant::now();
        catalog.insert(forgotten, repositories(&["widgets"]), now);
        catalog.insert(kept, repositories(&["gadgets"]), now);

        catalog.forget(forgotten);
        catalog.forget(Uuid::new_v4());

        assert!(catalog.get(forgotten, now).is_none());
        assert_eq!(names(&catalog.get(kept, now).expect("kept")), ["gadgets"]);
    }

    #[test]
    fn inserting_prunes_expired_entries() {
        let catalog = Catalog::default();
        let stale = Uuid::new_v4();
        let live = Uuid::new_v4();
        let fresh = Uuid::new_v4();
        let start = Instant::now();
        catalog.insert(stale, repositories(&["old"]), start);
        catalog.insert(live, repositories(&["recent"]), start + TTL / 2);

        let later = start + TTL;
        catalog.insert(fresh, repositories(&["new"]), later);

        assert_eq!(catalog.len(), 2);
        assert!(catalog.get(stale, later).is_none());
        assert!(catalog.get(live, later).is_some());
        assert!(catalog.get(fresh, later).is_some());
    }

    #[test]
    fn reinserting_replaces_and_renews_an_entry() {
        let catalog = Catalog::default();
        let installation = Uuid::new_v4();
        let start = Instant::now();
        catalog.insert(installation, repositories(&["widgets"]), start);

        let later = start + TTL / 2;
        catalog.insert(installation, repositories(&["gadgets"]), later);

        let served = catalog
            .get(installation, start + TTL)
            .expect("the renewed entry outlives the first");
        assert_eq!(names(&served), ["gadgets"]);
    }

    #[test]
    fn debug_counts_installations_without_listing_repositories() {
        let catalog = Catalog::default();
        catalog.insert(Uuid::new_v4(), repositories(&["widgets"]), Instant::now());

        let rendered = format!("{catalog:?}");

        assert!(rendered.contains("installations: 1"), "{rendered}");
        assert!(!rendered.contains("widgets"), "{rendered}");
    }
}
